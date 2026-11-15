//! 24-band parametric EQ (7 on by default) (bell, shelves, cuts, notch, band pass per band) with output gain, after
//! FabFilter Pro-Q: per-band stereo placement (stereo, mid, side, left, right), per-band dynamic gain
//! (range and threshold), band solo, auto gain and a live analyzer.

use super::{Effect, EffectDef, ParamInfo};
use crate::dsp::spectrum::{Analyzer, BANDS};
use crate::dsp::{db_to_gain, Biquad, BiquadKind};
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::sync::Arc;

/// Linear-phase engine: overlap-add FFT convolution with a symmetric FIR built from the curve.
const LP_BLOCK: usize = 2048;
const LP_FFT: usize = LP_BLOCK * 2;
const LP_KERNEL: usize = 2048;

struct LinearPhase {
    fwd: Arc<dyn Fft<f32>>,
    inv: Arc<dyn Fft<f32>>,
    scratch: Vec<Complex<f32>>,
    kernel: Vec<Complex<f32>>,
    buf: Vec<Complex<f32>>,
    tmp: Vec<f32>,
    input: [Vec<f32>; 2],
    output: [Vec<f32>; 2],
    overlap: [Vec<f32>; 2],
    pos: usize,
}

impl LinearPhase {
    fn new() -> Self {
        let mut pl = FftPlanner::new();
        let fwd = pl.plan_fft_forward(LP_FFT);
        let inv = pl.plan_fft_inverse(LP_FFT);
        let n = fwd.get_inplace_scratch_len().max(inv.get_inplace_scratch_len());
        let z = || vec![0.0f32; LP_BLOCK];
        LinearPhase {
            fwd,
            inv,
            scratch: vec![Complex::default(); n],
            kernel: vec![Complex::default(); LP_FFT],
            buf: vec![Complex::default(); LP_FFT],
            tmp: vec![0.0; LP_FFT],
            input: [z(), z()],
            output: [z(), z()],
            overlap: [z(), z()],
            pos: 0,
        }
    }

    /// Builds the kernel from a magnitude response in dB (`db(freq)`).
    fn design(&mut self, rate: f32, db: impl Fn(f32) -> f32) {
        for k in 0..LP_FFT {
            let bin = if k <= LP_FFT / 2 { k } else { LP_FFT - k };
            let f = bin as f32 * rate / LP_FFT as f32;
            self.buf[k] = Complex::new(db_to_gain(db(f.max(1.0))), 0.0);
        }
        self.inv.process_with_scratch(&mut self.buf, &mut self.scratch);
        // Zero-phase impulse → centre it at LP_KERNEL / 2, window it, zero the rest.
        let half = LP_KERNEL / 2;
        let h = &mut self.tmp;
        h.fill(0.0);
        for (i, v) in h.iter_mut().enumerate().take(LP_KERNEL) {
            let src = (i + LP_FFT - half) % LP_FFT;
            let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / LP_KERNEL as f32).cos();
            *v = self.buf[src].re / LP_FFT as f32 * w;
        }
        for (k, c) in self.kernel.iter_mut().enumerate() {
            *c = Complex::new(h[k], 0.0);
        }
        self.fwd.process_with_scratch(&mut self.kernel, &mut self.scratch);
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        for i in 0..l.len() {
            let (a, b) = (l[i], r[i]);
            l[i] = self.output[0][self.pos];
            r[i] = self.output[1][self.pos];
            self.input[0][self.pos] = a;
            self.input[1][self.pos] = b;
            self.pos += 1;
            if self.pos == LP_BLOCK {
                self.pos = 0;
                for ch in 0..2 {
                    for k in 0..LP_FFT {
                        self.buf[k] = Complex::new(if k < LP_BLOCK { self.input[ch][k] } else { 0.0 }, 0.0);
                    }
                    self.fwd.process_with_scratch(&mut self.buf, &mut self.scratch);
                    for (x, h) in self.buf.iter_mut().zip(&self.kernel) {
                        *x *= *h;
                    }
                    self.inv.process_with_scratch(&mut self.buf, &mut self.scratch);
                    let norm = 1.0 / LP_FFT as f32;
                    for k in 0..LP_BLOCK {
                        self.output[ch][k] = self.buf[k].re * norm + self.overlap[ch][k];
                        self.overlap[ch][k] = self.buf[k + LP_BLOCK].re * norm;
                    }
                }
            }
        }
    }

    fn reset(&mut self) {
        for v in self.input.iter_mut().chain(self.output.iter_mut()).chain(self.overlap.iter_mut()) {
            v.fill(0.0);
        }
        self.pos = 0;
    }
}

