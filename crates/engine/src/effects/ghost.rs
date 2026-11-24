//! **Draconic Ghost** – a dynamic resonance suppressor. It listens to the signal, finds the frequencies
//! that stick out of their neighbourhood *right now* (harsh 2–5 kHz in a vocal, a boomy room mode, a
//! ringing snare) and pulls only those down, only while they are there.
//!
//! Built to cost as little CPU as possible:
//! - **One analysis for both channels**: the two channels are packed into one complex FFT
//!   (`a + i·b`) and pulled apart by symmetry.
//! - **Band domain**: detection, the reference envelope, the shaping, the ballistics and the stereo link
//!   run on 60 bands (1/6 octave, 20 Hz – 20 kHz) – what the filter bank needs anyway – instead of on
//!   every FFT bin.
//! - **Ghost engine (Zero / Low)**: the cuts are a parallel bank, `y = x + Σ cᵢ·BPᵢ(x)`, of
//!   constant-coefficient band-passes – only the scalars `cᵢ` move (interpolated per sample, no zipper
//!   noise), the delta (what's removed) is exact, and a band that isn't cutting isn't computed at all.
//!   The target gains are corrected for the bands' overlap with a precomputed inverse interaction
//!   matrix. Zero: no latency; Low: 64 samples (1.3 ms) of lookahead so the cut starts before the
//!   transient.
//! - **Precise**: an STFT (√Hann, N = 1024, hop 256, both channels in one complex FFT), per-bin gains
//!   interpolated from the bands; 1024 samples latency.
//!
//! Per hop (128 samples in Zero / Low): band levels → focus-EQ weighting → reference envelope (a Gaussian
//! over the neighbouring bands, its width set by Selectivity) → excess over it with a soft (Soft) or
//! hard (Hard) knee → reduction = Depth² · sensitivity · (excess/12)^γ · 12, γ from Sharpness, capped
//! at 30 dB → low / high cut window → smoothing across bands (Sharpness) → attack / release → stereo
//! link → band gains. Delta, mix (latency-aligned dry), trim, bypass (10 ms crossfade) and a band solo
//! (listen to one focus band) follow.

use super::{Effect, EffectDef, ParamInfo};
use crate::dsp::{db_to_gain, Biquad, BiquadKind};
use rustfft::num_complex::Complex32;
use std::sync::Arc;

pub const ENGINES: &[&str] = &["Zero", "Low", "Precise"];
pub const CHARACTERS: &[&str] = &["Soft", "Hard"];
pub const RESOLUTIONS: &[&str] = &["1×", "2×", "4×"];
pub const STEREO: &[&str] = &["L/R", "M/S"];
pub const SOLO: &[&str] = &["Off", "1", "2", "3", "4", "5", "6"];

/// Bands of the analysis and the filter bank: 1/6 octave from 20 Hz.
pub const NB: usize = 60;
/// Analysis window (samples) and hop in Zero / Low.
const WIN: usize = 1024;
const HOP: usize = 128;
/// Precise: STFT size and hop.
const P_HOP: usize = 256;
/// Low: lookahead (samples).
const LOOKAHEAD: usize = 64;
/// Q of the filter-bank band-passes (1/6-octave spacing).
const BANK_Q: f32 = 8.65;

/// Centre frequency of band `b`.
pub fn band_hz(b: usize) -> f32 {
    20.0 * 2f32.powf(b as f32 / 6.0)
}

static PARAMS: &[ParamInfo] = &[
    ParamInfo::lin("depth", "Depth", 0.0, 100.0, 50.0, "%"),
    ParamInfo::lin("sharpness", "Sharpness", 0.0, 10.0, 5.0, ""),
    ParamInfo::lin("selectivity", "Selectivity", 0.0, 10.0, 5.0, ""),
    ParamInfo::log("attack", "Attack", 0.1, 50.0, 3.0, "ms"),
    ParamInfo::log("release", "Release", 5.0, 500.0, 60.0, "ms"),
    ParamInfo::lin("mix", "Mix", 0.0, 100.0, 100.0, "%"),
    ParamInfo::lin("trim", "Trim", -12.0, 12.0, 0.0, "dB"),
    ParamInfo::choice("engine", "Engine", ENGINES, 1.0),
    ParamInfo::choice("character", "Character", CHARACTERS, 0.0),
    ParamInfo::choice("resolution", "Resolution", RESOLUTIONS, 0.0),
    ParamInfo::choice("stereo", "Stereo", STEREO, 0.0),
    ParamInfo::lin("link", "Stereo link", 0.0, 100.0, 100.0, "%"),
    ParamInfo::lin("ms_balance", "M/S balance", -100.0, 100.0, 0.0, "%"),
    ParamInfo::toggle("delta", "Delta", false),
    ParamInfo::toggle("bypass", "Bypass", false),
    ParamInfo::toggle("sidechain", "Sidechain", false),
    ParamInfo::log("low_cut", "Low cut", 20.0, 1000.0, 40.0, "Hz"),
    ParamInfo::log("high_cut", "High cut", 1000.0, 20000.0, 18000.0, "Hz"),
    ParamInfo::choice("solo", "Band solo", SOLO, 0.0),
    // Focus EQ: low shelf, four bells, high shelf – they shape the sensitivity, not the sound.
    ParamInfo::log("f1", "Focus 1 freq", 20.0, 500.0, 60.0, "Hz"),
    ParamInfo::lin("g1", "Focus 1 gain", -12.0, 12.0, 0.0, "dB"),
    ParamInfo::log("f2", "Focus 2 freq", 40.0, 2000.0, 200.0, "Hz"),
    ParamInfo::lin("g2", "Focus 2 gain", -12.0, 12.0, 0.0, "dB"),
    ParamInfo::log("q2", "Focus 2 Q", 0.3, 8.0, 1.0, ""),
    ParamInfo::log("f3", "Focus 3 freq", 100.0, 5000.0, 600.0, "Hz"),
    ParamInfo::lin("g3", "Focus 3 gain", -12.0, 12.0, 0.0, "dB"),
    ParamInfo::log("q3", "Focus 3 Q", 0.3, 8.0, 1.0, ""),
    ParamInfo::log("f4", "Focus 4 freq", 400.0, 12000.0, 2000.0, "Hz"),
    ParamInfo::lin("g4", "Focus 4 gain", -12.0, 12.0, 0.0, "dB"),
    ParamInfo::log("q4", "Focus 4 Q", 0.3, 8.0, 1.0, ""),
    ParamInfo::log("f5", "Focus 5 freq", 1000.0, 18000.0, 5000.0, "Hz"),
    ParamInfo::lin("g5", "Focus 5 gain", -12.0, 12.0, 0.0, "dB"),
    ParamInfo::log("q5", "Focus 5 Q", 0.3, 8.0, 1.0, ""),
    ParamInfo::log("f6", "Focus 6 freq", 2000.0, 20000.0, 12000.0, "Hz"),
    ParamInfo::lin("g6", "Focus 6 gain", -12.0, 12.0, 0.0, "dB"),
];

