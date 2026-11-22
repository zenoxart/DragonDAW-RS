//! Dynamics: compressor (with sidechain), 4-band multiband dynamics, gate, true-peak limiter,
//! de-esser and transient shaper.

use super::{Effect, EffectDef, ParamInfo};
use crate::dsp::{db_to_gain, gain_to_db, Biquad, BiquadKind, Follower, PeakHold};

// ------------------------------------------------------------------------------------------------ compressor

/// Feed-forward RMS/peak compressor core (dB domain, soft knee).
#[derive(Debug, Clone)]
struct CompCore {
    rate: f32,
    threshold: f32,
    ratio: f32,
    knee: f32,
    attack: f32,
    release: f32,
    env_db: f32,
    gr: f32,
}

impl CompCore {
    fn new(rate: f32) -> Self {
        CompCore { rate, threshold: -18.0, ratio: 4.0, knee: 6.0, attack: 10.0, release: 120.0, env_db: -120.0, gr: 0.0 }
    }
    #[inline]
    fn static_curve(&self, x_db: f32) -> f32 {
        let (t, r, k) = (self.threshold, self.ratio.max(1.0), self.knee.max(0.0));
        if k > 0.0 && (x_db - t).abs() <= k / 2.0 {
            x_db + (1.0 / r - 1.0) * (x_db - t + k / 2.0).powi(2) / (2.0 * k)
        } else if x_db > t {
            t + (x_db - t) / r
        } else {
            x_db
        }
    }
    /// Gain (linear) for a detector level.
    #[inline]
    fn gain(&mut self, level: f32) -> f32 {
        let x_db = gain_to_db(level);
        let target = x_db - self.static_curve(x_db); // reduction, ≥ 0
        let coef = if target > self.gr { (-1.0 / (self.attack.max(0.01) * 0.001 * self.rate)).exp() } else { (-1.0 / (self.release.max(1.0) * 0.001 * self.rate)).exp() };
        self.gr = target + (self.gr - target) * coef;
        self.env_db = x_db;
        db_to_gain(-self.gr)
    }
}

pub struct Compressor {
    core: CompCore,
    makeup: f32,
    mix: f32,
    use_sidechain: bool,
    sc_hp: [Biquad; 2],
    sc_hp_freq: f32,
    auto_makeup: bool,
    /// Telemetry per block: input peak (dB), output peak (dB), gain reduction (dB).
    tele: [f32; 3],
    style: usize,
    /// Look-ahead in samples (audio is delayed, the detector is not).
    look: usize,
    delay_l: Vec<f32>,
    delay_r: Vec<f32>,
    dpos: usize,
    hold: f32,
    hold_left: f32,
    rms: f32,
    last_out: f32,
    sc_lp: [Biquad; 2],
    sc_lp_freq: f32,
    /// Keeps the detected level steady through a low-frequency wave's cycle.
    steady: PeakHold,
}

/// Compressor styles: detector, knee, release behaviour and feedback differ.
pub const COMP_STYLES: &[&str] = &["Clean", "Classic", "Opto", "Vocal", "Mastering", "Bus", "Punch", "Pumping"];

struct StyleSpec {
    /// RMS window in ms (0 = peak detector).
    rms_ms: f32,
    /// 0 = feed-forward, 1 = feedback (detector on the output, vintage).
    feedback: f32,
    /// Release slows by this factor per 6 dB of reduction (program-dependent release).
    program: f32,
    /// Extra knee added to the knee parameter (dB).
    knee: f32,
    /// Attack scale (Punch lets transients through with a longer attack).
    attack: f32,
    /// Release scale.
    release: f32,
}

fn style_spec(s: usize) -> StyleSpec {
    let st = |rms_ms, feedback, program, knee, attack, release| StyleSpec { rms_ms, feedback, program, knee, attack, release };
    match s {
        1 => st(0.0, 1.0, 0.3, 6.0, 1.0, 1.0),   // Classic: feedback, soft
        2 => st(5.0, 0.0, 1.5, 10.0, 3.0, 1.5),  // Opto: slow attack, release grows after heavy reduction
        3 => st(3.0, 0.0, 0.4, 10.0, 0.6, 0.7),  // Vocal: fast RMS, wide knee, quick to follow the voice
        4 => st(30.0, 0.0, 0.5, 18.0, 0.7, 1.0), // Mastering: transparent, slow detector, very soft knee
        5 => st(8.0, 0.3, 0.3, 6.0, 1.0, 1.0),   // Bus: glue, part feedback
        6 => st(0.0, 0.0, 0.0, 2.0, 2.5, 0.8),   // Punch: slower attack lets the hit through
        7 => st(0.0, 0.0, 0.0, 0.0, 0.5, 0.35),  // Pumping: hard knee, fast release
        _ => st(0.0, 0.0, 0.0, 0.0, 1.0, 1.0),   // Clean
    }
}

const MAX_LOOK_MS: f32 = 20.0;

const COMP_PARAMS: &[ParamInfo] = &[
    ParamInfo::lin("threshold", "Threshold", -60.0, 0.0, -18.0, "dB"),
    ParamInfo::log("ratio", "Ratio", 1.0, 20.0, 4.0, ":1"),
    ParamInfo::lin("knee", "Knee", 0.0, 24.0, 6.0, "dB"),
    ParamInfo::log("attack", "Attack", 0.05, 200.0, 10.0, "ms"),
    ParamInfo::log("release", "Release", 5.0, 2000.0, 120.0, "ms"),
    ParamInfo::lin("makeup", "Makeup", 0.0, 24.0, 0.0, "dB"),
    ParamInfo::toggle("auto_makeup", "Auto makeup", false),
    ParamInfo::lin("mix", "Mix", 0.0, 1.0, 1.0, ""),
    ParamInfo::toggle("sidechain", "External sidechain", false),
    ParamInfo::log("sc_hp", "Sidechain HPF", 20.0, 500.0, 20.0, "Hz"),
    ParamInfo::choice("style", "Style", COMP_STYLES, 0.0),
    ParamInfo::lin("lookahead", "Look-ahead", 0.0, MAX_LOOK_MS, 0.0, "ms"),
    ParamInfo::lin("hold", "Hold", 0.0, 500.0, 0.0, "ms"),
    ParamInfo::log("sc_lp", "Sidechain LPF", 1000.0, 20000.0, 20000.0, "Hz"),
];

impl Effect for Compressor {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "threshold" => self.core.threshold = v,
            "ratio" => self.core.ratio = v,
            "knee" => self.core.knee = v,
            "attack" => self.core.attack = v,
            "release" => self.core.release = v,
            "makeup" => self.makeup = v,
            "auto_makeup" => self.auto_makeup = v >= 0.5,
            "mix" => self.mix = v.clamp(0.0, 1.0),
            "sidechain" => self.use_sidechain = v >= 0.5,
            "sc_hp" => {
                self.sc_hp_freq = v;
                for b in &mut self.sc_hp {
                    b.set(BiquadKind::HighPass, v, 0.707, 0.0, self.core.rate);
                }
            }
            "sc_lp" => {
                self.sc_lp_freq = v;
                for b in &mut self.sc_lp {
                    b.set(BiquadKind::LowPass, v.min(self.core.rate * 0.45), 0.707, 0.0, self.core.rate);
                }
            }
            "style" => self.style = (v.round().max(0.0) as usize).min(COMP_STYLES.len() - 1),
            "lookahead" => {
                self.look = ((v.clamp(0.0, MAX_LOOK_MS) * 0.001 * self.core.rate) as usize).min(self.delay_l.len() - 1);
            }
            "hold" => self.hold = v * 0.001 * self.core.rate,
            _ => {}
        }
    }
    fn reset(&mut self) {
        self.core.gr = 0.0;
        self.sc_hp.iter_mut().for_each(Biquad::reset);
        self.sc_lp.iter_mut().for_each(Biquad::reset);
        self.delay_l.fill(0.0);
        self.delay_r.fill(0.0);
        self.rms = 0.0;
        self.last_out = 0.0;
        self.hold_left = 0.0;
        self.steady.reset();
    }
    fn latency(&self) -> usize {
        self.look
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], sc: Option<(&[f32], &[f32])>) {
        let spec = style_spec(self.style);
        let (attack, release, knee) = (self.core.attack, self.core.release, self.core.knee);
        self.core.knee = knee + spec.knee;
        let auto = if self.auto_makeup { -self.core.static_curve(0.0) * 0.5 } else { 0.0 };
        let makeup = db_to_gain(self.makeup + auto);
        let ext = if self.use_sidechain { sc } else { None };
        let rms_a = if spec.rms_ms > 0.0 { (-1.0 / (spec.rms_ms * 0.001 * self.core.rate)).exp() } else { 0.0 };
        let n = self.delay_l.len();
        let (mut pin, mut pout, mut gr) = (0.0f32, 0.0f32, 0.0f32);
        for i in 0..l.len() {
            pin = pin.max(l[i].abs()).max(r[i].abs());
            let (dl, dr) = match ext {
                Some((a, b)) => (a[i], b[i]),
                None => (l[i], r[i]),
            };
            let (dl, dr) = if self.sc_hp_freq > 21.0 { (self.sc_hp[0].process(dl), self.sc_hp[1].process(dr)) } else { (dl, dr) };
            let (dl, dr) = if self.sc_lp_freq < 19_500.0 { (self.sc_lp[0].process(dl), self.sc_lp[1].process(dr)) } else { (dl, dr) };
            let peak = self.steady.next(dl.abs().max(dr.abs()));
            let ff = if spec.rms_ms > 0.0 {
                self.rms = peak * peak + (self.rms - peak * peak) * rms_a;
                (self.rms * 2.0).sqrt()
            } else {
                peak
            };
            let det = ff * (1.0 - spec.feedback) + self.last_out * spec.feedback;
            // Program-dependent release and hold.
            self.core.attack = attack * spec.attack;
            self.core.release = release * spec.release * (1.0 + spec.program * self.core.gr / 6.0);
            let before = self.core.gr;
            let mut g = self.core.gain(det);
            if self.core.gr < before && self.hold_left > 0.0 {
                self.hold_left -= 1.0;
                self.core.gr = before;
                g = db_to_gain(-before);
            } else if self.core.gr >= before {
                self.hold_left = self.hold;
            }
            // Look-ahead: the gain acts on audio delayed by `look` samples.
            self.delay_l[self.dpos] = l[i];
            self.delay_r[self.dpos] = r[i];
            let rd = (self.dpos + n - self.look) % n;
            let (xl, xr) = (self.delay_l[rd], self.delay_r[rd]);
            self.dpos = (self.dpos + 1) % n;
            let w = g * makeup;
            l[i] = xl * (1.0 - self.mix) + xl * w * self.mix;
            r[i] = xr * (1.0 - self.mix) + xr * w * self.mix;
            self.last_out = (xl * g).abs().max((xr * g).abs());
            pout = pout.max(l[i].abs()).max(r[i].abs());
            gr = gr.max(self.core.gr);
        }
        self.core.attack = attack;
        self.core.release = release;
        self.core.knee = knee;
        let db = |x: f32| (20.0 * x.max(1e-6).log10()).max(-90.0);
        self.tele = [db(pin), db(pout), gr];
    }
    fn gain_reduction(&self) -> f32 {
        self.core.gr
    }
    fn telemetry(&self) -> &[f32] {
        &self.tele
    }
}

