//! Colour and utility: saturator, bit crusher, filter, utility (gain/pan/phase/width/mono), stereo imager
//! and the Philosopher's Stone one-knob mastering chain.

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
    ParamInfo::choice("tube", "Tube", crate::dsp::tube::TUBE_NAMES, 0.0),
    ParamInfo::lin("bias", "Bias", -1.0, 1.0, 0.0, ""),
    ParamInfo::lin("ing_warm", "Warm tube", 0.0, 1.0, 0.0, ""),
    ParamInfo::lin("ing_hot", "Hot triode", 0.0, 1.0, 0.0, ""),
    ParamInfo::lin("ing_crisp", "Crisp pentode", 0.0, 1.0, 0.0, ""),
];

/// The Crucible's ingredients: parallel tube stages blended into the brew — (name, tube model index,
/// extra drive in dB). Warm 6SN7 (soft, even), hot 12AX7 driven hard, crisp EF86 pentode (bright, odd).
pub const INGREDIENTS: [(&str, usize, f32); 3] = [("Warm tube", 3, 0.0), ("Hot triode", 0, 9.0), ("Crisp pentode", 4, 4.0)];

pub struct Saturator {
    rate: f32,
    mode: usize,
    drive: f32,
    out: f32,
    mix: f32,
    tone: [OnePole; 2],
    dc: [OnePole; 2],
    tubes: [crate::dsp::tube::TubeStage; 2],
    /// The ingredients' amounts (0 … 1) and their tube stages per channel.
    ing: [f32; 3],
    ing_tubes: [[crate::dsp::tube::TubeStage; 2]; 3],
    /// Input and output peaks of the last block (the cauldron's heat).
    tele: [f32; 2],
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
            "tube" => {
                let m = crate::dsp::tube::TUBES[(v.round() as usize).min(crate::dsp::tube::TUBES.len() - 1)];
                self.tubes.iter_mut().for_each(|t| t.set_model(m));
            }
            "bias" => self.tubes.iter_mut().chain(self.ing_tubes.iter_mut().flatten()).for_each(|t| t.bias = v.clamp(-1.0, 1.0)),
            "ing_warm" => self.ing[0] = v.clamp(0.0, 1.0),
            "ing_hot" => self.ing[1] = v.clamp(0.0, 1.0),
            "ing_crisp" => self.ing[2] = v.clamp(0.0, 1.0),
            _ => {}
        }
    }
    fn reset(&mut self) {
        self.tone.iter_mut().chain(self.dc.iter_mut()).for_each(OnePole::reset);
        self.tubes.iter_mut().chain(self.ing_tubes.iter_mut().flatten()).for_each(|t| t.reset());
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let peak = |l: &[f32], r: &[f32]| l.iter().chain(r.iter()).fold(0.0f32, |m, x| m.max(x.abs()));
        self.tele[0] = peak(l, r);
        // The brew: the character path, with the ingredients poured in alongside it. Their amounts
        // replace the character path's share as they add up (all at 0 = the plain character path).
        let total: f32 = self.ing.iter().sum();
        let base_w = (1.0 - total).max(0.0);
        let norm = 1.0 / (base_w + total).max(1e-6);
        let gains: [f32; 3] = std::array::from_fn(|k| db_to_gain(INGREDIENTS[k].2));
        for i in 0..l.len() {
            for (ch, x) in [&mut l[i], &mut r[i]].into_iter().enumerate() {
                let d = *x * self.drive;
                // Tube mode runs the selected tube model (with bias shift, Miller roll-off and sag).
                let mut y = if base_w > 0.0 {
                    let b = if self.mode == 0 { self.tubes[ch].process(d) } else { shape(self.mode, d) };
                    b * base_w
                } else {
                    0.0
                };
                for k in 0..3 {
                    if self.ing[k] > 0.0 {
                        y += self.ing_tubes[k][ch].process(d * gains[k]) / gains[k].sqrt() * self.ing[k];
                    }
                }
                let y = y * norm;
                let y = self.dc[ch].highpass(self.tone[ch].process(y)) * self.out;
                *x = *x * (1.0 - self.mix) + y * self.mix;
            }
        }
        self.tele[1] = peak(l, r);
    }
    fn telemetry(&self) -> &[f32] {
        &self.tele
    }
}

