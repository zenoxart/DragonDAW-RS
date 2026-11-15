//! Colour and utility: saturator, bit crusher, filter, utility (gain/pan/phase/width/mono), stereo imager
//! and the Dragon Particle one-knob mastering chain.

use super::dynamics;
use super::{Effect, EffectDef, ParamInfo};
use crate::dsp::{balance, db_to_gain, Biquad, BiquadKind, Follower, Lfo, LfoShape, OnePole, Svf};

// ------------------------------------------------------------------------------------------------ saturator

const SAT_PARAMS: &[ParamInfo] = &[
    ParamInfo::choice("mode", "Mode", &["Tube", "Tape", "Soft clip", "Hard clip", "Foldback", "Asymmetric"], 0.0),
    ParamInfo::lin("drive", "Drive", 0.0, 36.0, 6.0, "dB"),
    ParamInfo::log("tone", "Tone", 1000.0, 20000.0, 12000.0, "Hz"),
    ParamInfo::lin("output", "Output", -24.0, 12.0, -3.0, "dB"),
    ParamInfo::lin("mix", "Mix", 0.0, 1.0, 1.0, ""),
];

pub struct Saturator {
    rate: f32,
    mode: usize,
    drive: f32,
    out: f32,
    mix: f32,
    tone: [OnePole; 2],
    dc: [OnePole; 2],
}

#[inline]
/// The saturator's transfer curve for `mode` (also drawn by the editor).
pub fn shape(mode: usize, x: f32) -> f32 {
    match mode {
        0 => {
            // Tube: asymmetric soft saturation (even harmonics).
            let y = (x + 0.15 * x * x).tanh();
            y - 0.15f32.tanh() * 0.0
        }
        1 => (x * 0.9).tanh() * 1.05 + 0.02 * (x * 3.0).sin() * x.abs().min(1.0),
        2 => x / (1.0 + x.abs()),
        3 => x.clamp(-1.0, 1.0),
        4 => {
            let mut y = x;
            for _ in 0..4 {
                if y > 1.0 {
                    y = 2.0 - y;
                } else if y < -1.0 {
                    y = -2.0 - y;
                }
            }
            y
        }
        _ => {
            if x >= 0.0 {
                x.tanh()
            } else {
                (x * 0.6).tanh() / 0.6 * 0.8
            }
        }
    }
}

impl Effect for Saturator {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "mode" => self.mode = v.round().clamp(0.0, 5.0) as usize,
            "drive" => self.drive = db_to_gain(v),
            "tone" => self.tone.iter_mut().for_each(|f| f.set(v, self.rate)),
            "output" => self.out = db_to_gain(v),
            "mix" => self.mix = v,
            _ => {}
        }
    }
    fn reset(&mut self) {
        self.tone.iter_mut().chain(self.dc.iter_mut()).for_each(OnePole::reset);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        for i in 0..l.len() {
            for (ch, x) in [&mut l[i], &mut r[i]].into_iter().enumerate() {
                let y = shape(self.mode, *x * self.drive);
                let y = self.dc[ch].highpass(self.tone[ch].process(y)) * self.out;
                *x = *x * (1.0 - self.mix) + y * self.mix;
            }
        }
    }
}

pub fn saturator_def() -> EffectDef {
    EffectDef {
        id: "saturator",
        name: "Saturator",
        category: "Distortion",
        description: "Tube, tape, soft/hard clip, foldback and asymmetric waveshaping with tone and parallel mix.",
        params: SAT_PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(Saturator { rate, mode: 0, drive: 2.0, out: 0.7, mix: 1.0, tone: [OnePole::new(12000.0, rate); 2], dc: [OnePole::new(10.0, rate); 2] }),
    }
}

// ------------------------------------------------------------------------------------------------ bit crusher

const CRUSH_PARAMS: &[ParamInfo] = &[ParamInfo::lin("bits", "Bits", 2.0, 16.0, 8.0, "bit"), ParamInfo::log("downsample", "Downsample", 1.0, 64.0, 4.0, "×"), ParamInfo::lin("mix", "Mix", 0.0, 1.0, 1.0, "")];

pub struct BitCrusher {
    bits: f32,
    down: f32,
    mix: f32,
    phase: f32,
    held: [f32; 2],
}

