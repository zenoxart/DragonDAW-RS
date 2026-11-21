//! Algorithmic reverb: pre-delay → early reflections → 8-line feedback delay network with a Householder
//! matrix, RT60-based feedback, damping and modulated lines; width and dry/wet.
//!
//! Early reflections and the late reverb are separate parts with their own level and their own EQ (low cut,
//! high cut and a bell), so the early "room" can be bright and present while the tail is dark, or the other
//! way round. The early size spaces the reflections, and the early feed sets how much of them excites the
//! late network.

use super::{Effect, EffectDef, ParamInfo};
use crate::dsp::{Biquad, BiquadKind, DelayLine, Lfo, LfoShape, OnePole};

const LINES: usize = 8;
const BASE_MS: [f32; LINES] = [29.7, 37.1, 41.1, 43.7, 53.3, 59.9, 67.1, 73.3];
const EARLY_MS: [(f32, f32); 6] = [(7.0, 0.8), (11.3, 0.65), (17.9, 0.5), (23.1, 0.42), (31.4, 0.33), (41.7, 0.25)];

const PARAMS: &[ParamInfo] = &[
    ParamInfo::choice("mode", "Mode", &["Room", "Chamber", "Hall", "Plate", "Cathedral"], 2.0),
    ParamInfo::lin("size", "Size", 0.1, 1.0, 0.6, ""),
    ParamInfo::log("decay", "Decay", 0.1, 20.0, 2.2, "s"),
    ParamInfo::log("predelay", "Pre-delay", 0.1, 200.0, 12.0, "ms"),
    ParamInfo::log("damping", "Damping", 1000.0, 20000.0, 7000.0, "Hz"),
    ParamInfo::log("lowcut", "Input low cut", 20.0, 1000.0, 120.0, "Hz"),
    ParamInfo::lin("mod", "Modulation", 0.0, 1.0, 0.3, ""),
    ParamInfo::lin("width", "Width", 0.0, 1.5, 1.0, ""),
    ParamInfo::lin("mix", "Mix", 0.0, 1.0, 0.25, ""),
    // Decay-rate EQ (Pro-R): a curve of six points (Pro-R has six bands); each part of the tail decays that many times longer
    // (or shorter) than the decay time. Drag the points on the display.
    ParamInfo::log("dr_1", "Decay 1 time ×", 0.25, 4.0, 1.0, ""),
    ParamInfo::log("dr_2", "Decay 2 time ×", 0.25, 4.0, 1.0, ""),
    ParamInfo::log("dr_3", "Decay 3 time ×", 0.25, 4.0, 1.0, ""),
    ParamInfo::log("dr_4", "Decay 4 time ×", 0.25, 4.0, 1.0, ""),
    ParamInfo::log("dr_5", "Decay 5 time ×", 0.25, 4.0, 1.0, ""),
    ParamInfo::log("dr_6", "Decay 6 time ×", 0.25, 4.0, 1.0, ""),
    // The points' frequencies: free nodes, drag them sideways (kept in order, a third of an octave apart).
    ParamInfo::log("dr_f1", "Decay 1 freq", 20.0, 20000.0, 80.0, "Hz"),
    ParamInfo::log("dr_f2", "Decay 2 freq", 20.0, 20000.0, 300.0, "Hz"),
    ParamInfo::log("dr_f3", "Decay 3 freq", 20.0, 20000.0, 1200.0, "Hz"),
    ParamInfo::log("dr_f4", "Decay 4 freq", 20.0, 20000.0, 4800.0, "Hz"),
    ParamInfo::log("dr_f5", "Decay 5 freq", 20.0, 20000.0, 12000.0, "Hz"),
    ParamInfo::log("dr_f6", "Decay 6 freq", 20.0, 20000.0, 18000.0, "Hz"),
    // Each band's shape (Pro-R: low / high shelf, bell, notch) and width.
    ParamInfo::choice("dr_t1", "Decay 1 shape", DR_SHAPES, 1.0),
    ParamInfo::choice("dr_t2", "Decay 2 shape", DR_SHAPES, 0.0),
    ParamInfo::choice("dr_t3", "Decay 3 shape", DR_SHAPES, 0.0),
    ParamInfo::choice("dr_t4", "Decay 4 shape", DR_SHAPES, 0.0),
    ParamInfo::choice("dr_t5", "Decay 5 shape", DR_SHAPES, 0.0),
    ParamInfo::choice("dr_t6", "Decay 6 shape", DR_SHAPES, 2.0),
    ParamInfo::log("dr_q1", "Decay 1 Q", 0.3, 6.0, 1.0, ""),
    ParamInfo::log("dr_q2", "Decay 2 Q", 0.3, 6.0, 1.0, ""),
    ParamInfo::log("dr_q3", "Decay 3 Q", 0.3, 6.0, 1.0, ""),
    ParamInfo::log("dr_q4", "Decay 4 Q", 0.3, 6.0, 1.0, ""),
    ParamInfo::log("dr_q5", "Decay 5 Q", 0.3, 6.0, 1.0, ""),
    ParamInfo::log("dr_q6", "Decay 6 Q", 0.3, 6.0, 1.0, ""),
    // Early reflections.
    ParamInfo::lin("early", "Early level", 0.0, 1.0, 0.4, ""),
    ParamInfo::lin("er_size", "Early size", 0.2, 2.0, 1.0, ""),
    ParamInfo::lin("er_feed", "Early feed", 0.0, 1.0, 0.25, ""),
    ParamInfo::log("er_lo", "Early low cut", 20.0, 2000.0, 20.0, "Hz"),
    ParamInfo::log("er_hi", "Early high cut", 1000.0, 20000.0, 20000.0, "Hz"),
    ParamInfo::log("er_freq", "Early bell freq", 100.0, 10000.0, 1500.0, "Hz"),
    ParamInfo::lin("er_gain", "Early bell gain", -12.0, 12.0, 0.0, "dB"),
    // Late reverb.
    ParamInfo::lin("late", "Late level", 0.0, 1.0, 1.0, ""),
    ParamInfo::log("late_lo", "Late low cut", 20.0, 2000.0, 20.0, "Hz"),
    ParamInfo::log("late_hi", "Late high cut", 1000.0, 20000.0, 20000.0, "Hz"),
    ParamInfo::log("late_freq", "Late bell freq", 100.0, 10000.0, 1500.0, "Hz"),
    ParamInfo::lin("late_gain", "Late bell gain", -12.0, 12.0, 0.0, "dB"),
];