pub fn compressor_def() -> EffectDef {
    EffectDef {
        id: "compressor",
        name: "Dragontamer (compressor)",
        category: "Dynamics",
        description: "Compressor with eight styles (Clean, Classic, Opto, Vocal, Mastering, Bus, Punch, Pumping), look-ahead, hold, sidechain filters and external sidechain, auto makeup and parallel mix.",
        params: COMP_PARAMS,
        uses_sidechain: true,
        create: |rate| {
            Box::new(Compressor {
                core: CompCore::new(rate),
                makeup: 0.0,
                mix: 1.0,
                use_sidechain: false,
                sc_hp: [Biquad::new(BiquadKind::HighPass, 20.0, 0.707, 0.0, rate); 2],
                sc_hp_freq: 20.0,
                auto_makeup: false,
                tele: [-90.0, -90.0, 0.0],
                style: 0,
                look: 0,
                delay_l: vec![0.0; (MAX_LOOK_MS * 0.001 * rate) as usize + 2],
                delay_r: vec![0.0; (MAX_LOOK_MS * 0.001 * rate) as usize + 2],
                dpos: 0,
                hold: 0.0,
                hold_left: 0.0,
                rms: 0.0,
                last_out: 0.0,
                sc_lp: [Biquad::new(BiquadKind::LowPass, (rate * 0.45).min(20000.0), 0.707, 0.0, rate); 2],
                sc_lp_freq: 20000.0,
                steady: PeakHold::new(30.0, rate),
            })
        },
    }
}

// ------------------------------------------------------------------------------------------------ multiband

/// Linkwitz–Riley 4th order crossover (two cascaded Butterworth biquads).
#[derive(Debug, Clone, Copy)]
struct Crossover {
    lp: [Biquad; 2],
    hp: [Biquad; 2],
}

impl Crossover {
    fn new(freq: f32, rate: f32) -> Self {
        let lp = Biquad::new(BiquadKind::LowPass, freq, std::f32::consts::FRAC_1_SQRT_2, 0.0, rate);
        let hp = Biquad::new(BiquadKind::HighPass, freq, std::f32::consts::FRAC_1_SQRT_2, 0.0, rate);
        Crossover { lp: [lp, lp], hp: [hp, hp] }
    }
    /// Retunes the crossover keeping the filters' state (rebuilding them would click).
    fn retune(&mut self, freq: f32, rate: f32) {
        for b in &mut self.lp {
            b.set(BiquadKind::LowPass, freq, std::f32::consts::FRAC_1_SQRT_2, 0.0, rate);
        }
        for b in &mut self.hp {
            b.set(BiquadKind::HighPass, freq, std::f32::consts::FRAC_1_SQRT_2, 0.0, rate);
        }
    }
    #[inline]
    fn split(&mut self, x: f32) -> (f32, f32) {
        let a = self.lp[0].process(x);
        let lo = self.lp[1].process(a);
        let b = self.hp[0].process(x);
        let hi = self.hp[1].process(b);
        (lo, hi)
    }
}

/// Multiband dynamics: up to four bands (Linkwitz–Riley crossovers, all-pass compensated), each
/// compressing or expanding, downward or upward, with threshold, range (the most gain change), ratio, knee,
/// attack, release, gain, solo and bypass; global mix and output.
pub const MB_MODES: &[&str] = &["Compress", "Expand", "Upward compress", "Upward expand"];
pub const MB_BANDS: usize = 4;

macro_rules! mb_band {
    ($n:literal, $thr:expr) => {
        [
            ParamInfo::choice(concat!("b", $n, "_mode"), concat!("Band ", $n, " mode"), MB_MODES, 0.0),
            ParamInfo::lin(concat!("b", $n, "_thr"), concat!("Band ", $n, " threshold"), -60.0, 0.0, $thr, "dB"),
            ParamInfo::lin(concat!("b", $n, "_range"), concat!("Band ", $n, " range"), 0.0, 30.0, 12.0, "dB"),
            ParamInfo::log(concat!("b", $n, "_ratio"), concat!("Band ", $n, " ratio"), 1.0, 20.0, 3.0, ":1"),
            ParamInfo::lin(concat!("b", $n, "_knee"), concat!("Band ", $n, " knee"), 0.0, 24.0, 6.0, "dB"),
            ParamInfo::log(concat!("b", $n, "_attack"), concat!("Band ", $n, " attack"), 0.1, 200.0, 15.0, "ms"),
            ParamInfo::log(concat!("b", $n, "_release"), concat!("Band ", $n, " release"), 10.0, 2000.0, 150.0, "ms"),
            ParamInfo::lin(concat!("b", $n, "_gain"), concat!("Band ", $n, " gain"), -18.0, 18.0, 0.0, "dB"),
            ParamInfo::toggle(concat!("b", $n, "_solo"), concat!("Band ", $n, " solo"), false),
            ParamInfo::toggle(concat!("b", $n, "_bypass"), concat!("Band ", $n, " bypass"), false),
            ParamInfo::toggle(concat!("b", $n, "_sc"), concat!("Band ", $n, " external sidechain"), false),
        ]
    };
}
const MB_PB: usize = 11;
/// Global parameters before the bands.
const MB_G: usize = 7;
/// Processing modes: crossover bands (linear-phase sum, all-pass compensated) or dynamic
/// phase (each band is a dynamic shelf / bell on the full signal: no phase shift at all while a band
/// does nothing).
pub const MB_PHASE: &[&str] = &["Crossover", "Dynamic phase"];

const fn mb_params() -> [ParamInfo; MB_G + MB_BANDS * MB_PB] {
    let bands: [[ParamInfo; MB_PB]; MB_BANDS] = [mb_band!("1", -24.0), mb_band!("2", -24.0), mb_band!("3", -24.0), mb_band!("4", -24.0)];
    let mut out = [ParamInfo::choice("bands", "Bands", &["1", "2", "3", "4"], 3.0); MB_G + MB_BANDS * MB_PB];
    out[1] = ParamInfo::log("x1", "Crossover 1", 30.0, 1000.0, 200.0, "Hz");
    out[2] = ParamInfo::log("x2", "Crossover 2", 200.0, 8000.0, 2500.0, "Hz");
    out[3] = ParamInfo::log("x3", "Crossover 3", 1000.0, 16000.0, 8000.0, "Hz");
    out[4] = ParamInfo::lin("mix", "Mix", 0.0, 100.0, 100.0, "%");
    out[5] = ParamInfo::lin("output", "Output", -18.0, 18.0, 0.0, "dB");
    out[6] = ParamInfo::choice("phase", "Processing", MB_PHASE, 0.0);
    let mut i = 0;
    while i < MB_BANDS {
        let mut j = 0;
        while j < MB_PB {
            out[MB_G + i * MB_PB + j] = bands[i][j];
            j += 1;
        }
        i += 1;
    }
    out
}

static MB_PARAMS: [ParamInfo; MB_G + MB_BANDS * MB_PB] = mb_params();

#[derive(Clone, Copy, Default)]
struct MbBand {
    mode: usize,
    thr: f32,
    range: f32,
    ratio: f32,
    knee: f32,
    attack: f32,
    release: f32,
    gain: f32,
    solo: bool,
    bypass: bool,
    /// Detect on the external sidechain's same band instead of the band itself.
    sc: bool,
    env: f32,
    /// Gain change now (dB; negative = cut).
    change: f32,
    /// Keeps the band's detected level steady through its lowest frequency's cycle.
    steady: PeakHold,
}

impl MbBand {
    /// The gain change (dB) for a detector level (dB): compress / expand, downward / upward, limited by the range.
    fn target(&self, x: f32) -> f32 {
        let r = self.ratio.max(1.0);
        let over = x - self.thr;
        // Soft knee: the effective "over" eases in over the knee width.
        let k = self.knee.max(0.01);
        let soft = |o: f32| {
            if o < -k / 2.0 {
                0.0
            } else if o > k / 2.0 {
                o
            } else {
                (o + k / 2.0).powi(2) / (2.0 * k)
            }
        };
        let under = |o: f32| {
            if o > k / 2.0 {
                0.0
            } else if o < -k / 2.0 {
                o
            } else {
                -(k / 2.0 - o).powi(2) / (2.0 * k)
            }
        };
        let c = match self.mode {
            // Downward compression above the threshold.
            0 => -soft(over) * (1.0 - 1.0 / r),
            // Downward expansion below it.
            1 => under(over) * (r - 1.0),
            // Upward compression: quiet parts below the threshold are lifted.
            2 => -under(over) * (1.0 - 1.0 / r),
            // Upward expansion: parts above the threshold are lifted further.
            _ => soft(over) * (r - 1.0) * 0.5,
        };
        c.clamp(-self.range, self.range)
    }
}

pub struct Multiband {
    rate: f32,
    p: [f32; MB_G + MB_BANDS * MB_PB],
    x: [[Crossover; 3]; 2],
    /// All-pass compensation: band k is passed through the crossovers above it, so the bands sum flat.
    ap: [[[Crossover; 3]; MB_BANDS]; 2],
    /// The external sidechain's own band split (for per-band sidechain detection).
    xs: [[Crossover; 3]; 2],
    /// The crossover frequencies the filters are tuned to.
    xf: [f32; 3],
    /// Dynamic phase: one dynamic shelf / bell per band and channel, and the gain it is set to.
    dynf: [[Biquad; MB_BANDS]; 2],
    dynf_db: [f32; MB_BANDS],
    dynf_n: usize,
    bands: [MbBand; MB_BANDS],
    /// Telemetry: per band the detector level (dB) and the gain change (dB).
    tele: [f32; MB_BANDS * 2],
}

impl Multiband {
    fn v(&self, id: &str) -> f32 {
        MB_PARAMS.iter().position(|p| p.id == id).map(|k| self.p[k]).unwrap_or(0.0)
    }
    fn rebuild(&mut self) {
        let f = [self.v("x1"), self.v("x2").max(self.v("x1") * 1.25), self.v("x3").max(self.v("x2") * 1.25)];
        // Only when a crossover moved, and in place: the filters keep their state (no clicks while a
        // knob turns), and band settings alone touch no filter at all.
        if f != self.xf {
            self.xf = f;
            for ch in 0..2 {
                for k in 0..3 {
                    let fk = f[k].min(self.rate * 0.45);
                    self.x[ch][k].retune(fk, self.rate);
                    self.xs[ch][k].retune(fk, self.rate);
                    for b in 0..MB_BANDS {
                        self.ap[ch][b][k].retune(fk, self.rate);
                    }
                }
            }
            self.dynf_db = [f32::NAN; MB_BANDS];
        }
        for (b, band) in self.bands.iter_mut().enumerate() {
            // Hold for half a period of the band's lowest frequency (band 1 goes down to 20 Hz).
            let low = if b == 0 { 20.0 } else { self.xf[b - 1] };
            band.steady.set((600.0 / low.max(20.0)).min(30.0), self.rate);
            let g = |k: usize| self.p[MB_G + b * MB_PB + k];
            band.mode = (g(0).round().max(0.0) as usize).min(MB_MODES.len() - 1);
            band.thr = g(1);
            band.range = g(2);
            band.ratio = g(3);
            band.knee = g(4);
            band.attack = g(5);
            band.release = g(6);
            band.gain = g(7);
            band.solo = g(8) >= 0.5;
            band.bypass = g(9) >= 0.5;
            band.sc = g(10) >= 0.5;
        }
    }
}