impl Effect for BitCrusher {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "bits" => self.bits = v,
            "downsample" => self.down = v.max(1.0),
            "mix" => self.mix = v,
            _ => {}
        }
    }
    fn reset(&mut self) {}
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let steps = 2f32.powf(self.bits - 1.0);
        for i in 0..l.len() {
            self.phase += 1.0;
            if self.phase >= self.down {
                self.phase -= self.down;
                self.held = [(l[i] * steps).round() / steps, (r[i] * steps).round() / steps];
            }
            l[i] = l[i] * (1.0 - self.mix) + self.held[0] * self.mix;
            r[i] = r[i] * (1.0 - self.mix) + self.held[1] * self.mix;
        }
    }
}

pub fn bitcrusher_def() -> EffectDef {
    EffectDef {
        id: "bitcrusher",
        name: "Bit Crusher",
        category: "Distortion",
        description: "Bit depth and sample-rate reduction.",
        params: CRUSH_PARAMS,
        uses_sidechain: false,
        create: |_| Box::new(BitCrusher { bits: 8.0, down: 4.0, mix: 1.0, phase: 0.0, held: [0.0; 2] }),
    }
}

// ------------------------------------------------------------------------------------------------ filter

const FILTER_PARAMS: &[ParamInfo] = &[
    ParamInfo::choice("type", "Type", &["Low pass", "High pass", "Band pass"], 0.0),
    ParamInfo::log("cutoff", "Cutoff", 20.0, 20000.0, 2000.0, "Hz"),
    ParamInfo::lin("resonance", "Resonance", 0.0, 0.98, 0.3, ""),
    ParamInfo::log("lfo_rate", "LFO rate", 0.05, 20.0, 1.0, "Hz"),
    ParamInfo::lin("lfo_depth", "LFO depth", 0.0, 4.0, 0.0, "oct"),
    ParamInfo::lin("env_depth", "Envelope depth", -4.0, 4.0, 0.0, "oct"),
    ParamInfo::lin("mix", "Mix", 0.0, 1.0, 1.0, ""),
];

pub struct Filter {
    rate: f32,
    kind: usize,
    cutoff: f32,
    res: f32,
    lfo_rate: f32,
    lfo_depth: f32,
    env_depth: f32,
    mix: f32,
    svf: [Svf; 2],
    lfo: Lfo,
    env: Follower,
}

impl Effect for Filter {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "type" => self.kind = v.round().clamp(0.0, 2.0) as usize,
            "cutoff" => self.cutoff = v,
            "resonance" => self.res = v,
            "lfo_rate" => self.lfo_rate = v,
            "lfo_depth" => self.lfo_depth = v,
            "env_depth" => self.env_depth = v,
            "mix" => self.mix = v,
            _ => {}
        }
    }
    fn reset(&mut self) {
        self.svf = [Svf::default(); 2];
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        for i in 0..l.len() {
            let m = self.lfo.next(self.lfo_rate, self.rate, LfoShape::Sine) * self.lfo_depth;
            let e = self.env.next(l[i].abs().max(r[i].abs())).min(1.0) * self.env_depth;
            let f = self.cutoff * 2f32.powf(m + e);
            for (ch, x) in [&mut l[i], &mut r[i]].into_iter().enumerate() {
                let (lp, bp, hp) = self.svf[ch].process(*x, f, self.res, self.rate);
                let y = [lp, hp, bp][self.kind];
                *x = *x * (1.0 - self.mix) + y * self.mix;
            }
        }
    }
}

pub fn filter_def() -> EffectDef {
    EffectDef {
        id: "filter",
        name: "Filter",
        category: "Filter",
        description: "Resonant state-variable filter with LFO and envelope follower (auto-wah, sweeps).",
        params: FILTER_PARAMS,
        uses_sidechain: false,
        create: |rate| {
            Box::new(Filter { rate, kind: 0, cutoff: 2000.0, res: 0.3, lfo_rate: 1.0, lfo_depth: 0.0, env_depth: 0.0, mix: 1.0, svf: [Svf::default(); 2], lfo: Lfo::default(), env: Follower::new(5.0, 200.0, rate) })
        },
    }
}

// ------------------------------------------------------------------------------------------------ utility