/// Low cut, high cut and bell for one part (stereo), skipped when neutral.
#[derive(Clone)]
struct PartEq {
    lo: f32,
    hi: f32,
    freq: f32,
    gain: f32,
    f: [[Biquad; 3]; 2],
}

impl PartEq {
    fn new(rate: f32) -> Self {
        let mk = || [Biquad::new(BiquadKind::HighPass, 20.0, 0.707, 0.0, rate), Biquad::new(BiquadKind::LowPass, 20000.0f32.min(rate * 0.45), 0.707, 0.0, rate), Biquad::new(BiquadKind::Peak, 1500.0, 0.8, 0.0, rate)];
        PartEq { lo: 20.0, hi: 20000.0, freq: 1500.0, gain: 0.0, f: [mk(), mk()] }
    }

    fn update(&mut self, rate: f32) {
        let ny = rate * 0.45;
        for ch in &mut self.f {
            ch[0].set(BiquadKind::HighPass, self.lo.min(ny), 0.707, 0.0, rate);
            ch[1].set(BiquadKind::LowPass, self.hi.min(ny), 0.707, 0.0, rate);
            ch[2].set(BiquadKind::Peak, self.freq.min(ny), 0.8, self.gain, rate);
        }
    }

    #[inline]
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let (lo, hi, bell) = (self.lo > 20.5, self.hi < 19_900.0, self.gain.abs() > 0.05);
        let mut out = [l, r];
        for (ch, x) in self.f.iter_mut().zip(out.iter_mut()) {
            if lo {
                *x = ch[0].process(*x);
            }
            if hi {
                *x = ch[1].process(*x);
            }
            if bell {
                *x = ch[2].process(*x);
            }
        }
        (out[0], out[1])
    }

    fn reset(&mut self) {
        for b in self.f.iter_mut().flatten() {
            b.reset();
        }
    }
}