impl Multiband {
    /// Dynamic phase: the band's filter at `db` (low shelf for the lowest band, high shelf for the
    /// highest, a bell spanning its crossovers in between).
    fn set_dynf(&mut self, b: usize, n: usize, db: f32) {
        let x = [self.v("x1"), self.v("x2").max(self.v("x1") * 1.25), self.v("x3").max(self.v("x2") * 1.25)];
        let ny = self.rate * 0.45;
        let (kind, f, q) = if n == 1 || b >= n {
            (BiquadKind::Peak, 1000.0, 0.01)
        } else if b == 0 {
            (BiquadKind::LowShelf, x[0].min(ny), 0.707)
        } else if b == n - 1 {
            (BiquadKind::HighShelf, x[n - 2].min(ny), 0.707)
        } else {
            let (lo, hi) = (x[b - 1], x[b]);
            let fc = (lo * hi).sqrt();
            (BiquadKind::Peak, fc.min(ny), (fc / (hi - lo)).clamp(0.3, 4.0))
        };
        for ch in 0..2 {
            self.dynf[ch][b].set(kind, f, q, db, self.rate);
        }
        self.dynf_db[b] = db;
    }
}

impl Effect for Multiband {
    fn set_param(&mut self, id: &str, v: f32) {
        if let Some(k) = MB_PARAMS.iter().position(|p| p.id == id) {
            self.p[k] = v;
            self.rebuild();
        }
    }
    fn reset(&mut self) {
        self.rebuild();
        for b in self.bands.iter_mut() {
            b.env = -120.0;
            b.change = 0.0;
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], sc: Option<(&[f32], &[f32])>) {
        let n = (self.v("bands").round() as usize + 1).clamp(1, MB_BANDS);
        let mix = self.v("mix") / 100.0;
        let out_g = db_to_gain(self.v("output"));
        let any_solo = self.bands[..n].iter().any(|b| b.solo);
        if self.v("phase") >= 0.5 && (self.dynf_db.iter().any(|d| d.is_nan()) || n != self.dynf_n) {
            self.dynf_n = n;
            for b in 0..MB_BANDS {
                self.set_dynf(b, n, 0.0);
            }
        }
        let coefs: [(f32, f32); MB_BANDS] = std::array::from_fn(|k| {
            let b = &self.bands[k];
            ((-1.0 / (b.attack.max(0.05) * 0.001 * self.rate)).exp(), (-1.0 / (b.release.max(1.0) * 0.001 * self.rate)).exp())
        });
        let mut lvl = [-120.0f32; MB_BANDS];
        let mut chg = [0.0f32; MB_BANDS];
        let use_sc = sc.is_some() && self.bands[..n].iter().any(|b| b.sc);
        // Dynamic phase (soloing still listens through the crossovers).
        let dynamic = self.v("phase") >= 0.5 && !any_solo;
        for i in 0..l.len() {
            let x = [l[i], r[i]];
            let mut parts = [[0.0f32; MB_BANDS]; 2];
            // The sidechain's bands (detection only, so no all-pass compensation needed).
            let mut sparts = [[0.0f32; MB_BANDS]; 2];
            if let (true, Some((sa, sb))) = (use_sc, sc) {
                for (ch, src) in [sa[i], sb[i]].into_iter().enumerate() {
                    let mut rest = src;
                    for k in 0..n - 1 {
                        let (lo, hi) = self.xs[ch][k].split(rest);
                        sparts[ch][k] = lo;
                        rest = hi;
                    }
                    sparts[ch][n - 1] = rest;
                }
            }
            for ch in 0..2 {
                let mut rest = x[ch];
                for k in 0..n - 1 {
                    let (lo, hi) = self.x[ch][k].split(rest);
                    parts[ch][k] = lo;
                    rest = hi;
                }
                parts[ch][n - 1] = rest;
                // All-pass the lower bands through the crossovers above them.
                for b in 0..n.saturating_sub(2) {
                    for k in b + 1..n - 1 {
                        let (a, c) = self.ap[ch][b][k].split(parts[ch][b]);
                        parts[ch][b] = a + c;
                    }
                }
            }
            let mut y = [0.0f32; 2];
            for b in 0..n {
                let band = &mut self.bands[b];
                let peak = band.steady.next(if use_sc && band.sc { sparts[0][b].abs().max(sparts[1][b].abs()) } else { parts[0][b].abs().max(parts[1][b].abs()) });
                let db = gain_to_db(peak.max(1e-6));
                // Peak detector with the band's attack / release, then the static curve.
                let want = band.target(db);
                let (att, rel) = coefs[b];
                let c = if want.abs() > band.change.abs() { att } else { rel };
                band.change = want + (band.change - want) * c;
                band.env = db;
                lvl[b] = lvl[b].max(db);
                chg[b] = if band.change.abs() > chg[b].abs() { band.change } else { chg[b] };
                let g = if band.bypass { 1.0 } else { db_to_gain(band.change + band.gain) };
                let audible = !any_solo || band.solo;
                if audible {
                    y[0] += parts[0][b] * g;
                    y[1] += parts[1][b] * g;
                }
            }
            if dynamic {
                // Re-tune each band's filter when its gain moved (every 16 samples at most).
                if i % 16 == 0 {
                    for b in 0..n {
                        let band = self.bands[b];
                        let db = if band.bypass { 0.0 } else { band.change + band.gain };
                        if !(self.dynf_db[b] - db).abs().lt(&0.01) {
                            self.set_dynf(b, n, db);
                        }
                    }
                }
                y = x;
                for ch in 0..2 {
                    for b in 0..n {
                        // A band at 0 dB is skipped: untouched audio, no phase shift.
                        if self.dynf_db[b].abs() > 0.001 {
                            y[ch] = self.dynf[ch][b].process(y[ch]);
                        }
                    }
                }
            }
            l[i] = (x[0] + (y[0] - x[0]) * mix) * out_g;
            r[i] = (x[1] + (y[1] - x[1]) * mix) * out_g;
        }
        for b in 0..MB_BANDS {
            self.tele[b] = if b < n { lvl[b] } else { -120.0 };
            self.tele[MB_BANDS + b] = chg[b];
        }
    }
    fn gain_reduction(&self) -> f32 {
        self.bands.iter().map(|b| -b.change).fold(0.0, f32::max)
    }
    fn telemetry(&self) -> &[f32] {
        &self.tele
    }
}

pub fn multiband_def() -> EffectDef {
    EffectDef {
        id: "multiband",
        name: "Quintessence (multiband dynamics)",
        category: "Dynamics",
        description: "Multiband dynamics: up to four bands, each compressing or expanding (downward or upward) with threshold, range, ratio, knee, attack, release, gain, solo, bypass and per-band external sidechain.",
        params: &MB_PARAMS,
        uses_sidechain: true,
        create: |rate| {
            let mut m = Multiband {
                rate,
                p: std::array::from_fn(|k| MB_PARAMS[k].default),
                x: [[Crossover::new(1000.0, rate); 3]; 2],
                ap: [[[Crossover::new(1000.0, rate); 3]; MB_BANDS]; 2],
                xs: [[Crossover::new(1000.0, rate); 3]; 2],
                xf: [f32::NAN; 3],
                dynf: [[Biquad::default(); MB_BANDS]; 2],
                dynf_db: [f32::NAN; MB_BANDS],
                dynf_n: 0,
                bands: [MbBand::default(); MB_BANDS],
                tele: [0.0; MB_BANDS * 2],
            };
            m.rebuild();
            Box::new(m)
        },
    }
}

// ------------------------------------------------------------------------------------------------ gate

/// Gate / expander: threshold with hysteresis, range, ratio (gate … gentle expander), knee,
/// attack, hold, release, look-ahead, styles (Classic, Clean, Vocal, Guitar, Upward), sidechain filters,
/// external sidechain, flip (ducking) and mix.
pub const GATE_STYLES: &[&str] = &["Classic", "Clean", "Vocal", "Guitar", "Upward"];
const GATE_MAX_LOOK_MS: f32 = 10.0;

pub struct Gate {
    rate: f32,
    p: [f32; 17],
    open: bool,
    hold_left: f32,
    gain_db: f32,
    rms: f32,
    sc: [[Biquad; 2]; 2],
    delay: [Vec<f32>; 2],
    pos: usize,
    tele: [f32; 3],
}

const GATE_PARAMS: &[ParamInfo] = &[
    ParamInfo::lin("threshold", "Threshold", -80.0, 0.0, -40.0, "dB"),
    ParamInfo::lin("range", "Range", -80.0, 0.0, -80.0, "dB"),
    ParamInfo::log("attack", "Attack", 0.05, 50.0, 0.5, "ms"),
    ParamInfo::log("hold", "Hold", 1.0, 500.0, 20.0, "ms"),
    ParamInfo::log("release", "Release", 5.0, 2000.0, 100.0, "ms"),
    ParamInfo::toggle("sidechain", "External sidechain", false),
    ParamInfo::log("ratio", "Ratio", 1.0, 100.0, 100.0, ":1"),
    ParamInfo::lin("knee", "Knee", 0.0, 24.0, 0.0, "dB"),
    ParamInfo::lin("hysteresis", "Hysteresis", 0.0, 12.0, 3.0, "dB"),
    ParamInfo::lin("lookahead", "Look-ahead", 0.0, GATE_MAX_LOOK_MS, 0.0, "ms"),
    ParamInfo::choice("style", "Style", GATE_STYLES, 0.0),
    ParamInfo::log("sc_hp", "Sidechain high-pass", 20.0, 2000.0, 20.0, "Hz"),
    ParamInfo::log("sc_lp", "Sidechain low-pass", 500.0, 20000.0, 20000.0, "Hz"),
    ParamInfo::toggle("flip", "Flip (duck)", false),
    ParamInfo::lin("mix", "Mix", 0.0, 100.0, 100.0, "%"),
    ParamInfo::toggle("listen", "Listen to the sidechain", false),
    ParamInfo::lin("output", "Output", -18.0, 18.0, 0.0, "dB"),
];