const UTIL_PARAMS: &[ParamInfo] = &[
    ParamInfo::lin("gain", "Gain", -36.0, 36.0, 0.0, "dB"),
    ParamInfo::lin("pan", "Balance", -1.0, 1.0, 0.0, ""),
    ParamInfo::lin("width", "Width", 0.0, 2.0, 1.0, ""),
    ParamInfo::toggle("invert_l", "Invert left", false),
    ParamInfo::toggle("invert_r", "Invert right", false),
    ParamInfo::toggle("swap", "Swap L/R", false),
    ParamInfo::toggle("mono", "Mono", false),
    ParamInfo::toggle("dc", "DC filter", false),
];

pub struct Utility {
    gain: f32,
    pan: f32,
    width: f32,
    inv: [bool; 2],
    swap: bool,
    mono: bool,
    dc: bool,
    dcf: [OnePole; 2],
}

impl Effect for Utility {
    fn set_param(&mut self, id: &str, v: f32) {
        let on = v >= 0.5;
        match id {
            "gain" => self.gain = db_to_gain(v),
            "pan" => self.pan = v,
            "width" => self.width = v,
            "invert_l" => self.inv[0] = on,
            "invert_r" => self.inv[1] = on,
            "swap" => self.swap = on,
            "mono" => self.mono = on,
            "dc" => self.dc = on,
            _ => {}
        }
    }
    fn reset(&mut self) {}
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let (gl, gr) = balance(self.pan);
        for i in 0..l.len() {
            let (mut a, mut b) = (l[i], r[i]);
            if self.swap {
                std::mem::swap(&mut a, &mut b);
            }
            if self.inv[0] {
                a = -a;
            }
            if self.inv[1] {
                b = -b;
            }
            let mid = (a + b) * 0.5;
            let side = (a - b) * 0.5 * if self.mono { 0.0 } else { self.width };
            a = mid + side;
            b = mid - side;
            if self.dc {
                a = self.dcf[0].highpass(a);
                b = self.dcf[1].highpass(b);
            }
            l[i] = a * self.gain * gl;
            r[i] = b * self.gain * gr;
        }
    }
}

pub fn utility_def() -> EffectDef {
    EffectDef {
        id: "utility",
        name: "Utility",
        category: "Utility",
        description: "Gain, balance, stereo width, polarity per side, swap, mono and DC filter.",
        params: UTIL_PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(Utility { gain: 1.0, pan: 0.0, width: 1.0, inv: [false; 2], swap: false, mono: false, dc: false, dcf: [OnePole::new(8.0, rate); 2] }),
    }
}

// ------------------------------------------------------------------------------------------------ imager

const IMAGER_PARAMS: &[ParamInfo] =
    &[ParamInfo::lin("width", "Width", 0.0, 2.0, 1.3, ""), ParamInfo::log("mono_below", "Mono below", 20.0, 500.0, 120.0, "Hz"), ParamInfo::lin("output", "Output", -12.0, 12.0, 0.0, "dB")];

/// Points kept for the goniometer.
pub const SCOPE_POINTS: usize = 192;

/// A phase / stereo scope for displays: running correlation (−1 … +1), balance and the side/mid ratio,
/// plus the most recent output samples for a goniometer. Layout of [`PhaseScope::data`]:
/// `[correlation, balance, side/mid, l0, r0, l1, r1, …]`.
pub struct PhaseScope {
    lr: f32,
    ll: f32,
    rr: f32,
    mm: f32,
    ss: f32,
    coef: f32,
    decim: usize,
    k: usize,
    w: usize,
    pub data: Vec<f32>,
}

impl PhaseScope {
    pub fn new(rate: f32) -> Self {
        PhaseScope { lr: 0.0, ll: 0.0, rr: 0.0, mm: 0.0, ss: 0.0, coef: (-1.0 / (0.3 * rate)).exp(), decim: ((rate / 4000.0) as usize).max(1), k: 0, w: 0, data: vec![0.0; 3 + SCOPE_POINTS * 2] }
    }