/// The decay-rate EQ's default band frequencies (Hz); each band can be moved (`dr_f1` … `dr_f6`).
pub const DR_FREQS: [f32; 6] = [80.0, 300.0, 1200.0, 4800.0, 12000.0, 18000.0];
/// Six bands, like Pro-R's six-band decay-rate EQ.
pub const DR_BANDS: usize = 6;
/// Band shapes, as in Pro-R's decay-rate EQ.
pub const DR_SHAPES: &[&str] = &["Bell", "Low shelf", "High shelf", "Notch"];
/// The tail is split into this many fixed bands (log-spaced); each gets the curve's value at its centre.
const DR_SPLIT: usize = 12;

fn dr_splits() -> [f32; DR_SPLIT - 1] {
    let c = dr_centres();
    std::array::from_fn(|k| (c[k] * c[k + 1]).sqrt())
}

fn dr_centres() -> [f32; DR_SPLIT] {
    std::array::from_fn(|k| 30.0 * (18000.0f32 / 30.0).powf(k as f32 / (DR_SPLIT - 1) as f32))
}

/// One decay-rate band: shape, frequency, multiplier (×), Q.
#[derive(Clone, Copy)]
pub struct DrBand {
    pub shape: usize,
    pub freq: f32,
    pub mul: f32,
    pub q: f32,
}

/// The decay-rate curve: the decay-time multiplier at a frequency (sum of the bands' shapes in log2,
/// limited to ×¼ … ×4).
pub fn dr_curve(bands: &[DrBand], f: f32) -> f32 {
    let mut l = 0.0;
    for b in bands {
        let x = (f.max(1.0) / b.freq.max(1.0)).log2();
        let w = match b.shape {
            1 => 1.0 / (1.0 + (x * b.q * 4.0).exp()),
            2 => 1.0 / (1.0 + (-x * b.q * 4.0).exp()),
            3 => (-0.5 * (x * b.q * 6.0).powi(2)).exp(),
            _ => (-0.5 * (x * b.q * 1.5).powi(2)).exp(),
        };
        l += w * b.mul.max(0.01).log2();
    }
    2f32.powf(l.clamp(-2.0, 2.0))
}

/// The bands from a parameter lookup (for the UI curve).
pub fn dr_bands(get: impl Fn(&str) -> f32) -> [DrBand; DR_BANDS] {
    std::array::from_fn(|k| {
        let n = k + 1;
        DrBand { shape: (get(&format!("dr_t{n}")).round().max(0.0) as usize).min(DR_SHAPES.len() - 1), freq: get(&format!("dr_f{n}")), mul: get(&format!("dr_{n}")), q: get(&format!("dr_q{n}")) }
    })
}

pub struct Reverb {
    rate: f32,
    pre: [DelayLine; 2],
    early: [DelayLine; 2],
    lines: Vec<DelayLine>,
    damp: [OnePole; LINES],
    /// Crossovers per line split the tail into fixed log-spaced bands that follow the decay-rate curve.
    splits: [[OnePole; DR_SPLIT - 1]; LINES],
    /// Feedback gain of each split band per line.
    gains_dr: [[f32; DR_SPLIT]; LINES],
    dr: [DrBand; DR_BANDS],
    lowcut: [OnePole; 2],
    lfos: Vec<Lfo>,
    lengths: [f32; LINES],
    gains: [f32; LINES],
    size: f32,
    decay: f32,
    predelay: f32,
    damping: f32,
    lowcut_hz: f32,
    early_level: f32,
    early_size: f32,
    early_feed: f32,
    late_level: f32,
    early_eq: PartEq,
    late_eq: PartEq,
    modulation: f32,
    width: f32,
    mix: f32,
    mode: usize,
}

