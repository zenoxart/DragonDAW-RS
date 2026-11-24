//! The flat-parameter instruments: Hearthfire (4-operator FM), Basilica (tonewheel organ with rotary
//! speaker), Wyrmbass (808 bass), Chorale (strings, choir and pads) and Sandstorm (granular sampler).
//!
//! Each is described by an [`EffectDef`]-style parameter list (so the designed panels, value menus, manual and
//! factory presets work as on the effects) and stores its values in [`NativeParams`].

use super::{InstrumentVoice, NoteOn};
use crate::dsp::{db_to_gain, Biquad, BiquadKind, DelayLine, Rng};
use crate::effects::{EffectDef, ParamInfo};
use crate::model::Instrument as Model;
use crate::samples::{AudioData, SampleCache};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::f32::consts::TAU;
use std::path::PathBuf;
use std::sync::Arc;

/// The model of a native instrument: which one, its values, and a file (Sandstorm's sample).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct NativeParams {
    pub kind: String,
    #[serde(default)]
    pub params: BTreeMap<String, f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<PathBuf>,
}

impl NativeParams {
    pub fn new(kind: &str) -> NativeParams {
        NativeParams { kind: kind.into(), params: def(kind).map(|d| d.defaults()).unwrap_or_default(), file: None }
    }
    pub fn v(&self, id: &str) -> f32 {
        def(&self.kind).map(|d| crate::effects::value(d, &self.params, id)).unwrap_or(0.0)
    }
}

fn no_effect(rate: f32) -> Box<dyn crate::effects::Effect> {
    (crate::effects::find("utility").expect("utility").create)(rate)
}

pub const ALGORITHMS: &[&str] = &["4>3>2>1", "(3+4)>2>1", "4>3, 2>1", "(2+3+4)>1", "4>3>2, 1", "4>(1,2,3)", "4>3, 1, 2", "1+2+3+4"];
pub const PERCUSSION: &[&str] = &["Off", "2nd", "3rd"];
pub const VIBRATOS: &[&str] = &["Off", "V1", "V2", "V3", "C1", "C2", "C3"];
pub const ROTARY: &[&str] = &["Off", "Slow", "Fast"];
pub const ENSEMBLES: &[&str] = &["Strings", "Choir aah", "Choir ooh", "Warm pad", "Glass pad", "Brass"];

macro_rules! fm_op {
    ($n:literal, $ratio:expr, $level:expr, $d:expr, $s:expr) => {
        [
            ParamInfo::lin(concat!("op", $n, "_ratio"), concat!("Op ", $n, " ratio"), 0.5, 16.0, $ratio, "x"),
            ParamInfo::lin(concat!("op", $n, "_detune"), concat!("Op ", $n, " detune"), -50.0, 50.0, 0.0, "ct"),
            ParamInfo::lin(concat!("op", $n, "_level"), concat!("Op ", $n, " level"), 0.0, 1.0, $level, ""),
            ParamInfo::log(concat!("op", $n, "_attack"), concat!("Op ", $n, " attack"), 0.5, 5000.0, 2.0, "ms"),
            ParamInfo::log(concat!("op", $n, "_decay"), concat!("Op ", $n, " decay"), 5.0, 10000.0, $d, "ms"),
            ParamInfo::lin(concat!("op", $n, "_sustain"), concat!("Op ", $n, " sustain"), 0.0, 1.0, $s, ""),
            ParamInfo::log(concat!("op", $n, "_release"), concat!("Op ", $n, " release"), 5.0, 10000.0, 300.0, "ms"),
        ]
    };
}

fn fm_params() -> Vec<ParamInfo> {
    let mut v = vec![ParamInfo::choice("algorithm", "Algorithm", ALGORITHMS, 2.0), ParamInfo::lin("feedback", "Feedback", 0.0, 1.0, 0.1, ""), ParamInfo::lin("velocity", "Velocity → brightness", 0.0, 1.0, 0.6, "")];
    // Defaults: an electric piano (two pairs, 1:1 and 1:14 with a quick bell).
    v.extend(fm_op!("1", 1.0, 0.8, 2500.0, 0.0));
    v.extend(fm_op!("2", 1.0, 0.35, 1200.0, 0.1));
    v.extend(fm_op!("3", 1.0, 0.6, 3000.0, 0.0));
    v.extend(fm_op!("4", 14.0, 0.18, 250.0, 0.0));
    v.push(ParamInfo::lin("gain", "Gain", -24.0, 12.0, 0.0, "dB"));
    v
}

fn organ_params() -> Vec<ParamInfo> {
    const NAMES: [(&str, &str, f32); 9] =
        [("d1", "16'", 8.0), ("d2", "5 1/3'", 8.0), ("d3", "8'", 8.0), ("d4", "4'", 0.0), ("d5", "2 2/3'", 0.0), ("d6", "2'", 0.0), ("d7", "1 3/5'", 0.0), ("d8", "1 1/3'", 0.0), ("d9", "1'", 0.0)];
    let mut v: Vec<ParamInfo> = NAMES.iter().map(|(id, n, d)| ParamInfo::lin(id, n, 0.0, 8.0, *d, "")).collect();
    v.extend([
        ParamInfo::choice("percussion", "Percussion", PERCUSSION, 0.0),
        ParamInfo::toggle("perc_fast", "Fast decay", true),
        ParamInfo::choice("vibrato", "Vibrato / chorus", VIBRATOS, 6.0),
        ParamInfo::choice("rotary", "Rotary", ROTARY, 1.0),
        ParamInfo::lin("click", "Key click", 0.0, 100.0, 30.0, "%"),
        ParamInfo::lin("drive", "Drive", 0.0, 100.0, 20.0, "%"),
        ParamInfo::lin("gain", "Gain", -24.0, 12.0, 0.0, "dB"),
    ]);
    v
}