pub const EQ_BANDS: usize = 24;
const TYPES: &[&str] = &["Bell", "Low shelf", "High shelf", "Low pass", "High pass", "Band pass", "Notch"];
/// Where a band works (Pro-Q's stereo placement).
pub const PLACEMENTS: &[&str] = &["Stereo", "Mid", "Side", "Left", "Right"];
/// Pro-Q's processing modes: zero latency (minimum phase) or linear phase (no phase shift, ~64 ms
/// latency; bands are stereo and static there).
/// Natural phase runs the filters at twice the rate (analog-like curves up to the top, small latency).
pub const PHASE_MODES: &[&str] = &["Zero latency", "Linear phase", "Natural phase"];
/// Parameters per band.
pub const PER_BAND: usize = 8;

macro_rules! band {
    ($n:literal, $on:expr, $ty:expr, $f:expr) => {
        [
            ParamInfo::toggle(concat!("b", $n, "_on"), concat!("Band ", $n, " on"), $on),
            ParamInfo::choice(concat!("b", $n, "_type"), concat!("Band ", $n, " type"), TYPES, $ty),
            ParamInfo::log(concat!("b", $n, "_freq"), concat!("Band ", $n, " freq"), 20.0, 20000.0, $f, "Hz"),
            ParamInfo::lin(concat!("b", $n, "_gain"), concat!("Band ", $n, " gain"), -18.0, 18.0, 0.0, "dB"),
            ParamInfo::log(concat!("b", $n, "_q"), concat!("Band ", $n, " Q"), 0.1, 18.0, 0.9, ""),
            ParamInfo::choice(concat!("b", $n, "_stereo"), concat!("Band ", $n, " stereo"), PLACEMENTS, 0.0),
            ParamInfo::lin(concat!("b", $n, "_dyn"), concat!("Band ", $n, " dynamic range"), -18.0, 18.0, 0.0, "dB"),
            ParamInfo::lin(concat!("b", $n, "_thr"), concat!("Band ", $n, " threshold"), -60.0, 0.0, -24.0, "dB"),
        ]
    };
}