impl Reverb {
    fn new(rate: f32) -> Self {
        let max = (rate * 0.2) as usize;
        Reverb {
            rate,
            pre: [DelayLine::new((rate * 0.25) as usize), DelayLine::new((rate * 0.25) as usize)],
            // Room for the largest early size (2×) with the size control at its maximum.
            early: [DelayLine::new((rate * 0.16) as usize), DelayLine::new((rate * 0.16) as usize)],
            lines: (0..LINES).map(|_| DelayLine::new(max)).collect(),
            damp: [OnePole::new(7000.0, rate); LINES],
            splits: [std::array::from_fn(|k| OnePole::new(dr_splits()[k], rate)); LINES],
            gains_dr: [[0.0; DR_SPLIT]; LINES],
            dr: std::array::from_fn(|k| DrBand { shape: [1, 0, 0, 0, 0, 2][k], freq: DR_FREQS[k], mul: 1.0, q: 1.0 }),
            lowcut: [OnePole::new(120.0, rate); 2],
            lfos: (0..LINES).map(|i| Lfo::with_phase(i as f32 / LINES as f32)).collect(),
            lengths: [0.0; LINES],
            gains: [0.0; LINES],
            size: 0.6,
            decay: 2.2,
            predelay: 12.0,
            damping: 7000.0,
            lowcut_hz: 120.0,
            early_level: 0.4,
            early_size: 1.0,
            early_feed: 0.25,
            late_level: 1.0,
            early_eq: PartEq::new(rate),
            late_eq: PartEq::new(rate),
            modulation: 0.3,
            width: 1.0,
            mix: 0.25,
            mode: 2,
        }
    }

    fn update(&mut self) {
        // Mode scales size and colour: plate is dense and bright, cathedral huge and dark.
        let (size_mul, bright) = match self.mode {
            0 => (0.45, 1.0),
            1 => (0.7, 0.9),
            3 => (0.6, 1.3),
            4 => (1.6, 0.75),
            _ => (1.0, 1.0),
        };
        let splits = dr_splits();
        let centres = dr_centres();
        let muls: [f32; DR_SPLIT] = std::array::from_fn(|k| dr_curve(&self.dr, centres[k]));
        for i in 0..LINES {
            let len = BASE_MS[i] * 0.001 * self.rate * (0.3 + self.size * 1.7) * size_mul;
            self.lengths[i] = len.min(self.lines[i].len() as f32 - 8.0);
            // g = 10^(−3·len/(RT60·rate)) gives RT60 decay.
            self.gains[i] = 10f32.powf(-3.0 * self.lengths[i] / (self.decay.max(0.05) * self.rate));
            let g = |mul: f32| 10f32.powf(-3.0 * self.lengths[i] / ((self.decay * mul).max(0.05) * self.rate));
            self.gains_dr[i] = std::array::from_fn(|k| g(muls[k]));
            for (k, f) in self.splits[i].iter_mut().enumerate() {
                f.set(splits[k].min(self.rate * 0.45), self.rate);
            }
            self.damp[i].set((self.damping * bright).min(self.rate * 0.45), self.rate);
        }
        for f in &mut self.lowcut {
            f.set(self.lowcut_hz, self.rate);
        }
        self.early_eq.update(self.rate);
        self.late_eq.update(self.rate);
    }
}