fn bass_params() -> Vec<ParamInfo> {
    vec![
        ParamInfo::lin("tune", "Tune", -12.0, 12.0, 0.0, "st"),
        ParamInfo::lin("pitch_amt", "Pitch drop", 0.0, 36.0, 12.0, "st"),
        ParamInfo::log("pitch_time", "Pitch time", 5.0, 500.0, 45.0, "ms"),
        ParamInfo::log("decay", "Decay", 50.0, 8000.0, 1400.0, "ms"),
        ParamInfo::toggle("hold", "Hold while pressed", true),
        ParamInfo::log("glide", "Glide", 1.0, 1000.0, 60.0, "ms"),
        ParamInfo::lin("drive", "Drive", 0.0, 100.0, 30.0, "%"),
        ParamInfo::log("tone", "Tone", 200.0, 12000.0, 4000.0, "Hz"),
        ParamInfo::lin("click", "Click", 0.0, 100.0, 25.0, "%"),
        ParamInfo::lin("sub", "Sub octave", 0.0, 100.0, 0.0, "%"),
        ParamInfo::lin("gain", "Gain", -24.0, 12.0, 0.0, "dB"),
    ]
}

fn ensemble_params() -> Vec<ParamInfo> {
    vec![
        ParamInfo::choice("mode", "Ensemble", ENSEMBLES, 0.0),
        ParamInfo::lin("voices", "Unison", 1.0, 7.0, 5.0, ""),
        ParamInfo::lin("detune", "Detune", 0.0, 50.0, 14.0, "ct"),
        ParamInfo::lin("width", "Width", 0.0, 100.0, 80.0, "%"),
        ParamInfo::log("attack", "Attack", 1.0, 4000.0, 350.0, "ms"),
        ParamInfo::log("release", "Release", 10.0, 8000.0, 900.0, "ms"),
        ParamInfo::lin("brightness", "Brightness", 0.0, 100.0, 50.0, "%"),
        ParamInfo::lin("vibrato", "Vibrato", 0.0, 100.0, 25.0, "%"),
        ParamInfo::lin("vib_rate", "Vibrato rate", 2.0, 8.0, 5.2, "Hz"),
        ParamInfo::lin("air", "Breath / bow noise", 0.0, 100.0, 15.0, "%"),
        ParamInfo::lin("gain", "Gain", -24.0, 12.0, 0.0, "dB"),
    ]
}

fn granular_params() -> Vec<ParamInfo> {
    vec![
        ParamInfo::lin("position", "Position", 0.0, 100.0, 30.0, "%"),
        ParamInfo::lin("scan", "Scan", -100.0, 100.0, 5.0, "%/s"),
        ParamInfo::lin("spray", "Spray", 0.0, 100.0, 10.0, "%"),
        ParamInfo::log("size", "Grain size", 10.0, 500.0, 90.0, "ms"),
        ParamInfo::log("density", "Density", 2.0, 100.0, 24.0, "/s"),
        ParamInfo::lin("pitch_rand", "Pitch random", 0.0, 12.0, 0.1, "st"),
        ParamInfo::lin("reverse", "Reverse", 0.0, 100.0, 0.0, "%"),
        ParamInfo::lin("spread", "Spread", 0.0, 100.0, 70.0, "%"),
        ParamInfo::log("attack", "Attack", 1.0, 4000.0, 120.0, "ms"),
        ParamInfo::log("release", "Release", 10.0, 8000.0, 900.0, "ms"),
        ParamInfo::lin("gain", "Gain", -24.0, 12.0, 0.0, "dB"),
    ]
}

/// The instruments' definitions.
pub fn defs() -> &'static [EffectDef] {
    static D: std::sync::OnceLock<Vec<EffectDef>> = std::sync::OnceLock::new();
    D.get_or_init(|| {
        let leak = |v: Vec<ParamInfo>| -> &'static [ParamInfo] { Box::leak(v.into_boxed_slice()) };
        vec![
            EffectDef {
                id: "fm",
                name: "Hearthfire (FM synth)",
                category: "Instrument",
                description: "Four-operator FM synthesizer: eight algorithms, operator feedback, an envelope per operator and velocity that brightens. Electric pianos, bells, basses, brass.",
                params: leak(fm_params()),
                uses_sidechain: false,
                create: no_effect,
            },
            EffectDef {
                id: "organ",
                name: "Basilica (tonewheel organ)",
                category: "Instrument",
                description: "Tonewheel organ: nine drawbars, percussion, scanner vibrato / chorus, key click, drive and a rotary speaker with slow and fast speeds.",
                params: leak(organ_params()),
                uses_sidechain: false,
                create: no_effect,
            },
            EffectDef {
                id: "bass808",
                name: "Wyrmbass (808 bass)",
                category: "Instrument",
                description: "808-style bass: a sine with a pitch drop, long decay, glide between notes, drive, tone, click and a sub octave. Plays one note at a time.",
                params: leak(bass_params()),
                uses_sidechain: false,
                create: no_effect,
            },
            EffectDef {
                id: "ensemble",
                name: "Chorale (strings, choir, pads)",
                category: "Instrument",
                description: "Ensemble synth: strings, two choirs, two pads and brass from detuned unison voices, formant filters, vibrato and breath or bow noise.",
                params: leak(ensemble_params()),
                uses_sidechain: false,
                create: no_effect,
            },
            EffectDef {
                id: "granular_inst",
                name: "Sandstorm (granular)",
                category: "Instrument",
                description: "Granular sampler: plays clouds of grains from a sample (or its built-in texture) across the keyboard – position, scan, spray, size, density, pitch randomness, reverse, spread.",
                params: leak(granular_params()),
                uses_sidechain: false,
                create: no_effect,
            },
        ]
    })
}

pub fn def(kind: &str) -> Option<&'static EffectDef> {
    defs().iter().find(|d| d.id == kind)
}

/// Creates the engine of a native instrument.
pub fn create(p: &NativeParams, rate: f32) -> Option<Box<dyn InstrumentVoice>> {
    Some(match p.kind.as_str() {
        "fm" => Box::new(Fm::new(rate)),
        "organ" => Box::new(Organ::new(rate)),
        "bass808" => Box::new(Bass808::new(rate)),
        "ensemble" => Box::new(Ensemble::new(rate)),
        "granular_inst" => Box::new(Sandstorm::new(rate)),
        _ => return None,
    })
}

// ------------------------------------------------------------------------------------------------- shared