impl Gate {
    fn v(&self, id: &str) -> f32 {
        GATE_PARAMS.iter().position(|p| p.id == id).map(|k| self.p[k]).unwrap_or(0.0)
    }
    fn update(&mut self) {
        let (hp, lp) = (self.v("sc_hp"), self.v("sc_lp").min(self.rate * 0.45));
        for ch in 0..2 {
            self.sc[ch][0].set(BiquadKind::HighPass, hp, 0.707, 0.0, self.rate);
            self.sc[ch][1].set(BiquadKind::LowPass, lp, 0.707, 0.0, self.rate);
        }
    }
    fn look(&self) -> usize {
        ((self.v("lookahead") * 0.001 * self.rate) as usize).min(self.delay[0].len() - 1)
    }
}

impl Effect for Gate {
    fn set_param(&mut self, id: &str, v: f32) {
        if let Some(k) = GATE_PARAMS.iter().position(|p| p.id == id) {
            self.p[k] = v;
            if id.starts_with("sc_") {
                self.update();
            }
        }
    }
    fn reset(&mut self) {
        self.gain_db = 0.0;
        self.open = false;
        self.delay.iter_mut().for_each(|d| d.fill(0.0));
    }
    fn latency(&self) -> usize {
        self.look()
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], sc: Option<(&[f32], &[f32])>) {
        let style = (self.v("style").round().max(0.0) as usize).min(GATE_STYLES.len() - 1);
        let (thr, range, ratio, knee, hyst) = (self.v("threshold"), self.v("range"), self.v("ratio").max(1.0), self.v("knee"), self.v("hysteresis"));
        // Styles: detector and timing character.
        let (rms_ms, att_mul, rel_mul, hold_mul) = match style {
            1 => (2.0, 1.0, 1.0, 1.0),  // Clean: RMS, smooth
            2 => (10.0, 2.0, 1.5, 1.0), // Vocal: slower, breath-friendly
            3 => (0.0, 0.5, 2.0, 2.0),  // Guitar: fast open, long hold and release for sustain
            _ => (0.0, 1.0, 1.0, 1.0),  // Classic / Upward: peak
        };
        let att = (-1.0 / (self.v("attack") * att_mul * 0.001 * self.rate).max(1.0)).exp();
        let rel = (-1.0 / (self.v("release") * rel_mul * 0.001 * self.rate).max(1.0)).exp();
        let hold = self.v("hold") * hold_mul * 0.001 * self.rate;
        let rms_a = if rms_ms > 0.0 { (-1.0 / (rms_ms * 0.001 * self.rate)).exp() } else { 0.0 };
        let ext = if self.v("sidechain") >= 0.5 { sc } else { None };
        let (flip, listen) = (self.v("flip") >= 0.5, self.v("listen") >= 0.5);
        let mix = self.v("mix") / 100.0;
        let out = db_to_gain(self.v("output"));
        let look = self.look();
        let n = self.delay[0].len();
        let (mut pin, mut pout, mut gr) = (0.0f32, 0.0f32, 0.0f32);
        for i in 0..l.len() {
            let (dl, dr) = match ext {
                Some((a, b)) => (a[i], b[i]),
                None => (l[i], r[i]),
            };
            let fl = self.sc[0][0].process(dl);
            let fl = self.sc[0][1].process(fl);
            let fr = self.sc[1][0].process(dr);
            let fr = self.sc[1][1].process(fr);
            let peak = fl.abs().max(fr.abs());
            let det = if rms_ms > 0.0 {
                self.rms = peak * peak + (self.rms - peak * peak) * rms_a;
                (self.rms * 2.0).sqrt()
            } else {
                peak
            };
            let db = gain_to_db(det.max(1e-6));
            // Hysteresis: opens at the threshold, closes `hyst` dB below it; hold keeps it open.
            if db > thr {
                self.open = true;
                self.hold_left = hold;
            } else if db < thr - hyst {
                if self.hold_left > 0.0 {
                    self.hold_left -= 1.0;
                } else {
                    self.open = false;
                }
            }
            let target = if style == 4 {
                // Upward: lifts what crosses the threshold (by up to |range|, at most 24 dB).
                if self.open {
                    ((db - thr) * (1.0 - 1.0 / ratio)).clamp(0.0, (-range).min(24.0))
                } else {
                    0.0
                }
            } else if self.open {
                0.0
            } else {
                // Closed: downward expansion below the threshold, soft knee, floor at the range.
                let under = db - thr;
                let k = knee.max(0.01);
                let u = if under < -k / 2.0 { under } else { -(k / 2.0 - under).max(0.0).powi(2) / (2.0 * k) };
                (u * (ratio - 1.0)).max(range)
            };
            let c = if target > self.gain_db { att } else { rel };
            self.gain_db = target + (self.gain_db - target) * c;
            let g_db = if flip && style != 4 { range - self.gain_db } else { self.gain_db };
            let g = db_to_gain(g_db);
            // Look-ahead: the gain acts on audio delayed by `look`.
            self.delay[0][self.pos] = l[i];
            self.delay[1][self.pos] = r[i];
            let rd = (self.pos + n - look) % n;
            let (xl, xr) = (self.delay[0][rd], self.delay[1][rd]);
            self.pos = (self.pos + 1) % n;
            pin = pin.max(xl.abs()).max(xr.abs());
            if listen {
                l[i] = fl;
                r[i] = fr;
            } else {
                l[i] = (xl + (xl * g - xl) * mix) * out;
                r[i] = (xr + (xr * g - xr) * mix) * out;
            }
            pout = pout.max(l[i].abs()).max(r[i].abs());
            gr = gr.max(-g_db);
        }
        let db = |x: f32| (20.0 * x.max(1e-6).log10()).max(-90.0);
        self.tele = [db(pin), db(pout), gr];
    }
    fn gain_reduction(&self) -> f32 {
        (-self.gain_db).max(0.0)
    }
    fn telemetry(&self) -> &[f32] {
        &self.tele
    }
}

pub fn gate_def() -> EffectDef {
    EffectDef {
        id: "gate",
        name: "Portcullis (gate)",
        category: "Dynamics",
        description:
            "Gate / expander: threshold with hysteresis, range, ratio, knee, attack, hold, release, look-ahead, styles (Classic, Clean, Vocal, Guitar, Upward), sidechain filters, external sidechain, flip (ducking), mix.",
        params: GATE_PARAMS,
        uses_sidechain: true,
        create: |rate| {
            let mut g = Gate {
                rate,
                p: std::array::from_fn(|k| GATE_PARAMS[k].default),
                open: false,
                hold_left: 0.0,
                gain_db: 0.0,
                rms: 0.0,
                sc: [[Biquad::default(); 2]; 2],
                delay: [vec![0.0; (GATE_MAX_LOOK_MS * 0.001 * rate) as usize + 2], vec![0.0; (GATE_MAX_LOOK_MS * 0.001 * rate) as usize + 2]],
                pos: 0,
                tele: [-90.0, -90.0, 0.0],
            };
            g.update();
            Box::new(g)
        },
    }
}

// ------------------------------------------------------------------------------------------------ limiter

/// Look-ahead brickwall limiter. The gain is computed from the peak of the look-ahead window (with a
/// 4× oversampled estimate when *true peak* is on) and smoothed so it never overshoots the ceiling.
pub struct Limiter {
    rate: f32,
    ceiling: f32,
    gain_in: f32,
    release: f32,
    lookahead: usize,
    true_peak: bool,
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    peaks: Vec<f32>,
    pos: usize,
    gain: f32,
    tp: crate::dsp::loudness::TruePeak,
    gr: f32,
    style: usize,
    /// Slow release state for program-dependent styles.
    slow: f32,
}

const LIMITER_PARAMS: &[ParamInfo] = &[
    ParamInfo::lin("gain", "Input gain", 0.0, 24.0, 0.0, "dB"),
    ParamInfo::lin("ceiling", "Ceiling", -12.0, 0.0, -1.0, "dBTP"),
    ParamInfo::log("release", "Release", 1.0, 1000.0, 80.0, "ms"),
    ParamInfo::toggle("true_peak", "True peak", true),
    ParamInfo::choice("style", "Style", LIMITER_STYLES, 0.0),
    ParamInfo::choice("oversampling", "Oversampling", &["Off", "2×", "4×"], 0.0),
    ParamInfo::toggle("unity", "Unity-gain listen", false),
];

/// Limiter styles: how fast the gain recovers, how hard the attack is smoothed and
/// whether a soft clipper takes the first peaks.
pub const LIMITER_STYLES: &[&str] = &["Allround", "Transparent", "Punchy", "Dynamic", "Aggressive", "Modern", "Bus", "Safe"];

const LOOKAHEAD_MS: f32 = 3.0;

impl Effect for Limiter {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "gain" => self.gain_in = db_to_gain(v),
            "ceiling" => self.ceiling = db_to_gain(v),
            "release" => self.release = v,
            "true_peak" => self.true_peak = v >= 0.5,
            "style" => self.style = (v.round().max(0.0) as usize).min(LIMITER_STYLES.len() - 1),
            _ => {}
        }
    }
    fn reset(&mut self) {
        self.buf_l.iter_mut().for_each(|x| *x = 0.0);
        self.buf_r.iter_mut().for_each(|x| *x = 0.0);
        self.peaks.iter_mut().for_each(|x| *x = 0.0);
        self.gain = 1.0;
    }
    fn latency(&self) -> usize {
        self.lookahead
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let n = self.lookahead;
        // (release ×, attack sharpness, soft clip drive, program-dependent slow release share)
        let (rel_k, att_k, clip, prog) = match self.style {
            1 => (2.0, 0.6, 0.0, 0.6),  // Transparent: slow, smooth, program dependent
            2 => (0.5, 1.6, 0.0, 0.0),  // Punchy: fast recovery, sharper attack
            3 => (0.7, 1.0, 0.0, 0.8),  // Dynamic: fast after short peaks, slow after long ones
            4 => (0.4, 2.0, 0.35, 0.0), // Aggressive: soft clip first, fast
            5 => (1.0, 1.2, 0.15, 0.4), // Modern: a little clipping, adaptive
            6 => (1.5, 0.8, 0.0, 0.3),  // Bus: glue
            7 => (3.0, 0.5, 0.0, 0.5),  // Safe: no distortion, slowest
            _ => (1.0, 1.0, 0.0, 0.2),  // Allround
        };
        let rel = (-1.0 / (self.release * rel_k * 0.001 * self.rate)).exp();
        let rel_slow = (-1.0 / (self.release * rel_k * 6.0 * 0.001 * self.rate)).exp();
        let att = (-5.0 * att_k / n as f32).exp();
        let margin = if self.true_peak { 0.995 } else { 1.0 };
        for i in 0..l.len() {
            let (mut a, mut b) = (l[i] * self.gain_in, r[i] * self.gain_in);
            if clip > 0.0 {
                // Soft clipper before the limiter (takes the sharpest peaks, the "Aggressive" / "Modern" styles).
                let c = |x: f32| {
                    let k = self.ceiling * (1.0 + clip);
                    if x.abs() > self.ceiling * (1.0 - clip) {
                        x.signum() * (k * (x.abs() / k).tanh()).max(self.ceiling * (1.0 - clip))
                    } else {
                        x
                    }
                };
                a = c(a);
                b = c(b);
            }
            let p = if self.true_peak { self.tp.process(a, b).max(a.abs()).max(b.abs()) } else { a.abs().max(b.abs()) };
            self.buf_l[self.pos] = a;
            self.buf_r[self.pos] = b;
            self.peaks[self.pos] = p;
            self.pos = (self.pos + 1) % n;
            // Peak over the window = sample that will leave plus everything after it.
            let win_peak = self.peaks.iter().fold(0.0f32, |m, &x| m.max(x));
            let target = (self.ceiling * margin / win_peak.max(1e-9)).min(1.0);
            if target < self.gain {
                self.gain = target + (self.gain - target) * att;
                self.slow = self.slow.min(self.gain);
            } else {
                // Two release paths: the fast one, and a slow one that remembers sustained limiting.
                let fast = target + (self.gain - target) * rel;
                self.slow = target + (self.slow - target) * rel_slow;
                self.gain = fast * (1.0 - prog) + self.slow.max(self.gain).min(fast) * prog;
            }
            let g = self.gain;
            let out_l = self.buf_l[self.pos] * g;
            let out_r = self.buf_r[self.pos] * g;
            // Safety clip at the ceiling (catches anything the smoothing missed).
            l[i] = out_l.clamp(-self.ceiling, self.ceiling);
            r[i] = out_r.clamp(-self.ceiling, self.ceiling);
            self.gr = -gain_to_db(g);
        }
    }
    fn gain_reduction(&self) -> f32 {
        self.gr
    }
}