pub fn def() -> EffectDef {
    EffectDef {
        id: "ghost",
        name: "Draconic Ghost (resonance suppressor)",
        category: "Dynamics",
        description: "Dynamic resonance suppressor: finds the frequencies that stick out right now and pulls only those down, only while they're there. Zero-latency, low-latency and precise engines, focus EQ, soft / hard character, L/R or M/S with stereo link, sidechain, delta.",
        params: PARAMS,
        uses_sidechain: true,
        create: |rate| Box::new(Ghost::new(rate)),
    }
}

/// The focus nodes: (kind 0 = low shelf, 1 = bell, 2 = high shelf, frequency, gain dB, Q).
pub type Node = (u8, f32, f32, f32);

/// The focus EQ's sensitivity curve at `hz` (dB): smooth bell and shelf shapes (also drawn by the UI).
pub fn focus_db(nodes: &[Node; 6], hz: f32) -> f32 {
    let mut db = 0.0;
    for &(kind, f, g, q) in nodes {
        if g.abs() < 1e-3 {
            continue;
        }
        let x = (hz / f).log2();
        db += match kind {
            0 => g / (1.0 + (x * 4.0).exp()),
            2 => g / (1.0 + (-x * 4.0).exp()),
            _ => {
                // About an octave wide at Q 1, narrower with higher Q.
                let w = 0.6 / q.max(0.05);
                g * (-(x * x) / (2.0 * w * w)).exp()
            }
        };
    }
    db
}

/// The low / high cut window (0 … 1) at `hz`: 1 between the cuts, a raised-cosine fade over 1/3 octave
/// outside them.
pub fn cut_window(low: f32, high: f32, hz: f32) -> f32 {
    let fade = |o: f32| {
        if o <= 0.0 {
            1.0
        } else if o >= 1.0 / 3.0 {
            0.0
        } else {
            0.5 + 0.5 * (std::f32::consts::PI * o * 3.0).cos()
        }
    };
    fade((low / hz).log2()) * fade((hz / high).log2())
}

/// The constant band-pass bank (structure of arrays, so the per-sample loop over the active bands stays
/// tight).
struct Bank {
    b0: [f32; NB],
    a1: [f32; NB],
    a2: [f32; NB],
    z1: [[f32; NB]; 2],
    z2: [[f32; NB]; 2],
    /// Bands below 0.45 · the sample rate.
    valid: usize,
}

impl Bank {
    fn new(rate: f32) -> Self {
        let mut b = Bank { b0: [0.0; NB], a1: [0.0; NB], a2: [0.0; NB], z1: [[0.0; NB]; 2], z2: [[0.0; NB]; 2], valid: 0 };
        for k in 0..NB {
            let f = band_hz(k);
            if f > rate * 0.45 {
                break;
            }
            let w0 = std::f32::consts::TAU * f / rate;
            let alpha = w0.sin() / (2.0 * BANK_Q);
            let a0 = 1.0 + alpha;
            b.b0[k] = alpha / a0;
            b.a1[k] = -2.0 * w0.cos() / a0;
            b.a2[k] = (1.0 - alpha) / a0;
            b.valid = k + 1;
        }
        b
    }

    /// The band-pass of band `j` at `hz` (complex response).
    fn response(&self, j: usize, hz: f32, rate: f32) -> Complex32 {
        let w = std::f32::consts::TAU * hz / rate;
        let z1 = Complex32::from_polar(1.0, -w);
        let z2 = z1 * z1;
        let num = Complex32::new(self.b0[j], 0.0) - z2 * self.b0[j];
        let den = Complex32::new(1.0, 0.0) + z1 * self.a1[j] + z2 * self.a2[j];
        num / den
    }
}

/// Inverse of the bank's interaction matrix M[i][j] = Re(BPⱼ at band i's centre), so the band scalars
/// reach the wanted gain at each centre despite the overlap (row-major, `valid` × `valid`).
fn inverse_interaction(bank: &Bank, rate: f32) -> Vec<f32> {
    let n = bank.valid;
    let mut a = vec![0.0f64; n * n];
    let mut inv = vec![0.0f64; n * n];
    for i in 0..n {
        for j in 0..n {
            a[i * n + j] = bank.response(j, band_hz(i), rate).re as f64;
        }
        inv[i * n + i] = 1.0;
    }
    // Gauss–Jordan (diagonally dominant: no pivoting trouble).
    for c in 0..n {
        let p = a[c * n + c];
        for j in 0..n {
            a[c * n + j] /= p;
            inv[c * n + j] /= p;
        }
        for r in 0..n {
            if r != c {
                let f = a[r * n + c];
                if f != 0.0 {
                    for j in 0..n {
                        a[r * n + j] -= f * a[c * n + j];
                        inv[r * n + j] -= f * inv[c * n + j];
                    }
                }
            }
        }
    }
    inv.into_iter().map(|x| x as f32).collect()
}

/// Bin → band weights for one FFT size: per band the (bin, weight) list (a triangle on log frequency
/// spanning the neighbouring band centres; two interpolating bins where a band holds no bin).
struct BandMap {
    taps: Vec<Vec<(u16, f32)>>,
    /// Precise: per bin, (band position for interpolation, gain-applies) – band index + fraction.
    bin_band: Vec<f32>,
}

impl BandMap {
    fn new(n: usize, rate: f32) -> Self {
        let df = rate / n as f32;
        let half = n / 2;
        let mut taps = Vec::with_capacity(NB);
        for b in 0..NB {
            let (lo, c, hi) = (band_hz(b) / 2f32.powf(1.0 / 6.0), band_hz(b), band_hz(b) * 2f32.powf(1.0 / 6.0));
            let mut t: Vec<(u16, f32)> = Vec::new();
            let k0 = ((lo / df).floor() as usize).max(1);
            let k1 = ((hi / df).ceil() as usize).min(half - 1);
            for k in k0..=k1 {
                let f = k as f32 * df;
                if f <= lo || f >= hi {
                    continue;
                }
                let w = if f < c { (f / lo).log2() / (c / lo).log2() } else { (hi / f).log2() / (hi / c).log2() };
                if w > 0.0 {
                    t.push((k as u16, w));
                }
            }
            if t.is_empty() {
                // Narrower than a bin: interpolate the two bins around the centre.
                let x = (c / df).clamp(1.0, (half - 2) as f32);
                let k = x.floor() as usize;
                let fr = x - k as f32;
                t.push((k as u16, 1.0 - fr));
                t.push((k as u16 + 1, fr));
            }
            let s: f32 = t.iter().map(|x| x.1).sum();
            t.iter_mut().for_each(|x| x.1 /= s);
            taps.push(t);
        }
        let bin_band = (0..=half).map(|k| ((k as f32 * df).max(1.0) / 20.0).log2() * 6.0).collect();
        BandMap { taps, bin_band }
    }
}