/// A linear-attack, exponential decay / release ADSR (times in seconds).
#[derive(Clone, Copy, Default)]
struct Adsr {
    stage: u8, // 0 idle, 1 attack, 2 decay/sustain, 3 release
    level: f32,
}

impl Adsr {
    fn on(&mut self) {
        self.stage = 1;
    }
    fn off(&mut self) {
        if self.stage != 0 {
            self.stage = 3;
        }
    }
    #[inline]
    fn next(&mut self, a: f32, d: f32, s: f32, r: f32, rate: f32) -> f32 {
        match self.stage {
            1 => {
                self.level += 1.0 / (a * rate).max(1.0);
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = 2;
                }
            }
            2 => self.level = s + (self.level - s) * (-1.0 / (d * rate * 0.25).max(1.0)).exp(),
            3 => {
                self.level *= (-1.0 / (r * rate * 0.25).max(1.0)).exp();
                if self.level < 1e-4 {
                    self.level = 0.0;
                    self.stage = 0;
                }
            }
            _ => {}
        }
        self.level
    }
    fn active(&self) -> bool {
        self.stage != 0
    }
}

fn hz(note: f32) -> f32 {
    440.0 * 2f32.powf((note - 69.0) / 12.0)
}

/// A pool of voices with oldest-first stealing.
fn pick<V>(voices: &mut [V], age: impl Fn(&V) -> (bool, u64)) -> usize {
    let mut best = 0;
    let mut key = (true, u64::MAX);
    for (i, v) in voices.iter().enumerate() {
        let (busy, t) = age(v);
        if !busy {
            return i;
        }
        if (busy, t) < key {
            key = (busy, t);
            best = i;
        }
    }
    best
}

fn param_map(model: &Model) -> Option<&NativeParams> {
    match model {
        Model::Native(p) => Some(p),
        _ => None,
    }
}

// --------------------------------------------------------------------------------------------------- FM

const FM_VOICES: usize = 16;

/// Modulators of each operator and the carriers, per algorithm (operators 0 … 3 = 1 … 4).
const FM_ROUTES: [([&[usize]; 4], &[usize]); 8] = [
    ([&[1], &[2], &[3], &[]], &[0]),
    ([&[1], &[2, 3], &[], &[]], &[0]),
    ([&[1], &[], &[3], &[]], &[0, 2]),
    ([&[1, 2, 3], &[], &[], &[]], &[0]),
    ([&[], &[2], &[3], &[]], &[0, 1]),
    ([&[3], &[3], &[3], &[]], &[0, 1, 2]),
    ([&[], &[], &[3], &[]], &[0, 1, 2]),
    ([&[], &[], &[], &[]], &[0, 1, 2, 3]),
];

#[derive(Clone, Copy, Default)]
struct FmVoice {
    note: u8,
    vel: f32,
    t: u64,
    phase: [f32; 4],
    env: [Adsr; 4],
    out: [f32; 4],
    fb: [f32; 2],
}

pub struct Fm {
    rate: f32,
    p: BTreeMap<String, f32>,
    alg: usize,
    feedback: f32,
    velocity: f32,
    ops: [[f32; 7]; 4],
    gain: f32,
    voices: [FmVoice; FM_VOICES],
    clock: u64,
}

impl Fm {
    fn new(rate: f32) -> Fm {
        let mut f = Fm { rate, p: BTreeMap::new(), alg: 2, feedback: 0.1, velocity: 0.6, ops: [[0.0; 7]; 4], gain: 1.0, voices: [FmVoice::default(); FM_VOICES], clock: 0 };
        f.apply(&def("fm").expect("fm").defaults());
        f
    }
    fn apply(&mut self, p: &BTreeMap<String, f32>) {
        let d = def("fm").expect("fm");
        let v = |id: &str| crate::effects::value(d, p, id);
        self.alg = (v("algorithm").round().max(0.0) as usize).min(7);
        self.feedback = v("feedback");
        self.velocity = v("velocity");
        for k in 0..4 {
            let n = k + 1;
            for (j, s) in ["ratio", "detune", "level", "attack", "decay", "sustain", "release"].iter().enumerate() {
                self.ops[k][j] = v(&format!("op{n}_{s}"));
            }
        }
        self.gain = db_to_gain(v("gain"));
        self.p = p.clone();
    }
}

impl InstrumentVoice for Fm {
    fn note_on(&mut self, n: NoteOn) {
        self.clock += 1;
        let i = pick(&mut self.voices, |v| (v.env.iter().any(|e| e.active()), v.t));
        let v = &mut self.voices[i];
        *v = FmVoice { note: n.note, vel: n.velocity, t: self.clock, ..Default::default() };
        for e in v.env.iter_mut() {
            e.on();
        }
    }
    fn note_off(&mut self, note: u8) {
        for v in self.voices.iter_mut().filter(|v| v.note == note) {
            for e in v.env.iter_mut() {
                e.off();
            }
        }
    }
    fn choke(&mut self, _s: f32) {
        self.voices = [FmVoice::default(); FM_VOICES];
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let (mods, carriers) = FM_ROUTES[self.alg];
        let rate = self.rate;
        let norm = 0.5 / (carriers.len() as f32).sqrt();
        for v in self.voices.iter_mut().filter(|v| v.env.iter().any(|e| e.active())) {
            let f0 = hz(v.note as f32);
            let mut inc = [0.0f32; 4];
            for k in 0..4 {
                inc[k] = f0 * self.ops[k][0] * 2f32.powf(self.ops[k][1] / 1200.0) / rate;
            }
            // Velocity brightens: modulators scale with it, carriers stay.
            let vel_mod = 1.0 - self.velocity + self.velocity * v.vel;
            for i in 0..l.len() {
                let mut env = [0.0f32; 4];
                for k in (0..4).rev() {
                    let o = &self.ops[k];
                    env[k] = v.env[k].next(o[3] * 0.001, o[4] * 0.001, o[5], o[6] * 0.001, rate);
                }
                // Operators in descending order, so modulators are computed before what they modulate.
                for k in (0..4).rev() {
                    let mut pm = 0.0;
                    for &m in mods[k] {
                        pm += v.out[m];
                    }
                    if k == 3 {
                        pm += self.feedback * 0.5 * (v.fb[0] + v.fb[1]);
                    }
                    v.phase[k] = (v.phase[k] + inc[k]).fract();
                    let s = (TAU * v.phase[k] + pm * 2.0 * std::f32::consts::PI).sin();
                    let is_carrier = carriers.contains(&k);
                    let lvl = self.ops[k][2] * env[k] * if is_carrier { v.vel.sqrt() } else { vel_mod };
                    v.out[k] = s * lvl;
                    if k == 3 {
                        v.fb = [v.out[3], v.fb[0]];
                    }
                }
                let mut y = 0.0;
                for &c in carriers {
                    y += v.out[c];
                }
                let y = y * norm * self.gain;
                l[i] += y;
                r[i] += y;
            }
        }
    }
    fn update(&mut self, model: &Model, _c: &SampleCache) {
        if let Some(p) = param_map(model) {
            if p.params != self.p {
                let p = p.params.clone();
                self.apply(&p);
            }
        }
    }
    fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.env.iter().any(|e| e.active())).count()
    }
}

