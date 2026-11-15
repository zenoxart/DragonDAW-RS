//! Dynamics: compressor (with sidechain), 3-band multiband compressor, gate, true-peak limiter,
//! de-esser and transient shaper.

use super::{Effect, EffectDef, ParamInfo};
use crate::dsp::{db_to_gain, gain_to_db, Biquad, BiquadKind, Follower};

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
}

/// Compressor styles after FabFilter Pro-C: detector, knee, release behaviour and feedback differ.
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
            let peak = dl.abs().max(dr.abs());
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
        name: "Compressor",
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
    #[inline]
    fn split(&mut self, x: f32) -> (f32, f32) {
        let a = self.lp[0].process(x);
        let lo = self.lp[1].process(a);
        let b = self.hp[0].process(x);
        let hi = self.hp[1].process(b);
        (lo, hi)
    }
}

pub struct Multiband {
    rate: f32,
    x1: [Crossover; 2],
    x2: [Crossover; 2],
    // Keep the low band phase aligned with the others: all-pass the low band through the second crossover.
    ap: [Crossover; 2],
    bands: [CompCore; 3],
    gains: [f32; 3],
    f1: f32,
    f2: f32,
}

const MB_PARAMS: &[ParamInfo] = &[
    ParamInfo::log("low_mid", "Low/Mid", 60.0, 1000.0, 200.0, "Hz"),
    ParamInfo::log("mid_high", "Mid/High", 1000.0, 12000.0, 3000.0, "Hz"),
    ParamInfo::lin("low_threshold", "Low threshold", -60.0, 0.0, -20.0, "dB"),
    ParamInfo::log("low_ratio", "Low ratio", 1.0, 20.0, 3.0, ":1"),
    ParamInfo::lin("low_gain", "Low gain", -18.0, 18.0, 0.0, "dB"),
    ParamInfo::lin("mid_threshold", "Mid threshold", -60.0, 0.0, -20.0, "dB"),
    ParamInfo::log("mid_ratio", "Mid ratio", 1.0, 20.0, 3.0, ":1"),
    ParamInfo::lin("mid_gain", "Mid gain", -18.0, 18.0, 0.0, "dB"),
    ParamInfo::lin("high_threshold", "High threshold", -60.0, 0.0, -20.0, "dB"),
    ParamInfo::log("high_ratio", "High ratio", 1.0, 20.0, 3.0, ":1"),
    ParamInfo::lin("high_gain", "High gain", -18.0, 18.0, 0.0, "dB"),
    ParamInfo::log("attack", "Attack", 0.1, 200.0, 15.0, "ms"),
    ParamInfo::log("release", "Release", 10.0, 2000.0, 150.0, "ms"),
];

impl Multiband {
    fn rebuild(&mut self) {
        self.x1 = [Crossover::new(self.f1, self.rate); 2];
        self.x2 = [Crossover::new(self.f2, self.rate); 2];
        self.ap = [Crossover::new(self.f2, self.rate); 2];
    }
}