const EXTRA: usize = 4;
const SOLO_NAMES: [&str; EQ_BANDS + 1] = ["Off", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16", "17", "18", "19", "20", "21", "22", "23", "24"];

const fn flatten() -> [ParamInfo; EQ_BANDS * PER_BAND + EXTRA] {
    let bands: [[ParamInfo; PER_BAND]; EQ_BANDS] = [
        band!("1", false, 4.0, 30.0),
        band!("2", true, 1.0, 100.0),
        band!("3", true, 0.0, 300.0),
        band!("4", true, 0.0, 1000.0),
        band!("5", true, 0.0, 3000.0),
        band!("6", true, 2.0, 8000.0),
        band!("7", false, 3.0, 18000.0),
        band!("8", false, 0.0, 40.0),
        band!("9", false, 0.0, 58.0),
        band!("10", false, 0.0, 85.0),
        band!("11", false, 0.0, 123.0),
        band!("12", false, 0.0, 179.0),
        band!("13", false, 0.0, 260.0),
        band!("14", false, 0.0, 378.0),
        band!("15", false, 0.0, 550.0),
        band!("16", false, 0.0, 800.0),
        band!("17", false, 0.0, 1163.0),
        band!("18", false, 0.0, 1692.0),
        band!("19", false, 0.0, 2460.0),
        band!("20", false, 0.0, 3578.0),
        band!("21", false, 0.0, 5203.0),
        band!("22", false, 0.0, 7566.0),
        band!("23", false, 0.0, 11002.0),
        band!("24", false, 0.0, 16000.0),
    ];
    let mut out = [ParamInfo::lin("output", "Output", -18.0, 18.0, 0.0, "dB"); EQ_BANDS * PER_BAND + EXTRA];
    out[EQ_BANDS * PER_BAND + 1] = ParamInfo::toggle("auto_gain", "Auto gain", false);
    out[EQ_BANDS * PER_BAND + 2] = ParamInfo::choice("solo", "Solo band (listen)", &SOLO_NAMES, 0.0);
    out[EQ_BANDS * PER_BAND + 3] = ParamInfo::choice("phase", "Processing", PHASE_MODES, 0.0);
    let mut i = 0;
    while i < EQ_BANDS {
        let mut j = 0;
        while j < PER_BAND {
            out[i * PER_BAND + j] = bands[i][j];
            j += 1;
        }
        i += 1;
    }
    out
}

static PARAMS: [ParamInfo; EQ_BANDS * PER_BAND + EXTRA] = flatten();

/// Gain that keeps the loudness about the same: minus the curve's average boost over 50 Hz–12 kHz,
/// weighted towards the midrange (FabFilter-style auto gain).
fn auto_gain(curve: impl Fn(f32) -> f32) -> f32 {
    let (mut sum, mut wsum) = (0.0, 0.0);
    for k in 0..40 {
        let f = 50.0 * 240f32.powf(k as f32 / 39.0);
        let w = 1.0 / (1.0 + ((f / 1500.0).log2() / 2.5).powi(2));
        sum += curve(f) * w;
        wsum += w;
    }
    -(sum / wsum).clamp(-18.0, 18.0)
}

/// Pro-Q style EQ match: from the average spectra (dB per analyzer band, see `dsp::spectrum`) of this
/// track and a reference, a set of up to `max` bell bands (freq, gain dB, Q) whose sum follows the smoothed
/// difference (reference − own), limited to ±`limit` dB. `amount` 0 … 1 scales the result.
pub fn match_bands(own: &[f32], reference: &[f32], max: usize, limit: f32, amount: f32, rate: f32) -> Vec<(f32, f32, f32)> {
    use crate::dsp::spectrum::{band_freq, BANDS};
    if own.len() < BANDS || reference.len() < BANDS {
        return Vec::new();
    }
    // Level-match first (only the shape is matched), then smooth over ±3 bands (≈ ⅓ octave).
    let mean = |v: &[f32]| v[8..BANDS - 8].iter().sum::<f32>() / (BANDS - 16) as f32;
    let off = mean(reference) - mean(own);
    let raw: Vec<f32> = (0..BANDS).map(|b| (reference[b] - own[b] - off).clamp(-limit, limit)).collect();
    let mut target: Vec<f32> = (0..BANDS)
        .map(|b| {
            let (lo, hi) = (b.saturating_sub(3), (b + 3).min(BANDS - 1));
            raw[lo..=hi].iter().sum::<f32>() / (hi - lo + 1) as f32 * amount
        })
        .collect();
    let mut out = Vec::new();
    for _ in 0..max {
        // Largest remaining deviation inside 30 Hz … 16 kHz.
        let Some((b, &dev)) = target.iter().enumerate().filter(|(b, _)| (30.0..16000.0).contains(&band_freq(*b))).max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())) else { break };
        if dev.abs() < 0.5 {
            break;
        }
        let f = band_freq(b);
        // Width: how far the deviation keeps its sign and at least half its size.
        let mut w = 1;
        while b >= w && b + w < BANDS && target[b - w].signum() == dev.signum() && target[b + w].signum() == dev.signum() && target[b - w].abs() > dev.abs() * 0.5 && target[b + w].abs() > dev.abs() * 0.5 {
            w += 1;
        }
        let octaves = (w as f32 * 2.0) * 10.0 / BANDS as f32; // the analyzer spans ~10 octaves
        let q = (1.0 / octaves.max(0.15)).clamp(0.3, 6.0);
        let bq = Biquad::new(BiquadKind::Peak, f, q, dev, rate);
        for (k, t) in target.iter_mut().enumerate() {
            *t -= bq.response_db(band_freq(k), rate);
        }
        out.push((f, dev, q));
    }
    out
}

