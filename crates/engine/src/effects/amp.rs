//! Wyrmstack: a guitar / bass amplifier and speaker cabinet for direct (DI) input.
//!
//! Chain per side: noise gate → input high-pass (and the lead model's mid push) → preamp of one to three
//! valve stages at twice the sample rate → tone stack (bass shelf, mid bell, treble shelf) → power amp with
//! supply sag and the presence control → cabinet and microphone → master.
//!
//! A valve stage is `y = tanh(g·x + β) − tanh(β)`: the bias β makes it asymmetric (even harmonics), the
//! subtraction keeps it centred; a coupling high-pass between stages removes the offset the asymmetry leaves
//! and lets bass "blocking" happen as in the real circuit. The fuzz model clips hard and asymmetrically
//! instead: `y = sign(u)·(1 − e^{−|u|})` with `u = g·x + β`, the positive side pushed 1.6× harder.

use super::{Effect, EffectDef, ParamInfo};
use crate::dsp::oversample::Oversampler;
use crate::dsp::{db_to_gain, Biquad, BiquadKind};

pub const MODELS: &[&str] = &["Clean", "Crunch", "Lead", "Bass", "Fuzz"];
pub const CABS: &[&str] = &["Off", "1×12 open", "2×12", "4×12 closed", "Bass 4×10"];
pub const MICS: &[&str] = &["On axis", "Off axis", "Edge"];

const PARAMS: &[ParamInfo] = &[
    ParamInfo::choice("model", "Amp", MODELS, 1.0),
    ParamInfo::lin("gate", "Gate", -90.0, -20.0, -90.0, "dB"),
    ParamInfo::lin("drive", "Gain", 0.0, 100.0, 50.0, "%"),
    ParamInfo::lin("bass", "Bass", 0.0, 10.0, 5.0, ""),
    ParamInfo::lin("mid", "Middle", 0.0, 10.0, 5.0, ""),
    ParamInfo::lin("treble", "Treble", 0.0, 10.0, 5.0, ""),
    ParamInfo::lin("presence", "Presence", 0.0, 10.0, 5.0, ""),
    ParamInfo::lin("sag", "Sag", 0.0, 100.0, 30.0, "%"),
    ParamInfo::choice("cab", "Cabinet", CABS, 3.0),
    ParamInfo::choice("mic", "Mic", MICS, 0.0),
    ParamInfo::lin("master", "Master", -36.0, 12.0, 0.0, "dB"),
];

pub fn amp_def() -> EffectDef {
    EffectDef {
        id: "amp",
        name: "Wyrmstack (amp & cabinet)",
        category: "Distortion",
        description:
            "Guitar and bass amplifier with speaker cabinet for direct input: five amps (clean, crunch, lead, bass, fuzz), gate, tone stack, presence, power-amp sag, four cabinets and three microphone positions.",
        params: PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(Wyrmstack::new(rate)),
    }
}

/// (gain range low and high in dB, valve stages, bias, input high-pass Hz, clean blend for the lows).
fn model_spec(m: usize) -> (f32, f32, usize, f32, f32, f32) {
    match m {
        0 => (0.0, 26.0, 1, 0.10, 70.0, 0.0),
        1 => (8.0, 38.0, 2, 0.20, 90.0, 0.0),
        2 => (16.0, 56.0, 3, 0.25, 110.0, 0.0),
        3 => (0.0, 30.0, 1, 0.15, 30.0, 0.5),
        _ => (24.0, 52.0, 1, 0.30, 80.0, 0.0),
    }
}

#[derive(Clone)]
struct Side {
    gate_env: f32,
    gate_g: f32,
    hp: Biquad,
    push: Biquad,
    os: Oversampler,
    couple: [Biquad; 3],
    stage_lp: [Biquad; 3],
    low_split: Biquad,
    bass: Biquad,
    mid: Biquad,
    treble: Biquad,
    presence: Biquad,
    sag_env: f32,
    cab: [Biquad; 6],
    mic: Biquad,
}