impl Effect for Reverb {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "mode" => self.mode = v.round().clamp(0.0, 4.0) as usize,
            "size" => self.size = v,
            "decay" => self.decay = v,
            "predelay" => self.predelay = v,
            "damping" => self.damping = v,
            "lowcut" => self.lowcut_hz = v,
            "early" => self.early_level = v,
            "er_size" => self.early_size = v,
            "er_feed" => self.early_feed = v,
            "er_lo" => self.early_eq.lo = v,
            "er_hi" => self.early_eq.hi = v,
            "er_freq" => self.early_eq.freq = v,
            "er_gain" => self.early_eq.gain = v,
            "late" => self.late_level = v,
            "late_lo" => self.late_eq.lo = v,
            "late_hi" => self.late_eq.hi = v,
            "late_freq" => self.late_eq.freq = v,
            "late_gain" => self.late_eq.gain = v,
            "mod" => self.modulation = v,
            "width" => self.width = v,
            "mix" => self.mix = v,
            "dr_1" => self.dr[0].mul = v,
            "dr_2" => self.dr[1].mul = v,
            "dr_3" => self.dr[2].mul = v,
            "dr_4" => self.dr[3].mul = v,
            "dr_5" => self.dr[4].mul = v,
            "dr_6" => self.dr[5].mul = v,
            "dr_f1" => self.dr[0].freq = v,
            "dr_t1" => self.dr[0].shape = (v.round().max(0.0) as usize).min(DR_SHAPES.len() - 1),
            "dr_q1" => self.dr[0].q = v,
            "dr_f2" => self.dr[1].freq = v,
            "dr_t2" => self.dr[1].shape = (v.round().max(0.0) as usize).min(DR_SHAPES.len() - 1),
            "dr_q2" => self.dr[1].q = v,
            "dr_f3" => self.dr[2].freq = v,
            "dr_t3" => self.dr[2].shape = (v.round().max(0.0) as usize).min(DR_SHAPES.len() - 1),
            "dr_q3" => self.dr[2].q = v,
            "dr_f4" => self.dr[3].freq = v,
            "dr_t4" => self.dr[3].shape = (v.round().max(0.0) as usize).min(DR_SHAPES.len() - 1),
            "dr_q4" => self.dr[3].q = v,
            "dr_f5" => self.dr[4].freq = v,
            "dr_t5" => self.dr[4].shape = (v.round().max(0.0) as usize).min(DR_SHAPES.len() - 1),
            "dr_q5" => self.dr[4].q = v,
            "dr_f6" => self.dr[5].freq = v,
            "dr_t6" => self.dr[5].shape = (v.round().max(0.0) as usize).min(DR_SHAPES.len() - 1),
            "dr_q6" => self.dr[5].q = v,
            _ => return,
        }
        self.update();
    }

    fn reset(&mut self) {
        self.pre.iter_mut().chain(self.early.iter_mut()).chain(self.lines.iter_mut()).for_each(DelayLine::clear);
        self.damp.iter_mut().chain(self.lowcut.iter_mut()).chain(self.splits.iter_mut().flatten()).for_each(OnePole::reset);
        self.early_eq.reset();
        self.late_eq.reset();
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let pre = self.predelay * 0.001 * self.rate;
        let mod_depth = self.modulation * 0.0015 * self.rate;
        for i in 0..l.len() {
            let (dl, dr) = (l[i], r[i]);
            self.pre[0].write(self.lowcut[0].highpass(dl));
            self.pre[1].write(self.lowcut[1].highpass(dr));
            let (pl, pr) = (self.pre[0].read(pre), self.pre[1].read(pre));
            self.early[0].write(pl);
            self.early[1].write(pr);
            let mut el = 0.0;
            let mut er = 0.0;
            for (k, (ms, g)) in EARLY_MS.iter().enumerate() {
                let d = (ms * 0.001 * self.rate * (0.5 + self.size) * self.early_size).min(self.early[0].len() as f32 - 4.0);
                if k % 2 == 0 {
                    el += self.early[0].read(d) * g;
                    er += self.early[1].read(d * 1.07) * g;
                } else {
                    el += self.early[1].read(d) * g * 0.7;
                    er += self.early[0].read(d * 0.93) * g * 0.7;
                }
            }
            // Read the network.
            let mut outs = [0.0f32; LINES];
            for k in 0..LINES {
                let m = self.lfos[k].next(0.3 + k as f32 * 0.07, self.rate, LfoShape::Sine) * mod_depth;
                let x = self.damp[k].process(self.lines[k].read(self.lengths[k] + m));
                // Decay-rate EQ: each split band gets its own feedback gain (bands = differences of
                // one-pole low-passes, so they sum back to the input).
                let mut prev = 0.0;
                let mut acc = 0.0;
                for b in 0..DR_SPLIT {
                    let lp = if b < DR_SPLIT - 1 { self.splits[k][b].process(x) } else { x };
                    acc += (lp - prev) * self.gains_dr[k][b];
                    prev = lp;
                }
                outs[k] = acc;
            }
            // Householder mix: y = x − 2/N·Σx.
            let sum: f32 = outs.iter().sum::<f32>() * (2.0 / LINES as f32);
            let input = [pl, pr];
            for k in 0..LINES {
                let fb = outs[k] - sum;
                self.lines[k].write(fb + input[k % 2] * 0.35 + if k % 2 == 0 { el } else { er } * (self.early_feed * 0.4));
            }
            let mut wl = 0.0;
            let mut wr = 0.0;
            for k in 0..LINES {
                if k % 2 == 0 {
                    wl += outs[k];
                } else {
                    wr += outs[k];
                }
            }
            // Each part through its own EQ, then mixed at its own level.
            let (ll, lr) = self.late_eq.process(wl * 0.5, wr * 0.5);
            let (eql, eqr) = self.early_eq.process(el, er);
            wl = ll * self.late_level + eql * self.early_level;
            wr = lr * self.late_level + eqr * self.early_level;
            let mid = (wl + wr) * 0.5;
            let side = (wl - wr) * 0.5 * self.width;
            l[i] = dl * (1.0 - self.mix) + (mid + side) * self.mix;
            r[i] = dr * (1.0 - self.mix) + (mid - side) * self.mix;
        }
    }
}