#[derive(Debug, Clone, Copy)]
struct Band {
    on: bool,
    kind: BiquadKind,
    freq: f32,
    gain: f32,
    q: f32,
    place: usize,
    dyn_range: f32,
    thr: f32,
}

/// The dynamic part of a band: a band-pass detector and an envelope in dB.
#[derive(Debug, Clone, Copy, Default)]
struct Dyn {
    det: Biquad,
    env: f32,
    /// The gain change currently applied (dB).
    applied: f32,
}

pub struct ParametricEq {
    rate: f32,
    bands: [Band; EQ_BANDS],
    filters: [[Biquad; 2]; EQ_BANDS],
    output: f32,
    dirty: bool,
    auto_gain: bool,
    dyns: [Dyn; EQ_BANDS],
    linear: bool,
    natural: bool,
    /// The rate the band filters run at (twice the rate in natural phase).
    frate: f32,
    ovs: [crate::dsp::oversample::Oversampler; 2],
    up: [Vec<f32>; 2],
    lp: Box<LinearPhase>,
    solo: usize,
    solo_f: [Biquad; 2],
    pre: Analyzer,
    post: Analyzer,
    sc: Analyzer,
    /// Telemetry: the input spectrum, then the output spectrum ([`BANDS`] dB values each), then the
    /// dynamic gain change of each band (dB), then the sidechain input's spectrum.
    tele: Vec<f32>,
}

impl ParametricEq {
    pub fn new(rate: f32) -> Self {
        let b = Band { on: false, kind: BiquadKind::Peak, freq: 1000.0, gain: 0.0, q: 0.9, place: 0, dyn_range: 0.0, thr: -24.0 };
        ParametricEq {
            rate,
            bands: [b; EQ_BANDS],
            filters: [[Biquad::default(); 2]; EQ_BANDS],
            output: 1.0,
            dirty: true,
            auto_gain: false,
            dyns: [Dyn::default(); EQ_BANDS],
            linear: false,
            natural: false,
            frate: rate,
            ovs: [crate::dsp::oversample::Oversampler::new(2), crate::dsp::oversample::Oversampler::new(2)],
            up: [Vec::new(), Vec::new()],
            lp: Box::new(LinearPhase::new()),
            solo: 0,
            solo_f: [Biquad::default(); 2],
            pre: Analyzer::new(rate),
            post: Analyzer::new(rate),
            sc: Analyzer::new(rate),
            tele: vec![crate::dsp::spectrum::FLOOR; BANDS * 3 + EQ_BANDS],
        }
    }

    fn update(&mut self) {
        for (i, b) in self.bands.iter().enumerate() {
            let g = b.gain + self.dyns[i].applied;
            for f in &mut self.filters[i] {
                f.set(b.kind, b.freq, b.q, g, self.frate);
            }
            self.dyns[i].det.set(BiquadKind::BandPass, b.freq, b.q.max(0.5), 0.0, self.frate);
        }
        if self.linear {
            let rate = self.rate;
            let fl: Vec<Biquad> = self.bands.iter().filter(|b| b.on).map(|b| Biquad::new(b.kind, b.freq, b.q, b.gain, rate)).collect();
            self.lp.design(rate, |f| fl.iter().map(|b| b.response_db(f, rate)).sum());
        }
        if self.solo > 0 {
            let b = self.bands[self.solo - 1];
            for f in &mut self.solo_f {
                f.set(BiquadKind::BandPass, b.freq, b.q.max(0.3), 0.0, self.rate);
            }
        }
        self.dirty = false;
    }

    fn auto_gain_db(&self) -> f32 {
        auto_gain(|f| self.bands.iter().zip(&self.filters).filter(|(b, _)| b.on).map(|(_, fl)| fl[0].response_db(f, self.rate)).sum())
    }