fn hann(n: usize) -> Vec<f32> {
    (0..n).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos()).collect()
}

pub struct Ghost {
    rate: f32,
    // Settings.
    depth: f32,
    sharpness: f32,
    selectivity: f32,
    attack: f32,
    release: f32,
    mix: f32,
    trim: f32,
    engine: usize,
    hard: bool,
    resolution: usize,
    ms: bool,
    link: f32,
    ms_balance: f32,
    delta: bool,
    bypass: bool,
    use_sc: bool,
    low_cut: f32,
    high_cut: f32,
    solo: usize,
    nodes: [Node; 6],
    focus: [f32; NB],
    /// Depth multiplier per band from the focus EQ (2^(W/6)).
    sens: [f32; NB],
    cut: [f32; NB],
    shape_dirty: bool,
    /// Cached kernels: (width they were made for, weights for offsets −r … r).
    env_k: (f32, Vec<f32>),
    smooth_k: (f32, Vec<f32>),
    // Analysis.
    ffts: [Arc<dyn rustfft::Fft<f32>>; 3],
    maps: [BandMap; 3],
    win: Vec<f32>,
    ring: [Vec<f32>; 2],
    pos: usize,
    since: usize,
    frame: Vec<Complex32>,
    scratch: Vec<Complex32>,
    /// Per-bin powers of the two channels (scratch for the band sums).
    bin_pow: [Vec<f32>; 2],
    // Reduction per channel (dB ≥ 0): smoothed (ballistics) and the bank's scalars before / after.
    red: [[f32; NB]; 2],
    c_from: [[f32; NB]; 2],
    c_to: [[f32; NB]; 2],
    active: [[bool; NB]; 2],
    /// The active bands per channel, as a compact list (the per-sample loop runs over these only).
    act_idx: [[u8; NB]; 2],
    act_n: [usize; 2],
    /// This hop runs every band (many cut).
    dense: [bool; 2],
    hop_k: usize,
    bank: Bank,
    inv: Vec<f32>,
    // Lookahead (Low) and the dry path aligned to the latency.
    look: [Vec<f32>; 2],
    look_pos: usize,
    dry: [Vec<f32>; 2],
    dry_pos: usize,
    // Precise STFT.
    p_fft: Arc<dyn rustfft::Fft<f32>>,
    p_ifft: Arc<dyn rustfft::Fft<f32>>,
    p_win: Vec<f32>,
    p_in: [Vec<f32>; 2],
    p_out: [Vec<f32>; 2],
    p_pos: usize,
    p_since: usize,
    // Output.
    byp: f32,
    solo_f: [Biquad; 2],
    solo_cur: usize,
    /// Telemetry: [in dB, out dB, GR dB, latency, 60 band levels (dB), 60 reductions (dB ≥ 0)].
    tele: Vec<f32>,
    disp: [f32; NB],
    gr: f32,
    /// Samples of silence in a row (input and key).
    silent: usize,
}

impl Ghost {
    pub fn new(rate: f32) -> Self {
        let mut planner = rustfft::FftPlanner::<f32>::new();
        let ffts: [Arc<dyn rustfft::Fft<f32>>; 3] = [planner.plan_fft_forward(WIN), planner.plan_fft_forward(WIN * 2), planner.plan_fft_forward(WIN * 4)];
        let maps = [BandMap::new(WIN, rate), BandMap::new(WIN * 2, rate), BandMap::new(WIN * 4, rate)];
        let p_fft = planner.plan_fft_forward(WIN);
        let p_ifft = planner.plan_fft_inverse(WIN);
        let scratch_len = ffts.iter().map(|f| f.get_inplace_scratch_len()).chain([p_fft.get_inplace_scratch_len(), p_ifft.get_inplace_scratch_len()]).max().unwrap_or(0);
        let bank = Bank::new(rate);
        let inv = inverse_interaction(&bank, rate);
        let p_win: Vec<f32> = hann(WIN).into_iter().map(f32::sqrt).collect();
        let mut g = Ghost {
            rate,
            depth: 0.25,
            sharpness: 5.0,
            selectivity: 5.0,
            attack: 3.0,
            release: 60.0,
            mix: 1.0,
            trim: 1.0,
            engine: 1,
            hard: false,
            resolution: 0,
            ms: false,
            link: 1.0,
            ms_balance: 0.0,
            delta: false,
            bypass: false,
            use_sc: false,
            low_cut: 40.0,
            high_cut: 18000.0,
            solo: 0,
            nodes: [(0, 60.0, 0.0, 0.7), (1, 200.0, 0.0, 1.0), (1, 600.0, 0.0, 1.0), (1, 2000.0, 0.0, 1.0), (1, 5000.0, 0.0, 1.0), (2, 12000.0, 0.0, 0.7)],
            focus: [0.0; NB],
            sens: [1.0; NB],
            cut: [1.0; NB],
            shape_dirty: true,
            env_k: (-1.0, Vec::with_capacity(128)),
            smooth_k: (-1.0, Vec::with_capacity(128)),
            ffts,
            maps,
            win: hann(WIN),
            ring: [vec![0.0; WIN], vec![0.0; WIN]],
            pos: 0,
            since: 0,
            frame: vec![Complex32::new(0.0, 0.0); WIN * 4],
            scratch: vec![Complex32::new(0.0, 0.0); scratch_len],
            bin_pow: [vec![0.0; WIN * 2 + 1], vec![0.0; WIN * 2 + 1]],
            red: [[0.0; NB]; 2],
            c_from: [[0.0; NB]; 2],
            c_to: [[0.0; NB]; 2],
            active: [[false; NB]; 2],
            act_idx: [[0; NB]; 2],
            act_n: [0; 2],
            dense: [false; 2],
            hop_k: 0,
            bank,
            inv,
            look: [vec![0.0; LOOKAHEAD], vec![0.0; LOOKAHEAD]],
            look_pos: 0,
            dry: [vec![0.0; WIN], vec![0.0; WIN]],
            dry_pos: 0,
            p_fft,
            p_ifft,
            p_win,
            p_in: [vec![0.0; WIN], vec![0.0; WIN]],
            p_out: [vec![0.0; WIN], vec![0.0; WIN]],
            p_pos: 0,
            p_since: 0,
            byp: 0.0,
            solo_f: [Biquad::default(), Biquad::default()],
            solo_cur: 0,
            tele: vec![-120.0; 4 + 2 * NB],
            disp: [-120.0; NB],
            gr: 0.0,
            silent: 0,
        };
        g.tele[3] = LOOKAHEAD as f32;
        g.reshape();
        g
    }