pub fn saturator_def() -> EffectDef {
    EffectDef {
        id: "saturator",
        name: "Crucible (saturator)",
        category: "Distortion",
        description: "Tube (12AX7, 12AT7, 12AU7, 6SN7, EF86, EL34, 6L6, KT88, 300B models with bias, blocking and sag), tape, soft/hard clip, foldback and asymmetric waveshaping with tone and parallel mix, plus three parallel tube ingredients to blend into the brew: a warm 6SN7, a hot 12AX7 and a crisp EF86 pentode.",
        params: SAT_PARAMS,
        uses_sidechain: false,
        create: |rate| {
            let t = crate::dsp::tube::TubeStage::new(crate::dsp::tube::TUBES[0], rate);
            let ing = |k: usize| [crate::dsp::tube::TubeStage::new(crate::dsp::tube::TUBES[INGREDIENTS[k].1], rate); 2];
            Box::new(Saturator { rate, mode: 0, drive: 2.0, out: 0.7, mix: 1.0, tone: [OnePole::new(12000.0, rate); 2], dc: [OnePole::new(5.0, rate); 2], tubes: [t; 2], ing: [0.0; 3], ing_tubes: [ing(0), ing(1), ing(2)], tele: [0.0; 2] })
        },
    }
}

#[cfg(test)]
mod crucible_tests {
    use super::*;

