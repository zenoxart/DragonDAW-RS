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
}

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
            _ => {}
        }
    }
    fn reset(&mut self) {
        self.core.gr = 0.0;
        self.sc_hp.iter_mut().for_each(Biquad::reset);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], sc: Option<(&[f32], &[f32])>) {
        let auto = if self.auto_makeup { -self.core.static_curve(0.0) * 0.5 } else { 0.0 };
        let makeup = db_to_gain(self.makeup + auto);
        let ext = if self.use_sidechain { sc } else { None };
        for i in 0..l.len() {
            let (dl, dr) = match ext {
                Some((a, b)) => (a[i], b[i]),
                None => (l[i], r[i]),
            };
            let (dl, dr) = if self.sc_hp_freq > 21.0 { (self.sc_hp[0].process(dl), self.sc_hp[1].process(dr)) } else { (dl, dr) };
            let g = self.core.gain(dl.abs().max(dr.abs()));
            let w = g * makeup;
            l[i] = l[i] * (1.0 - self.mix) + l[i] * w * self.mix;
            r[i] = r[i] * (1.0 - self.mix) + r[i] * w * self.mix;
        }
    }
    fn gain_reduction(&self) -> f32 {
        self.core.gr
    }
}

pub fn compressor_def() -> EffectDef {
    EffectDef {
        id: "compressor",
        name: "Compressor",
        category: "Dynamics",
        description: "Soft-knee compressor with external sidechain, sidechain high-pass, auto makeup and parallel mix.",
        params: COMP_PARAMS,
        uses_sidechain: true,
        create: |rate| {
            Box::new(Compressor { core: CompCore::new(rate), makeup: 0.0, mix: 1.0, use_sidechain: false, sc_hp: [Biquad::new(BiquadKind::HighPass, 20.0, 0.707, 0.0, rate); 2], sc_hp_freq: 20.0, auto_makeup: false })
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
}

const LIMITER_PARAMS: &[ParamInfo] = &[
    ParamInfo::lin("gain", "Input gain", 0.0, 24.0, 0.0, "dB"),
    ParamInfo::lin("ceiling", "Ceiling", -12.0, 0.0, -1.0, "dBTP"),
    ParamInfo::log("release", "Release", 1.0, 1000.0, 80.0, "ms"),
    ParamInfo::toggle("true_peak", "True peak", true),
];

const LOOKAHEAD_MS: f32 = 3.0;

impl Effect for Limiter {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "gain" => self.gain_in = db_to_gain(v),
            "ceiling" => self.ceiling = db_to_gain(v),
            "release" => self.release = v,
            "true_peak" => self.true_peak = v >= 0.5,
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
        let rel = (-1.0 / (self.release * 0.001 * self.rate)).exp();
        let att = (-5.0 / n as f32).exp();
        let margin = if self.true_peak { 0.995 } else { 1.0 };
        for i in 0..l.len() {
            let (a, b) = (l[i] * self.gain_in, r[i] * self.gain_in);
            let p = if self.true_peak { self.tp.process(a, b).max(a.abs()).max(b.abs()) } else { a.abs().max(b.abs()) };
            self.buf_l[self.pos] = a;
            self.buf_r[self.pos] = b;
            self.peaks[self.pos] = p;
            self.pos = (self.pos + 1) % n;
            // Peak over the window = sample that will leave plus everything after it.
            let win_peak = self.peaks.iter().fold(0.0f32, |m, &x| m.max(x));
            let target = (self.ceiling * margin / win_peak.max(1e-9)).min(1.0);
            self.gain = if target < self.gain { target + (self.gain - target) * att } else { target + (self.gain - target) * rel };
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
        description: "Look-ahead brickwall limiter with true-peak detection for mastering.",
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

pub struct TransientShaper {
    fast: Follower,
    slow: Follower,
    attack: f32,
    sustain: f32,
    output: f32,
}

const TS_PARAMS: &[ParamInfo] = &[ParamInfo::lin("attack", "Attack", -100.0, 100.0, 0.0, "%"), ParamInfo::lin("sustain", "Sustain", -100.0, 100.0, 0.0, "%"), ParamInfo::lin("output", "Output", -18.0, 18.0, 0.0, "dB")];

impl Effect for TransientShaper {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "attack" => self.attack = v / 100.0,
            "sustain" => self.sustain = v / 100.0,
            "output" => self.output = db_to_gain(v),
            _ => {}
        }
    }
    fn reset(&mut self) {}
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        for i in 0..l.len() {
            let x = l[i].abs().max(r[i].abs());
            let f = self.fast.next(x);
            let s = self.slow.next(x);
            let diff = gain_to_db(f.max(1e-6)) - gain_to_db(s.max(1e-6)); // > 0 on transients
            let g_db = if diff > 0.0 { diff.min(24.0) * self.attack } else { (-diff).min(24.0) * self.sustain };
            let g = db_to_gain(g_db.clamp(-24.0, 24.0)) * self.output;
            l[i] *= g;
            r[i] *= g;
        }
    }
}

pub fn transient_def() -> EffectDef {
    EffectDef {
        id: "transient",
        name: "Transient Shaper",
        category: "Dynamics",
        description: "Boosts or softens attacks and sustain independently of level.",
        params: TS_PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(TransientShaper { fast: Follower::new(0.5, 20.0, rate), slow: Follower::new(20.0, 200.0, rate), attack: 0.0, sustain: 0.0, output: 1.0 }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

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
}