    fn reshape(&mut self) {
        for b in 0..NB {
            let f = band_hz(b);
            self.focus[b] = focus_db(&self.nodes, f);
            self.sens[b] = 2f32.powf(self.focus[b] / 6.0);
            self.cut[b] = cut_window(self.low_cut, self.high_cut, f);
        }
        self.shape_dirty = false;
    }

    /// Gaussian weights for offsets −3σ … 3σ (rebuilt only when σ changes; the capacity is reserved,
    /// so this doesn't allocate on the audio thread).
    fn kernel(k: &mut (f32, Vec<f32>), sig: f32) -> i32 {
        let r = (sig * 3.0).ceil() as i32;
        if (k.0 - sig).abs() > 1e-4 {
            k.0 = sig;
            k.1.clear();
            for o in -r..=r {
                k.1.push((-(o * o) as f32 / (2.0 * sig * sig)).exp());
            }
        }
        r
    }

    fn latency_now(&self) -> usize {
        match self.engine {
            0 => 0,
            1 => LOOKAHEAD,
            _ => WIN,
        }
    }

    /// The reduction targets (dB ≥ 0) for both detector channels from their band powers; ballistics
    /// over `hop` samples, stereo link. Leaves the smoothed reductions in `self.red`.
    fn detect(&mut self, power: &[[f32; NB]; 2], hop: usize) {
        let d = self.depth;
        let gamma = 0.6 + 0.24 * self.sharpness + if self.hard { 0.5 } else { 0.0 };
        let t = if self.hard { 0.5 } else { 3.0 };
        // Reference envelope width (bands) and the cut curve's smoothing width (bands).
        let sig = (2.0 * (1.0f32 / 24.0).powf(self.selectivity / 10.0) * 6.0).max(0.75);
        let smooth = ((1.0 / 3.0) * (1.0f32 / 16.0).powf(self.sharpness / 10.0) * 6.0 * 0.5) + if self.hard { 0.0 } else { 0.25 };
        let att = 1.0 - (-(hop as f32) / (self.attack.max(0.05) * 0.001 * self.rate)).exp();
        let rel_ms = self.release * if self.hard { 1.0 } else { 1.3 };
        let rel = 1.0 - (-(hop as f32) / (rel_ms * 0.001 * self.rate)).exp();
        let ch_depth = if self.ms {
            let b = self.ms_balance;
            [if b > 0.0 { 1.0 - b } else { 1.0 }, if b < 0.0 { 1.0 + b } else { 1.0 }]
        } else {
            [1.0, 1.0]
        };
        let n = self.bank.valid;
        let mut target = [[0.0f32; NB]; 2];
        let r = Self::kernel(&mut self.env_k, sig);
        let rr = if smooth > 0.3 { Self::kernel(&mut self.smooth_k, smooth) } else { 0 };
        let (ek, sk) = (&self.env_k.1, &self.smooth_k.1);
        for ch in 0..2 {
            let mut base = [0.0f32; NB];
            let mut lv = [0.0f32; NB];
            for b in 0..n {
                base[b] = 10.0 * (power[ch][b] + 1e-12).log10();
                lv[b] = base[b] + self.focus[b];
            }
            let mut raw = [0.0f32; NB];
            for b in 0..n {
                if base[b] < -80.0 || self.cut[b] <= 0.0 {
                    continue;
                }
                // Reference: a Gaussian over the neighbours (the band itself left out, so a spike stands
                // out even with the narrowest envelope).
                let (mut s, mut w) = (0.0f32, 0.0f32);
                let lo = (b as i32 - r).max(0);
                let hi = (b as i32 + r).min(n as i32 - 1);
                for j in lo..=hi {
                    if j == b as i32 {
                        continue;
                    }
                    let k = ek[(j - b as i32 + r) as usize];
                    s += lv[j as usize] * k;
                    w += k;
                }
                if w <= 0.0 {
                    continue;
                }
                let x = lv[b] - s / w;
                // Soft knee of width 2T.
                let ex = if x <= 0.0 {
                    0.0
                } else if x < 2.0 * t {
                    x * x / (4.0 * t)
                } else {
                    x - t
                };
                if ex <= 0.0 {
                    continue;
                }
                raw[b] = (d * ch_depth[ch] * self.sens[b] * (ex / 12.0).powf(gamma) * 12.0).min(30.0) * self.cut[b];
            }
            // Smoothing across bands (Sharpness; Soft adds a little).
            if rr > 0 && raw.iter().any(|x| *x > 0.0) {
                for b in 0..n {
                    let (mut s, mut w) = (0.0f32, 0.0f32);
                    let lo = (b as i32 - rr).max(0);
                    let hi = (b as i32 + rr).min(n as i32 - 1);
                    for j in lo..=hi {
                        let k = sk[(j - b as i32 + rr) as usize];
                        s += raw[j as usize] * k;
                        w += k;
                    }
                    target[ch][b] = s / w;
                }
            } else {
                target[ch] = raw;
            }
        }
        // Ballistics.
        for ch in 0..2 {
            for b in 0..n {
                let cur = self.red[ch][b];
                let tg = target[ch][b];
                self.red[ch][b] = cur + (tg - cur) * if tg > cur { att } else { rel };
            }
        }
        // Stereo link: towards the deeper of the two.
        if self.link > 0.0 {
            for b in 0..n {
                let deep = self.red[0][b].max(self.red[1][b]);
                for ch in 0..2 {
                    self.red[ch][b] = (1.0 - self.link) * self.red[ch][b] + self.link * deep;
                }
            }
        }
    }

    /// Band powers of both channels from the packed spectrum in `frame` (size `n`): each bin's two
    /// powers once, then the bands' weighted sums.
    fn band_powers(&mut self, n: usize, map: &BandMap) -> [[f32; NB]; 2] {
        let half = n / 2;
        for k in 0..=half {
            let x = self.frame[k];
            let y = self.frame[(n - k) % n].conj();
            let a = (x + y) * 0.5;
            let b = (x - y) * Complex32::new(0.0, -0.5);
            self.bin_pow[0][k] = a.norm_sqr();
            self.bin_pow[1][k] = b.norm_sqr();
        }
        let mut out = [[0.0f32; NB]; 2];
        for b in 0..self.bank.valid {
            let (mut pa, mut pb) = (0.0f32, 0.0f32);
            for &(k, w) in &map.taps[b] {
                pa += self.bin_pow[0][k as usize] * w;
                pb += self.bin_pow[1][k as usize] * w;
            }
            out[0][b] = pa;
            out[1][b] = pb;
        }
        out
    }