// ------------------------------------------------------------------------------------------------- organ

const ORGAN_RATIOS: [f32; 9] = [0.5, 1.5, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 8.0];
const ORGAN_VOICES: usize = 32;

#[derive(Clone, Copy, Default)]
struct OrganVoice {
    note: u8,
    on: bool,
    gate: f32,
    phase: [f32; 9],
    perc: f32,
    click: f32,
    t: u64,
}

pub struct Organ {
    rate: f32,
    p: BTreeMap<String, f32>,
    bars: [f32; 9],
    percussion: usize,
    perc_fast: bool,
    vibrato: usize,
    rotary: usize,
    click: f32,
    drive: f32,
    gain: f32,
    voices: [OrganVoice; ORGAN_VOICES],
    rng: Rng,
    clock: u64,
    vib_line: DelayLine,
    vib_ph: f32,
    horn_ph: f32,
    drum_ph: f32,
    horn_speed: f32,
    drum_speed: f32,
    split: Biquad,
    horn_line: [DelayLine; 2],
    mono: Vec<f32>,
}

impl Organ {
    fn new(rate: f32) -> Organ {
        let mut o = Organ {
            rate,
            p: BTreeMap::new(),
            bars: [0.0; 9],
            percussion: 0,
            perc_fast: true,
            vibrato: 6,
            rotary: 1,
            click: 0.3,
            drive: 0.2,
            gain: 1.0,
            voices: [OrganVoice::default(); ORGAN_VOICES],
            rng: Rng::new(0x0e9),
            clock: 0,
            vib_line: DelayLine::new((0.004 * rate) as usize + 4),
            vib_ph: 0.0,
            horn_ph: 0.0,
            drum_ph: 0.25,
            horn_speed: 0.8,
            drum_speed: 0.7,
            split: Biquad::new(BiquadKind::LowPass, 800.0, 0.707, 0.0, rate),
            horn_line: [DelayLine::new((0.003 * rate) as usize + 4), DelayLine::new((0.003 * rate) as usize + 4)],
            mono: vec![0.0; 4096],
        };
        o.apply(&def("organ").expect("organ").defaults());
        o
    }
    fn apply(&mut self, p: &BTreeMap<String, f32>) {
        let d = def("organ").expect("organ");
        let v = |id: &str| crate::effects::value(d, p, id);
        for k in 0..9 {
            self.bars[k] = v(&format!("d{}", k + 1)) / 8.0;
        }
        self.percussion = (v("percussion").round().max(0.0) as usize).min(2);
        self.perc_fast = v("perc_fast") >= 0.5;
        self.vibrato = (v("vibrato").round().max(0.0) as usize).min(6);
        self.rotary = (v("rotary").round().max(0.0) as usize).min(2);
        self.click = v("click") / 100.0;
        self.drive = v("drive") / 100.0;
        self.gain = db_to_gain(v("gain"));
        self.p = p.clone();
    }
}