    fn run(params: &[(&str, f32)]) -> Vec<f32> {
        let def = saturator_def();
        let mut fx = (def.create)(48_000.0);
        for p in def.params {
            fx.set_param(p.id, p.default);
        }
        for (id, v) in params {
            fx.set_param(id, *v);
        }
        let n = 4800;
        let mut l: Vec<f32> = (0..n).map(|i| 0.5 * (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin()).collect();
        let mut r = l.clone();
        fx.process(&mut l, &mut r, None);
        l
    }

    #[test]
    fn no_ingredients_is_the_plain_character_path() {
        // With the three amounts at 0 the brew is exactly the saturator as it was.
        let def = saturator_def();
        let mut a = (def.create)(48_000.0);
        for p in def.params {
            a.set_param(p.id, p.default);
        }
        let n = 4800;
        let src: Vec<f32> = (0..n).map(|i| 0.5 * (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin()).collect();
        let (mut l, mut r) = (src.clone(), src.clone());
        a.process(&mut l, &mut r, None);
        // Reference: the same chain by hand.
        let mut tube = crate::dsp::tube::TubeStage::new(crate::dsp::tube::TUBES[0], 48_000.0);
        let mut tone = OnePole::new(12000.0, 48_000.0);
        let mut dc = OnePole::new(5.0, 48_000.0);
        let (drive, out) = (db_to_gain(6.0), db_to_gain(-3.0));
        for (i, x) in src.iter().enumerate() {
            let y = dc.highpass(tone.process(tube.process(x * drive))) * out;
            assert!((y - l[i]).abs() < 1e-6, "sample {i}: {y} vs {}", l[i]);
        }
    }

    #[test]
    fn each_ingredient_changes_the_sound_and_stays_sane() {
        let plain = run(&[]);
        for id in ["ing_warm", "ing_hot", "ing_crisp"] {
            let y = run(&[(id, 1.0)]);
            assert!(y.iter().all(|v| v.is_finite() && v.abs() < 4.0), "{id} misbehaves");
            let diff: f32 = y.iter().zip(&plain).map(|(a, b)| (a - b).abs()).sum::<f32>() / y.len() as f32;
            assert!(diff > 1e-3, "{id} does nothing ({diff})");
        }
        // Different ingredients, different results.
        let (w, h) = (run(&[("ing_warm", 1.0)]), run(&[("ing_hot", 1.0)]));
        assert!(w.iter().zip(&h).map(|(a, b)| (a - b).abs()).sum::<f32>() > 1.0);
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
        name: "Corroder (bit crusher)",
        category: "Distortion",
        description: "Bit depth and sample-rate reduction.",
        params: CRUSH_PARAMS,
        uses_sidechain: false,
        create: |_| Box::new(BitCrusher { bits: 8.0, down: 4.0, mix: 1.0, phase: 0.0, held: [0.0; 2] }),
    }
}

// ------------------------------------------------------------------------------------------------ filter

/// Filter types (Volcano): the classic responses plus notch, peak and shelves.
pub const FILTER_TYPES: &[&str] = &["Low pass", "High pass", "Band pass", "Notch", "Peak", "Low shelf", "High shelf"];
/// Slopes: one, two or four state-variable stages (12 / 24 / 48 dB per octave for the pass types).
pub const FILTER_SLOPES: &[&str] = &["12 dB/oct", "24 dB/oct", "48 dB/oct"];
/// Styles: how the filter saturates around its resonance (Volcano's Clean / Smooth / Ram / Hack).
pub const FILTER_STYLES: &[&str] = &["Clean", "Smooth", "Ram", "Hack"];

const FILTER_PARAMS: &[ParamInfo] = &[
    ParamInfo::choice("type", "Type", FILTER_TYPES, 0.0),
    ParamInfo::log("cutoff", "Cutoff", 20.0, 20000.0, 2000.0, "Hz"),
    ParamInfo::lin("resonance", "Resonance", 0.0, 0.98, 0.3, ""),
    ParamInfo::log("lfo_rate", "LFO rate", 0.05, 20.0, 1.0, "Hz"),
    ParamInfo::lin("lfo_depth", "LFO depth", 0.0, 4.0, 0.0, "oct"),
    ParamInfo::lin("env_depth", "Envelope depth", -4.0, 4.0, 0.0, "oct"),
    ParamInfo::lin("mix", "Mix", 0.0, 1.0, 1.0, ""),
    ParamInfo::choice("slope", "Slope", FILTER_SLOPES, 0.0),
    ParamInfo::choice("style", "Style", FILTER_STYLES, 0.0),
    ParamInfo::lin("drive", "Drive", 0.0, 24.0, 0.0, "dB"),
    ParamInfo::lin("gain", "Peak / shelf gain", -24.0, 24.0, 6.0, "dB"),
    ParamInfo::lin("pan", "Stereo spread (cutoff L/R)", -1.0, 1.0, 0.0, "oct"),
    ParamInfo::toggle("b_on", "Filter B on", false),
    ParamInfo::choice("b_type", "Filter B type", FILTER_TYPES, 1.0),
    ParamInfo::log("b_cutoff", "Filter B cutoff", 20.0, 20000.0, 200.0, "Hz"),
    ParamInfo::lin("b_resonance", "Filter B resonance", 0.0, 0.98, 0.3, ""),
    ParamInfo::choice("b_slope", "Filter B slope", FILTER_SLOPES, 0.0),
    ParamInfo::choice("routing", "Routing", &["Serial", "Parallel"], 0.0),
    ParamInfo::lin("b_follow", "Filter B follows A's modulation", 0.0, 1.0, 0.0, ""),
];

const STAGES: usize = 4;

/// One filter (A or B): up to four SVF stages per channel.
#[derive(Clone, Copy, Default)]
struct FilterUnit {
    kind: usize,
    cutoff: f32,
    res: f32,
    slope: usize,
    svf: [[Svf; STAGES]; 2],
}

impl FilterUnit {
    fn stages(&self) -> usize {
        [1, 2, 4][self.slope.min(2)]
    }
    /// One sample of channel `ch` at cutoff `f`.
    #[inline]
    fn run(&mut self, ch: usize, x: f32, f: f32, rate: f32, gain_db: f32, style: usize) -> f32 {
        let n = self.stages();
        let mut y = x;
        // Resonance on the last stage only (the others are Butterworth-ish), so a 48 dB slope does not
        // ring four times.
        for k in 0..n {
            let res = if k + 1 == n { self.res } else { 0.3 };
            let input = match style {
                // Ram: drive into each stage; Hack: hard-limited feedback path; Smooth: gentle tanh.
                1 => (y * 0.9).tanh() / 0.9,
                2 => (y * 1.6).tanh() / 1.2,
                3 => y.clamp(-0.9, 0.9),
                _ => y,
            };
            let (lp, bp, hp) = self.svf[ch][k].process(input, f, res, rate);
            let g = crate::dsp::db_to_gain(gain_db) - 1.0;
            y = match self.kind {
                0 => lp,
                1 => hp,
                2 => bp,
                3 => lp + hp,
                4 => input + bp * g,
                5 => input + lp * g,
                _ => input + hp * g,
            };
            // Peak and shelves are one stage whatever the slope.
            if self.kind >= 4 {
                break;
            }
        }
        y
    }
}

pub struct Filter {
    rate: f32,
    a: FilterUnit,
    b: FilterUnit,
    b_on: bool,
    parallel: bool,
    b_follow: f32,
    lfo_rate: f32,
    lfo_depth: f32,
    env_depth: f32,
    mix: f32,
    style: usize,
    drive: f32,
    gain: f32,
    spread: f32,
    lfo: Lfo,
    env: Follower,
    /// Telemetry: the modulated cutoff of A (Hz) for the display.
    tele: [f32; 1],
}

impl Effect for Filter {
    fn set_param(&mut self, id: &str, v: f32) {
        let idx = |v: f32, n: usize| (v.round().max(0.0) as usize).min(n - 1);
        match id {
            "type" => self.a.kind = idx(v, FILTER_TYPES.len()),
            "cutoff" => self.a.cutoff = v,
            "resonance" => self.a.res = v,
            "slope" => self.a.slope = idx(v, 3),
            "b_type" => self.b.kind = idx(v, FILTER_TYPES.len()),
            "b_cutoff" => self.b.cutoff = v,
            "b_resonance" => self.b.res = v,
            "b_slope" => self.b.slope = idx(v, 3),
            "b_on" => self.b_on = v >= 0.5,
            "routing" => self.parallel = v >= 0.5,
            "b_follow" => self.b_follow = v,
            "lfo_rate" => self.lfo_rate = v,
            "lfo_depth" => self.lfo_depth = v,
            "env_depth" => self.env_depth = v,
            "mix" => self.mix = v,
            "style" => self.style = idx(v, FILTER_STYLES.len()),
            "drive" => self.drive = v,
            "gain" => self.gain = v,
            "pan" => self.spread = v,
            _ => {}
        }
    }
    fn reset(&mut self) {
        self.a.svf = Default::default();
        self.b.svf = Default::default();
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let drive = crate::dsp::db_to_gain(self.drive);
        for i in 0..l.len() {
            let m = self.lfo.next(self.lfo_rate, self.rate, LfoShape::Sine) * self.lfo_depth;
            let e = self.env.next(l[i].abs().max(r[i].abs())).min(1.0) * self.env_depth;
            let fa = self.a.cutoff * 2f32.powf(m + e);
            let fb = self.b.cutoff * 2f32.powf((m + e) * self.b_follow);
            self.tele[0] = fa;
            for (ch, x) in [&mut l[i], &mut r[i]].into_iter().enumerate() {
                // Stereo spread: the left and right cutoffs move apart.
                let sp = 2f32.powf(if ch == 0 { -self.spread } else { self.spread } * 0.5);
                let dry = *x;
                let inp = if self.drive > 0.01 { (dry * drive).tanh() / drive.sqrt() } else { dry };
                let ya = self.a.run(ch, inp, (fa * sp).clamp(20.0, 20000.0), self.rate, self.gain, self.style);
                let y = if !self.b_on {
                    ya
                } else if self.parallel {
                    0.5 * (ya + self.b.run(ch, inp, (fb * sp).clamp(20.0, 20000.0), self.rate, self.gain, self.style))
                } else {
                    self.b.run(ch, ya, (fb * sp).clamp(20.0, 20000.0), self.rate, self.gain, self.style)
                };
                *x = dry * (1.0 - self.mix) + y * self.mix;
            }
        }
    }
    fn telemetry(&self) -> &[f32] {
        &self.tele
    }
}

pub fn filter_def() -> EffectDef {
    EffectDef {
        id: "filter",
        name: "Alembic (filter)",
        category: "Filter",
        description: "Filter after Volcano: two filters (serial or parallel), low / high / band pass, notch, peak and shelves, 12 / 24 / 48 dB slopes, styles (clean, smooth, ram, hack), drive, stereo spread, LFO and envelope follower.",
        params: FILTER_PARAMS,
        uses_sidechain: false,
        create: |rate| {
            let a = FilterUnit { kind: 0, cutoff: 2000.0, res: 0.3, slope: 0, svf: Default::default() };
            let b = FilterUnit { kind: 1, cutoff: 200.0, res: 0.3, slope: 0, svf: Default::default() };
            Box::new(Filter {
                rate,
                a,
                b,
                b_on: false,
                parallel: false,
                b_follow: 0.0,
                lfo_rate: 1.0,
                lfo_depth: 0.0,
                env_depth: 0.0,
                mix: 1.0,
                style: 0,
                drive: 0.0,
                gain: 6.0,
                spread: 0.0,
                lfo: Lfo::default(),
                env: Follower::new(5.0, 200.0, rate),
                tele: [2000.0],
            })
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
        name: "Phial (utility)",
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
            "mono_below" => {
                // Retuned in place: a new filter would lose its state and click.
                for b in &mut self.lp {
                    b.set(BiquadKind::LowPass, v, 0.707, 0.0, self.rate);
                }
            }
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
        name: "Wingspan (stereo imager)",
        category: "Utility",
        description: "Widens the stereo image and keeps the bass mono; a goniometer and correlation meter show the phase relation of the result.",
        params: IMAGER_PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(Imager { rate, width: 1.3, out: 1.0, lp: [Biquad::new(BiquadKind::LowPass, 120.0, 0.707, 0.0, rate); 2], scope: PhaseScope::new(rate) }),
    }
}

// ------------------------------------------------------------------------------------------------ Philosopher's Stone

/// Philosopher's Stone defaults: (amount, low dB, high dB, ceiling dBTP, character, punch, width, clip).
const DP_DEFAULT: (f32, f32, f32, f32, usize, f32, f32, f32) = (0.6, 1.0, 1.0, -0.3, 3, 0.45, 0.45, 0.5);

const DP_PARAMS: &[ParamInfo] = &[
    // Defaults voiced like the big one-knob mastering processors (Cradle's God Particle): loud and
    // finished out of the box – a solid, tight low end, forward mids, open air, a clipped-then-limited
    // top end close to 0 dBTP.
    ParamInfo::lin("amount", "Amount", 0.0, 1.0, DP_DEFAULT.0, ""),
    ParamInfo::lin("input", "Input", -12.0, 12.0, 0.0, "dB"),
    ParamInfo::lin("low", "Low", -6.0, 6.0, DP_DEFAULT.1, "dB"),
    ParamInfo::lin("mid", "Mid", -6.0, 6.0, 0.0, "dB"),
    ParamInfo::lin("high", "High", -6.0, 6.0, DP_DEFAULT.2, "dB"),
    ParamInfo::lin("ceiling", "Ceiling", -3.0, 0.0, DP_DEFAULT.3, "dBTP"),
    ParamInfo::choice("character", "Character", &["Clean", "Warm", "Bright", "Punchy"], DP_DEFAULT.4 as f32),
    ParamInfo::lin("punch", "Punch", 0.0, 1.0, DP_DEFAULT.5, ""),
    ParamInfo::lin("width", "Width", 0.0, 1.0, DP_DEFAULT.6, ""),
    ParamInfo::lin("clip", "Clipper", 0.0, 1.0, DP_DEFAULT.7, ""),
    ParamInfo::toggle("auto_gain", "Auto gain (A/B at equal loudness)", false),
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
    punch_fx: Box<dyn Effect>,
    character: usize,
    punch: f32,
    widen: f32,
    clip: f32,
    auto_gain: bool,
    /// Running input / output power for auto gain and the meters.
    in_pow: f32,
    out_pow: f32,
    ag: f32,
    /// [input RMS, output RMS, gain reduction dB, auto-gain dB]
    tele: [f32; 4],
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
        // Character: tilts of the tone stage and the tube used for colour.
        let (cl, cm, ch, tube) = match self.character {
            0 => (0.0, 0.0, 0.0, 2.0),
            2 => (-0.5, 0.3, 1.5, 2.0),
            // Punchy: weight in the sub and kick, presence, a touch of air; a warm tube for density.
            3 => (1.4, 0.6, 0.8, 3.0),
            _ => (0.8, -0.3, 0.3, 3.0),
        };
        let low_f = if self.character == 3 { 70.0 } else { 90.0 };
        let shape = [(BiquadKind::LowShelf, low_f, self.low + a * 1.5 + cl * a), (BiquadKind::Peak, 1200.0, self.mid - a * 0.8 + cm * a), (BiquadKind::HighShelf, 9000.0, self.high + a * 2.0 + ch * a)];
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
        // Make-up for the glue's reduction (its detector reads the true peaks), so more amount = louder.
        self.glue.set_param("makeup", a * 10.0);
        self.sat.set_param("mode", 0.0);
        self.sat.set_param("tube", tube);
        self.sat.set_param("drive", a * 9.0);
        self.sat.set_param("output", -a * 6.0);
        self.sat.set_param("mix", if self.character == 0 { 0.1 + a * 0.15 } else { 0.25 + a * 0.35 });
        self.punch_fx.set_param("attack", self.punch * 60.0 * (0.5 + a));
        self.punch_fx.set_param("att_dur", 18.0);
        self.punch_fx.set_param("att_shape", 1.0);
        self.punch_fx.set_param("guard", 0.0);
        self.width.set_param("width", 1.0 + self.widen * 0.6 * (0.4 + a));
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
            "character" => self.character = v.round() as usize,
            "punch" => self.punch = v.clamp(0.0, 1.0),
            "width" => self.widen = v.clamp(0.0, 1.0),
            "clip" => self.clip = v.clamp(0.0, 1.0),
            "auto_gain" => self.auto_gain = v > 0.5,
            _ => return,
        }
        self.update();
    }
    fn telemetry(&self) -> &[f32] {
        &self.tele
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
        let n = l.len().max(1);
        let pin: f32 = l.iter().zip(r.iter()).map(|(a, b)| a * a + b * b).sum::<f32>() / (2 * n) as f32;
        for i in 0..l.len() {
            let (mut a, mut b) = (l[i] * self.input, r[i] * self.input);
            for band in &mut self.eq {
                a = band[0].process(a);
                b = band[1].process(b);
            }
            l[i] = a;
            r[i] = b;
        }
        if self.punch > 0.0 {
            self.punch_fx.process(l, r, None);
        }
        self.glue.process(l, r, None);
        self.sat.process(l, r, None);
        self.width.process(l, r, None);
        // Soft clipper ahead of the limiter: shaves peaks so the limiter works less (louder, less pumping).
        if self.clip > 0.0 {
            let knee = db_to_gain(self.ceiling) * (1.0 - self.clip * 0.35);
            for x in l.iter_mut().chain(r.iter_mut()) {
                let ax = x.abs();
                if ax > knee {
                    let over = ax - knee;
                    *x = x.signum() * (knee + over / (1.0 + over / (knee * 0.5 + 1e-6)));
                }
            }
        }
        self.limiter.process(l, r, None);
        // Auto gain: match the output's loudness to the input so the effect can be judged, not just heard
        // as louder.
        let pout: f32 = l.iter().zip(r.iter()).map(|(a, b)| a * a + b * b).sum::<f32>() / (2 * n) as f32;
        let k = (-(n as f32) / (self.rate * 3.0)).exp();
        self.in_pow = self.in_pow * k + pin * (1.0 - k);
        self.out_pow = self.out_pow * k + pout * (1.0 - k);
        let target = if self.auto_gain && self.out_pow > 1e-9 { (self.in_pow / self.out_pow).sqrt().clamp(0.1, 2.0) } else { 1.0 };
        let from = self.ag;
        self.ag = target;
        for (i, (a, b)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
            let g = from + (target - from) * i as f32 / n as f32;
            *a *= g;
            *b *= g;
        }
        self.tele = [self.in_pow.sqrt(), self.out_pow.sqrt() * self.ag, self.gain_reduction(), crate::dsp::gain_to_db(self.ag.max(1e-6))];
    }
    fn gain_reduction(&self) -> f32 {
        self.glue.gain_reduction() + self.limiter.gain_reduction()
    }
}

pub fn dragon_particle_def() -> EffectDef {
    EffectDef {
        id: "dragon_particle",
        name: "Philosopher's Stone (one-knob mastering)",
        category: "Mastering",
        description: "One-knob mastering: tone EQ with character, transient punch, glue compression, tube colour, widening, a soft clipper and a true-peak limiter, with auto gain for honest A/B.",
        params: DP_PARAMS,
        uses_sidechain: false,
        create: |rate| {
            let mut d = DragonParticle {
                rate,
                amount: DP_DEFAULT.0,
                input: 1.0,
                low: DP_DEFAULT.1,
                mid: 0.0,
                high: DP_DEFAULT.2,
                ceiling: DP_DEFAULT.3,
                eq: [[Biquad::default(); 2]; 3],
                glue: (dynamics::compressor_def().create)(rate),
                sat: (saturator_def().create)(rate),
                width: (imager_def().create)(rate),
                limiter: (dynamics::limiter_def().create)(rate),
                punch_fx: (dynamics::transient_def().create)(rate),
                character: DP_DEFAULT.4,
                punch: DP_DEFAULT.5,
                widen: DP_DEFAULT.6,
                clip: DP_DEFAULT.7,
                auto_gain: false,
                in_pow: 0.0,
                out_pow: 0.0,
                ag: 1.0,
                tele: [0.0; 4],
            };
            d.update();
            Box::new(d)
        },
    }
}

#[cfg(test)]
mod scope_tests {
    use super::*;