    /// Meters, the display spectrum and the GR from the band powers (channel 0 + 1).
    fn telemetry_from(&mut self, power: &[[f32; NB]; 2], hop: usize, norm: f32) {
        let fall = 1.0 - (-(hop as f32) / (0.2 * self.rate)).exp();
        let (mut num, mut den) = (0.0f32, 0.0f32);
        for b in 0..self.bank.valid {
            let p = (power[0][b] + power[1][b]) * norm;
            let lv = 10.0 * (p + 1e-12).log10();
            self.disp[b] = if lv > self.disp[b] { lv } else { self.disp[b] + (lv - self.disp[b]) * fall };
            let r = self.red[0][b].max(self.red[1][b]);
            num += p * db_to_gain(-r).powi(2);
            den += p;
            self.tele[4 + b] = self.disp[b];
            self.tele[4 + NB + b] = r;
        }
        self.gr = if den > 1e-12 { -10.0 * (num / den).max(1e-12).log10() } else { 0.0 };
        self.tele[2] = self.gr;
    }

    /// Zero / Low: a hop of analysis – window the ring, one packed FFT, bands, detection, new scalars.
    fn analyse(&mut self) {
        let n = WIN << self.resolution;
        for (i, w) in self.win.iter().enumerate() {
            let k = (self.pos + i) % WIN;
            self.frame[i] = Complex32::new(self.ring[0][k] * w, self.ring[1][k] * w);
        }
        self.frame[WIN..n].iter_mut().for_each(|x| *x = Complex32::new(0.0, 0.0));
        let fft = self.ffts[self.resolution].clone();
        fft.process_with_scratch(&mut self.frame[..n], &mut self.scratch);
        let map = std::mem::replace(&mut self.maps[self.resolution], BandMap { taps: Vec::new(), bin_band: Vec::new() });
        let power = self.band_powers(n, &map);
        self.maps[self.resolution] = map;
        self.detect(&power, HOP);
        // Hann window power gain: Σw² = 3N/8; a full-scale sine's bin power ≈ (N/4)².
        let norm = 4.0 / (WIN as f32 * WIN as f32 * 0.375) * 0.25;
        self.telemetry_from(&power, HOP, norm);
        // New bank scalars: wanted linear gain at each centre, corrected for the overlap.
        let nv = self.bank.valid;
        for ch in 0..2 {
            self.act_n[ch] = 0;
            self.c_from[ch] = self.c_to[ch];
            // Wanted gain − 1 per band (cuts under 0.05 dB count as none); the overlap correction only
            // near the bands that cut (its weights fade within a few bands), so idle bands stay idle.
            const NEAR: usize = 4;
            let mut want = [0.0f32; NB];
            let mut near = [false; NB];
            for b in 0..nv {
                if self.red[ch][b] > 0.05 {
                    want[b] = db_to_gain(-self.red[ch][b]) - 1.0;
                    for k in b.saturating_sub(NEAR)..(b + NEAR + 1).min(nv) {
                        near[k] = true;
                    }
                }
            }
            for i in 0..nv {
                let c = if near[i] {
                    let (lo, hi) = (i.saturating_sub(2 * NEAR), (i + 2 * NEAR + 1).min(nv));
                    let row = &self.inv[i * nv..(i + 1) * nv];
                    row[lo..hi].iter().zip(&want[lo..hi]).map(|(a, b)| a * b).sum::<f32>()
                } else {
                    0.0
                };
                let c = c.clamp(-1.0, 0.25);
                self.c_to[ch][i] = if c.abs() < 5e-4 { 0.0 } else { c };
                let on = self.c_from[ch][i] != 0.0 || self.c_to[ch][i] != 0.0;
                if on && !self.active[ch][i] && !self.dense[ch] {
                    // A band coming in starts from rest (its scalar starts at 0: no transient).
                    self.bank.z1[ch][i] = 0.0;
                    self.bank.z2[ch][i] = 0.0;
                }
                self.active[ch][i] = on;
                if on {
                    self.act_idx[ch][self.act_n[ch]] = i as u8;
                    self.act_n[ch] += 1;
                }
            }
            // Dense hops keep every band's state running (the next sparse hop may resume any of them).
            self.dense[ch] = self.act_n[ch] > 12;
        }
        self.hop_k = 0;
    }

    /// Precise: one STFT hop on both channels – window, FFT, detection from this frame, per-bin gains,
    /// inverse FFT, overlap-add.
    fn precise_hop(&mut self) {
        for i in 0..WIN {
            let k = (self.p_pos + i) % WIN;
            self.frame[i] = Complex32::new(self.p_in[0][k] * self.p_win[i], self.p_in[1][k] * self.p_win[i]);
        }
        let fft = self.p_fft.clone();
        fft.process_with_scratch(&mut self.frame[..WIN], &mut self.scratch);
        let map = std::mem::replace(&mut self.maps[0], BandMap { taps: Vec::new(), bin_band: Vec::new() });
        let power = self.band_powers(WIN, &map);
        self.detect(&power, P_HOP);
        let norm = 4.0 / (WIN as f32 * WIN as f32 * 0.5) * 0.25;
        self.telemetry_from(&power, P_HOP, norm);
        // Per-bin gains from the bands (interpolated on log frequency), applied to both channels.
        let half = WIN / 2;
        let nv = self.bank.valid as f32;
        let gain_at = |red: &[f32; NB], pos: f32| -> f32 {
            if pos < 0.0 || pos >= nv - 1.0 {
                return 1.0;
            }
            let i = pos.floor() as usize;
            let fr = pos - i as f32;
            db_to_gain(-(red[i] * (1.0 - fr) + red[i + 1] * fr))
        };
        for k in 0..=half {
            let pos = map.bin_band[k];
            let (ga, gb) = (gain_at(&self.red[0], pos), gain_at(&self.red[1], pos));
            let x = self.frame[k];
            let y = self.frame[(WIN - k) % WIN].conj();
            let a = (x + y) * 0.5;
            let b = (x - y) * Complex32::new(0.0, -0.5);
            let out = a * ga + b * Complex32::new(0.0, 1.0) * gb;
            self.frame[k] = out;
            if k != 0 && k != half {
                // The mirror bin keeps the spectra real.
                self.frame[WIN - k] = (a.conj() * ga) + (b.conj() * Complex32::new(0.0, 1.0) * gb);
            }
        }
        self.maps[0] = map;
        let ifft = self.p_ifft.clone();
        ifft.process_with_scratch(&mut self.frame[..WIN], &mut self.scratch);
        // √Hann² = Hann; with hop N/4 the overlapping Hanns sum to 2.
        let scale = 1.0 / (WIN as f32 * 2.0);
        for i in 0..WIN {
            let k = (self.p_pos + i) % WIN;
            let w = self.p_win[i] * scale;
            self.p_out[0][k] += self.frame[i].re * w;
            self.p_out[1][k] += self.frame[i].im * w;
        }
    }
}