pub fn reverb_def() -> EffectDef {
    EffectDef {
        id: "reverb",
        name: "Dragon's Lair (reverb)",
        category: "Space",
        description: "Room/hall/plate reverb (8-line FDN) with pre-delay, damping and width; early reflections and late reverb each have their own level and EQ.",
        params: PARAMS,
        uses_sidechain: false,
        create: |rate| {
            let mut r = Reverb::new(rate);
            r.update();
            Box::new(r)
        },
    }
}

#[cfg(test)]
mod tests {
    fn impulse_response(params: &[(&str, f32)]) -> Vec<f32> {
        let mut p = std::collections::BTreeMap::new();
        p.insert("mix".into(), 1.0);
        for (k, v) in params {
            p.insert(k.to_string(), *v);
        }
        let mut e = crate::effects::create("reverb", 48000.0, &p).unwrap();
        let mut l = vec![0.0f32; 48000];
        let mut r = l.clone();
        l[0] = 1.0;
        r[0] = 1.0;
        e.process(&mut l, &mut r, None);
        l
    }

    fn energy(x: &[f32]) -> f32 {
        x.iter().map(|v| v * v).sum()
    }

    /// Energy above ~6 kHz (first difference ≈ high-pass).
    fn bright(x: &[f32]) -> f32 {
        x.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum()
    }

    #[test]
    fn early_and_late_levels_are_independent() {
        // Pre-delay 0.1 ms: early reflections arrive within ~60 ms, the late tail fills the rest.
        let base = [("predelay", 0.1f32), ("decay", 2.0)];
        let both = impulse_response(&base);
        let no_early = impulse_response(&[base[0], base[1], ("early", 0.0)]);
        let no_late = impulse_response(&[base[0], base[1], ("late", 0.0)]);
        // Without the late part the tail (after 150 ms) is silent; without the early part the first 15 ms are.
        assert!(energy(&no_late[7200..]) < energy(&both[7200..]) * 1e-4, "late level 0 removes the tail");
        assert!(energy(&no_early[..700]) < energy(&both[..700]) * 0.05, "early level 0 removes the first reflections");
        assert!(energy(&no_early[7200..]) > energy(&both[7200..]) * 0.3, "the tail stays");
    }

    #[test]
    fn early_and_late_eq_act_on_their_own_part() {
        let base = [("predelay", 0.1f32), ("decay", 2.0)];
        let plain = impulse_response(&base);
        let dark_late = impulse_response(&[base[0], base[1], ("late_hi", 1500.0)]);
        let dark_early = impulse_response(&[base[0], base[1], ("er_hi", 1500.0)]);
        let (early, late) = (..700, 7200..);
        // Late high cut darkens the tail but leaves the first reflections.
        assert!(bright(&dark_late[late.clone()]) < bright(&plain[late.clone()]) * 0.3);
        assert!((bright(&dark_late[early]) / bright(&plain[early]) - 1.0).abs() < 0.05);
        // Early high cut darkens the first reflections.
        assert!(bright(&dark_early[early]) < bright(&plain[early]) * 0.3);
        // Bell boosts: +12 dB at 1.5 kHz raises the energy of the part.
        let boosted = impulse_response(&[base[0], base[1], ("late_gain", 12.0)]);
        assert!(energy(&boosted[late.clone()]) > energy(&plain[late]) * 1.5);
    }

    #[test]
    fn early_size_spreads_the_reflections() {
        let base = [("predelay", 0.1f32), ("late", 0.0)];
        let small = impulse_response(&[base[0], base[1], ("er_size", 0.3)]);
        let large = impulse_response(&[base[0], base[1], ("er_size", 2.0)]);
        let last = |x: &[f32]| x.iter().rposition(|v| v.abs() > 1e-4).unwrap_or(0);
        assert!(last(&large) > last(&small) * 3, "{} vs {}", last(&large), last(&small));
    }