impl InstrumentVoice for Organ {
    fn note_on(&mut self, n: NoteOn) {
        self.clock += 1;
        // Single-trigger percussion: only when no other key is held (as on the tonewheel organs).
        let perc = if self.voices.iter().any(|v| v.on) { 0.0 } else { 1.0 };
        let i = pick(&mut self.voices, |v| (v.on || v.gate > 0.0, v.t));
        self.voices[i] = OrganVoice { note: n.note, on: true, gate: self.voices[i].gate, phase: [0.0; 9], perc, click: 1.0, t: self.clock };
    }
    fn note_off(&mut self, note: u8) {
        for v in self.voices.iter_mut().filter(|v| v.note == note) {
            v.on = false;
        }
    }
    fn choke(&mut self, _s: f32) {
        for v in self.voices.iter_mut() {
            v.on = false;
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len().min(self.mono.len());
        let rate = self.rate;
        let ramp = 1.0 / (0.005 * rate); // 5 ms gate ramps (a tonewheel contact, not a click)
        let perc_k = (-1.0 / ((if self.perc_fast { 0.2 } else { 1.0 }) * rate * 0.25)).exp();
        let perc_ratio = if self.percussion == 1 { 2.0 } else { 3.0 };
        let mono = &mut self.mono[..n];
        mono.fill(0.0);
        let bar_sum: f32 = self.bars.iter().sum::<f32>().max(1.0);
        for v in self.voices.iter_mut().filter(|v| v.on || v.gate > 0.0) {
            let f0 = hz(v.note as f32);
            for m in mono.iter_mut() {
                v.gate = if v.on { (v.gate + ramp).min(1.0) } else { (v.gate - ramp).max(0.0) };
                let mut s = 0.0;
                for k in 0..9 {
                    v.phase[k] = (v.phase[k] + f0 * ORGAN_RATIOS[k] / rate).fract();
                    if self.bars[k] > 0.0 {
                        s += (TAU * v.phase[k]).sin() * self.bars[k];
                    }
                }
                s /= bar_sum;
                if self.percussion > 0 {
                    let ph = (v.phase[2] * perc_ratio).fract();
                    s += (TAU * ph).sin() * v.perc * 0.6;
                    v.perc *= perc_k;
                }
                // Key click: a few milliseconds of filtered noise at the contact.
                if v.click > 1e-3 {
                    s += self.rng.bipolar() * v.click * self.click * 0.25;
                    v.click *= 0.995;
                }
                *m += s * v.gate * 0.35;
            }
        }
        // Scanner vibrato / chorus: a delay swept at 6.9 Hz; chorus mixes it with the dry signal.
        let depth = [0.0, 0.3, 0.6, 1.0, 0.3, 0.6, 1.0][self.vibrato] * 0.0012 * rate;
        let chorus = self.vibrato >= 4;
        for m in mono.iter_mut() {
            self.vib_line.write(*m);
            if depth > 0.0 {
                self.vib_ph = (self.vib_ph + 6.9 / rate).fract();
                let d = 1.0 + depth * (0.5 + 0.5 * (TAU * self.vib_ph).sin());
                let w = self.vib_line.read(d);
                *m = if chorus { 0.5 * (*m + w) } else { w };
            }
            if self.drive > 0.0 {
                let k = 1.0 + 4.0 * self.drive;
                *m = (*m * k).tanh() / k.sqrt();
            }
        }
        // Rotary speaker: the horn (above 800 Hz) and the drum (below) spin at their own speeds, each
        // speeding up and slowing down with its own inertia; amplitude and Doppler follow the rotation.
        let (horn_goal, drum_goal) = match self.rotary {
            0 => (0.0, 0.0),
            1 => (0.8, 0.7),
            _ => (6.7, 5.8),
        };
        let horn_k = 1.0 / (0.6 * rate);
        let drum_k = 1.0 / (2.5 * rate);
        for i in 0..n {
            let x = mono[i];
            if self.rotary == 0 {
                l[i] += x * self.gain;
                r[i] += x * self.gain;
                continue;
            }
            self.horn_speed += (horn_goal - self.horn_speed) * horn_k;
            self.drum_speed += (drum_goal - self.drum_speed) * drum_k;
            self.horn_ph = (self.horn_ph + self.horn_speed / rate).fract();
            self.drum_ph = (self.drum_ph + self.drum_speed / rate).fract();
            let low = self.split.process(x);
            let high = x - low;
            let (hs, hc) = (TAU * self.horn_ph).sin_cos();
            let ds = (TAU * self.drum_ph).sin();
            let mut out = [0.0f32; 2];
            for (c, o) in out.iter_mut().enumerate() {
                let side = if c == 0 { 1.0 } else { -1.0 };
                self.horn_line[c].write(high);
                let doppler = 1.0 + 0.0012 * rate * (0.5 + 0.5 * side * hc);
                let h = self.horn_line[c].read(doppler) * (0.75 + 0.25 * side * hs);
                let d = low * (0.85 + 0.15 * side * ds);
                *o = (h + d) * self.gain;
            }
            l[i] += out[0];
            r[i] += out[1];
        }
    }
    fn update(&mut self, model: &Model, _c: &SampleCache) {
        if let Some(p) = param_map(model) {
            if p.params != self.p {
                let p = p.params.clone();
                self.apply(&p);
            }
        }
    }
    fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.on || v.gate > 0.0).count()
    }
}

// --------------------------------------------------------------------------------------------------- 808

pub struct Bass808 {
    rate: f32,
    p: BTreeMap<String, f32>,
    v: [f32; 11],
    held: [u8; 16],
    n_held: usize,
    note: f32,
    target: f32,
    phase: f32,
    sub_phase: f32,
    amp: f32,
    pitch_env: f32,
    click: f32,
    gate: bool,
    tone: Biquad,
    vel: f32,
}

impl Bass808 {
    fn new(rate: f32) -> Bass808 {
        let mut b = Bass808 {
            rate,
            p: BTreeMap::new(),
            v: [0.0; 11],
            held: [0; 16],
            n_held: 0,
            note: 36.0,
            target: 36.0,
            phase: 0.0,
            sub_phase: 0.0,
            amp: 0.0,
            pitch_env: 0.0,
            click: 0.0,
            gate: false,
            tone: Biquad::default(),
            vel: 1.0,
        };
        b.apply(&def("bass808").expect("808").defaults());
        b
    }
    fn apply(&mut self, p: &BTreeMap<String, f32>) {
        let d = def("bass808").expect("808");
        for (k, prm) in d.params.iter().enumerate() {
            self.v[k] = crate::effects::value(d, p, prm.id);
        }
        self.tone.set(BiquadKind::LowPass, self.v[7].min(self.rate * 0.45), 0.707, 0.0, self.rate);
        self.p = p.clone();
    }
}