impl Effect for Ghost {
    fn set_param(&mut self, id: &str, v: f32) {
        let node = |s: &mut Self, k: usize, part: u8, v: f32| {
            match part {
                0 => s.nodes[k].1 = v,
                1 => s.nodes[k].2 = v,
                _ => s.nodes[k].3 = v,
            }
            s.shape_dirty = true;
        };
        match id {
            "depth" => self.depth = (v / 100.0).clamp(0.0, 1.0).powi(2),
            "sharpness" => self.sharpness = v.clamp(0.0, 10.0),
            "selectivity" => self.selectivity = v.clamp(0.0, 10.0),
            "attack" => self.attack = v,
            "release" => self.release = v,
            "mix" => self.mix = (v / 100.0).clamp(0.0, 1.0),
            "trim" => self.trim = db_to_gain(v),
            "engine" => {
                let e = (v.round().max(0.0) as usize).min(2);
                if e != self.engine {
                    self.engine = e;
                    self.reset();
                }
            }
            "character" => self.hard = v >= 0.5,
            "resolution" => self.resolution = (v.round().max(0.0) as usize).min(2),
            "stereo" => self.ms = v >= 0.5,
            "link" => self.link = (v / 100.0).clamp(0.0, 1.0),
            "ms_balance" => self.ms_balance = (v / 100.0).clamp(-1.0, 1.0),
            "delta" => self.delta = v >= 0.5,
            "bypass" => self.bypass = v >= 0.5,
            "sidechain" => self.use_sc = v >= 0.5,
            "low_cut" => {
                self.low_cut = v;
                self.shape_dirty = true;
            }
            "high_cut" => {
                self.high_cut = v;
                self.shape_dirty = true;
            }
            "solo" => self.solo = (v.round().max(0.0) as usize).min(6),
            "f1" => node(self, 0, 0, v),
            "g1" => node(self, 0, 1, v),
            "f2" => node(self, 1, 0, v),
            "g2" => node(self, 1, 1, v),
            "q2" => node(self, 1, 2, v),
            "f3" => node(self, 2, 0, v),
            "g3" => node(self, 2, 1, v),
            "q3" => node(self, 2, 2, v),
            "f4" => node(self, 3, 0, v),
            "g4" => node(self, 3, 1, v),
            "q4" => node(self, 3, 2, v),
            "f5" => node(self, 4, 0, v),
            "g5" => node(self, 4, 1, v),
            "q5" => node(self, 4, 2, v),
            "f6" => node(self, 5, 0, v),
            "g6" => node(self, 5, 1, v),
            _ => {}
        }
    }

    fn reset(&mut self) {
        for ch in 0..2 {
            self.ring[ch].fill(0.0);
            self.look[ch].fill(0.0);
            self.dry[ch].fill(0.0);
            self.p_in[ch].fill(0.0);
            self.p_out[ch].fill(0.0);
            self.bank.z1[ch] = [0.0; NB];
            self.bank.z2[ch] = [0.0; NB];
            self.solo_f[ch].reset();
        }
        self.red = [[0.0; NB]; 2];
        self.c_from = [[0.0; NB]; 2];
        self.c_to = [[0.0; NB]; 2];
        self.active = [[false; NB]; 2];
        self.act_n = [0; 2];
        self.pos = 0;
        self.since = 0;
        self.hop_k = 0;
        self.look_pos = 0;
        self.dry_pos = 0;
        self.p_pos = 0;
        self.p_since = 0;
        self.tele[3] = self.latency_now() as f32;
    }

    fn latency(&self) -> usize {
        self.latency_now()
    }

    fn gain_reduction(&self) -> f32 {
        self.gr
    }