    /// Decay-rate EQ: with the low part decaying 4× longer, the tail's low end lasts longer than its top.
    #[test]
    fn decay_rate_eq_shapes_the_tail() {
        let tail = |set: &[(&str, f32)]| {
            let mut p = super::reverb_def().defaults();
            p.insert("mix".into(), 1.0);
            p.insert("early".into(), 0.0);
            p.insert("decay".into(), 1.0);
            for (k, v) in set {
                p.insert((*k).into(), *v);
            }
            let mut e = crate::effects::create("reverb", 48000.0, &p).unwrap();
            let mut rng = crate::dsp::Rng::new(3);
            let mut l: Vec<f32> = (0..96000).map(|i| if i < 4800 { rng.bipolar() * 0.5 } else { 0.0 }).collect();
            let mut r = l.clone();
            for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
                e.process(a, b, None);
            }
            // Low vs high energy 1 s after the burst.
            let seg = &l[48000 + 4800..48000 + 4800 + 9600];
            let mut lp = crate::dsp::OnePole::new(200.0, 48000.0);
            let low: f32 = seg.iter().map(|x| lp.process(*x).powi(2)).sum();
            let mut lp2 = crate::dsp::OnePole::new(5000.0, 48000.0);
            let high: f32 = seg.iter().map(|x| (x - lp2.process(*x)).powi(2)).sum();
            low / high.max(1e-12)
        };
        let flat = tail(&[]);
        let warm = tail(&[("dr_1", 4.0), ("dr_2", 2.0), ("dr_4", 0.5), ("dr_5", 0.25)]);
        assert!(warm > flat * 4.0, "{flat} → {warm}");
        // Free nodes (Pro-R): the same ×4 bell moved from 300 Hz up to 6 kHz lengthens the top instead.
        let low_node = tail(&[("dr_2", 4.0)]);
        let moved = tail(&[("dr_2", 4.0), ("dr_f2", 6000.0)]);
        assert!(moved < low_node * 0.5 && moved < flat, "{low_node} → {moved} (flat {flat})");
        // Shapes: a ×4 low shelf at 1 kHz lengthens everything below it; a bell only around it; a notch
        // only a narrow region; a high shelf everything above.
        let b = |shape: usize, q: f32| [super::DrBand { shape, freq: 1000.0, mul: 4.0, q }];
        let c = super::dr_curve;
        assert!(c(&b(1, 1.0), 100.0) > 3.8 && c(&b(1, 1.0), 10000.0) < 1.1, "low shelf");
        assert!(c(&b(2, 1.0), 100.0) < 1.1 && c(&b(2, 1.0), 10000.0) > 3.8, "high shelf");
        assert!(c(&b(0, 1.0), 1000.0) > 3.9 && c(&b(0, 1.0), 2000.0) > 1.5 && c(&b(0, 1.0), 8000.0) < 1.2, "bell");
        assert!(c(&b(3, 1.0), 1000.0) > 3.9 && c(&b(3, 1.0), 1500.0) < c(&b(0, 1.0), 1500.0) * 0.8, "notch is narrower");
        // Q narrows a bell.
        assert!(c(&b(0, 4.0), 1500.0) < c(&b(0, 1.0), 1500.0));
        // Shelf in the engine: a ×4 low shelf keeps the low end of the tail longer.
        let shelf = tail(&[("dr_1", 4.0), ("dr_f1", 400.0)]);
        assert!(shelf > flat * 2.0, "{flat} → {shelf}");
    }

    #[test]
    fn reverb_tail_decays() {
        let mut p = std::collections::BTreeMap::new();
        p.insert("mix".into(), 1.0);
        p.insert("decay".into(), 0.5);
        let mut e = crate::effects::create("reverb", 48000.0, &p).unwrap();
        let mut l = vec![0.0f32; 96000];
        let mut r = l.clone();
        l[0] = 1.0;
        r[0] = 1.0;
        e.process(&mut l, &mut r, None);
        let early: f32 = l[1000..10000].iter().map(|x| x * x).sum();
        let late: f32 = l[80000..89000].iter().map(|x| x * x).sum();
        assert!(early > 0.0 && late < early * 1e-3, "{early} {late}");
    }
}