impl Effect for Multiband {
    fn set_param(&mut self, id: &str, v: f32) {
        let band = |s: &str| match s {
            "low" => Some(0),
            "mid" => Some(1),
            "high" => Some(2),
            _ => None,
        };
        match id {
            "low_mid" => {
                self.f1 = v.min(self.f2 * 0.8);
                self.rebuild();
            }
            "mid_high" => {
                self.f2 = v.max(self.f1 * 1.25);
                self.rebuild();
            }
            "attack" => self.bands.iter_mut().for_each(|b| b.attack = v),
            "release" => self.bands.iter_mut().for_each(|b| b.release = v),
            _ => {
                if let Some((b, k)) = id.split_once('_') {
                    if let Some(i) = band(b) {
                        match k {
                            "threshold" => self.bands[i].threshold = v,
                            "ratio" => self.bands[i].ratio = v,
                            "gain" => self.gains[i] = db_to_gain(v),
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    fn reset(&mut self) {
        self.rebuild();
        self.bands.iter_mut().for_each(|b| b.gr = 0.0);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        for i in 0..l.len() {
            let mut out = [0.0f32; 2];
            let mut parts = [[0.0f32; 3]; 2];
            for (ch, x) in [l[i], r[i]].into_iter().enumerate() {
                let (lo, rest) = self.x1[ch].split(x);
                let (mid, hi) = self.x2[ch].split(rest);
                let (a, b) = self.ap[ch].split(lo);
                parts[ch] = [a + b, mid, hi];
            }
            for (bi, band) in self.bands.iter_mut().enumerate() {
                let level = parts[0][bi].abs().max(parts[1][bi].abs());
                let g = band.gain(level) * self.gains[bi];
                out[0] += parts[0][bi] * g;
                out[1] += parts[1][bi] * g;
            }
            l[i] = out[0];
            r[i] = out[1];
        }
    }
    fn gain_reduction(&self) -> f32 {
        self.bands.iter().map(|b| b.gr).fold(0.0, f32::max)
    }
}

pub fn multiband_def() -> EffectDef {
    EffectDef {
        id: "multiband",
        name: "Multiband Compressor",
        category: "Dynamics",
        description: "3-band compressor with Linkwitz–Riley crossovers.",
        params: MB_PARAMS,
        uses_sidechain: false,
        create: |rate| {
            let mut m = Multiband {
                rate,
                x1: [Crossover::new(200.0, rate); 2],
                x2: [Crossover::new(3000.0, rate); 2],
                ap: [Crossover::new(3000.0, rate); 2],
                bands: [CompCore::new(rate), CompCore::new(rate), CompCore::new(rate)],
                gains: [1.0; 3],
                f1: 200.0,
                f2: 3000.0,
            };
            m.rebuild();
            Box::new(m)
        },
    }
}

// ------------------------------------------------------------------------------------------------ gate

pub struct Gate {
    rate: f32,
    threshold: f32,
    range: f32,
    attack: f32,
    hold: f32,
    release: f32,
    follower: Follower,
    gain: f32,
    hold_left: f32,
    use_sidechain: bool,
}

const GATE_PARAMS: &[ParamInfo] = &[
    ParamInfo::lin("threshold", "Threshold", -80.0, 0.0, -40.0, "dB"),
    ParamInfo::lin("range", "Range", -80.0, 0.0, -80.0, "dB"),
    ParamInfo::log("attack", "Attack", 0.05, 50.0, 0.5, "ms"),
    ParamInfo::log("hold", "Hold", 1.0, 500.0, 20.0, "ms"),
    ParamInfo::log("release", "Release", 5.0, 2000.0, 100.0, "ms"),
    ParamInfo::toggle("sidechain", "External sidechain", false),
];

impl Effect for Gate {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "threshold" => self.threshold = v,
            "range" => self.range = v,
            "attack" => self.attack = v,
            "hold" => self.hold = v,
            "release" => self.release = v,
            "sidechain" => self.use_sidechain = v >= 0.5,
            _ => {}
        }
    }
    fn reset(&mut self) {
        self.gain = 1.0;
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], sc: Option<(&[f32], &[f32])>) {
        let floor = db_to_gain(self.range);
        let thr = db_to_gain(self.threshold);
        let att = 1.0 - (-1.0 / (self.attack * 0.001 * self.rate)).exp();
        let rel = 1.0 - (-1.0 / (self.release * 0.001 * self.rate)).exp();
        let ext = if self.use_sidechain { sc } else { None };
        for i in 0..l.len() {
            let d = match ext {
                Some((a, b)) => a[i].abs().max(b[i].abs()),
                None => l[i].abs().max(r[i].abs()),
            };
            let level = self.follower.next(d);
            let open = if level > thr {
                self.hold_left = self.hold * 0.001 * self.rate;
                true
            } else if self.hold_left > 0.0 {
                self.hold_left -= 1.0;
                true
            } else {
                false
            };
            let target = if open { 1.0 } else { floor };
            self.gain += (target - self.gain) * if target > self.gain { att } else { rel };
            l[i] *= self.gain;
            r[i] *= self.gain;
        }
    }
    fn gain_reduction(&self) -> f32 {
        -gain_to_db(self.gain)
    }
}

pub fn gate_def() -> EffectDef {
    EffectDef {
        id: "gate",
        name: "Gate",
        category: "Dynamics",
        description: "Noise gate / expander with hold, range and external sidechain (ducking, gated effects).",
        params: GATE_PARAMS,
        uses_sidechain: true,
        create: |rate| Box::new(Gate { rate, threshold: -40.0, range: -80.0, attack: 0.5, hold: 20.0, release: 100.0, follower: Follower::new(0.1, 30.0, rate), gain: 1.0, hold_left: 0.0, use_sidechain: false }),
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
];

/// Limiter styles after FabFilter Pro-L: how fast the gain recovers, how hard the attack is smoothed and
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
                // Soft clipper before the limiter (takes the sharpest peaks, Pro-L "Aggressive"/"Modern").
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

pub fn limiter_def() -> EffectDef {
    EffectDef {
        id: "limiter",
        name: "Limiter",
        category: "Dynamics",
        description: "Look-ahead brickwall limiter with true-peak detection and eight styles (Allround, Transparent, Punchy, Dynamic, Aggressive, Modern, Bus, Safe) for mastering.",
        params: LIMITER_PARAMS,
        uses_sidechain: false,
        create: |rate| {
            let n = ((LOOKAHEAD_MS * 0.001 * rate) as usize).max(8);
            Box::new(Limiter {
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
            })
        },
    }
}

// ------------------------------------------------------------------------------------------------ de-esser

pub struct DeEsser {
    rate: f32,
    freq: f32,
    det: [Biquad; 2],
    core: CompCore,
    listen: bool,
}

const DEESS_PARAMS: &[ParamInfo] = &[
    ParamInfo::log("freq", "Frequency", 2000.0, 12000.0, 6500.0, "Hz"),
    ParamInfo::lin("threshold", "Threshold", -60.0, 0.0, -30.0, "dB"),
    ParamInfo::log("ratio", "Ratio", 1.0, 20.0, 6.0, ":1"),
    ParamInfo::toggle("listen", "Listen", false),
];

impl Effect for DeEsser {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "freq" => {
                self.freq = v;
                for b in &mut self.det {
                    b.set(BiquadKind::HighPass, v, 0.9, 0.0, self.rate);
                }
            }
            "threshold" => self.core.threshold = v,
            "ratio" => self.core.ratio = v,
            "listen" => self.listen = v >= 0.5,
            _ => {}
        }
    }
    fn reset(&mut self) {
        self.det.iter_mut().for_each(Biquad::reset);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        for i in 0..l.len() {
            let (a, b) = (self.det[0].process(l[i]), self.det[1].process(r[i]));
            let g = self.core.gain(a.abs().max(b.abs()));
            if self.listen {
                l[i] = a;
                r[i] = b;
            } else {
                // Reduce only the sibilant band: x − hp + hp·g.
                l[i] += a * (g - 1.0);
                r[i] += b * (g - 1.0);
            }
        }
    }
    fn gain_reduction(&self) -> f32 {
        self.core.gr
    }
}

pub fn deesser_def() -> EffectDef {
    EffectDef {
        id: "deesser",
        name: "De-esser",
        category: "Dynamics",
        description: "Split-band de-esser that compresses only the sibilant range.",
        params: DEESS_PARAMS,
        uses_sidechain: false,
        create: |rate| {
            let mut core = CompCore::new(rate);
            core.attack = 0.5;
            core.release = 60.0;
            core.knee = 3.0;
            Box::new(DeEsser { rate, freq: 6500.0, det: [Biquad::new(BiquadKind::HighPass, 6500.0, 0.9, 0.0, rate); 2], core, listen: false })
        },
    }
}

// ------------------------------------------------------------------------------------------------ transient shaper

/// Attack shapes (after Waves Smack Attack's manual): Needle – very sharp rise and fall; Nail – sharp rise,
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

/// Guard modes (Smack Attack): Off, Clip (hard clip, output never above −0.1 dBFS), Limit (fast limiter to
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
        name: "Transient Shaper",
        category: "Dynamics",
        description: "Transient designer after Waves Smack Attack: attack (Needle / Nail / Blunt) and sustain (Linear / Non-linear / Soft linear) sections with level, sensitivity, duration and shape, mix, output, and a guard (Off / Clip / Limit / Soft clip) after the output.",
        params: TS_PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(TransientShaper::new(rate)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

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

    /// Pro-C-style look-ahead: reported as latency, and the compressor is already reducing when the
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

    /// Smack Attack's manual: Needle and Nail rise instantly (Nail falls more slowly), Blunt rises slowly;
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
}