fn limiter_core(rate: f32) -> Limiter {
    let n = ((LOOKAHEAD_MS * 0.001 * rate) as usize).max(8);
    Limiter {
        rate,
        ceiling: db_to_gain(-1.0),
        gain_in: 1.0,
        release: 80.0,
        lookahead: n,
        true_peak: true,
        buf_l: vec![0.0; n],
        buf_r: vec![0.0; n],
        peaks: vec![0.0; n],
        style: 0,
        slow: 1.0,
        pos: 0,
        gain: 1.0,
        tp: Default::default(),
        gr: 0.0,
    }
}

/// The limiter with oversampling: the core runs at 1×, 2× or 4× the rate (gain changes alias
/// less, inter-sample peaks are caught), behind half-band filters; a level / gain-reduction history for the
/// display.
pub struct OversampledLimiter {
    cores: [Limiter; 3],
    os: usize,
    ovs: [[crate::dsp::oversample::Oversampler; 2]; 3],
    up_l: Vec<f32>,
    up_r: Vec<f32>,
    tele: [f32; 3],
    /// Unity-gain listening: the output drops by the input gain, so only the limiting is heard.
    unity: bool,
}

impl Effect for OversampledLimiter {
    fn set_param(&mut self, id: &str, v: f32) {
        if id == "unity" {
            self.unity = v >= 0.5;
            return;
        }
        if id == "oversampling" {
            let k = (v.round().max(0.0) as usize).min(2);
            if k != self.os {
                self.os = k;
                self.cores[k].reset();
                self.ovs[k].iter_mut().for_each(|o| o.reset());
            }
            return;
        }
        for c in &mut self.cores {
            c.set_param(id, v);
        }
    }
    fn reset(&mut self) {
        for c in &mut self.cores {
            c.reset();
        }
        self.ovs.iter_mut().flatten().for_each(|o| o.reset());
    }
    fn latency(&self) -> usize {
        self.cores[self.os].latency() / (1 << self.os) + self.ovs[self.os][0].latency()
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], sc: Option<(&[f32], &[f32])>) {
        let pin = l.iter().chain(r.iter()).fold(0.0f32, |m, x| m.max(x.abs()));
        if self.os == 0 {
            self.cores[0].process(l, r, sc);
        } else {
            let f = 1 << self.os;
            let n = l.len();
            self.up_l.resize(n * f, 0.0);
            self.up_r.resize(n * f, 0.0);
            let [ol, or] = &mut self.ovs[self.os];
            for i in 0..n {
                ol.up(l[i], &mut self.up_l[i * f..i * f + f]);
                or.up(r[i], &mut self.up_r[i * f..i * f + f]);
            }
            self.cores[self.os].process(&mut self.up_l, &mut self.up_r, None);
            let ceil = self.cores[self.os].ceiling;
            for i in 0..n {
                // The down filter can ring a hair over the ceiling; the final clip holds it.
                l[i] = ol.down(&self.up_l[i * f..i * f + f]).clamp(-ceil, ceil);
                r[i] = or.down(&self.up_r[i * f..i * f + f]).clamp(-ceil, ceil);
            }
        }
        if self.unity {
            let g = 1.0 / self.cores[self.os].gain_in.max(1e-6);
            l.iter_mut().chain(r.iter_mut()).for_each(|x| *x *= g);
        }
        let pout = l.iter().chain(r.iter()).fold(0.0f32, |m, x| m.max(x.abs()));
        let db = |x: f32| (20.0 * x.max(1e-6).log10()).max(-90.0);
        self.tele = [db(pin), db(pout), self.cores[self.os].gr];
    }
    fn gain_reduction(&self) -> f32 {
        self.cores[self.os].gr
    }
    fn telemetry(&self) -> &[f32] {
        &self.tele
    }
}

pub fn limiter_def() -> EffectDef {
    EffectDef {
        id: "limiter",
        name: "Lodestone (limiter)",
        category: "Dynamics",
        description: "Look-ahead brickwall limiter with true-peak detection, eight styles (Allround, Transparent, Punchy, Dynamic, Aggressive, Modern, Bus, Safe) and 2× / 4× oversampling for mastering.",
        params: LIMITER_PARAMS,
        uses_sidechain: false,
        create: |rate| {
            Box::new(OversampledLimiter {
                cores: [limiter_core(rate), limiter_core(rate * 2.0), limiter_core(rate * 4.0)],
                os: 0,
                ovs: std::array::from_fn(|k| [crate::dsp::oversample::Oversampler::new(1 << k), crate::dsp::oversample::Oversampler::new(1 << k)]),
                up_l: Vec::new(),
                up_r: Vec::new(),
                tele: [-90.0, -90.0, 0.0],
                unity: false,
            })
        },
    }
}

// ------------------------------------------------------------------------------------------------ de-esser

/// De-esser after Pro-DS: single-vocal (relative: reacts to sibilance compared with the voice) or allround
/// (absolute) detection, split-band or wide-band processing, detection band (high-pass … low-pass), threshold,
/// range (the most reduction), look-ahead, stereo link and listen.
pub const DS_MODES: &[&str] = &["Single vocal", "Allround"];
const DS_MAX_LOOK_MS: f32 = 15.0;

pub struct DeEsser {
    rate: f32,
    p: [f32; 9],
    det_hp: [Biquad; 2],
    det_lp: [Biquad; 2],
    split: [Biquad; 2],
    full_env: [f32; 2],
    sib_env: [f32; 2],
    red: [f32; 2],
    delay: [Vec<f32>; 2],
    pos: usize,
    gr: f32,
    tele: [f32; 3],
}

const DEESS_PARAMS: &[ParamInfo] = &[
    ParamInfo::log("freq", "Detection high-pass", 2000.0, 12000.0, 5500.0, "Hz"),
    ParamInfo::lin("threshold", "Threshold", -60.0, 0.0, -30.0, "dB"),
    ParamInfo::lin("range", "Range", 0.0, 24.0, 12.0, "dB"),
    ParamInfo::toggle("listen", "Listen", false),
    ParamInfo::log("freq_hi", "Detection low-pass", 4000.0, 20000.0, 16000.0, "Hz"),
    ParamInfo::choice("mode", "Mode", DS_MODES, 0.0),
    ParamInfo::choice("processing", "Processing", &["Split band", "Wide band"], 0.0),
    ParamInfo::lin("lookahead", "Look-ahead", 0.0, DS_MAX_LOOK_MS, 0.0, "ms"),
    ParamInfo::lin("link", "Stereo link", 0.0, 100.0, 100.0, "%"),
];

impl DeEsser {
    fn v(&self, id: &str) -> f32 {
        DEESS_PARAMS.iter().position(|p| p.id == id).map(|k| self.p[k]).unwrap_or(0.0)
    }
    fn update(&mut self) {
        let (hp, lp) = (self.v("freq"), self.v("freq_hi").max(self.v("freq") * 1.2).min(self.rate * 0.45));
        for ch in 0..2 {
            self.det_hp[ch].set(BiquadKind::HighPass, hp, 0.9, 0.0, self.rate);
            self.det_lp[ch].set(BiquadKind::LowPass, lp, 0.707, 0.0, self.rate);
            let red = self.red[ch];
            self.split[ch].set(BiquadKind::HighShelf, (hp * 0.8).min(self.rate * 0.4), 0.707, -red, self.rate);
        }
    }
    fn look(&self) -> usize {
        ((self.v("lookahead") * 0.001 * self.rate) as usize).min(self.delay[0].len() - 1)
    }
}

impl Effect for DeEsser {
    fn set_param(&mut self, id: &str, v: f32) {
        if let Some(k) = DEESS_PARAMS.iter().position(|p| p.id == id) {
            self.p[k] = v;
            self.update();
        }
    }
    fn reset(&mut self) {
        self.det_hp.iter_mut().chain(self.det_lp.iter_mut()).chain(self.split.iter_mut()).for_each(Biquad::reset);
        self.delay.iter_mut().for_each(|d| d.fill(0.0));
        self.red = [0.0; 2];
    }
    fn latency(&self) -> usize {
        self.look()
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let (thr, range) = (self.v("threshold"), self.v("range"));
        let single = self.v("mode") < 0.5;
        let wide = self.v("processing") >= 0.5;
        let listen = self.v("listen") >= 0.5;
        let link = self.v("link") / 100.0;
        let look = self.look();
        let n = self.delay[0].len();
        let fast = (-1.0 / (0.0005 * self.rate)).exp();
        let rel = (-1.0 / (0.06 * self.rate)).exp();
        let slow = (-1.0 / (0.03 * self.rate)).exp();
        let shelf_f = (self.v("freq") * 0.8).min(self.rate * 0.4);
        let (mut pin, mut pout, mut gr) = (0.0f32, 0.0f32, 0.0f32);
        for i in 0..l.len() {
            let x = [l[i], r[i]];
            let mut want = [0.0f32; 2];
            let mut sib = [0.0f32; 2];
            for ch in 0..2 {
                let h = self.det_hp[ch].process(x[ch]);
                sib[ch] = self.det_lp[ch].process(h);
                let s = sib[ch].abs();
                self.sib_env[ch] = s + (self.sib_env[ch] - s) * if s > self.sib_env[ch] { fast } else { rel };
                let f = x[ch].abs();
                self.full_env[ch] = f + (self.full_env[ch] - f) * slow;
                let s_db = gain_to_db(self.sib_env[ch].max(1e-6));
                // Single vocal: sibilance counts when it stands out from the voice itself (relative),
                // so a quiet "s" is still caught and a loud vowel is left alone.
                let over = if single {
                    let rel_db = s_db - gain_to_db(self.full_env[ch].max(1e-6));
                    (s_db - thr).min(rel_db + 6.0)
                } else {
                    s_db - thr
                };
                want[ch] = (over.max(0.0) * 0.8).min(range);
            }
            // Stereo link: each side moves towards the louder side's reduction.
            let mx = want[0].max(want[1]);
            for ch in 0..2 {
                let w = want[ch] + (mx - want[ch]) * link;
                self.red[ch] = w + (self.red[ch] - w) * if w > self.red[ch] { fast } else { rel };
            }
            // Look-ahead on the audio path.
            let d = &mut self.delay;
            d[0][self.pos] = x[0];
            d[1][self.pos] = x[1];
            let rd = (self.pos + n - look) % n;
            let y = [d[0][rd], d[1][rd]];
            self.pos = (self.pos + 1) % n;
            for ch in 0..2 {
                let g = db_to_gain(-self.red[ch]);
                // Split band: a dynamic high shelf (re-tuned every 16 samples) cuts only above the
                // detection frequency, so the voice body below it is untouched and the phase stays clean.
                if i % 16 == 0 {
                    self.split[ch].set(BiquadKind::HighShelf, shelf_f, 0.707, -self.red[ch], self.rate);
                }
                let hf = self.split[ch].process(y[ch]);
                let o = if listen {
                    sib[ch]
                } else if wide {
                    y[ch] * g
                } else {
                    hf
                };
                if ch == 0 {
                    l[i] = o;
                } else {
                    r[i] = o;
                }
                pin = pin.max(y[ch].abs());
                pout = pout.max(o.abs());
                gr = gr.max(self.red[ch]);
            }
        }
        self.gr = gr;
        let db = |x: f32| (20.0 * x.max(1e-6).log10()).max(-90.0);
        self.tele = [db(pin), db(pout), gr];
    }
    fn gain_reduction(&self) -> f32 {
        self.gr
    }
    fn telemetry(&self) -> &[f32] {
        &self.tele
    }
}