impl Side {
    fn new() -> Side {
        Side {
            gate_env: 0.0,
            gate_g: 1.0,
            hp: Biquad::default(),
            push: Biquad::default(),
            os: Oversampler::new(2),
            couple: Default::default(),
            stage_lp: Default::default(),
            low_split: Biquad::default(),
            bass: Biquad::default(),
            mid: Biquad::default(),
            treble: Biquad::default(),
            presence: Biquad::default(),
            sag_env: 0.0,
            cab: Default::default(),
            mic: Biquad::default(),
        }
    }
}

pub struct Wyrmstack {
    rate: f32,
    model: usize,
    gate: f32,
    drive: f32,
    bass: f32,
    mid: f32,
    treble: f32,
    presence: f32,
    sag: f32,
    cab: usize,
    mic: usize,
    master: f32,
    sides: [Side; 2],
    level: f32,
    tele: [f32; 2],
}

impl Wyrmstack {
    pub fn new(rate: f32) -> Self {
        let mut w =
            Wyrmstack { rate, model: 1, gate: -90.0, drive: 0.5, bass: 5.0, mid: 5.0, treble: 5.0, presence: 5.0, sag: 0.3, cab: 3, mic: 0, master: 1.0, sides: [Side::new(), Side::new()], level: 0.0, tele: [0.0; 2] };
        w.update();
        w
    }

    /// Retunes every filter (state kept).
    fn update(&mut self) {
        let rate = self.rate;
        let os_rate = rate * 2.0;
        let (_, _, stages, _, hp, _) = model_spec(self.model);
        let bass_model = self.model == 3;
        for s in self.sides.iter_mut() {
            s.hp.set(BiquadKind::HighPass, hp, 0.707, 0.0, rate);
            // The lead model's pre-boost tightens the low end and pushes the mids into the preamp.
            s.push.set(BiquadKind::Peak, 800.0, 0.8, if self.model == 2 { 6.0 } else { 0.0 }, rate);
            for k in 0..3 {
                s.couple[k].set(BiquadKind::HighPass, if k < stages { 25.0 + 15.0 * k as f32 } else { 10.0 }, 0.6, 0.0, os_rate);
                s.stage_lp[k].set(BiquadKind::LowPass, 9000.0 - 1500.0 * k as f32, 0.6, 0.0, os_rate);
            }
            s.low_split.set(BiquadKind::LowPass, 180.0, 0.707, 0.0, rate);
            // Tone stack: ±12 dB at the ends, flat at 5.
            s.bass.set(BiquadKind::LowShelf, if bass_model { 80.0 } else { 110.0 }, 0.6, (self.bass - 5.0) * 2.4, rate);
            s.mid.set(BiquadKind::Peak, if bass_model { 500.0 } else { 700.0 }, 0.7, (self.mid - 5.0) * 2.4, rate);
            s.treble.set(BiquadKind::HighShelf, 3200.0, 0.6, (self.treble - 5.0) * 2.4, rate);
            s.presence.set(BiquadKind::HighShelf, 5500.0, 0.7, (self.presence - 5.0) * 1.6, rate);
            cab_filters(&mut s.cab, self.cab, rate);
            let (kind, f, q, g) = match self.mic {
                0 => (BiquadKind::Peak, 3500.0, 1.0, 2.5),
                1 => (BiquadKind::HighShelf, 3000.0, 0.7, -6.0),
                _ => (BiquadKind::Peak, 1200.0, 0.9, -3.0),
            };
            s.mic.set(kind, f, q, if self.cab == 0 { 0.0 } else { g }, rate);
        }
    }

    fn drive_gain(&self) -> f32 {
        let (lo, hi, ..) = model_spec(self.model);
        db_to_gain(lo + (hi - lo) * self.drive)
    }
}