    /// More amount = louder; auto gain brings the output back to the input loudness; the ceiling holds.
    #[test]
    fn dragon_particle_amount_and_auto_gain() {
        let x: Vec<f32> = (0..96000).map(|i| ((i as f32 * 0.031).sin() * 0.3 + (i as f32 * 0.0071).sin() * 0.2) * if (i / 6000) % 2 == 0 { 1.0 } else { 0.4 }).collect();
        let run = |params: &[(&str, f32)]| {
            let mut e = (dragon_particle_def().create)(48000.0);
            for (k, v) in params {
                e.set_param(k, *v);
            }
            let (mut l, mut r) = (x.clone(), x.clone());
            for (a, b) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
                e.process(a, b, None);
            }
            l
        };
        let rms = |v: &[f32]| (v[48000..].iter().map(|s| s * s).sum::<f32>() / 48000.0).sqrt();
        let lo = run(&[("amount", 0.1), ("ceiling", -1.0)]);
        let hi = run(&[("amount", 0.9), ("ceiling", -1.0)]);
        assert!(rms(&hi) > rms(&lo) * 1.3, "louder with more amount: {} vs {}", rms(&hi), rms(&lo));
        assert!(hi.iter().all(|v| v.abs() < 0.95), "under the ceiling");
        let ag = run(&[("amount", 0.9), ("auto_gain", 1.0)]);
        let ratio = rms(&ag) / rms(&x);
        assert!((0.75..1.35).contains(&ratio), "auto gain matches the input loudness: {ratio}");
    }

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