    #[inline]
    pub fn push(&mut self, l: f32, r: f32) {
        let a = 1.0 - self.coef;
        self.lr = self.lr * self.coef + l * r * a;
        self.ll = self.ll * self.coef + l * l * a;
        self.rr = self.rr * self.coef + r * r * a;
        let (m, s) = ((l + r) * 0.5, (l - r) * 0.5);
        self.mm = self.mm * self.coef + m * m * a;
        self.ss = self.ss * self.coef + s * s * a;
        self.k += 1;
        if self.k >= self.decim {
            self.k = 0;
            self.data[3 + self.w * 2] = l;
            self.data[3 + self.w * 2 + 1] = r;
            self.w = (self.w + 1) % SCOPE_POINTS;
        }
    }

    /// Updates the summary values (call once per block).
    pub fn finish(&mut self) {
        let den = (self.ll * self.rr).sqrt();
        self.data[0] = if den > 1e-9 { (self.lr / den).clamp(-1.0, 1.0) } else { 1.0 };
        let tot = self.ll + self.rr;
        self.data[1] = if tot > 1e-12 { ((self.rr - self.ll) / tot).clamp(-1.0, 1.0) } else { 0.0 };
        self.data[2] = if self.mm > 1e-12 { (self.ss / self.mm).sqrt().min(4.0) } else { 0.0 };
    }
}

pub struct Imager {
    rate: f32,
    width: f32,
    out: f32,
    lp: [Biquad; 2],
    scope: PhaseScope,
}

impl Effect for Imager {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "width" => self.width = v,
            "mono_below" => self.lp = [Biquad::new(BiquadKind::LowPass, v, 0.707, 0.0, self.rate); 2],
            "output" => self.out = db_to_gain(v),
            _ => {}
        }
    }
    fn reset(&mut self) {
        self.lp.iter_mut().for_each(Biquad::reset);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        for i in 0..l.len() {
            let mid = (l[i] + r[i]) * 0.5;
            let side = (l[i] - r[i]) * 0.5;
            // Remove the low part of the side signal (bass stays mono), widen the rest.
            let low_side = self.lp[0].process(side);
            let side = (side - low_side) * self.width;
            l[i] = (mid + side) * self.out;
            r[i] = (mid - side) * self.out;
            self.scope.push(l[i], r[i]);
        }
        self.scope.finish();
    }
    fn telemetry(&self) -> &[f32] {
        &self.scope.data
    }
}

pub fn imager_def() -> EffectDef {
    EffectDef {
        id: "imager",
        name: "Stereo Imager",
        category: "Utility",
        description: "Widens the stereo image and keeps the bass mono; a goniometer and correlation meter show the phase relation of the result.",
        params: IMAGER_PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(Imager { rate, width: 1.3, out: 1.0, lp: [Biquad::new(BiquadKind::LowPass, 120.0, 0.707, 0.0, rate); 2], scope: PhaseScope::new(rate) }),
    }
}

// ------------------------------------------------------------------------------------------------ Dragon Particle

const DP_PARAMS: &[ParamInfo] = &[
    ParamInfo::lin("amount", "Amount", 0.0, 1.0, 0.4, ""),
    ParamInfo::lin("input", "Input", -12.0, 12.0, 0.0, "dB"),
    ParamInfo::lin("low", "Low", -6.0, 6.0, 0.0, "dB"),
    ParamInfo::lin("mid", "Mid", -6.0, 6.0, 0.0, "dB"),
    ParamInfo::lin("high", "High", -6.0, 6.0, 0.0, "dB"),
    ParamInfo::lin("ceiling", "Ceiling", -3.0, 0.0, -1.0, "dBTP"),
];

/// One knob drives a whole mastering chain: tone EQ → glue compressor → harmonic saturation → stereo widening
/// → true-peak limiter. "Transparent / Polish / Mastered / Loud / Heavy" as the amount rises.
pub struct DragonParticle {
    rate: f32,
    amount: f32,
    input: f32,
    low: f32,
    mid: f32,
    high: f32,
    ceiling: f32,
    eq: [[Biquad; 2]; 3],
    glue: Box<dyn Effect>,
    sat: Box<dyn Effect>,
    width: Box<dyn Effect>,
    limiter: Box<dyn Effect>,
}