/// The cabinet as six filters (unused ones flat): the box's low cut and resonance, the speaker's presence
/// region and break-up, and its steep top.
fn cab_filters(f: &mut [Biquad; 6], cab: usize, rate: f32) {
    use BiquadKind::*;
    let set: [(BiquadKind, f32, f32, f32); 6] = match cab {
        1 => [(HighPass, 90.0, 0.7, 0.0), (Peak, 140.0, 1.2, 3.0), (Peak, 2200.0, 1.0, 4.0), (Peak, 800.0, 1.4, -3.0), (LowPass, 6000.0, 0.8, 0.0), (LowPass, 6600.0, 0.7, 0.0)],
        2 => [(HighPass, 80.0, 0.75, 0.0), (Peak, 120.0, 1.4, 4.0), (Peak, 2400.0, 1.0, 4.5), (Peak, 4000.0, 2.0, -4.0), (LowPass, 5600.0, 0.85, 0.0), (LowPass, 6200.0, 0.7, 0.0)],
        3 => [(HighPass, 70.0, 0.8, 0.0), (Peak, 105.0, 1.6, 5.0), (Peak, 2600.0, 1.1, 4.0), (Notch, 4300.0, 2.5, 0.0), (LowPass, 5200.0, 0.9, 0.0), (LowPass, 5800.0, 0.7, 0.0)],
        4 => [(HighPass, 45.0, 0.7, 0.0), (Peak, 90.0, 1.2, 3.0), (Peak, 1800.0, 1.0, 3.0), (Peak, 600.0, 1.2, -2.0), (LowPass, 4500.0, 0.8, 0.0), (LowPass, 5200.0, 0.7, 0.0)],
        _ => [(Peak, 1000.0, 0.7, 0.0); 6],
    };
    for (b, (k, fr, q, g)) in f.iter_mut().zip(set) {
        b.set(k, fr, q, g, rate);
    }
}

/// The cabinet and microphone filters of `cab` / `mic` (for displays): empty when the cabinet is off.
pub fn cab_response(cab: usize, mic: usize, rate: f32) -> Vec<Biquad> {
    if cab == 0 {
        return Vec::new();
    }
    let mut f: [Biquad; 6] = Default::default();
    cab_filters(&mut f, cab, rate);
    let mut v = f.to_vec();
    let (kind, fr, q, g) = match mic {
        0 => (BiquadKind::Peak, 3500.0, 1.0, 2.5),
        1 => (BiquadKind::HighShelf, 3000.0, 0.7, -6.0),
        _ => (BiquadKind::Peak, 1200.0, 0.9, -3.0),
    };
    v.push(Biquad::new(kind, fr, q, g, rate));
    v
}