    /// Volcano-style filter: steeper slopes cut more an octave above the cutoff; the notch removes its
    /// frequency; two filters in series make a band from a low pass and a high pass.
    #[test]
    fn filter_slopes_types_and_routing() {
        let level = |set: &[(&str, f32)], f: f32| {
            let mut p = filter_def().defaults();
            p.insert("resonance".into(), 0.0);
            for (k, v) in set {
                p.insert((*k).into(), *v);
            }
            let mut e = crate::effects::create("filter", 48000.0, &p).unwrap();
            let mut l: Vec<f32> = (0..48000).map(|i| (std::f32::consts::TAU * f * i as f32 / 48000.0).sin() * 0.5).collect();
            let mut r = l.clone();
            e.process(&mut l, &mut r, None);
            l[24000..].iter().fold(0.0f32, |m, x| m.max(x.abs())) / 0.5
        };
        let at = |slope: f32| level(&[("cutoff", 1000.0), ("slope", slope)], 2000.0);
        assert!(at(2.0) < at(1.0) && at(1.0) < at(0.0), "{} {} {}", at(0.0), at(1.0), at(2.0));
        assert!(level(&[("type", 3.0), ("cutoff", 1000.0)], 1000.0) < 0.2, "notch");
        // Band from LP 2 kHz → HP 500 Hz in series: 1 kHz passes, 100 Hz and 8 kHz do not.
        let band = |f: f32| level(&[("cutoff", 2000.0), ("slope", 1.0), ("b_on", 1.0), ("b_type", 1.0), ("b_cutoff", 500.0), ("b_slope", 1.0)], f);
        assert!(band(1000.0) > 0.6 && band(100.0) < 0.1 && band(8000.0) < 0.1, "{} {} {}", band(100.0), band(1000.0), band(8000.0));
    }
}