impl InstrumentVoice for Bass808 {
    fn note_on(&mut self, n: NoteOn) {
        let legato = self.gate && self.amp > 0.01;
        if self.n_held < self.held.len() {
            self.held[self.n_held] = n.note;
            self.n_held += 1;
        }
        self.target = n.note as f32;
        if !legato && !n.slide {
            // A new hit: pitch drop and click from the top.
            self.note = self.target;
            self.pitch_env = 1.0;
            self.click = 1.0;
            self.phase = 0.0;
        }
        self.amp = 1.0;
        self.vel = n.velocity;
        self.gate = true;
    }
    fn note_off(&mut self, note: u8) {
        if let Some(i) = self.held[..self.n_held].iter().position(|h| *h == note) {
            self.held.copy_within(i + 1..self.n_held, i);
            self.n_held -= 1;
        }
        if self.n_held == 0 {
            self.gate = false;
        } else {
            // Last-note priority: glide back to the newest key still held.
            self.target = self.held[self.n_held - 1] as f32;
        }
    }
    fn choke(&mut self, _s: f32) {
        self.amp = 0.0;
        self.gate = false;
        self.n_held = 0;
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let rate = self.rate;
        let [tune, pitch_amt, pitch_time, decay, hold, glide, drive, _tone, click, sub, gain] = self.v;
        let dec_k = (-6.9 / (decay * 0.001 * rate)).exp();
        let rel_k = (-6.9 / (0.08 * rate)).exp();
        let pk = (-1.0 / (pitch_time * 0.001 * rate)).exp();
        let gk = 1.0 - (-1.0 / (glide * 0.001 * rate)).exp();
        let dk = 1.0 + 9.0 * drive / 100.0;
        let g = db_to_gain(gain) * self.vel;
        for i in 0..l.len() {
            if self.amp < 1e-5 {
                break;
            }
            self.note += (self.target - self.note) * gk;
            let f = hz(self.note + tune + pitch_amt * self.pitch_env);
            self.pitch_env *= pk;
            self.phase = (self.phase + f / rate).fract();
            self.sub_phase = (self.sub_phase + 0.5 * f / rate).fract();
            let mut s = (TAU * self.phase).sin() + (TAU * self.sub_phase).sin() * sub / 100.0;
            s += self.click * click / 100.0 * (TAU * self.phase * 6.0).sin();
            self.click *= 0.97;
            s = (s * dk).tanh() / dk.powf(0.35);
            s = self.tone.process(s) * self.amp * g;
            // Holding keeps the level while held; otherwise it decays like the drum machine's.
            self.amp *= if self.gate && hold >= 0.5 {
                1.0 - (1.0 - dec_k) * 0.1
            } else if self.gate {
                dec_k
            } else {
                rel_k.min(dec_k)
            };
            l[i] += s;
            r[i] += s;
        }
    }
    fn update(&mut self, model: &Model, _c: &SampleCache) {
        if let Some(p) = param_map(model) {
            if p.params != self.p {
                let p = p.params.clone();
                self.apply(&p);
            }
        }
    }
    fn active_voices(&self) -> usize {
        usize::from(self.amp > 1e-5)
    }
}

// ---------------------------------------------------------------------------------------------- ensemble

const ENS_VOICES: usize = 16;
const MAX_UNISON: usize = 7;
/// Vowel formants (Hz, gain): a and u.
const VOWELS: [[(f32, f32); 3]; 2] = [[(730.0, 1.0), (1090.0, 0.5), (2440.0, 0.25)], [(300.0, 1.0), (870.0, 0.35), (2240.0, 0.1)]];

#[derive(Clone, Copy, Default)]
struct EnsVoice {
    note: u8,
    vel: f32,
    t: u64,
    env: Adsr,
    phase: [f32; MAX_UNISON],
    lp: [[Biquad; 2]; 2],
    form: [[Biquad; 3]; 2],
}

pub struct Ensemble {
    rate: f32,
    p: BTreeMap<String, f32>,
    mode: usize,
    unison: usize,
    detune: f32,
    width: f32,
    attack: f32,
    release: f32,
    bright: f32,
    vib: f32,
    vib_rate: f32,
    air: f32,
    gain: f32,
    voices: [EnsVoice; ENS_VOICES],
    clock: u64,
    lfo: f32,
    rng: Rng,
}

impl Ensemble {
    fn new(rate: f32) -> Ensemble {
        let mut e = Ensemble {
            rate,
            p: BTreeMap::new(),
            mode: 0,
            unison: 5,
            detune: 14.0,
            width: 0.8,
            attack: 0.35,
            release: 0.9,
            bright: 0.5,
            vib: 0.25,
            vib_rate: 5.2,
            air: 0.15,
            gain: 1.0,
            voices: [EnsVoice::default(); ENS_VOICES],
            clock: 0,
            lfo: 0.0,
            rng: Rng::new(0xc0),
        };
        e.apply(&def("ensemble").expect("ensemble").defaults());
        e
    }
    fn apply(&mut self, p: &BTreeMap<String, f32>) {
        let d = def("ensemble").expect("ensemble");
        let v = |id: &str| crate::effects::value(d, p, id);
        self.mode = (v("mode").round().max(0.0) as usize).min(ENSEMBLES.len() - 1);
        self.unison = v("voices").round().clamp(1.0, 7.0) as usize;
        self.detune = v("detune");
        self.width = v("width") / 100.0;
        self.attack = v("attack") * 0.001;
        self.release = v("release") * 0.001;
        self.bright = v("brightness") / 100.0;
        self.vib = v("vibrato") / 100.0;
        self.vib_rate = v("vib_rate");
        self.air = v("air") / 100.0;
        self.gain = db_to_gain(v("gain"));
        self.p = p.clone();
        for vo in self.voices.iter_mut() {
            Self::tune_voice(vo, self.mode, self.bright, self.rate);
        }
    }
    fn tune_voice(v: &mut EnsVoice, mode: usize, bright: f32, rate: f32) {
        let f = hz(v.note as f32);
        // Cutoff: tracks the key a little, opened by the brightness.
        let fc = match mode {
            0 => 1500.0 + 9000.0 * bright,
            3 => 600.0 + 2500.0 * bright,
            4 => 3000.0 + 12000.0 * bright,
            5 => 900.0 + 6000.0 * bright,
            _ => 2500.0 + 6000.0 * bright,
        } * (f / 261.6).powf(0.3);
        for side in v.lp.iter_mut() {
            for b in side.iter_mut() {
                b.set(BiquadKind::LowPass, fc.min(rate * 0.45), 0.6, 0.0, rate);
            }
        }
        if mode == 1 || mode == 2 {
            let vowel = VOWELS[mode - 1];
            for side in v.form.iter_mut() {
                for (b, (ff, _)) in side.iter_mut().zip(vowel) {
                    b.set(BiquadKind::BandPass, ff, 6.0, 0.0, rate);
                }
            }
        }
    }
}