impl Effect for Wyrmstack {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "model" => self.model = (v.round().max(0.0) as usize).min(MODELS.len() - 1),
            "gate" => self.gate = v,
            "drive" => self.drive = v.clamp(0.0, 100.0) / 100.0,
            "bass" => self.bass = v.clamp(0.0, 10.0),
            "mid" => self.mid = v.clamp(0.0, 10.0),
            "treble" => self.treble = v.clamp(0.0, 10.0),
            "presence" => self.presence = v.clamp(0.0, 10.0),
            "sag" => self.sag = v.clamp(0.0, 100.0) / 100.0,
            "cab" => self.cab = (v.round().max(0.0) as usize).min(CABS.len() - 1),
            "mic" => self.mic = (v.round().max(0.0) as usize).min(MICS.len() - 1),
            "master" => self.master = db_to_gain(v),
            _ => return,
        }
        if !matches!(id, "gate" | "drive" | "sag" | "master") {
            self.update();
        }
    }

    fn reset(&mut self) {
        self.sides = [Side::new(), Side::new()];
        self.update();
    }

    fn latency(&self) -> usize {
        self.sides[0].os.latency()
    }

    fn telemetry(&self) -> &[f32] {
        &self.tele
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let (_, _, stages, bias, _, blend) = model_spec(self.model);
        let fuzz = self.model == 4;
        let g = self.drive_gain();
        // Output trim: higher gain settings come out at a similar level.
        let trim = db_to_gain(-6.0 - 0.25 * 20.0 * g.log10()) * self.master;
        let gate_thr = db_to_gain(self.gate);
        let gate_on = self.gate > -89.5;
        let (ga, gr) = ((-1.0 / (0.001 * self.rate)).exp(), (-1.0 / (0.08 * self.rate)).exp());
        let sag_c = (-1.0 / (0.05 * self.rate)).exp();
        let shaped = |u: f32| -> f32 {
            if fuzz {
                let v = if u > 0.0 { u * 1.6 } else { u };
                v.signum() * (1.0 - (-v.abs()).exp())
            } else {
                (u + bias).tanh() - bias.tanh()
            }
        };
        let mut peak = 0.0f32;
        for (c, buf) in [&mut *l, &mut *r].into_iter().enumerate() {
            let s = &mut self.sides[c];
            for x in buf.iter_mut() {
                let mut v = *x;
                // Gate: a fast envelope opens it, it closes over 80 ms below the threshold.
                if gate_on {
                    s.gate_env = v.abs().max(s.gate_env * gr);
                    let goal = if s.gate_env > gate_thr { 1.0 } else { 0.0 };
                    s.gate_g = goal + (s.gate_g - goal) * if goal > s.gate_g { ga } else { gr };
                    v *= s.gate_g;
                }
                let dry_low = s.low_split.process(v);
                v = s.push.process(s.hp.process(v)) * g;
                // Preamp at 2×.
                let mut up = [0.0f32; 2];
                s.os.up(v, &mut up);
                for u in up.iter_mut() {
                    let mut y = *u;
                    for k in 0..stages {
                        y = shaped(y);
                        y = s.stage_lp[k].process(s.couple[k].process(y));
                        if k + 1 < stages {
                            y *= 4.0;
                        }
                    }
                    *u = y;
                }
                v = s.os.down(&up);
                // The bass amp keeps a share of the clean lows under the distortion.
                v += blend * dry_low * g.min(4.0) * 0.25;
                v = s.treble.process(s.mid.process(s.bass.process(v)));
                // Power amp: supply sag lowers the gain under load, then a soft ceiling.
                s.sag_env = v.abs().max(s.sag_env * sag_c);
                let sag = 1.0 / (1.0 + self.sag * 1.5 * s.sag_env);
                v = (1.4 * v * sag).tanh() / 1.4;
                v = s.presence.process(v);
                if self.cab > 0 {
                    for f in s.cab.iter_mut() {
                        v = f.process(v);
                    }
                    v = s.mic.process(v);
                }
                *x = v * trim;
                peak = peak.max(x.abs());
            }
        }
        self.level = self.level * 0.9 + peak * 0.1;
        self.tele = [self.level, self.sides[0].gate_g];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// More gain, more harmonics; every model stays finite and bounded.
    #[test]
    fn models_distort_more_with_gain() {
        let rate = 48000.0;
        for m in 0..MODELS.len() {
            let mut thd = [0.0f32; 2];
            for (k, drive) in [10.0, 90.0].iter().enumerate() {
                let mut a = Wyrmstack::new(rate);
                a.set_param("model", m as f32);
                a.set_param("cab", 0.0);
                a.set_param("drive", *drive);
                let n = 9600;
                let f = 200.0;
                let mut l: Vec<f32> = (0..n).map(|i| (std::f32::consts::TAU * f * i as f32 / rate).sin() * 0.3).collect();
                let mut r = l.clone();
                a.process(&mut l, &mut r, None);
                assert!(l.iter().all(|x| x.is_finite() && x.abs() < 8.0), "{}", MODELS[m]);
                // Energy outside the fundamental (projection on the 200 Hz sine / cosine removed).
                let tail = &l[4800..];
                let (mut s, mut co, mut e) = (0.0f32, 0.0f32, 0.0f32);
                for (i, v) in tail.iter().enumerate() {
                    let ph = std::f32::consts::TAU * f * (i + 4800) as f32 / rate;
                    s += v * ph.sin();
                    co += v * ph.cos();
                    e += v * v;
                }
                let fund = 2.0 * (s * s + co * co) / tail.len() as f32;
                thd[k] = (e - fund).max(0.0) / e.max(1e-12);
            }
            assert!(thd[1] > thd[0], "{}: {thd:?}", MODELS[m]);
        }
    }
}