pub fn deesser_def() -> EffectDef {
    EffectDef {
        id: "deesser",
        name: "Serpent's Hush (de-esser)",
        category: "Dynamics",
        description: "De-esser after Pro-DS: single-vocal (relative) or allround detection, split- or wide-band processing, detection band, threshold, range, look-ahead, stereo link and listen.",
        params: DEESS_PARAMS,
        uses_sidechain: false,
        create: |rate| {
            let look = (DS_MAX_LOOK_MS * 0.001 * rate) as usize + 2;
            let mut d = DeEsser {
                rate,
                p: std::array::from_fn(|k| DEESS_PARAMS[k].default),
                det_hp: [Biquad::default(); 2],
                det_lp: [Biquad::default(); 2],
                split: [Biquad::default(); 2],
                full_env: [0.0; 2],
                sib_env: [0.0; 2],
                red: [0.0; 2],
                delay: [vec![0.0; look], vec![0.0; look]],
                pos: 0,
                gr: 0.0,
                tele: [-90.0, -90.0, 0.0],
            };
            d.update();
            Box::new(d)
        },
    }
}

// ------------------------------------------------------------------------------------------------ transient shaper

/// Attack shapes : Needle – very sharp rise and fall; Nail – sharp rise,
/// slower sloped fall; Blunt – slower at both ends, for non-percussive sources.
pub const TS_SHAPES: &[&str] = &["Needle", "Nail", "Blunt"];
/// Sustain shapes: Linear; Non-linear (the curve follows the sustain amount); Soft linear (smooth ends).
pub const TS_SUS_SHAPES: &[&str] = &["Linear", "Non-linear", "Soft linear"];

/// Attack shape `k` at `x` (0 … 1 of its duration) → weight 0 … 1.
pub fn ts_shape(k: usize, x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    match k {
        0 => (1.0 - x).powi(4),
        1 => (1.0 - x).powf(1.5),
        _ => {
            // Blunt: rises over the first quarter, then falls smoothly.
            let rise = (x / 0.25).min(1.0);
            let rise = rise * rise * (3.0 - 2.0 * rise);
            let fall = if x < 0.25 { 1.0 } else { 1.0 - ((x - 0.25) / 0.75).powi(2) };
            rise * fall
        }
    }
}

/// Sustain shape `k` at `x` (0 … 1 of its duration), for a sustain `amount` (−1 … 1).
pub fn ts_sus_shape(k: usize, x: f32, amount: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    match k {
        1 => (1.0 - x).powf(1.0 + 2.0 * amount.abs()),
        2 => 0.5 + 0.5 * (std::f32::consts::PI * x).cos(),
        _ => 1.0 - x,
    }
}

/// Guard modes: Off, Clip (hard clip, output never above −0.1 dBFS), Limit (fast limiter to
/// −0.1 dBFS); Soft clip is ours.
pub const TS_GUARDS: &[&str] = &["Off", "Clip", "Limit", "Soft clip"];

/// Transient designer with separate attack and sustain sections (amount, sensitivity, duration and
/// shape each), a clip / soft-clip guard, dry/wet mix and a listen mode that solos the change.
pub struct TransientShaper {
    rate: f32,
    fast: Follower,
    slow: Follower,
    peak: f32,
    att: [f32; 4],
    sus: [f32; 4],
    guard: usize,
    listen: usize,
    mix: f32,
    output: f32,
    /// Seconds since the last detected attack (∞ = none yet), and the re-arm hold.
    since: f32,
    armed: bool,
    tele: [f32; 4],
    /// Guard limiter gain.
    lim: f32,
}

const TS_PARAMS: &[ParamInfo] = &[
    ParamInfo::lin("attack", "Attack amount", -100.0, 100.0, 0.0, "%"),
    ParamInfo::lin("att_sens", "Attack sensitivity", 0.0, 100.0, 60.0, "%"),
    ParamInfo::log("att_dur", "Attack duration", 1.0, 200.0, 25.0, "ms"),
    ParamInfo::choice("att_shape", "Attack shape", TS_SHAPES, 1.0),
    ParamInfo::lin("sustain", "Sustain amount", -100.0, 100.0, 0.0, "%"),
    ParamInfo::lin("sus_sens", "Sustain sensitivity", 0.0, 100.0, 60.0, "%"),
    ParamInfo::log("sus_dur", "Sustain duration", 10.0, 2000.0, 250.0, "ms"),
    ParamInfo::choice("sus_shape", "Sustain shape", TS_SUS_SHAPES, 0.0),
    ParamInfo::choice("guard", "Guard", TS_GUARDS, 2.0),
    ParamInfo::choice("listen", "Listen", &["Output", "Attack only", "Sustain only"], 0.0),
    ParamInfo::lin("mix", "Mix", 0.0, 1.0, 1.0, ""),
    ParamInfo::lin("output", "Output", -18.0, 18.0, 0.0, "dB"),
];

impl TransientShaper {
    pub fn new(rate: f32) -> Self {
        TransientShaper {
            rate,
            fast: Follower::new(0.2, 8.0, rate),
            slow: Follower::new(15.0, 120.0, rate),
            peak: 0.0,
            att: [0.0, 0.6, 0.025, 1.0],
            sus: [0.0, 0.6, 0.25, 0.0],
            guard: 2,
            listen: 0,
            mix: 1.0,
            output: 1.0,
            since: f32::INFINITY,
            armed: true,
            tele: [0.0; 4],
            lim: 1.0,
        }
    }

    /// Weights (attack, sustain) at `t` seconds after an attack.
    pub fn weights(&self, t: f32) -> (f32, f32) {
        let (ad, sd) = (self.att[2].max(0.001), self.sus[2].max(0.01));
        let wa = if t < ad { ts_shape(self.att[3] as usize, t / ad) } else { 0.0 };
        // The sustain section starts as the attack fades and fades in over 5 ms.
        let s0 = ad * 0.6;
        let ws = if t >= s0 && t < s0 + sd { ((t - s0) / 0.005).min(1.0) * ts_sus_shape(self.sus[3] as usize, (t - s0) / sd, self.sus[0]) } else { 0.0 };
        (wa, ws)
    }
}

impl Effect for TransientShaper {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "attack" => self.att[0] = v / 100.0,
            "att_sens" => self.att[1] = v / 100.0,
            "att_dur" => self.att[2] = v / 1000.0,
            "att_shape" => self.att[3] = v.round().clamp(0.0, 2.0),
            "sustain" => self.sus[0] = v / 100.0,
            "sus_sens" => self.sus[1] = v / 100.0,
            "sus_dur" => self.sus[2] = v / 1000.0,
            "sus_shape" => self.sus[3] = v.round().clamp(0.0, 2.0),
            "guard" => self.guard = (v.round().max(0.0) as usize).min(TS_GUARDS.len() - 1),
            "listen" => self.listen = v.round() as usize,
            "mix" => self.mix = v.clamp(0.0, 1.0),
            "output" => self.output = db_to_gain(v),
            _ => {}
        }
    }
    fn reset(&mut self) {
        self.since = f32::INFINITY;
        self.armed = true;
        self.peak = 0.0;
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let dt = 1.0 / self.rate;
        // Attack sensitivity: the jump (fast over slow envelope) that counts as a hit, 14 dB … 1.5 dB.
        let thresh = 14.0 - 12.5 * self.att[1];
        // Sustain sensitivity: how far below the hit's peak the tail still counts (10 … 50 dB).
        let floor = 10.0 + 40.0 * self.sus[1];
        let peak_rel = (-dt / 0.4).exp();
        self.tele = [0.0; 4];
        for i in 0..l.len() {
            let x = l[i].abs().max(r[i].abs());
            let f = self.fast.next(x);
            let s = self.slow.next(x);
            let jump = gain_to_db(f.max(1e-6)) - gain_to_db(s.max(1e-6));
            if self.armed && jump > thresh && f > 1e-4 {
                self.since = 0.0;
                self.armed = false;
                self.peak = f;
            }
            if !self.armed && jump < thresh * 0.4 && self.since > 0.01 {
                self.armed = true;
            }
            self.peak = (self.peak * peak_rel).max(f);
            let (wa, mut ws) = self.weights(self.since);
            // The tail only counts while it is within `floor` dB of the hit.
            if gain_to_db(s.max(1e-6)) < gain_to_db(self.peak.max(1e-6)) - floor {
                ws = 0.0;
            }
            self.since += dt;
            let ga = self.att[0] * 18.0 * wa;
            let gs = self.sus[0] * 18.0 * ws;
            let g = db_to_gain(ga + gs);
            let ceil = db_to_gain(-0.1);
            // Output comes before the guard (manual), so the limiter looks at the output-scaled level.
            let wet_of = |dry: f32| match self.listen {
                1 => dry * (db_to_gain(ga) - 1.0),
                2 => dry * (db_to_gain(gs) - 1.0),
                _ => dry * g,
            };
            if self.guard == 2 {
                let pk = [l[i], r[i]].iter().map(|d| ((d * (1.0 - self.mix) + wet_of(*d) * self.mix) * self.output).abs()).fold(0.0f32, f32::max);
                let want = if pk > ceil { ceil / pk } else { 1.0 };
                self.lim = if want < self.lim { want } else { want + (self.lim - want) * (-dt / 0.05).exp() };
            }
            for buf in [&mut l[i], &mut r[i]] {
                let dry = *buf;
                let wet = wet_of(dry);
                let mut y = (dry * (1.0 - self.mix) + wet * self.mix) * self.output;
                y = match self.guard {
                    1 => y.clamp(-ceil, ceil),
                    2 => (y * self.lim).clamp(-ceil, ceil),
                    3 => {
                        if y.abs() > 0.8 {
                            y.signum() * (0.8 + 0.2 * ((y.abs() - 0.8) / 0.2).tanh())
                        } else {
                            y
                        }
                    }
                    _ => y,
                };
                *buf = y;
            }
            // Display: input level, attack weight, sustain weight, gain (dB) – the block's maxima.
            self.tele[0] = self.tele[0].max(x);
            self.tele[1] = self.tele[1].max(wa);
            self.tele[2] = self.tele[2].max(ws);
            self.tele[3] = if (ga + gs).abs() > self.tele[3].abs() { ga + gs } else { self.tele[3] };
        }
    }
    fn telemetry(&self) -> &[f32] {
        &self.tele
    }
}