    /// Combined response in dB at a frequency, computed from a parameter map (for the UI curve).
    pub fn curve(params: &std::collections::BTreeMap<String, f32>, freq: f32, rate: f32) -> f32 {
        let d = def();
        let mut sum = 0.0;
        for n in 1..=EQ_BANDS {
            let get = |k: &str| super::value(&d, params, &format!("b{n}_{k}"));
            if get("on") < 0.5 {
                continue;
            }
            let b = Biquad::new(BiquadKind::from_index(get("type")), get("freq"), get("q"), get("gain"), rate);
            sum += b.response_db(freq, rate);
        }
        sum + super::value(&d, params, "output")
    }

    /// The minimum-phase band filters (dynamic bands and stereo placement included) at `self.frate`.
    fn run_bands(&mut self, l: &mut [f32], r: &mut [f32]) {
        const SUB: usize = 32;
        let att = (-1.0 / (0.005 * self.frate)).exp();
        let rel = (-1.0 / (0.080 * self.frate)).exp();
        for i in 0..EQ_BANDS {
            let b = self.bands[i];
            if !b.on {
                continue;
            }
            let dynamic = b.dyn_range.abs() > 0.05;
            let mut start = 0;
            while start < l.len() {
                let end = (start + SUB).min(l.len());
                if dynamic {
                    // Detector on this band's region of the signal it sees; the gain moves from the static
                    // gain towards gain + range as the level rises over the threshold (6 dB to full range).
                    let d = &mut self.dyns[i];
                    for k in start..end {
                        let x = d.det.process(0.5 * (l[k] + r[k])).abs();
                        let c = if x > d.env { att } else { rel };
                        d.env = x + (d.env - x) * c;
                    }
                    let over = (crate::dsp::gain_to_db(d.env.max(1e-6)) - b.thr) / 6.0;
                    let want = b.dyn_range * over.clamp(0.0, 1.0);
                    if (want - d.applied).abs() > 0.05 {
                        d.applied = want;
                        for f in &mut self.filters[i] {
                            f.set(b.kind, b.freq, b.q, b.gain + want, self.frate);
                        }
                    }
                } else if self.dyns[i].applied != 0.0 {
                    self.dyns[i].applied = 0.0;
                    for f in &mut self.filters[i] {
                        f.set(b.kind, b.freq, b.q, b.gain, self.frate);
                    }
                }
                let [fl, fr] = &mut self.filters[i];
                for k in start..end {
                    let (a, c) = (l[k], r[k]);
                    let (na, nc) = match b.place {
                        1 => {
                            let (m, s) = (0.5 * (a + c), 0.5 * (a - c));
                            let m = fl.process(m);
                            (m + s, m - s)
                        }
                        2 => {
                            let (m, s) = (0.5 * (a + c), 0.5 * (a - c));
                            let s = fl.process(s);
                            (m + s, m - s)
                        }
                        3 => (fl.process(a), c),
                        4 => (a, fr.process(c)),
                        _ => (fl.process(a), fr.process(c)),
                    };
                    l[k] = na;
                    r[k] = nc;
                }
                start = end;
            }
        }
    }

    /// The auto-gain compensation (dB) for a parameter map, for the UI.
    pub fn auto_gain_for(params: &std::collections::BTreeMap<String, f32>, rate: f32) -> f32 {
        let d = def();
        let filters: Vec<Biquad> = (1..=EQ_BANDS)
            .filter(|n| super::value(&d, params, &format!("b{n}_on")) >= 0.5)
            .map(|n| {
                let get = |k: &str| super::value(&d, params, &format!("b{n}_{k}"));
                Biquad::new(BiquadKind::from_index(get("type")), get("freq"), get("q"), get("gain"), rate)
            })
            .collect();
        auto_gain(|f| filters.iter().map(|b| b.response_db(f, rate)).sum())
    }
}