impl DragonParticle {
    pub fn mode_name(amount: f32) -> &'static str {
        match amount {
            a if a < 0.15 => "Transparent",
            a if a < 0.35 => "Polish",
            a if a < 0.55 => "Mastered",
            a if a < 0.75 => "Loud",
            _ => "Heavy",
        }
    }

    fn update(&mut self) {
        let a = self.amount;
        let shape = [(BiquadKind::LowShelf, 90.0, self.low + a * 1.5), (BiquadKind::Peak, 1200.0, self.mid - a * 0.8), (BiquadKind::HighShelf, 9000.0, self.high + a * 2.0)];
        for (i, (k, f, g)) in shape.iter().enumerate() {
            for b in &mut self.eq[i] {
                b.set(*k, *f, 0.7, *g, self.rate);
            }
        }
        self.glue.set_param("threshold", -6.0 - a * 18.0);
        self.glue.set_param("ratio", 1.5 + a * 2.5);
        self.glue.set_param("attack", 30.0);
        self.glue.set_param("release", 200.0);
        self.glue.set_param("knee", 8.0);
        self.glue.set_param("makeup", a * 6.0);
        self.sat.set_param("mode", 0.0);
        self.sat.set_param("drive", a * 9.0);
        self.sat.set_param("output", -a * 6.0);
        self.sat.set_param("mix", 0.25 + a * 0.35);
        self.width.set_param("width", 1.0 + a * 0.35);
        self.width.set_param("mono_below", 120.0);
        self.limiter.set_param("gain", a * 8.0);
        self.limiter.set_param("ceiling", self.ceiling);
        self.limiter.set_param("release", 60.0 + (1.0 - a) * 120.0);
    }
}

impl Effect for DragonParticle {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "amount" => self.amount = v.clamp(0.0, 1.0),
            "input" => self.input = db_to_gain(v),
            "low" => self.low = v,
            "mid" => self.mid = v,
            "high" => self.high = v,
            "ceiling" => self.ceiling = v,
            _ => return,
        }
        self.update();
    }
    fn reset(&mut self) {
        self.glue.reset();
        self.sat.reset();
        self.width.reset();
        self.limiter.reset();
    }
    fn latency(&self) -> usize {
        self.limiter.latency()
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        for i in 0..l.len() {
            let (mut a, mut b) = (l[i] * self.input, r[i] * self.input);
            for band in &mut self.eq {
                a = band[0].process(a);
                b = band[1].process(b);
            }
            l[i] = a;
            r[i] = b;
        }
        self.glue.process(l, r, None);
        self.sat.process(l, r, None);
        self.width.process(l, r, None);
        self.limiter.process(l, r, None);
    }
    fn gain_reduction(&self) -> f32 {
        self.glue.gain_reduction() + self.limiter.gain_reduction()
    }
}

pub fn dragon_particle_def() -> EffectDef {
    EffectDef {
        id: "dragon_particle",
        name: "Dragon Particle",
        category: "Mastering",
        description: "One-knob mastering: tone EQ, glue compression, saturation, widening and a true-peak limiter.",
        params: DP_PARAMS,
        uses_sidechain: false,
        create: |rate| {
            let mut d = DragonParticle {
                rate,
                amount: 0.4,
                input: 1.0,
                low: 0.0,
                mid: 0.0,
                high: 0.0,
                ceiling: -1.0,
                eq: [[Biquad::default(); 2]; 3],
                glue: (dynamics::compressor_def().create)(rate),
                sat: (saturator_def().create)(rate),
                width: (imager_def().create)(rate),
                limiter: (dynamics::limiter_def().create)(rate),
            };
            d.update();
            Box::new(d)
        },
    }
}

#[cfg(test)]
mod scope_tests {
    use super::*;

    /// Correlation: +1 for mono, −1 for polarity-inverted channels, ~0 for unrelated noise.
    #[test]
    fn phase_scope_correlation() {
        let mut rng = crate::dsp::Rng::new(3);
        for (mk, want) in [(0, 1.0f32), (1, -1.0), (2, 0.0)] {
            let mut s = PhaseScope::new(48000.0);
            for i in 0..48000 {
                let x = (i as f32 * 0.05).sin();
                let (l, r) = match mk {
                    0 => (x, x),
                    1 => (x, -x),
                    _ => (rng.next_f32() * 2.0 - 1.0, rng.next_f32() * 2.0 - 1.0),
                };
                s.push(l, r);
            }
            s.finish();
            assert!((s.data[0] - want).abs() < 0.1, "case {mk}: {}", s.data[0]);
        }
    }
}