impl InstrumentVoice for Ensemble {
    fn note_on(&mut self, n: NoteOn) {
        self.clock += 1;
        let i = pick(&mut self.voices, |v| (v.env.active(), v.t));
        let mut v = EnsVoice { note: n.note, vel: n.velocity, t: self.clock, ..Default::default() };
        for (k, ph) in v.phase.iter_mut().enumerate() {
            *ph = (k as f32 * 0.37 + self.rng.next_f32()).fract();
        }
        Self::tune_voice(&mut v, self.mode, self.bright, self.rate);
        v.env.on();
        self.voices[i] = v;
    }
    fn note_off(&mut self, note: u8) {
        for v in self.voices.iter_mut().filter(|v| v.note == note) {
            v.env.off();
        }
    }
    fn choke(&mut self, _s: f32) {
        self.voices = [EnsVoice::default(); ENS_VOICES];
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let rate = self.rate;
        let n = l.len();
        let u = self.unison;
        let lfo0 = self.lfo;
        self.lfo = (self.lfo + self.vib_rate * n as f32 / rate).fract();
        let norm = 0.35 / (u as f32).sqrt();
        let (mode, attack, release, width, vib, air, gain) = (self.mode, self.attack, self.release, self.width, self.vib, self.air, self.gain);
        for v in self.voices.iter_mut().filter(|v| v.env.active()) {
            let f0 = hz(v.note as f32);
            for i in 0..n {
                let lfo = (TAU * (lfo0 + self.vib_rate * i as f32 / rate)).sin();
                // Vibrato fades in with the note (players start straight).
                let env = v.env.next(attack, 1.0, 1.0, release, rate);
                let vib_c = vib * 18.0 * env * lfo;
                let (mut sl, mut sr) = (0.0f32, 0.0f32);
                for k in 0..u {
                    // Unison spread −1 … 1, each its own detune and pan.
                    let pos = if u == 1 { 0.0 } else { k as f32 / (u - 1) as f32 * 2.0 - 1.0 };
                    let f = f0 * 2f32.powf((pos * self.detune + vib_c) / 1200.0);
                    let dt = f / rate;
                    let ph = &mut v.phase[k];
                    *ph += dt;
                    if *ph >= 1.0 {
                        *ph -= 1.0;
                    }
                    let saw = 2.0 * *ph - 1.0 - blep(*ph, dt);
                    let s = match mode {
                        4 => (TAU * *ph).sin() * 0.8 + (TAU * (*ph * 2.0).fract()).sin() * 0.3,
                        _ => saw,
                    };
                    let pan = pos * width;
                    sl += s * (1.0 - pan) * 0.5;
                    sr += s * (1.0 + pan) * 0.5;
                }
                let noise = self.rng.bipolar() * air * 0.25;
                let mut out = [sl * norm + noise, sr * norm + noise];
                for (c, o) in out.iter_mut().enumerate() {
                    let first = v.lp[c][0].process(*o);
                    let mut x = v.lp[c][1].process(first);
                    if mode == 1 || mode == 2 {
                        let vowel = VOWELS[mode - 1];
                        let mut y = 0.0;
                        for (b, (_, g)) in v.form[c].iter_mut().zip(vowel) {
                            y += b.process(x) * g;
                        }
                        x = y * 2.5;
                    }
                    if mode == 5 {
                        // Brass: brighter as it swells (a cheap filter envelope).
                        x *= 0.6 + 0.6 * env;
                    }
                    *o = x * env * v.vel.sqrt() * gain;
                }
                l[i] += out[0];
                r[i] += out[1];
            }
        }
    }
    fn update(&mut self, model: &Model, _c: &SampleCache) {
        if let Some(p) = param_map(model) {
            if p.params != self.p {
                let p = p.params.clone();
                self.apply(&p);
            }
        }
    }
    fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.env.active()).count()
    }
}

fn blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt;
        2.0 * x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + 2.0 * x + 1.0
    } else {
        0.0
    }
}

// --------------------------------------------------------------------------------------------- Sandstorm

const SS_VOICES: usize = 8;
const SS_GRAINS: usize = 24;

#[derive(Clone, Copy, Default)]
struct SsGrain {
    on: bool,
    pos: f64,
    speed: f64,
    len: f64,
    age: f64,
    gl: f32,
    gr: f32,
}

#[derive(Clone, Copy, Default)]
struct SsVoice {
    note: u8,
    vel: f32,
    t: u64,
    env: Adsr,
    grains: [SsGrain; SS_GRAINS],
    until: f64,
    scan: f64,
}

pub struct Sandstorm {
    rate: f32,
    p: BTreeMap<String, f32>,
    file: Option<PathBuf>,
    src: Arc<AudioData>,
    v: [f32; 11],
    voices: [SsVoice; SS_VOICES],
    clock: u64,
    rng: Rng,
}

/// The built-in texture when no sample is loaded: two seconds of a slowly evolving vowel-like chord at C4.
fn builtin_texture(rate: f32) -> AudioData {
    let n = (2.0 * rate) as usize;
    let mut rng = Rng::new(7);
    let mut l = vec![0.0f32; n];
    let mut r = vec![0.0f32; n];
    for (k, f) in [261.63f32, 329.63, 392.0, 523.25].iter().enumerate() {
        let mut ph = [rng.next_f32(), rng.next_f32()];
        for i in 0..n {
            let t = i as f32 / rate;
            let wob = 1.0 + 0.003 * (TAU * (0.3 + k as f32 * 0.11) * t).sin();
            for (c, p) in ph.iter_mut().enumerate() {
                *p = (*p + f * wob * (1.0 + c as f32 * 0.002) / rate).fract();
                let s = (TAU * *p).sin() + 0.4 * (TAU * 2.0 * *p).sin() * (0.5 + 0.5 * (t * 1.7 + k as f32).sin()) + 0.2 * (TAU * 3.0 * *p).sin();
                let v = s * 0.12;
                if c == 0 {
                    l[i] += v;
                } else {
                    r[i] += v;
                }
            }
        }
    }
    AudioData { rate: rate as u32, l, r, source_channels: 2, source_rate: rate as u32 }
}

impl Sandstorm {
    fn new(rate: f32) -> Sandstorm {
        let mut s = Sandstorm { rate, p: BTreeMap::new(), file: None, src: Arc::new(builtin_texture(rate)), v: [0.0; 11], voices: [SsVoice::default(); SS_VOICES], clock: 0, rng: Rng::new(0x55) };
        s.apply(&def("granular_inst").expect("sandstorm").defaults());
        s
    }
    fn apply(&mut self, p: &BTreeMap<String, f32>) {
        let d = def("granular_inst").expect("sandstorm");
        for (k, prm) in d.params.iter().enumerate() {
            self.v[k] = crate::effects::value(d, p, prm.id);
        }
        self.p = p.clone();
    }
}