impl Effect for ParametricEq {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "output" => {
                self.output = db_to_gain(v);
                return;
            }
            "auto_gain" => {
                self.auto_gain = v >= 0.5;
                return;
            }
            "phase" => {
                let mode = v.round() as i32;
                let (lin, nat) = (mode == 1, mode == 2);
                if lin != self.linear {
                    self.linear = lin;
                    self.lp.reset();
                }
                if nat != self.natural {
                    self.natural = nat;
                    self.frate = if nat { self.rate * 2.0 } else { self.rate };
                    self.ovs.iter_mut().for_each(|o| o.reset());
                    for f in self.filters.iter_mut().flatten() {
                        f.reset();
                    }
                }
                self.dirty = true;
                return;
            }
            "solo" => {
                self.solo = (v.round().max(0.0) as usize).min(EQ_BANDS);
                self.dirty = true;
                return;
            }
            _ => {}
        }
        let Some(rest) = id.strip_prefix('b') else { return };
        let Some((n, key)) = rest.split_once('_') else { return };
        let Ok(n) = n.parse::<usize>() else { return };
        if n == 0 || n > EQ_BANDS {
            return;
        }
        let b = &mut self.bands[n - 1];
        match key {
            "on" => b.on = v >= 0.5,
            "type" => b.kind = BiquadKind::from_index(v),
            "freq" => b.freq = v,
            "gain" => b.gain = v,
            "q" => b.q = v,
            "stereo" => b.place = (v.round().max(0.0) as usize).min(PLACEMENTS.len() - 1),
            "dyn" => b.dyn_range = v,
            "thr" => b.thr = v,
            _ => return,
        }
        self.dirty = true;
    }

    fn reset(&mut self) {
        for f in self.filters.iter_mut().flatten() {
            f.reset();
        }
        self.pre.reset();
        self.post.reset();
        self.lp.reset();
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], sc: Option<(&[f32], &[f32])>) {
        if let Some((a, b)) = sc {
            self.sc.push(a, b);
        }
        if self.dirty {
            self.update();
        }
        self.pre.push(l, r);
        if self.linear {
            self.lp.process(l, r);
        }
        if self.natural {
            let n = l.len();
            self.up[0].resize(n * 2, 0.0);
            self.up[1].resize(n * 2, 0.0);
            for i in 0..n {
                self.ovs[0].up(l[i], &mut self.up[0][i * 2..i * 2 + 2]);
                self.ovs[1].up(r[i], &mut self.up[1][i * 2..i * 2 + 2]);
            }
            let [mut ul, mut ur] = std::mem::take(&mut self.up);
            self.run_bands(&mut ul, &mut ur);
            for i in 0..n {
                l[i] = self.ovs[0].down(&ul[i * 2..i * 2 + 2]);
                r[i] = self.ovs[1].down(&ur[i * 2..i * 2 + 2]);
            }
            self.up = [ul, ur];
        } else if !self.linear {
            self.run_bands(l, r);
        }
        let out = self.output * if self.auto_gain { db_to_gain(self.auto_gain_db()) } else { 1.0 };
        if self.solo > 0 {
            // Listen to the soloed band's region only, like holding a band's headphones in Pro-Q.
            let [fl, fr] = &mut self.solo_f;
            for x in l.iter_mut() {
                *x = fl.process(*x);
            }
            for x in r.iter_mut() {
                *x = fr.process(*x);
            }
        }
        if (out - 1.0).abs() > 1e-6 {
            for x in l.iter_mut().chain(r.iter_mut()) {
                *x *= out;
            }
        }
        self.post.push(l, r);
        self.tele[..BANDS].copy_from_slice(&self.pre.bands);
        self.tele[BANDS..BANDS * 2].copy_from_slice(&self.post.bands);
        for i in 0..EQ_BANDS {
            self.tele[BANDS * 2 + i] = self.dyns[i].applied;
        }
        let o = BANDS * 2 + EQ_BANDS;
        self.tele[o..o + BANDS].copy_from_slice(&self.sc.bands);
    }
    fn telemetry(&self) -> &[f32] {
        &self.tele
    }
    fn latency(&self) -> usize {
        if self.linear {
            LP_BLOCK + LP_KERNEL / 2
        } else if self.natural {
            self.ovs[0].latency()
        } else {
            0
        }
    }
}