pub fn transient_def() -> EffectDef {
    EffectDef {
        id: "transient",
        name: "Talon (transient shaper)",
        category: "Dynamics",
        description: "Transient designer: attack (Needle / Nail / Blunt) and sustain (Linear / Non-linear / Soft linear) sections with level, sensitivity, duration and shape, mix, output, and a guard (Off / Clip / Limit / Soft clip) after the output.",
        params: TS_PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(TransientShaper::new(rate)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn run_fx(def: EffectDef, params: &[(&str, f32)], l: &[f32], r: &[f32], sc: Option<(&[f32], &[f32])>) -> (Vec<f32>, Vec<f32>, f32) {
        let mut e = (def.create)(48000.0);
        for (k, v) in params {
            e.set_param(k, *v);
        }
        let (mut a, mut b) = (l.to_vec(), r.to_vec());
        for (i, (x, y)) in a.chunks_mut(256).zip(b.chunks_mut(256)).enumerate() {
            let s = sc.map(|(p, q)| (&p[i * 256..i * 256 + x.len()], &q[i * 256..i * 256 + x.len()]));
            e.process(x, y, s);
        }
        let gr = e.gain_reduction();
        (a, b, gr)
    }
    fn rms_of(v: &[f32]) -> f32 {
        (v.iter().map(|s| s * s).sum::<f32>() / v.len().max(1) as f32).sqrt()
    }
    fn sine(f: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| (i as f32 / 48000.0 * f * std::f32::consts::TAU).sin() * amp).collect()
    }

    /// Idle bands sum flat; one band compressing touches only its range; solo isolates a band;
    /// upward compression lifts a quiet band.
    #[test]
    fn multiband_bands_modes_and_solo() {
        let n = 48000;
        let lo = sine(100.0, 0.5, n);
        let hi = sine(6000.0, 0.5, n);
        let mix: Vec<f32> = lo.iter().zip(&hi).map(|(a, b)| a + b).collect();
        let idle = [("b1_thr", 0.0), ("b2_thr", 0.0), ("b3_thr", 0.0), ("b4_thr", 0.0)];
        let (y, _, _) = run_fx(multiband_def(), &idle, &mix, &mix, None);
        let ratio = rms_of(&y[24000..]) / rms_of(&mix[24000..]);
        assert!((ratio - 1.0).abs() < 0.08, "idle multiband must sum ~flat, got {ratio}");
        // Compress only the low band hard.
        let (y, _, gr) = run_fx(multiband_def(), &[("b1_thr", -30.0), ("b1_ratio", 20.0), ("b1_range", 30.0), ("b2_thr", 0.0), ("b3_thr", 0.0), ("b4_thr", 0.0)], &lo, &lo, None);
        assert!(gr > 6.0 && rms_of(&y[24000..]) < rms_of(&lo[24000..]) * 0.5);
        let (y, _, _) = run_fx(multiband_def(), &[("b1_thr", -30.0), ("b1_ratio", 20.0), ("b1_range", 30.0), ("b2_thr", 0.0), ("b3_thr", 0.0), ("b4_thr", 0.0)], &hi, &hi, None);
        assert!(rms_of(&y[24000..]) > rms_of(&hi[24000..]) * 0.85, "the high band is left alone");
        // Solo band 1: the 6 kHz tone disappears.
        let (y, _, _) = run_fx(multiband_def(), &[("b1_solo", 1.0), ("b1_thr", 0.0), ("b2_thr", 0.0), ("b3_thr", 0.0), ("b4_thr", 0.0)], &hi, &hi, None);
        assert!(rms_of(&y[24000..]) < rms_of(&hi[24000..]) * 0.1);
        // Upward compression lifts a quiet signal in its band.
        let quiet = sine(100.0, 0.01, n);
        let (y, _, _) = run_fx(multiband_def(), &[("b1_mode", 2.0), ("b1_thr", -20.0), ("b1_ratio", 4.0), ("b1_range", 12.0), ("b2_thr", 0.0), ("b3_thr", 0.0), ("b4_thr", 0.0)], &quiet, &quiet, None);
        assert!(rms_of(&y[24000..]) > rms_of(&quiet[24000..]) * 2.0);
        // Per-band sidechain: band 1 ducks the bass when the sidechain's low band is loud (a kick
        // ducking the bass), and does nothing on a quiet sidechain.
        let set = [("b1_sc", 1.0), ("b1_thr", -30.0), ("b1_ratio", 20.0), ("b1_range", 30.0), ("b2_thr", 0.0), ("b3_thr", 0.0)];
        let kick = sine(60.0, 0.8, n);
        let (y, _, _) = run_fx(multiband_def(), &set, &quiet, &quiet, Some((&kick, &kick)));
        assert!(rms_of(&y[24000..]) < rms_of(&quiet[24000..]) * 0.3, "ducked by the sidechain");
        let silent = vec![0.0f32; n];
        let (y, _, _) = run_fx(multiband_def(), &set, &quiet, &quiet, Some((&silent, &silent)));
        assert!(rms_of(&y[24000..]) > rms_of(&quiet[24000..]) * 0.9, "left alone");
        // Dynamic phase: idle bands pass the signal untouched (bit-exact, no phase shift)…
        let idle_dp = [("phase", 1.0), ("b1_thr", 0.0), ("b2_thr", 0.0), ("b3_thr", 0.0), ("b4_thr", 0.0)];
        let (y, _, _) = run_fx(multiband_def(), &idle_dp, &mix, &mix, None);
        assert!(y.iter().zip(&mix).all(|(a, b)| (a - b).abs() < 1e-6), "dynamic phase idle = untouched");
        // …and a compressing low band still pulls only the low tone down.
        let low = [("phase", 1.0), ("b1_thr", -30.0), ("b1_ratio", 20.0), ("b1_range", 30.0), ("b2_thr", 0.0), ("b3_thr", 0.0)];
        let (y, _, _) = run_fx(multiband_def(), &low, &lo, &lo, None);
        assert!(rms_of(&y[24000..]) < rms_of(&lo[24000..]) * 0.5, "low band compressed");
        let (y, _, _) = run_fx(multiband_def(), &low, &hi, &hi, None);
        assert!(rms_of(&y[24000..]) > rms_of(&hi[24000..]) * 0.9, "high band untouched");
    }

    /// The gate closes on quiet input down to the range, opens on loud; flip ducks instead; the external
    /// sidechain keys it; look-ahead reports latency; the expander ratio cuts less than a gate.
    #[test]
    fn gate_range_flip_sidechain_and_lookahead() {
        let n = 48000;
        let quiet = sine(440.0, 0.003, n);
        let loud = sine(440.0, 0.5, n);
        let (y, _, _) = run_fx(gate_def(), &[("threshold", -30.0), ("range", -40.0)], &quiet, &quiet, None);
        let cut = rms_of(&y[24000..]) / rms_of(&quiet[24000..]);
        assert!((cut - 0.01).abs() < 0.005, "closed gate sits at the range (-40 dB), got {cut}");
        let (y, _, _) = run_fx(gate_def(), &[("threshold", -30.0)], &loud, &loud, None);
        assert!(rms_of(&y[24000..]) > rms_of(&loud[24000..]) * 0.95);
        let (y, _, _) = run_fx(gate_def(), &[("threshold", -30.0), ("range", -40.0), ("flip", 1.0)], &loud, &loud, None);
        assert!(rms_of(&y[24000..]) < rms_of(&loud[24000..]) * 0.05, "flip ducks when open");
        let (y, _, _) = run_fx(gate_def(), &[("threshold", -30.0), ("sidechain", 1.0)], &quiet, &quiet, Some((&loud, &loud)));
        assert!(rms_of(&y[24000..]) > rms_of(&quiet[24000..]) * 0.95, "keyed open by the sidechain");
        let (y, _, _) = run_fx(gate_def(), &[("threshold", -30.0), ("range", -80.0), ("ratio", 2.0), ("knee", 0.0)], &quiet, &quiet, None);
        let soft = rms_of(&y[24000..]) / rms_of(&quiet[24000..]);
        assert!(soft > 0.02 && soft < 0.5, "2:1 expansion cuts gently, got {soft}");
        let mut g = (gate_def().create)(48000.0);
        g.set_param("lookahead", 5.0);
        assert_eq!(g.latency(), 240);
    }

    /// Pro-DS: sibilance is reduced, a low vowel is left alone, the range limits the cut, split band
    /// leaves the lows of a mixed signal untouched, and listen outputs the detection band.
    #[test]
    fn deesser_modes_range_and_split() {
        let n = 48000;
        let vowel = sine(300.0, 0.5, n);
        let ess = sine(7000.0, 0.5, n);
        let (y, _, gr) = run_fx(deesser_def(), &[("mode", 1.0), ("threshold", -30.0), ("range", 12.0)], &ess, &ess, None);
        assert!(gr > 6.0 && gr <= 12.01, "range caps the reduction, got {gr}");
        assert!(rms_of(&y[24000..]) < rms_of(&ess[24000..]) * 0.6);
        let (y, _, _) = run_fx(deesser_def(), &[("threshold", -30.0)], &vowel, &vowel, None);
        assert!(rms_of(&y[24000..]) > rms_of(&vowel[24000..]) * 0.97, "vowels pass");
        let both: Vec<f32> = vowel.iter().zip(&ess).map(|(a, b)| a + b).collect();
        let (split, _, _) = run_fx(deesser_def(), &[("mode", 1.0), ("threshold", -40.0), ("range", 24.0)], &both, &both, None);
        let (wide, _, _) = run_fx(deesser_def(), &[("mode", 1.0), ("threshold", -40.0), ("range", 24.0), ("processing", 1.0)], &both, &both, None);
        assert!(rms_of(&split[24000..]) > rms_of(&wide[24000..]) * 1.3, "split band keeps the vowel, wide band ducks it all");
        let (l, _, _) = run_fx(deesser_def(), &[("listen", 1.0)], &vowel, &vowel, None);
        assert!(rms_of(&l[24000..]) < 0.05, "listen plays only the sibilance band");
        let mut d = (deesser_def().create)(48000.0);
        d.set_param("lookahead", 10.0);
        assert_eq!(d.latency(), 480);
    }

    /// Attack +100 % lifts the first milliseconds of a hit, sustain −100 % ducks its tail; the guard keeps
    /// the boosted attack under full scale.
    #[test]
    fn transient_designer_shapes_hits() {
        let rate = 48000.0;
        // A drum-like hit every 0.5 s: instant attack, 300 ms exponential tail.
        let hit = |i: usize| {
            let t = (i % 24000) as f32 / rate;
            (t * 180.0 * std::f32::consts::TAU).sin() * 0.7 * (-t / 0.08).exp()
        };
        let x: Vec<f32> = (0..96000).map(hit).collect();
        let run = |params: &[(&str, f32)]| {
            let mut e = TransientShaper::new(rate);
            for (k, v) in params {
                e.set_param(k, *v);
            }
            let (mut l, mut r) = (x.clone(), x.clone());
            for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
                e.process(a, b, None);
            }
            l
        };
        let rms = |v: &[f32], a: usize, b: usize| (v[a..b].iter().map(|s| s * s).sum::<f32>() / (b - a) as f32).sqrt();
        let base = rms(&x, 24000, 24000 + 480);
        let atk = run(&[("attack", 100.0), ("guard", 0.0)]);
        assert!(rms(&atk, 24000, 24000 + 480) > base * 1.6, "attack boosted");
        let sus = run(&[("sustain", -100.0), ("sus_dur", 400.0)]);
        let (b2, s2) = (rms(&x, 24000 + 4800, 24000 + 9600), rms(&sus, 24000 + 4800, 24000 + 9600));
        assert!(s2 < b2 * 0.6, "tail ducked: {s2} vs {b2}");
        let guarded = run(&[("attack", 100.0), ("output", 12.0), ("guard", 1.0)]);
        assert!(guarded.iter().all(|v| v.abs() <= 1.0001));
    }

    #[test]
    fn limiter_holds_ceiling() {
        let mut p = BTreeMap::new();
        p.insert("gain".into(), 12.0);
        p.insert("ceiling".into(), -1.0);
        let mut e = crate::effects::create("limiter", 48000.0, &p).unwrap();
        let mut l: Vec<f32> = (0..48000).map(|i| (i as f32 * 0.05).sin() * 0.9).collect();
        let mut r = l.clone();
        e.process(&mut l, &mut r, None);
        let peak = l.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak <= db_to_gain(-1.0) + 1e-4, "{peak}");
        assert!(peak > db_to_gain(-3.0), "{peak}");
    }

    #[test]
    fn compressor_reduces_loud_signal() {
        let mut p = BTreeMap::new();
        p.insert("threshold".into(), -20.0);
        p.insert("ratio".into(), 10.0);
        p.insert("knee".into(), 0.0);
        let mut e = crate::effects::create("compressor", 48000.0, &p).unwrap();
        let mut l = vec![0.5f32; 48000];
        let mut r = l.clone();
        e.process(&mut l, &mut r, None);
        // −6 dB in, threshold −20, ratio 10 → −18.6 dB out.
        let out_db = gain_to_db(l[47999]);
        assert!((out_db + 18.6).abs() < 0.5, "{out_db}");
    }

    #[test]
    fn sidechain_ducks() {
        let mut p = BTreeMap::new();
        p.insert("sidechain".into(), 1.0);
        p.insert("threshold".into(), -30.0);
        p.insert("ratio".into(), 20.0);
        let mut e = crate::effects::create("compressor", 48000.0, &p).unwrap();
        let mut l = vec![0.1f32; 4800];
        let mut r = l.clone();
        let kick = vec![0.9f32; 4800];
        e.process(&mut l, &mut r, Some((&kick, &kick)));
        assert!(l[4799] < 0.05, "{}", l[4799]);
    }

    fn make(id: &str, set: &[(&str, f32)]) -> Box<dyn Effect> {
        let def = crate::effects::find(id).unwrap();
        let mut p = def.defaults();
        for (k, v) in set {
            p.insert((*k).into(), *v);
        }
        crate::effects::create(id, 48000.0, &p).unwrap()
    }

    /// Look-ahead: reported as latency, and the compressor is already reducing when the
    /// delayed hit arrives; without it the first samples of the hit pass at full level.
    #[test]
    fn compressor_lookahead_catches_the_attack() {
        let hit = |e: &mut Box<dyn Effect>| {
            let mut l: Vec<f32> = (0..9600).map(|i| if i < 4800 { 0.0 } else { 0.9 }).collect();
            let mut r = l.clone();
            e.process(&mut l, &mut r, None);
            l
        };
        let mut plain = make("compressor", &[("threshold", -30.0), ("ratio", 10.0), ("attack", 5.0)]);
        let mut look = make("compressor", &[("threshold", -30.0), ("ratio", 10.0), ("attack", 5.0), ("lookahead", 10.0)]);
        assert_eq!(plain.latency(), 0);
        assert_eq!(look.latency(), 480);
        let (a, b) = (hit(&mut plain), hit(&mut look));
        let first = |v: &[f32], from: usize| v[from..from + 48].iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(first(&a, 4800) > 0.8, "no look-ahead: the hit passes");
        assert!(first(&b, 4800 + 480) < first(&a, 4800) * 0.8, "look-ahead: {} vs {}", first(&b, 5280), first(&a, 4800));
    }

    /// The styles behave differently on the same material, and every style stays stable.
    #[test]
    fn compressor_styles_differ() {
        let mut outs = Vec::new();
        for s in 0..COMP_STYLES.len() {
            let mut e = make("compressor", &[("threshold", -24.0), ("ratio", 6.0), ("style", s as f32)]);
            let mut l: Vec<f32> = (0..48000).map(|i| ((i as f32 * 0.05).sin()) * if (i / 6000) % 2 == 0 { 0.9 } else { 0.1 }).collect();
            let mut r = l.clone();
            e.process(&mut l, &mut r, None);
            assert!(l.iter().all(|x| x.is_finite()));
            outs.push(l);
        }
        // Every pair of styles differs audibly somewhere (> 1 dB-ish on average).
        for a in 0..outs.len() {
            for b in a + 1..outs.len() {
                let diff: f32 = outs[a].iter().zip(&outs[b]).map(|(x, y)| (x - y).powi(2)).sum::<f32>();
                let base: f32 = outs[a].iter().map(|x| x * x).sum::<f32>();
                assert!(diff / base > 0.01, "{} and {} sound the same ({})", COMP_STYLES[a], COMP_STYLES[b], diff / base);
            }
        }
    }

    /// Hold keeps the reduction after the signal drops.
    #[test]
    fn compressor_hold_delays_release() {
        let run = |hold: f32| {
            let mut e = make("compressor", &[("threshold", -30.0), ("ratio", 10.0), ("release", 20.0), ("hold", hold)]);
            let mut l: Vec<f32> = (0..9600).map(|i| if i < 4800 { 0.9 } else { 0.05 }).collect();
            let mut r = l.clone();
            e.process(&mut l, &mut r, None);
            e.gain_reduction()
        };
        assert!(run(200.0) > run(0.0) + 3.0, "{} vs {}", run(200.0), run(0.0));
    }

    /// Every limiter style keeps the ceiling.
    #[test]
    fn limiter_styles_hold_the_ceiling() {
        for s in 0..LIMITER_STYLES.len() {
            let mut e = make("limiter", &[("gain", 18.0), ("ceiling", -1.0), ("style", s as f32), ("true_peak", 0.0)]);
            let mut rng = crate::dsp::Rng::new(3);
            let mut l: Vec<f32> = (0..48000).map(|_| rng.bipolar() * 0.5).collect();
            let mut r = l.clone();
            e.process(&mut l, &mut r, None);
            let peak = l.iter().chain(&r).fold(0.0f32, |m, x| m.max(x.abs()));
            assert!(peak <= db_to_gain(-1.0) + 1e-4, "{} peak {peak}", LIMITER_STYLES[s]);
        }
    }

    /// Attack shapes: Needle and Nail rise instantly (Nail falls more slowly), Blunt rises slowly;
    /// the guard's Clip and Limit keep the output at −0.1 dBFS, after the output control.
    #[test]
    fn transient_shapes_and_guard_follow_smack_attack() {
        assert_eq!(ts_shape(0, 0.0), 1.0);
        assert_eq!(ts_shape(1, 0.0), 1.0);
        assert!(ts_shape(2, 0.0) < 0.05 && ts_shape(2, 0.25) > 0.95, "blunt rises");
        assert!(ts_shape(1, 0.3) > ts_shape(0, 0.3), "nail falls more slowly than needle");
        assert!(ts_sus_shape(1, 0.5, 1.0) < ts_sus_shape(1, 0.5, 0.2), "non-linear follows the amount");
        for guard in [1.0, 2.0] {
            let mut e = make("transient", &[("attack", 100.0), ("output", 12.0), ("guard", guard)]);
            let mut l: Vec<f32> = (0..48000).map(|i| if i % 12000 < 2400 { ((i as f32) * 0.3).sin() * 0.8 } else { 0.0 }).collect();
            let mut r = l.clone();
            e.process(&mut l, &mut r, None);
            let peak = l.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            assert!(peak <= db_to_gain(-0.1) + 1e-5, "guard {guard}: {peak}");
        }
    }

    /// Oversampling: the latency grows by the filters, the ceiling holds, and inter-sample peaks (measured
    /// on a 4× upsampled copy) come out lower than without it.
    #[test]
    fn limiter_oversampling_catches_inter_sample_peaks() {
        let tp = |y: &[f32]| {
            let mut o = crate::dsp::oversample::Oversampler::new(4);
            let mut b = [0.0f32; 4];
            y.iter().fold(0.0f32, |m, v| {
                o.up(*v, &mut b);
                m.max(b.iter().fold(0.0f32, |a, x| a.max(x.abs())))
            })
        };
        let mut peaks = Vec::new();
        for os in [0.0, 2.0] {
            let mut e = make("limiter", &[("gain", 12.0), ("ceiling", -1.0), ("true_peak", 0.0), ("oversampling", os)]);
            if os > 0.0 {
                assert!(e.latency() > make("limiter", &[]).latency());
            }
            // Bright material: a high tone and noise have peaks between samples.
            let mut rng = crate::dsp::Rng::new(5);
            let mut l: Vec<f32> = (0..48000).map(|i| (i as f32 * 2.7).sin() * 0.4 + rng.bipolar() * 0.3).collect();
            let mut r = l.clone();
            for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
                e.process(a, b, None);
            }
            let sample_peak = l.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            assert!(sample_peak <= db_to_gain(-1.0) + 1e-4, "{sample_peak}");
            peaks.push(tp(&l[4800..]));
        }
        assert!(peaks[1] < peaks[0], "inter-sample peaks: {peaks:?}");
    }
}