impl InstrumentVoice for Sandstorm {
    fn note_on(&mut self, n: NoteOn) {
        self.clock += 1;
        let i = pick(&mut self.voices, |v| (v.env.active(), v.t));
        let mut v = SsVoice { note: n.note, vel: n.velocity, t: self.clock, ..Default::default() };
        v.env.on();
        v.scan = self.v[0] as f64 / 100.0;
        self.voices[i] = v;
    }
    fn note_off(&mut self, note: u8) {
        for v in self.voices.iter_mut().filter(|v| v.note == note) {
            v.env.off();
        }
    }
    fn choke(&mut self, _s: f32) {
        self.voices = [SsVoice::default(); SS_VOICES];
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let rate = self.rate;
        let [_pos, scan, spray, size, density, pitch_rand, reverse, spread, attack, release, gain] = self.v;
        let src = self.src.clone();
        let len_src = src.l.len().max(2) as f64;
        let glen = (size * 0.001 * rate) as f64;
        let period = rate as f64 / density as f64;
        let norm = 1.0 / (density * size * 0.001).max(1.0).sqrt() * db_to_gain(gain);
        for v in self.voices.iter_mut().filter(|v| v.env.active()) {
            let base = 2f64.powf((v.note as f64 - 60.0) / 12.0);
            for i in 0..l.len() {
                let env = v.env.next(attack * 0.001, 1.0, 1.0, release * 0.001, rate);
                v.scan = (v.scan + scan as f64 / 100.0 / rate as f64).rem_euclid(1.0);
                v.until -= 1.0;
                if v.until <= 0.0 {
                    v.until += period * (0.75 + 0.5 * self.rng.next_f32() as f64);
                    if let Some(g) = v.grains.iter_mut().find(|g| !g.on) {
                        let mut speed = base * 2f64.powf((pitch_rand * self.rng.bipolar()) as f64 / 12.0);
                        if self.rng.next_f32() * 100.0 < reverse {
                            speed = -speed;
                        }
                        let start = (v.scan + spray as f64 / 100.0 * self.rng.bipolar() as f64 * 0.5).rem_euclid(1.0) * len_src;
                        let pan = spread / 100.0 * self.rng.bipolar();
                        let a = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
                        *g = SsGrain { on: true, pos: start, speed, len: glen, age: 0.0, gl: a.cos(), gr: a.sin() };
                    }
                }
                let (mut sl, mut sr) = (0.0f32, 0.0f32);
                for g in v.grains.iter_mut().filter(|g| g.on) {
                    let w = (0.5 - 0.5 * (std::f64::consts::TAU * g.age / g.len).cos()) as f32;
                    let p = g.pos.rem_euclid(len_src);
                    let k = p as usize;
                    let f = (p - k as f64) as f32;
                    let k1 = (k + 1) % src.l.len().max(1);
                    let a = src.l[k] + (src.l[k1] - src.l[k]) * f;
                    let b = src.r[k] + (src.r[k1] - src.r[k]) * f;
                    sl += a * w * g.gl;
                    sr += b * w * g.gr;
                    g.pos += g.speed;
                    g.age += 1.0;
                    if g.age >= g.len {
                        g.on = false;
                    }
                }
                let k = env * v.vel * norm * 1.4;
                l[i] += sl * k;
                r[i] += sr * k;
            }
        }
    }
    fn update(&mut self, model: &Model, cache: &SampleCache) {
        if let Some(p) = param_map(model) {
            if p.params != self.p {
                let pp = p.params.clone();
                self.apply(&pp);
            }
            if p.file != self.file {
                self.file = p.file.clone();
                self.src = match &self.file {
                    Some(f) => cache.get(f, self.rate as u32).unwrap_or_else(|_| Arc::new(builtin_texture(self.rate))),
                    None => Arc::new(builtin_texture(self.rate)),
                };
            }
        }
    }
    fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.env.active()).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every native instrument sounds on a note, stays bounded, and goes quiet after the release.
    #[test]
    fn every_native_instrument_plays_and_releases() {
        let rate = 48000.0;
        let cache = SampleCache::default();
        for d in defs() {
            let p = NativeParams::new(d.id);
            let mut inst = create(&p, rate).unwrap();
            inst.update(&Model::Native(Box::new(p.clone())), &cache);
            inst.note_on(NoteOn { note: 48, velocity: 0.9, pan: 0.0, detune: 0.0, slide: false });
            let (mut l, mut r) = (vec![0.0f32; 128], vec![0.0f32; 128]);
            let mut e_on = 0.0f32;
            for _ in 0..375 {
                l.fill(0.0);
                r.fill(0.0);
                inst.process(&mut l, &mut r);
                e_on += l.iter().map(|x| x * x).sum::<f32>();
                assert!(l.iter().chain(&r).all(|x| x.is_finite() && x.abs() < 8.0), "{} out of range", d.id);
            }
            assert!(e_on > 1.0, "{}: silent ({e_on})", d.id);
            inst.note_off(48);
            let mut e_late = 0.0f32;
            for k in 0..3750 {
                l.fill(0.0);
                r.fill(0.0);
                inst.process(&mut l, &mut r);
                if k > 3000 {
                    e_late += l.iter().map(|x| x * x).sum::<f32>();
                }
            }
            assert!(e_late < e_on * 1e-3, "{}: still sounding ({e_late} vs {e_on})", d.id);
        }
    }

    /// The 808 glides between overlapping notes instead of retriggering.
    #[test]
    fn bass_glides() {
        let mut b = Bass808::new(48000.0);
        b.note_on(NoteOn { note: 36, velocity: 1.0, pan: 0.0, detune: 0.0, slide: false });
        let (mut l, mut r) = (vec![0.0f32; 4800], vec![0.0f32; 4800]);
        b.process(&mut l, &mut r);
        b.note_on(NoteOn { note: 43, velocity: 1.0, pan: 0.0, detune: 0.0, slide: false });
        assert!(b.note < 37.0 && b.pitch_env < 0.5, "a held note glides");
        b.process(&mut l, &mut r);
        // 100 ms into a 60 ms glide (one-pole): 1 − e^{−100/60} ≈ 81 % of the way.
        assert!(b.note > 41.0, "{}", b.note);
    }
}