pub fn def() -> EffectDef {
    EffectDef {
        id: "eq",
        name: "Parametric EQ",
        category: "EQ",
        description: "7-band parametric EQ with shelves, cuts and notch per band.",
        params: &PARAMS,
        // The sidechain only feeds an analyzer: the reference for EQ match.
        uses_sidechain: true,
        create: |r| Box::new(ParametricEq::new(r)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curve_matches_band_gain() {
        let mut p = def().defaults();
        p.insert("b4_gain".into(), 6.0);
        let at = ParametricEq::curve(&p, 1000.0, 48000.0);
        assert!((at - 6.0).abs() < 0.3, "{at}");
    }

    #[test]
    fn auto_gain_offsets_a_broad_boost_and_solo_isolates_the_band() {
        let mut p = def().defaults();
        p.insert("b4_gain".into(), 6.0);
        p.insert("b4_q".into(), 0.3);
        let g = ParametricEq::auto_gain_for(&p, 48000.0);
        assert!(g < -2.0 && g > -6.5, "{g}");
        // Solo band 4 (1 kHz): a 100 Hz tone nearly vanishes, a 1 kHz tone passes.
        let mut e = ParametricEq::new(48000.0);
        for (k, v) in &def().defaults() {
            e.set_param(k, *v);
        }
        e.set_param("solo", 4.0);
        let level = |e: &mut ParametricEq, f: f32| {
            let mut l: Vec<f32> = (0..48000).map(|i| (std::f32::consts::TAU * f * i as f32 / 48000.0).sin()).collect();
            let mut r = l.clone();
            e.process(&mut l, &mut r, None);
            l[24000..].iter().fold(0.0f32, |m, x| m.max(x.abs()))
        };
        assert!(level(&mut e, 1000.0) > 0.7);
        assert!(level(&mut e, 100.0) < 0.15);
    }

    fn tone(f: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| (std::f32::consts::TAU * f * i as f32 / 48000.0).sin() * amp).collect()
    }

    /// Mid placement leaves the side signal alone; side placement leaves the mid alone.
    #[test]
    fn bands_work_in_mid_or_side() {
        for (place, mid_changes) in [(1.0, true), (2.0, false)] {
            let mut e = ParametricEq::new(48000.0);
            for (k, v) in &def().defaults() {
                e.set_param(k, *v);
            }
            for b in 1..=7 {
                e.set_param(&format!("b{b}_on"), 0.0);
            }
            e.set_param("b4_on", 1.0);
            e.set_param("b4_gain", -18.0);
            e.set_param("b4_stereo", place);
            // A mono 1 kHz tone is pure mid.
            let mut l = tone(1000.0, 0.5, 48000);
            let mut r = l.clone();
            e.process(&mut l, &mut r, None);
            let peak = l[24000..].iter().fold(0.0f32, |m, x| m.max(x.abs()));
            assert_eq!(peak < 0.2, mid_changes, "placement {place}: {peak}");
        }
    }

    /// Linear phase: same magnitude as the curve, the latency it reports, and no phase shift (a click
    /// comes out symmetric).
    #[test]
    fn linear_phase_matches_the_curve() {
        let mut e = ParametricEq::new(48000.0);
        for (k, v) in &def().defaults() {
            e.set_param(k, *v);
        }
        e.set_param("b4_gain", 9.0);
        e.set_param("phase", 1.0);
        let lat = e.latency();
        assert_eq!(lat, LP_BLOCK + LP_KERNEL / 2);
        let mut l = tone(1000.0, 0.1, 48000);
        let mut r = l.clone();
        e.process(&mut l, &mut r, None);
        let peak = l[24000..].iter().fold(0.0f32, |m, x| m.max(x.abs()));
        let want = 0.1
            * db_to_gain(ParametricEq::curve(
                &{
                    let mut p = def().defaults();
                    p.insert("b4_gain".into(), 9.0);
                    p
                },
                1000.0,
                48000.0,
            ));
        assert!((peak / want - 1.0).abs() < 0.08, "{peak} vs {want}");
        // An impulse: symmetric around the latency.
        e.reset();
        let mut l = vec![0.0f32; 8192];
        l[0] = 1.0;
        let mut r = l.clone();
        e.process(&mut l, &mut r, None);
        for k in 1..200 {
            assert!((l[lat + k] - l[lat - k]).abs() < 1e-3, "asymmetric at {k}");
        }
    }

    /// EQ match: a dip in the reference around 1 kHz becomes a cut there.
    #[test]
    fn match_follows_the_reference() {
        use crate::dsp::spectrum::{band_freq, BANDS};
        let own = vec![-30.0f32; BANDS];
        let reference: Vec<f32> = (0..BANDS).map(|b| -30.0 - 8.0 * (-((band_freq(b) / 1000.0).log2() / 0.5).powi(2)).exp()).collect();
        let bands = match_bands(&own, &reference, 6, 12.0, 1.0, 48000.0);
        assert!(!bands.is_empty());
        let (f, g, _) = bands[0];
        assert!((f / 1000.0 - 1.0).abs() < 0.25 && g < -4.0, "{bands:?}");
        let total = |hz: f32| bands.iter().map(|(f, g, q)| Biquad::new(BiquadKind::Peak, *f, *q, *g, 48000.0).response_db(hz, 48000.0)).sum::<f32>();
        assert!(total(1000.0) < -5.0 && total(100.0).abs() < 2.0 && total(10000.0).abs() < 2.0, "{} {} {}", total(1000.0), total(100.0), total(10000.0));
    }

    /// Natural phase: near the top of the spectrum the bell keeps its analog shape (no cramping): at 16 kHz a
    /// 12 kHz bell measures closer to the analog curve than in zero-latency mode.
    #[test]
    fn natural_phase_avoids_cramping() {
        let measure = |phase: f32| {
            let mut e = ParametricEq::new(48000.0);
            for (k, v) in &def().defaults() {
                e.set_param(k, *v);
            }
            for b in 1..=EQ_BANDS {
                e.set_param(&format!("b{b}_on"), 0.0);
            }
            e.set_param("b4_on", 1.0);
            e.set_param("b4_freq", 12000.0);
            e.set_param("b4_gain", 12.0);
            e.set_param("b4_q", 1.0);
            e.set_param("phase", phase);
            let mut l = tone(16000.0, 0.1, 48000);
            let mut r = l.clone();
            e.process(&mut l, &mut r, None);
            crate::dsp::gain_to_db(l[24000..].iter().fold(0.0f32, |m, x| m.max(x.abs())) / 0.1)
        };
        // Analog bell (s-domain) at 16 kHz, centre 12 kHz, Q 1, +12 dB.
        let analog = {
            let (w, a) = (16000.0f32 / 12000.0, 10f32.powf(12.0 / 40.0));
            let num = ((1.0 - w * w).powi(2) + (w * a).powi(2)).sqrt();
            let den = ((1.0 - w * w).powi(2) + (w / a).powi(2)).sqrt();
            20.0 * (num / den).log10()
        };
        let (zero, natural) = (measure(0.0), measure(2.0));
        assert!((natural - analog).abs() < (zero - analog).abs(), "analog {analog}, zero {zero}, natural {natural}");
    }

    /// A dynamic band cuts only while the band is loud.
    #[test]
    fn dynamic_band_follows_the_level() {
        let mut e = ParametricEq::new(48000.0);
        for (k, v) in &def().defaults() {
            e.set_param(k, *v);
        }
        e.set_param("b4_gain", 0.0);
        e.set_param("b4_dyn", -12.0);
        e.set_param("b4_thr", -30.0);
        let run = |e: &mut ParametricEq, amp: f32| {
            let mut l = tone(1000.0, amp, 24000);
            let mut r = l.clone();
            e.process(&mut l, &mut r, None);
            l[12000..].iter().fold(0.0f32, |m, x| m.max(x.abs())) / amp
        };
        let quiet = run(&mut e, 0.005);
        let loud = run(&mut e, 0.7);
        assert!(quiet > 0.9, "quiet passes: {quiet}");
        assert!(loud < 0.4, "loud is cut: {loud}");
        assert!(e.telemetry()[BANDS * 2 + 3] < -6.0);
    }
}