    fn telemetry(&self) -> &[f32] {
        &self.tele
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], sidechain: Option<(&[f32], &[f32])>) {
        if self.shape_dirty {
            self.reshape();
        }
        // Silence: once two windows of it have gone through, every buffer inside is empty and the output
        // is silence too – skip the work until the signal is back (an idle track costs nothing).
        let quiet = l.iter().chain(r.iter()).all(|x| x.abs() < 1e-9) && sidechain.is_none_or(|(a, b)| !self.use_sc || a.iter().chain(b.iter()).all(|x| x.abs() < 1e-9));
        if quiet && self.silent >= 2 * WIN && self.byp == if self.bypass { 1.0 } else { 0.0 } {
            if self.act_n != [0; 2] || self.gr != 0.0 {
                self.red = [[0.0; NB]; 2];
                self.c_from = [[0.0; NB]; 2];
                self.c_to = [[0.0; NB]; 2];
                self.active = [[false; NB]; 2];
                self.act_n = [0; 2];
                self.gr = 0.0;
                self.tele[2] = 0.0;
                self.tele[4 + NB..].iter_mut().for_each(|x| *x = 0.0);
            }
            l.fill(0.0);
            r.fill(0.0);
            self.tele[0] = -120.0;
            self.tele[1] = -120.0;
            return;
        }
        self.silent = if quiet { self.silent + l.len() } else { 0 };
        // Band solo: a band-pass (shelves: low / high pass) on the output at that focus node.
        if self.solo != self.solo_cur {
            self.solo_cur = self.solo;
            if self.solo > 0 {
                let (kind, f, _, q) = self.nodes[self.solo - 1];
                let (k, q) = match kind {
                    0 => (BiquadKind::LowPass, 0.707),
                    2 => (BiquadKind::HighPass, 0.707),
                    _ => (BiquadKind::BandPass, q.max(0.3)),
                };
                for s in &mut self.solo_f {
                    s.set(k, f.min(self.rate * 0.45), q, 0.0, self.rate);
                }
            }
        }
        let lat = self.latency_now();
        let byp_step = 1.0 / (0.01 * self.rate);
        let (mut pin, mut pout) = (0.0f32, 0.0f32);
        let sc = if self.use_sc { sidechain } else { None };
        for i in 0..l.len() {
            let (xl, xr) = (l[i], r[i]);
            pin = pin.max(xl.abs()).max(xr.abs());
            // Into the processing domain (L/R or M/S).
            let (a, b) = if self.ms { ((xl + xr) * 0.5, (xl - xr) * 0.5) } else { (xl, xr) };
            // Detector input: the key (sidechain) or the signal itself.
            let (da, db) = match sc {
                Some((sl, sr)) if i < sl.len() => {
                    let (kl, kr) = (sl[i], sr[i]);
                    if self.ms {
                        ((kl + kr) * 0.5, (kl - kr) * 0.5)
                    } else {
                        (kl, kr)
                    }
                }
                _ => (a, b),
            };
            // The dry signal, aligned to the latency.
            let (dl, dr) = if lat > 0 {
                if self.dry_pos >= lat {
                    self.dry_pos = 0;
                }
                let k = self.dry_pos;
                let o = (self.dry[0][k], self.dry[1][k]);
                self.dry[0][k] = xl;
                self.dry[1][k] = xr;
                self.dry_pos += 1;
                o
            } else {
                (xl, xr)
            };
            let (wa, wb, ra, rb);
            if self.engine == 2 {
                // Precise: the STFT (detector frames from the key when it's on; the audio frames
                // carry the processing – the key only steers when they are the same signal).
                self.p_in[0][self.p_pos] = a;
                self.p_in[1][self.p_pos] = b;
                let oa = self.p_out[0][self.p_pos];
                let ob = self.p_out[1][self.p_pos];
                self.p_out[0][self.p_pos] = 0.0;
                self.p_out[1][self.p_pos] = 0.0;
                self.p_pos = (self.p_pos + 1) % WIN;
                self.p_since += 1;
                if self.p_since >= P_HOP {
                    self.p_since = 0;
                    self.precise_hop();
                }
                let (dla, dlb) = if self.ms { ((dl + dr) * 0.5, (dl - dr) * 0.5) } else { (dl, dr) };
                wa = oa;
                wb = ob;
                ra = dla - oa;
                rb = dlb - ob;
            } else {
                // Zero / Low: analysis ring (undelayed) …
                self.ring[0][self.pos] = da;
                self.ring[1][self.pos] = db;
                self.pos = (self.pos + 1) % WIN;
                self.since += 1;
                if self.since >= HOP {
                    self.since = 0;
                    self.analyse();
                }
                // … audio through the lookahead (Low) …
                let (pa, pb) = if self.engine == 1 {
                    let k = self.look_pos;
                    let o = (self.look[0][k], self.look[1][k]);
                    self.look[0][k] = a;
                    self.look[1][k] = b;
                    self.look_pos = (self.look_pos + 1) % LOOKAHEAD;
                    o
                } else {
                    (a, b)
                };
                // … and the bank: y = x + Σ c·BP(x) over the bands that cut.
                let t = (self.hop_k as f32 + 0.5) / HOP as f32;
                self.hop_k = (self.hop_k + 1).min(HOP);
                let mut rem = [0.0f32; 2];
                for (ch, x) in [pa, pb].into_iter().enumerate() {
                    let (from, to) = (&self.c_from[ch], &self.c_to[ch]);
                    let (z1, z2) = (&mut self.bank.z1[ch], &mut self.bank.z2[ch]);
                    let (b0, a1, a2) = (&self.bank.b0, &self.bank.a1, &self.bank.a2);
                    let acc = if self.act_n[ch] > 12 {
                        // Many bands cut: all of them, branch-free in lanes of 4 (vectorised); the idle
                        // ones have c = 0.
                        let mut lanes = [0.0f32; 4];
                        for c in 0..NB / 4 {
                            for l in 0..4 {
                                let j = c * 4 + l;
                                let y = b0[j] * x + z1[j];
                                z1[j] = -a1[j] * y + z2[j];
                                z2[j] = -b0[j] * x - a2[j] * y;
                                lanes[l] += (from[j] + (to[j] - from[j]) * t) * y;
                            }
                        }
                        lanes[0] + lanes[1] + lanes[2] + lanes[3]
                    } else {
                        let mut acc = 0.0f32;
                        for &j in &self.act_idx[ch][..self.act_n[ch]] {
                            let j = j as usize;
                            let y = b0[j] * x + z1[j];
                            z1[j] = -a1[j] * y + z2[j];
                            z2[j] = -b0[j] * x - a2[j] * y;
                            acc += (from[j] + (to[j] - from[j]) * t) * y;
                        }
                        acc
                    };
                    rem[ch] = -acc;
                }
                wa = pa - rem[0];
                wb = pb - rem[1];
                ra = rem[0];
                rb = rem[1];
            }
            // Delta (what's removed) or the processed signal, back to L/R.
            let (oa, ob) = if self.delta { (ra, rb) } else { (wa, wb) };
            let (mut ol, mut or) = if self.ms { (oa + ob, oa - ob) } else { (oa, ob) };
            if !self.delta && self.mix < 1.0 {
                ol = self.mix * ol + (1.0 - self.mix) * dl;
                or = self.mix * or + (1.0 - self.mix) * dr;
            }
            ol *= self.trim;
            or *= self.trim;
            // Bypass: a 10 ms crossfade to the aligned dry signal.
            let want = if self.bypass { 1.0 } else { 0.0 };
            if self.byp != want {
                self.byp = if self.byp < want { (self.byp + byp_step).min(want) } else { (self.byp - byp_step).max(want) };
            }
            if self.byp > 0.0 {
                ol = ol * (1.0 - self.byp) + dl * self.byp;
                or = or * (1.0 - self.byp) + dr * self.byp;
            }
            if self.solo > 0 {
                ol = self.solo_f[0].process(ol);
                or = self.solo_f[1].process(or);
            }
            l[i] = ol;
            r[i] = or;
            pout = pout.max(ol.abs()).max(or.abs());
        }
        self.tele[0] = 20.0 * pin.max(1e-6).log10();
        self.tele[1] = 20.0 * pout.max(1e-6).log10();
        self.tele[3] = lat as f32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(n: usize, seed: u32) -> Vec<f32> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                (s as f32 / u32::MAX as f32) * 2.0 - 1.0
            })
            .collect()
    }

    /// Pink-ish noise (a −3 dB/oct slope from white noise): nothing sticks out.
    fn pink(n: usize) -> Vec<f32> {
        let w = noise(n, 7);
        let (mut b0, mut b1, mut b2) = (0.0f32, 0.0f32, 0.0f32);
        w.iter()
            .map(|x| {
                b0 = 0.99765 * b0 + x * 0.0990460;
                b1 = 0.96300 * b1 + x * 0.2965164;
                b2 = 0.57000 * b2 + x * 1.0526913;
                (b0 + b1 + b2 + x * 0.1848) * 0.08
            })
            .collect()
    }

    fn run(g: &mut Ghost, l: &mut [f32], r: &mut [f32]) {
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            g.process(a, b, None);
        }
    }

    fn set(g: &mut Ghost, kv: &[(&str, f32)]) {
        for (k, v) in kv {
            g.set_param(k, *v);
        }
    }

    /// The overlap correction: the bank's scalars reach the wanted cut at a band's centre.
    #[test]
    fn bank_reaches_the_wanted_cut() {
        let rate = 48_000.0;
        let bank = Bank::new(rate);
        let inv = inverse_interaction(&bank, rate);
        let n = bank.valid;
        // −6 dB on band 30 only.
        let mut want = vec![0.0f32; n];
        want[30] = db_to_gain(-6.0) - 1.0;
        let c: Vec<f32> = (0..n).map(|i| inv[i * n..(i + 1) * n].iter().zip(&want).map(|(a, b)| a * b).sum()).collect();
        let h = |hz: f32| {
            let mut s = Complex32::new(1.0, 0.0);
            for j in 0..n {
                s += bank.response(j, hz, rate) * c[j];
            }
            20.0 * s.norm().log10()
        };
        let at = h(band_hz(30));
        assert!((at + 6.0).abs() < 1.0, "−6 dB at the centre: {at}");
        assert!(h(band_hz(45)).abs() < 0.3 && h(band_hz(15)).abs() < 0.3, "untouched far away");
    }

    /// Pink noise has no resonances: (almost) no reduction.
    #[test]
    fn pink_noise_is_left_alone() {
        let mut g = Ghost::new(48_000.0);
        set(&mut g, &[("depth", 100.0)]);
        let mut l = pink(48_000 * 2);
        let mut r = l.clone();
        run(&mut g, &mut l, &mut r);
        assert!(g.gr < 1.5, "GR on pink noise: {}", g.gr);
    }

    /// A ringing resonance in noise (a strong 3 kHz tone) is found and pulled down; the noise around it
    /// stays.
    #[test]
    fn a_resonance_is_pulled_down() {
        let rate = 48_000.0;
        for engine in [0.0, 1.0, 2.0] {
            let mut g = Ghost::new(rate);
            set(&mut g, &[("depth", 100.0), ("engine", engine), ("sharpness", 7.0)]);
            let n = 48_000 * 2;
            let bg = pink(n);
            let tone: Vec<f32> = (0..n).map(|i| (std::f32::consts::TAU * 3000.0 * i as f32 / rate).sin() * 0.3).collect();
            let mut l: Vec<f32> = bg.iter().zip(&tone).map(|(a, b)| a + b).collect();
            let mut r = l.clone();
            run(&mut g, &mut l, &mut r);
            let b = (6.0 * (3000.0f32 / 20.0).log2()).round() as usize;
            let red = g.red[0][b].max(g.red[0][b + 1]).max(g.red[0][b.saturating_sub(1)]);
            assert!(red > 6.0, "engine {engine}: the 3 kHz resonance is cut by {red:.1} dB");
            let far = g.red[0][b - 18];
            assert!(far < 1.0, "engine {engine}: 3 octaves lower stays ({far:.1} dB)");
        }
    }

    /// With nothing to cut, every engine passes the signal exactly, delayed by its reported latency.
    #[test]
    fn engines_pass_untouched_signal_at_their_latency() {
        for engine in [0.0, 1.0, 2.0] {
            let mut g = Ghost::new(48_000.0);
            set(&mut g, &[("depth", 0.0), ("engine", engine)]);
            let lat = g.latency();
            let n = 8192;
            let src = noise(n, 3);
            let (mut l, mut r) = (src.clone(), src.clone());
            run(&mut g, &mut l, &mut r);
            let err = (lat + WIN..n).map(|i| (l[i] - src[i - lat]).abs()).fold(0.0f32, f32::max);
            assert!(err < 2e-3, "engine {engine}: out = in delayed by {lat}: max error {err}");
        }
    }

    /// Silence after signal: the effect stops working once its buffers are empty, and picks the signal up
    /// again exactly when it returns.
    #[test]
    fn silence_is_skipped_and_signal_resumes() {
        let mut g = Ghost::new(48_000.0);
        set(&mut g, &[("depth", 0.0), ("engine", 1.0)]);
        let lat = g.latency();
        let n = 48_000;
        let mut src = noise(n, 9);
        src[8_000..30_000].iter_mut().for_each(|x| *x = 0.0);
        let (mut l, mut r) = (src.clone(), src.clone());
        run(&mut g, &mut l, &mut r);
        let err = (lat..n).map(|i| (l[i] - src[i - lat]).abs()).fold(0.0f32, f32::max);
        assert!(err < 2e-3, "same output through the silence: {err}");
    }

    /// Delta plays what's removed: wet + delta = dry (Zero engine, exactly).
    #[test]
    fn delta_is_the_removed_part() {
        let rate = 48_000.0;
        let n = 48_000;
        let src: Vec<f32> = pink(n).iter().enumerate().map(|(i, x)| x + (std::f32::consts::TAU * 2500.0 * i as f32 / rate).sin() * 0.3).collect();
        let mut wet = Ghost::new(rate);
        let mut del = Ghost::new(rate);
        set(&mut wet, &[("depth", 100.0), ("engine", 0.0)]);
        set(&mut del, &[("depth", 100.0), ("engine", 0.0), ("delta", 1.0)]);
        let (mut a, mut b) = (src.clone(), src.clone());
        let (mut c, mut d) = (src.clone(), src.clone());
        run(&mut wet, &mut a, &mut b);
        run(&mut del, &mut c, &mut d);
        let err = (0..n).map(|i| (a[i] + c[i] - src[i]).abs()).fold(0.0f32, f32::max);
        assert!(err < 1e-4, "wet + delta = dry: {err}");
        assert!(c[n / 2..].iter().any(|x| x.abs() > 0.01), "something was removed");
    }
}

#[cfg(test)]
mod bench {
    use super::*;

    /// CPU per instance (`cargo test --release -p dragon-engine ghost_cpu -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn ghost_cpu() {
        let rate = 48_000.0;
        let n = 48_000 * 10;
        let mut s = 1u32;
        let src: Vec<f32> = (0..n)
            .map(|i| {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                ((s as f32 / u32::MAX as f32) * 2.0 - 1.0) * 0.2 + (std::f32::consts::TAU * 3000.0 * i as f32 / rate).sin() * 0.3
            })
            .collect();
        for (engine, depth) in [(0.0, 50.0), (1.0, 50.0), (1.0, 100.0), (2.0, 50.0), (1.0, 0.0)] {
            let mut g = Ghost::new(rate);
            g.set_param("engine", engine);
            g.set_param("depth", depth);
            let (mut l, mut r) = (src.clone(), src.clone());
            let t0 = std::time::Instant::now();
            for (a, b) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
                g.process(a, b, None);
            }
            let secs = t0.elapsed().as_secs_f64();
            println!("engine {engine} depth {depth}: {:.2} % of one core (real time)", secs / 10.0 * 100.0);
        }
    }
}
