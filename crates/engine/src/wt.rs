//! Dragon Wave: the wavetable synth's model (oscillators, filters, envelopes, LFOs, modulation matrix, FX)
//! and its wavetables – factory tables made procedurally and tables imported from audio files – with
//! band-limited copies per octave so high notes don't alias.

use crate::model::Envelope;
use rustfft::{num_complex::Complex, FftPlanner};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// Samples in one wavetable frame (one cycle).
pub const FRAME: usize = 2048;
/// Band-limited copies: level k keeps harmonics up to `512 >> k`.
pub const LEVELS: usize = 9;
const MAX_FRAMES: usize = 256;

// ------------------------------------------------------------------------------------------------ model

/// Where an oscillator's wavetable comes from.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TableSource {
    /// A built-in table, by name ([`FACTORY`]).
    Factory(String),
    /// An audio file: single cycles of 2048 samples one after another, or any sound cut into cycles.
    File(PathBuf),
}

impl Default for TableSource {
    fn default() -> Self {
        TableSource::Factory("Basic Shapes".into())
    }
}

impl TableSource {
    pub fn name(&self) -> String {
        match self {
            TableSource::Factory(n) => n.clone(),
            TableSource::File(p) => p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "file".into()),
        }
    }
}

/// How an oscillator bends its wave (applied to the phase or to the output).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Warp {
    #[default]
    Off,
    /// Hard sync to a faster cycle.
    Sync,
    /// Phase bent towards the start (+) or the end (−) of the cycle.
    BendPlus,
    BendMinus,
    /// The first half squeezed, like pulse-width modulation.
    Pwm,
    /// The wave mixed with its own half-cycle shift (odd / even balance).
    Asym,
    /// Phase modulation from the other oscillator (A ← B, B ← A, C ← B).
    Fm,
    /// Wave folding.
    Fold,
    /// Fewer amplitude steps (bit crushing).
    Crush,
}

impl Warp {
    pub const ALL: [Warp; 9] = [Warp::Off, Warp::Sync, Warp::BendPlus, Warp::BendMinus, Warp::Pwm, Warp::Asym, Warp::Fm, Warp::Fold, Warp::Crush];
    pub fn name(self) -> &'static str {
        match self {
            Warp::Off => "Off",
            Warp::Sync => "Sync",
            Warp::BendPlus => "Bend +",
            Warp::BendMinus => "Bend −",
            Warp::Pwm => "PWM",
            Warp::Asym => "Asym",
            Warp::Fm => "FM",
            Warp::Fold => "Fold",
            Warp::Crush => "Crush",
        }
    }
}

/// Where a sound source goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Route {
    #[default]
    FilterA,
    FilterB,
    Both,
    /// Past the filters.
    Direct,
}

impl Route {
    pub const ALL: [Route; 4] = [Route::FilterA, Route::FilterB, Route::Both, Route::Direct];
    pub fn name(self) -> &'static str {
        match self {
            Route::FilterA => "Filter 1",
            Route::FilterB => "Filter 2",
            Route::Both => "Both",
            Route::Direct => "Direct",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WtOsc {
    pub on: bool,
    pub table: TableSource,
    /// Position in the table, 0 … 1.
    pub pos: f32,
    pub warp: Warp,
    pub warp_amount: f32,
    pub octave: i32,
    pub semi: i32,
    /// Cents.
    pub fine: f32,
    pub level: f32,
    pub pan: f32,
    /// 1 … 16 voices.
    pub unison: u32,
    /// 0 … 1 (1 = ±50 cents at the outer voices).
    pub detune: f32,
    /// Level of the outer unison voices against the centre.
    pub blend: f32,
    /// Stereo spread of the unison voices.
    pub width: f32,
    /// Start phase 0 … 1, and how much it is randomised per note.
    pub phase: f32,
    pub random_phase: f32,
    pub route: Route,
}

impl Default for WtOsc {
    fn default() -> Self {
        WtOsc {
            on: false,
            table: TableSource::default(),
            pos: 0.0,
            warp: Warp::Off,
            warp_amount: 0.0,
            octave: 0,
            semi: 0,
            fine: 0.0,
            level: 0.75,
            pan: 0.0,
            unison: 1,
            detune: 0.25,
            blend: 0.75,
            width: 1.0,
            phase: 0.0,
            random_phase: 1.0,
            route: Route::FilterA,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SubShape {
    #[default]
    Sine,
    Triangle,
    Saw,
    Square,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SubOsc {
    pub on: bool,
    pub shape: SubShape,
    pub octave: i32,
    pub level: f32,
    pub pan: f32,
    pub route: Route,
}

impl Default for SubOsc {
    fn default() -> Self {
        SubOsc { on: false, shape: SubShape::Sine, octave: -1, level: 0.6, pan: 0.0, route: Route::Direct }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NoiseOsc {
    pub on: bool,
    /// 0 = dark, 0.5 = white, 1 = bright.
    pub color: f32,
    pub level: f32,
    pub pan: f32,
    pub route: Route,
}

impl Default for NoiseOsc {
    fn default() -> Self {
        NoiseOsc { on: false, color: 0.5, level: 0.3, pan: 0.0, route: Route::FilterA }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FilterKind {
    #[default]
    Lp12,
    Lp24,
    Hp12,
    Hp24,
    Bp,
    Notch,
    /// Comb filters: peaks (+) or notches (−) at the cutoff and its harmonics.
    CombPlus,
    CombMinus,
    /// Vowel formants; the cutoff moves through A – E – I – O – U.
    Formant,
}

impl FilterKind {
    pub const ALL: [FilterKind; 9] = [FilterKind::Lp12, FilterKind::Lp24, FilterKind::Hp12, FilterKind::Hp24, FilterKind::Bp, FilterKind::Notch, FilterKind::CombPlus, FilterKind::CombMinus, FilterKind::Formant];
    pub fn name(self) -> &'static str {
        match self {
            FilterKind::Lp12 => "Low pass 12",
            FilterKind::Lp24 => "Low pass 24",
            FilterKind::Hp12 => "High pass 12",
            FilterKind::Hp24 => "High pass 24",
            FilterKind::Bp => "Band pass",
            FilterKind::Notch => "Notch",
            FilterKind::CombPlus => "Comb +",
            FilterKind::CombMinus => "Comb −",
            FilterKind::Formant => "Formant",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WtFilter {
    pub on: bool,
    pub kind: FilterKind,
    /// Hz.
    pub cutoff: f32,
    pub resonance: f32,
    pub drive: f32,
    pub mix: f32,
    /// How far the cutoff follows the note (1 = an octave per octave).
    pub keytrack: f32,
}

impl Default for WtFilter {
    fn default() -> Self {
        WtFilter { on: false, kind: FilterKind::Lp24, cutoff: 2000.0, resonance: 0.2, drive: 0.0, mix: 1.0, keytrack: 0.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LfoShape {
    #[default]
    Sine,
    Triangle,
    SawUp,
    SawDown,
    Square,
    /// Sample and hold.
    Random,
    /// Smoothly gliding random.
    Smooth,
}

impl LfoShape {
    pub const ALL: [LfoShape; 7] = [LfoShape::Sine, LfoShape::Triangle, LfoShape::SawUp, LfoShape::SawDown, LfoShape::Square, LfoShape::Random, LfoShape::Smooth];
    pub fn name(self) -> &'static str {
        match self {
            LfoShape::Sine => "Sine",
            LfoShape::Triangle => "Triangle",
            LfoShape::SawUp => "Saw up",
            LfoShape::SawDown => "Saw down",
            LfoShape::Square => "Square",
            LfoShape::Random => "Random",
            LfoShape::Smooth => "Smooth random",
        }
    }

    /// The shape at `phase` (0 … 1) for drawing and for the deterministic shapes (−1 … 1).
    pub fn at(self, phase: f32) -> f32 {
        let p = phase.rem_euclid(1.0);
        match self {
            LfoShape::Sine => (p * std::f32::consts::TAU).sin(),
            LfoShape::Triangle => 1.0 - 4.0 * (p - 0.25).rem_euclid(1.0).sub_abs_half(),
            LfoShape::SawUp => 2.0 * p - 1.0,
            LfoShape::SawDown => 1.0 - 2.0 * p,
            LfoShape::Square => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            // Drawn as a fixed staircase / curve.
            LfoShape::Random => [0.6, -0.4, 0.9, -0.8, 0.1, -0.2, 0.7, -0.9][(p * 8.0) as usize % 8],
            LfoShape::Smooth => (p * std::f32::consts::TAU).sin() * 0.6 + (p * 3.0 * std::f32::consts::TAU).sin() * 0.3,
        }
    }
}

trait SubAbsHalf {
    fn sub_abs_half(self) -> f32;
}
impl SubAbsHalf for f32 {
    fn sub_abs_half(self) -> f32 {
        (self - 0.5).abs()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LfoMode {
    /// Restarts with every note.
    #[default]
    Retrig,
    /// Runs on for all notes.
    Free,
    /// Runs one cycle per note and stops (an extra envelope).
    OneShot,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WtLfo {
    pub shape: LfoShape,
    /// Hz when not synced.
    pub rate: f32,
    /// Synced to the tempo: one cycle per `beats`.
    pub sync: bool,
    pub beats: f32,
    pub mode: LfoMode,
    /// Seconds before it starts, and to fade in.
    pub delay: f32,
    pub rise: f32,
    /// 0 … 1 instead of −1 … 1.
    pub unipolar: bool,
}

impl Default for WtLfo {
    fn default() -> Self {
        WtLfo { shape: LfoShape::Sine, rate: 2.0, sync: false, beats: 1.0, mode: LfoMode::Retrig, delay: 0.0, rise: 0.0, unipolar: false }
    }
}

/// Synced LFO lengths (beats per cycle) and their names.
pub const LFO_BEATS: [(f32, &str); 12] =
    [(16.0, "4 bars"), (8.0, "2 bars"), (4.0, "1 bar"), (2.0, "1/2"), (1.0, "1/4"), (0.75, "1/8 ."), (2.0 / 3.0, "1/4 T"), (0.5, "1/8"), (1.0 / 3.0, "1/8 T"), (0.25, "1/16"), (0.125, "1/32"), (0.0625, "1/64")];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModSource {
    Env(u8),
    Lfo(u8),
    Velocity,
    /// The note, −1 (C-1) … 0 (C4) … +1.
    Note,
    /// A random value per note, 0 … 1.
    Random,
    Macro(u8),
}

impl ModSource {
    pub fn all() -> Vec<ModSource> {
        let mut v: Vec<ModSource> = (0..4).map(ModSource::Env).collect();
        v.extend((0..4).map(ModSource::Lfo));
        v.extend([ModSource::Velocity, ModSource::Note, ModSource::Random]);
        v.extend((0..4).map(ModSource::Macro));
        v
    }
    pub fn name(self) -> String {
        match self {
            ModSource::Env(0) => "Env 1 (amp)".into(),
            ModSource::Env(k) => format!("Env {}", k + 1),
            ModSource::Lfo(k) => format!("LFO {}", k + 1),
            ModSource::Velocity => "Velocity".into(),
            ModSource::Note => "Note".into(),
            ModSource::Random => "Random".into(),
            ModSource::Macro(k) => format!("Macro {}", k + 1),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModDest {
    OscPos(u8),
    OscWarp(u8),
    /// ±24 semitones at full amount.
    OscPitch(u8),
    OscLevel(u8),
    OscPan(u8),
    OscDetune(u8),
    SubLevel,
    NoiseLevel,
    /// ±8 octaves at full amount.
    FilterCutoff(u8),
    FilterReso(u8),
    FilterDrive(u8),
    FilterMix(u8),
    /// ×/÷ 16 at full amount.
    LfoRate(u8),
    /// All oscillators' pitch, ±24 semitones.
    Pitch,
    Master,
}

impl ModDest {
    pub fn all() -> Vec<ModDest> {
        let mut v = Vec::new();
        for k in 0..3 {
            v.extend([ModDest::OscPos(k), ModDest::OscWarp(k), ModDest::OscPitch(k), ModDest::OscLevel(k), ModDest::OscPan(k), ModDest::OscDetune(k)]);
        }
        v.extend([ModDest::SubLevel, ModDest::NoiseLevel]);
        for k in 0..2 {
            v.extend([ModDest::FilterCutoff(k), ModDest::FilterReso(k), ModDest::FilterDrive(k), ModDest::FilterMix(k)]);
        }
        v.extend((0..4).map(ModDest::LfoRate));
        v.extend([ModDest::Pitch, ModDest::Master]);
        v
    }
    pub fn name(self) -> String {
        let o = |k: u8| ["A", "B", "C"][k as usize % 3];
        match self {
            ModDest::OscPos(k) => format!("Osc {} position", o(k)),
            ModDest::OscWarp(k) => format!("Osc {} warp", o(k)),
            ModDest::OscPitch(k) => format!("Osc {} pitch", o(k)),
            ModDest::OscLevel(k) => format!("Osc {} level", o(k)),
            ModDest::OscPan(k) => format!("Osc {} pan", o(k)),
            ModDest::OscDetune(k) => format!("Osc {} detune", o(k)),
            ModDest::SubLevel => "Sub level".into(),
            ModDest::NoiseLevel => "Noise level".into(),
            ModDest::FilterCutoff(k) => format!("Filter {} cutoff", k + 1),
            ModDest::FilterReso(k) => format!("Filter {} resonance", k + 1),
            ModDest::FilterDrive(k) => format!("Filter {} drive", k + 1),
            ModDest::FilterMix(k) => format!("Filter {} mix", k + 1),
            ModDest::LfoRate(k) => format!("LFO {} rate", k + 1),
            ModDest::Pitch => "Pitch (all)".into(),
            ModDest::Master => "Master level".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModSlot {
    pub source: ModSource,
    pub dest: ModDest,
    /// −1 … 1.
    pub amount: f32,
    #[serde(default = "yes")]
    pub on: bool,
}

fn yes() -> bool {
    true
}

/// An effect in the synth's own FX rack (a stock effect by id).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FxSlot {
    pub id: String,
    #[serde(default = "yes")]
    pub on: bool,
    #[serde(default)]
    pub params: BTreeMap<String, f32>,
}

/// The stock effects offered in the synth's FX rack.
pub const FX_CHOICES: [&str; 12] = ["saturator", "bitcrusher", "chorus", "flanger", "phaser", "delay", "reverb", "filter", "eq", "compressor", "imager", "clipper"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FilterRouting {
    /// Filter 1 into filter 2.
    #[default]
    Serial,
    Parallel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WavetableParams {
    /// The preset this started from (shown in the editor).
    pub preset: String,
    pub osc: [WtOsc; 3],
    pub sub: SubOsc,
    pub noise: NoiseOsc,
    pub filters: [WtFilter; 2],
    pub filter_routing: FilterRouting,
    /// Env 1 is the amp envelope.
    pub env: [Envelope; 4],
    pub lfo: [WtLfo; 4],
    pub macros: [f32; 4],
    pub macro_names: [String; 4],
    pub matrix: Vec<ModSlot>,
    pub fx: Vec<FxSlot>,
    pub polyphony: u32,
    pub mono: bool,
    pub legato: bool,
    /// Seconds.
    pub glide: f32,
    /// How much velocity changes the level, 0 … 1.
    pub velocity: f32,
    /// Linear gain.
    pub master: f32,
}

impl Default for WavetableParams {
    fn default() -> Self {
        let env = |a: f32, d: f32, s: f32, r: f32| Envelope { enabled: true, attack: a, decay: d, sustain: s, release: r, ..Envelope::default() };
        WavetableParams {
            preset: "Init".into(),
            osc: [WtOsc { on: true, pos: 0.66, ..WtOsc::default() }, WtOsc::default(), WtOsc { table: TableSource::Factory("Harmonic Sweep".into()), ..WtOsc::default() }],
            sub: SubOsc::default(),
            noise: NoiseOsc::default(),
            filters: [WtFilter::default(), WtFilter { kind: FilterKind::Hp12, cutoff: 80.0, ..WtFilter::default() }],
            filter_routing: FilterRouting::Serial,
            env: [env(0.002, 0.3, 1.0, 0.15), env(0.002, 0.4, 0.0, 0.3), env(0.5, 1.0, 0.5, 0.5), env(0.01, 0.2, 0.0, 0.2)],
            lfo: [
                WtLfo::default(),
                WtLfo { shape: LfoShape::Triangle, rate: 0.5, ..WtLfo::default() },
                WtLfo { shape: LfoShape::Random, rate: 8.0, ..WtLfo::default() },
                WtLfo { sync: true, beats: 1.0, ..WtLfo::default() },
            ],
            macros: [0.0; 4],
            macro_names: ["Macro 1".into(), "Macro 2".into(), "Macro 3".into(), "Macro 4".into()],
            matrix: Vec::new(),
            fx: Vec::new(),
            polyphony: 16,
            mono: false,
            legato: false,
            glide: 0.0,
            velocity: 0.5,
            master: 0.7,
        }
    }
}

// ------------------------------------------------------------------------------------------------ presets

/// The factory presets, by name.
#[allow(clippy::vec_init_then_push)]
pub fn presets() -> Vec<(&'static str, WavetableParams)> {
    let base = WavetableParams::default;
    let env = |a: f32, d: f32, s: f32, r: f32| Envelope { enabled: true, attack: a, decay: d, sustain: s, release: r, ..Envelope::default() };
    let osc = |table: &str, pos: f32| WtOsc { on: true, table: TableSource::Factory(table.into()), pos, ..WtOsc::default() };
    let slot = |source: ModSource, dest: ModDest, amount: f32| ModSlot { source, dest, amount, on: true };
    let fx = |id: &str, params: &[(&str, f32)]| FxSlot { id: id.into(), on: true, params: params.iter().map(|(k, v)| (k.to_string(), *v)).collect() };
    let lp = |cutoff: f32, reso: f32| WtFilter { on: true, kind: FilterKind::Lp24, cutoff, resonance: reso, ..WtFilter::default() };
    let mut v: Vec<(&'static str, WavetableParams)> = Vec::new();
    v.push(("Init", base()));
    v.push((
        "Supersaw Lead",
        WavetableParams {
            osc: [WtOsc { unison: 9, detune: 0.35, width: 1.0, ..osc("Basic Shapes", 0.66) }, WtOsc { unison: 5, detune: 0.2, octave: 1, level: 0.35, ..osc("Basic Shapes", 0.66) }, WtOsc::default()],
            filters: [lp(7000.0, 0.15), WtFilter { on: true, kind: FilterKind::Hp12, cutoff: 120.0, ..WtFilter::default() }],
            env: [env(0.005, 0.4, 0.85, 0.35), env(0.001, 0.6, 0.2, 0.3), env(0.5, 1.0, 0.5, 0.5), env(0.01, 0.2, 0.0, 0.2)],
            matrix: vec![slot(ModSource::Env(1), ModDest::FilterCutoff(0), 0.25)],
            fx: vec![fx("chorus", &[]), fx("delay", &[("mix", 0.2)]), fx("reverb", &[("mix", 0.25)])],
            ..base()
        },
    ));
    v.push((
        "Reese Bass",
        WavetableParams {
            osc: [WtOsc { unison: 2, detune: 0.18, width: 0.3, ..osc("Basic Shapes", 0.66) }, WtOsc { unison: 2, detune: 0.12, semi: 0, fine: 9.0, level: 0.6, ..osc("Basic Shapes", 0.66) }, WtOsc::default()],
            sub: SubOsc { on: true, level: 0.5, ..SubOsc::default() },
            filters: [lp(900.0, 0.25), WtFilter::default()],
            env: [env(0.003, 0.3, 1.0, 0.12), env(0.5, 1.0, 0.5, 0.5), env(0.5, 1.0, 0.5, 0.5), env(0.01, 0.2, 0.0, 0.2)],
            lfo: [WtLfo { rate: 0.3, ..WtLfo::default() }, WtLfo::default(), WtLfo::default(), WtLfo::default()],
            matrix: vec![slot(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.12)],
            fx: vec![fx("saturator", &[]), fx("imager", &[])],
            polyphony: 1,
            mono: true,
            legato: true,
            glide: 0.04,
            ..base()
        },
    ));
    v.push((
        "Growl Bass",
        WavetableParams {
            osc: [WtOsc { warp: Warp::Fm, warp_amount: 0.35, ..osc("Dragon Growl", 0.3) }, WtOsc { level: 0.0, ..osc("Basic Shapes", 0.0) }, WtOsc::default()],
            sub: SubOsc { on: true, level: 0.45, ..SubOsc::default() },
            filters: [WtFilter { on: true, kind: FilterKind::Formant, cutoff: 600.0, resonance: 0.55, drive: 0.4, ..WtFilter::default() }, lp(5000.0, 0.1)],
            lfo: [WtLfo { sync: true, beats: 0.5, shape: LfoShape::Triangle, ..WtLfo::default() }, WtLfo::default(), WtLfo::default(), WtLfo::default()],
            matrix: vec![slot(ModSource::Lfo(0), ModDest::OscPos(0), 0.5), slot(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.3), slot(ModSource::Macro(0), ModDest::OscWarp(0), 0.6)],
            fx: vec![fx("saturator", &[]), fx("multisat", &[]), fx("compressor", &[])],
            polyphony: 1,
            mono: true,
            legato: true,
            glide: 0.03,
            ..base()
        },
    ));
    v.push((
        "Wobble Bass",
        WavetableParams {
            osc: [osc("Basic Shapes", 0.66), WtOsc { octave: -1, level: 0.5, ..osc("Basic Shapes", 1.0) }, WtOsc::default()],
            sub: SubOsc { on: true, level: 0.5, ..SubOsc::default() },
            filters: [lp(300.0, 0.6), WtFilter::default()],
            lfo: [WtLfo { sync: true, beats: 0.5, shape: LfoShape::Sine, ..WtLfo::default() }, WtLfo::default(), WtLfo::default(), WtLfo::default()],
            matrix: vec![slot(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.45)],
            fx: vec![fx("saturator", &[])],
            polyphony: 1,
            mono: true,
            legato: true,
            ..base()
        },
    ));
    v.push((
        "Deep Sub",
        WavetableParams {
            osc: [WtOsc { level: 0.2, ..osc("Basic Shapes", 0.33) }, WtOsc::default(), WtOsc::default()],
            sub: SubOsc { on: true, level: 0.9, octave: 0, ..SubOsc::default() },
            env: [env(0.003, 0.3, 1.0, 0.1), env(0.5, 1.0, 0.5, 0.5), env(0.5, 1.0, 0.5, 0.5), env(0.01, 0.2, 0.0, 0.2)],
            polyphony: 1,
            mono: true,
            glide: 0.05,
            ..base()
        },
    ));
    v.push((
        "Pluck",
        WavetableParams {
            osc: [WtOsc { unison: 3, detune: 0.15, ..osc("Harmonic Sweep", 0.8) }, WtOsc { octave: 1, level: 0.3, ..osc("Basic Shapes", 0.33) }, WtOsc::default()],
            filters: [lp(400.0, 0.3), WtFilter::default()],
            env: [env(0.001, 0.35, 0.0, 0.3), env(0.001, 0.25, 0.0, 0.2), env(0.5, 1.0, 0.5, 0.5), env(0.01, 0.2, 0.0, 0.2)],
            matrix: vec![slot(ModSource::Env(1), ModDest::FilterCutoff(0), 0.55), slot(ModSource::Velocity, ModDest::FilterCutoff(0), 0.15)],
            fx: vec![fx("delay", &[("mix", 0.25)]), fx("reverb", &[("mix", 0.2)])],
            ..base()
        },
    ));
    v.push((
        "Soft Pad",
        WavetableParams {
            osc: [WtOsc { unison: 6, detune: 0.3, ..osc("Harmonic Sweep", 0.35) }, WtOsc { unison: 4, detune: 0.2, octave: 1, level: 0.4, ..osc("Formant Vowels", 0.2) }, WtOsc::default()],
            noise: NoiseOsc { on: true, level: 0.06, color: 0.3, ..NoiseOsc::default() },
            filters: [lp(2500.0, 0.1), WtFilter::default()],
            env: [env(0.8, 1.0, 0.9, 1.6), env(1.5, 2.0, 0.6, 1.5), env(0.5, 1.0, 0.5, 0.5), env(0.01, 0.2, 0.0, 0.2)],
            lfo: [WtLfo { rate: 0.15, ..WtLfo::default() }, WtLfo { rate: 0.23, shape: LfoShape::Triangle, ..WtLfo::default() }, WtLfo::default(), WtLfo::default()],
            matrix: vec![slot(ModSource::Lfo(0), ModDest::OscPos(0), 0.25), slot(ModSource::Lfo(1), ModDest::OscPos(1), 0.3), slot(ModSource::Env(1), ModDest::FilterCutoff(0), 0.2)],
            fx: vec![fx("chorus", &[]), fx("reverb", &[("mix", 0.4)])],
            ..base()
        },
    ));
    v.push((
        "Glass Keys",
        WavetableParams {
            osc: [osc("FM Bells", 0.35), WtOsc { octave: 1, level: 0.25, ..osc("Basic Shapes", 0.0) }, WtOsc::default()],
            env: [env(0.002, 1.2, 0.2, 0.8), env(0.001, 0.8, 0.0, 0.5), env(0.5, 1.0, 0.5, 0.5), env(0.01, 0.2, 0.0, 0.2)],
            matrix: vec![slot(ModSource::Env(1), ModDest::OscPos(0), 0.4), slot(ModSource::Velocity, ModDest::OscPos(0), 0.2)],
            fx: vec![fx("chorus", &[]), fx("reverb", &[("mix", 0.3)])],
            ..base()
        },
    ));
    v.push((
        "Vowel Lead",
        WavetableParams {
            osc: [WtOsc { unison: 3, detune: 0.12, ..osc("Formant Vowels", 0.0) }, WtOsc::default(), WtOsc::default()],
            lfo: [WtLfo { rate: 0.6, shape: LfoShape::Triangle, ..WtLfo::default() }, WtLfo { rate: 5.5, ..WtLfo { delay: 0.3, rise: 0.4, ..WtLfo::default() } }, WtLfo::default(), WtLfo::default()],
            matrix: vec![slot(ModSource::Lfo(0), ModDest::OscPos(0), 0.5), slot(ModSource::Lfo(1), ModDest::Pitch, 0.012)],
            fx: vec![fx("delay", &[("mix", 0.2)]), fx("reverb", &[("mix", 0.2)])],
            polyphony: 1,
            mono: true,
            legato: true,
            glide: 0.06,
            ..base()
        },
    ));
    v.push((
        "Sync Lead",
        WavetableParams {
            osc: [WtOsc { warp: Warp::Sync, warp_amount: 0.3, unison: 2, detune: 0.1, ..osc("Basic Shapes", 0.66) }, WtOsc::default(), WtOsc::default()],
            filters: [lp(6000.0, 0.2), WtFilter::default()],
            env: [env(0.003, 0.5, 0.8, 0.3), env(0.001, 0.7, 0.0, 0.3), env(0.5, 1.0, 0.5, 0.5), env(0.01, 0.2, 0.0, 0.2)],
            matrix: vec![slot(ModSource::Env(1), ModDest::OscWarp(0), 0.6), slot(ModSource::Macro(0), ModDest::OscWarp(0), 0.4)],
            fx: vec![fx("saturator", &[]), fx("delay", &[("mix", 0.2)])],
            polyphony: 1,
            mono: true,
            glide: 0.05,
            ..base()
        },
    ));
    v.push((
        "Digital Arp",
        WavetableParams {
            osc: [osc("Digital Steps", 0.5), WtOsc { octave: -1, level: 0.4, ..osc("Pulse Width", 0.3) }, WtOsc::default()],
            filters: [lp(3000.0, 0.35), WtFilter::default()],
            env: [env(0.001, 0.18, 0.0, 0.12), env(0.001, 0.15, 0.0, 0.1), env(0.5, 1.0, 0.5, 0.5), env(0.01, 0.2, 0.0, 0.2)],
            lfo: [WtLfo { sync: true, beats: 4.0, shape: LfoShape::Triangle, ..WtLfo::default() }, WtLfo::default(), WtLfo::default(), WtLfo::default()],
            matrix: vec![slot(ModSource::Lfo(0), ModDest::OscPos(0), 0.45), slot(ModSource::Env(1), ModDest::FilterCutoff(0), 0.35)],
            fx: vec![fx("delay", &[("mix", 0.3)])],
            ..base()
        },
    ));
    v.push((
        "Brass Stab",
        WavetableParams {
            osc: [WtOsc { unison: 4, detune: 0.18, ..osc("Basic Shapes", 0.66) }, WtOsc { semi: 7, level: 0.35, ..osc("Basic Shapes", 0.66) }, WtOsc::default()],
            filters: [lp(700.0, 0.2), WtFilter::default()],
            env: [env(0.03, 0.4, 0.7, 0.25), env(0.05, 0.35, 0.3, 0.3), env(0.5, 1.0, 0.5, 0.5), env(0.01, 0.2, 0.0, 0.2)],
            matrix: vec![slot(ModSource::Env(1), ModDest::FilterCutoff(0), 0.45)],
            fx: vec![fx("reverb", &[("mix", 0.2)])],
            ..base()
        },
    ));
    v.push((
        "Dark Drone",
        WavetableParams {
            osc: [WtOsc { unison: 5, detune: 0.4, octave: -1, ..osc("Noise Spectra", 0.2) }, WtOsc { unison: 3, detune: 0.3, ..osc("Fold", 0.4) }, WtOsc::default()],
            filters: [lp(900.0, 0.4), WtFilter { on: true, kind: FilterKind::CombPlus, cutoff: 110.0, resonance: 0.6, mix: 0.4, ..WtFilter::default() }],
            filter_routing: FilterRouting::Parallel,
            env: [env(2.0, 1.0, 1.0, 3.0), env(0.5, 1.0, 0.5, 0.5), env(0.5, 1.0, 0.5, 0.5), env(0.01, 0.2, 0.0, 0.2)],
            lfo: [WtLfo { rate: 0.07, shape: LfoShape::Smooth, ..WtLfo::default() }, WtLfo { rate: 0.11, ..WtLfo::default() }, WtLfo::default(), WtLfo::default()],
            matrix: vec![slot(ModSource::Lfo(0), ModDest::OscPos(0), 0.6), slot(ModSource::Lfo(1), ModDest::FilterCutoff(0), 0.2)],
            fx: vec![fx("phaser", &[]), fx("reverb", &[("mix", 0.5)])],
            ..base()
        },
    ));
    v.push((
        "Noise Riser",
        WavetableParams {
            osc: [WtOsc { unison: 4, detune: 0.5, level: 0.3, ..osc("Basic Shapes", 0.66) }, WtOsc::default(), WtOsc::default()],
            noise: NoiseOsc { on: true, level: 0.6, color: 0.6, ..NoiseOsc::default() },
            filters: [WtFilter { on: true, kind: FilterKind::Bp, cutoff: 300.0, resonance: 0.5, ..WtFilter::default() }, WtFilter::default()],
            env: [env(0.01, 1.0, 1.0, 1.0), env(4.0, 0.1, 1.0, 0.5), env(0.5, 1.0, 0.5, 0.5), env(0.01, 0.2, 0.0, 0.2)],
            matrix: vec![slot(ModSource::Env(1), ModDest::FilterCutoff(0), 0.6), slot(ModSource::Env(1), ModDest::Pitch, 0.5)],
            fx: vec![fx("reverb", &[("mix", 0.4)])],
            ..base()
        },
    ));
    v.push((
        "Laser Zap",
        WavetableParams {
            osc: [osc("Basic Shapes", 0.66), WtOsc::default(), WtOsc::default()],
            env: [env(0.001, 0.25, 0.0, 0.1), env(0.001, 0.12, 0.0, 0.1), env(0.5, 1.0, 0.5, 0.5), env(0.01, 0.2, 0.0, 0.2)],
            matrix: vec![slot(ModSource::Env(1), ModDest::Pitch, 1.0)],
            ..base()
        },
    ));
    v.push((
        "Organ",
        WavetableParams {
            osc: [osc("Organ", 0.5), WtOsc::default(), WtOsc::default()],
            env: [env(0.005, 0.1, 1.0, 0.08), env(0.5, 1.0, 0.5, 0.5), env(0.5, 1.0, 0.5, 0.5), env(0.01, 0.2, 0.0, 0.2)],
            lfo: [WtLfo { rate: 6.0, ..WtLfo::default() }, WtLfo::default(), WtLfo::default(), WtLfo::default()],
            matrix: vec![slot(ModSource::Lfo(0), ModDest::OscPos(0), 0.04), slot(ModSource::Macro(0), ModDest::OscPos(0), 0.5)],
            fx: vec![fx("chorus", &[]), fx("reverb", &[("mix", 0.2)])],
            ..base()
        },
    ));
    for (name, p) in v.iter_mut() {
        p.preset = name.to_string();
    }
    v
}

// ------------------------------------------------------------------------------------------------ tables

/// A wavetable: frames (single cycles) and their band-limited copies.
pub struct Wavetable {
    pub name: String,
    /// The frames as made / loaded (FRAME samples each), for drawing.
    pub frames: Vec<Vec<f32>>,
    /// `mip[level][frame]`: the frame with harmonics up to `512 >> level`, `level_len(level)` samples.
    mip: Vec<Vec<Vec<f32>>>,
}

/// Samples in a band-limited copy (four per harmonic, 64 … 2048).
pub fn level_len(level: usize) -> usize {
    (4 * (512 >> level)).clamp(64, FRAME)
}

impl Wavetable {
    /// Builds the band-limited copies of `frames` (each resampled to FRAME samples, DC removed, peak 1).
    pub fn new(name: &str, mut frames: Vec<Vec<f32>>) -> Self {
        if frames.is_empty() {
            frames.push((0..FRAME).map(|i| (i as f32 / FRAME as f32 * std::f32::consts::TAU).sin()).collect());
        }
        frames.truncate(MAX_FRAMES);
        let mut planner = FftPlanner::<f32>::new();
        let fwd = planner.plan_fft_forward(FRAME);
        let inv: Vec<_> = (0..LEVELS).map(|l| planner.plan_fft_inverse(level_len(l))).collect();
        let mut mip: Vec<Vec<Vec<f32>>> = (0..LEVELS).map(|_| Vec::with_capacity(frames.len())).collect();
        // One gain for the whole table so positions keep their relative levels.
        let peak = frames.iter().flat_map(|f| f.iter()).fold(0.0f32, |m, v| m.max(v.abs())).max(1e-9);
        for f in frames.iter_mut() {
            let mean = f.iter().sum::<f32>() / f.len() as f32;
            f.iter_mut().for_each(|v| *v = (*v - mean) / peak);
            let mut spec: Vec<Complex<f32>> = f.iter().map(|v| Complex::new(*v, 0.0)).collect();
            fwd.process(&mut spec);
            for (l, plan) in inv.iter().enumerate() {
                let n = level_len(l);
                let h = 512 >> l;
                let mut buf = vec![Complex::new(0.0f32, 0.0); n];
                for k in 1..=h.min(n / 2 - 1) {
                    buf[k] = spec[k];
                    buf[n - k] = spec[FRAME - k];
                }
                plan.process(&mut buf);
                mip[l].push(buf.iter().map(|c| c.re / FRAME as f32).collect());
            }
        }
        Wavetable { name: name.into(), frames, mip }
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// The wave at `phase` (0 … 1), table position `pos` (0 … 1) and band-limit `level` (see
    /// [`level_for`]).
    #[inline]
    pub fn sample(&self, pos: f32, level: f32, phase: f64) -> f32 {
        let fp = pos.clamp(0.0, 1.0) * (self.frames.len() - 1) as f32;
        let f0 = fp as usize;
        let f1 = (f0 + 1).min(self.frames.len() - 1);
        let ff = fp - f0 as f32;
        // The copy whose harmonics all stay below Nyquist (never a brighter one: that would alias).
        let l = (level.ceil() as usize).min(LEVELS - 1);
        let n = level_len(l);
        let ph = if (0.0..1.0).contains(&phase) { phase } else { phase.rem_euclid(1.0) };
        let x = ph * n as f64;
        let i = (x as usize).min(n - 1);
        let j = if i + 1 == n { 0 } else { i + 1 };
        let t = (x - x.floor()) as f32;
        let a = &self.mip[l][f0];
        let b = &self.mip[l][f1];
        let va = a[i] + (a[j] - a[i]) * t;
        let vb = b[i] + (b[j] - b[i]) * t;
        va + (vb - va) * ff
    }
}

/// The band-limit level for a note of `hz` at `rate`: the copy used keeps every harmonic below 0.45 × rate.
#[inline]
pub fn level_for(hz: f32, rate: f32) -> f32 {
    let allowed = (0.45 * rate / hz.max(1.0)).max(1.0);
    (512.0 / allowed).log2().clamp(0.0, (LEVELS - 1) as f32)
}

/// The factory tables, in menu order.
pub const FACTORY: [&str; 12] = ["Basic Shapes", "Pulse Width", "Harmonic Sweep", "Formant Vowels", "Sync Sweep", "FM Bells", "Digital Steps", "Fold", "Odd Even", "Organ", "Noise Spectra", "Dragon Growl"];

fn additive(n_frames: usize, f: impl Fn(f32, usize) -> f32) -> Vec<Vec<f32>> {
    // f(position, harmonic) = amplitude of the harmonic (sine phase).
    (0..n_frames)
        .map(|k| {
            let pos = k as f32 / (n_frames - 1).max(1) as f32;
            let amps: Vec<(usize, f32)> = (1..=256).map(|h| (h, f(pos, h))).filter(|(_, a)| a.abs() > 1e-5).collect();
            (0..FRAME)
                .map(|i| {
                    let x = i as f32 / FRAME as f32 * std::f32::consts::TAU;
                    amps.iter().map(|(h, a)| a * (x * *h as f32).sin()).sum::<f32>()
                })
                .collect()
        })
        .collect()
}

fn shaped(n_frames: usize, f: impl Fn(f32, f32) -> f32) -> Vec<Vec<f32>> {
    // f(position, phase 0..1) = sample.
    (0..n_frames).map(|k| (0..FRAME).map(|i| f(k as f32 / (n_frames - 1).max(1) as f32, i as f32 / FRAME as f32)).collect()).collect()
}

fn factory_frames(name: &str) -> Vec<Vec<f32>> {
    use std::f32::consts::{PI, TAU};
    let tri = |p: f32| 1.0 - 4.0 * (p - 0.25).rem_euclid(1.0).sub_abs_half();
    match name {
        // Sine → triangle → saw → square.
        "Basic Shapes" => shaped(64, |pos, p| {
            let shapes = [(p * TAU).sin(), tri(p), 2.0 * (p + 0.5).rem_euclid(1.0) - 1.0, if p < 0.5 { 0.8 } else { -0.8 }];
            let x = pos * 3.0;
            let i = (x as usize).min(2);
            let t = x - i as f32;
            shapes[i] * (1.0 - t) + shapes[i + 1] * t
        }),
        "Pulse Width" => shaped(64, |pos, p| {
            let w = 0.5 - pos * 0.46;
            if p < w {
                1.0 - w
            } else {
                -w
            }
        }),
        // A saw opening up harmonic by harmonic.
        "Harmonic Sweep" => additive(64, |pos, h| {
            let open = 1.0 + pos * 80.0;
            if (h as f32) <= open {
                1.0 / h as f32
            } else {
                0.0
            }
        }),
        // A–E–I–O–U formants on a saw.
        "Formant Vowels" => {
            const V: [[f32; 3]; 5] = [[800.0, 1150.0, 2900.0], [400.0, 1600.0, 2700.0], [270.0, 2100.0, 3000.0], [450.0, 800.0, 2830.0], [325.0, 700.0, 2530.0]];
            additive(64, |pos, h| {
                let x = pos * 4.0;
                let i = (x as usize).min(3);
                let t = x - i as f32;
                let f0 = 110.0;
                let hz = f0 * h as f32;
                let mut a = 0.0;
                for k in 0..3 {
                    let fc = V[i][k] * (1.0 - t) + V[i + 1][k] * t;
                    let bw = 80.0 + 40.0 * k as f32;
                    a += (1.0 / (1.0 + ((hz - fc) / bw).powi(2))) * [1.0, 0.6, 0.35][k];
                }
                a / (h as f32).sqrt()
            })
        }
        "Sync Sweep" => shaped(64, |pos, p| {
            let ratio = 1.0 + pos * 7.0;
            let s = (p * ratio).rem_euclid(1.0);
            (2.0 * s - 1.0) * (1.0 - p).powf(0.3)
        }),
        "FM Bells" => shaped(64, |pos, p| {
            let index = pos * 8.0;
            (p * TAU + index * (p * TAU * 3.5).sin()).sin()
        }),
        "Digital Steps" => shaped(64, |pos, p| {
            let steps = (2.0 + (1.0 - pos) * 30.0).round();
            ((p * TAU).sin() * steps).round() / steps
        }),
        "Fold" => shaped(64, |pos, p| ((p * TAU).sin() * (1.0 + pos * 7.0) * PI / 2.0).sin()),
        // Odd harmonics (square-ish) → even harmonics.
        "Odd Even" => additive(64, |pos, h| {
            let a = 1.0 / h as f32;
            if h == 1 {
                a
            } else if h % 2 == 1 {
                a * (1.0 - pos)
            } else {
                a * pos
            }
        }),
        // Drawbar registrations.
        "Organ" => {
            const BARS: [[f32; 9]; 4] =
                [[8.0, 8.0, 8.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], [8.0, 6.0, 8.0, 6.0, 0.0, 4.0, 0.0, 0.0, 0.0], [6.0, 8.0, 8.0, 8.0, 6.0, 6.0, 4.0, 4.0, 8.0], [8.0, 0.0, 8.0, 0.0, 8.0, 0.0, 6.0, 0.0, 8.0]];
            const HARM: [usize; 9] = [1, 3, 2, 4, 6, 8, 10, 12, 16];
            additive(32, |pos, h| {
                let x = pos * 3.0;
                let i = (x as usize).min(2);
                let t = x - i as f32;
                HARM.iter().enumerate().filter(|(_, hh)| **hh == h).map(|(k, _)| (BARS[i][k] * (1.0 - t) + BARS[i + 1][k] * t) / 8.0).sum()
            })
        }
        // Random spectra that morph (fixed seeds, so the table is always the same).
        "Noise Spectra" => {
            let mut rng = crate::dsp::Rng::new(77);
            let keys: Vec<Vec<f32>> = (0..5).map(|_| (0..256).map(|h| rng.next_f32() / (1.0 + h as f32 * 0.05)).collect()).collect();
            additive(64, |pos, h| {
                let x = pos * 4.0;
                let i = (x as usize).min(3);
                let t = x - i as f32;
                keys[i][h - 1] * (1.0 - t) + keys[i + 1][h - 1] * t
            })
        }
        // A saw through a moving resonant formant, folded a little: a ready-made growl.
        "Dragon Growl" => {
            let base = additive(64, |pos, h| {
                let fc = 3.0 + pos * 28.0;
                let res = 1.0 + 2.5 / (1.0 + ((h as f32 - fc) / 1.8).powi(2));
                res / h as f32
            });
            base.into_iter()
                .enumerate()
                .map(|(k, f)| {
                    let drive = 1.0 + k as f32 / 63.0 * 2.5;
                    let peak = f.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-9);
                    f.iter().map(|v| ((v / peak) * drive).tanh()).collect()
                })
                .collect()
        }
        _ => factory_frames("Basic Shapes"),
    }
}

/// Cuts an audio file into frames: whole 2048-sample cycles when the length is a multiple of 2048
/// (wavetable files), else the sound split into up to 64 cycles (each resampled to 2048 samples).
pub fn frames_from_audio(mono: &[f32]) -> Vec<Vec<f32>> {
    if mono.is_empty() {
        return Vec::new();
    }
    if mono.len().is_multiple_of(FRAME) {
        return mono.chunks(FRAME).take(MAX_FRAMES).map(|c| c.to_vec()).collect();
    }
    let n = if mono.len() <= FRAME * 2 { 1 } else { (mono.len() / FRAME).clamp(2, 64) };
    let seg = mono.len() / n;
    (0..n)
        .map(|k| {
            let s = &mono[k * seg..(k + 1) * seg];
            (0..FRAME)
                .map(|i| {
                    let x = i as f64 / FRAME as f64 * (s.len() - 1) as f64;
                    let a = x as usize;
                    let b = (a + 1).min(s.len() - 1);
                    let t = (x - a as f64) as f32;
                    s[a] + (s[b] - s[a]) * t
                })
                .collect()
        })
        .collect()
}

type TableCache = Mutex<HashMap<TableSource, Arc<Wavetable>>>;

fn cache() -> &'static TableCache {
    static C: OnceLock<TableCache> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The table for `src` (built once, then shared). A missing or unreadable file gives the basic table.
pub fn table(src: &TableSource) -> Arc<Wavetable> {
    if let Some(t) = cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(src) {
        return t.clone();
    }
    let t = Arc::new(match src {
        TableSource::Factory(name) => Wavetable::new(name, factory_frames(name)),
        TableSource::File(path) => Wavetable::new(&src.name(), load_file(path)),
    });
    cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(src.clone(), t.clone());
    t
}

fn load_file(path: &Path) -> Vec<Vec<f32>> {
    match crate::samples::decode(path) {
        Ok(d) => {
            let mono: Vec<f32> = d.l.iter().zip(d.r.iter()).map(|(a, b)| 0.5 * (a + b)).collect();
            frames_from_audio(&mono)
        }
        Err(_) => factory_frames("Basic Shapes"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_limited_copies_hold_no_harmonics_above_their_limit() {
        let t = table(&TableSource::Factory("Basic Shapes".into()));
        assert_eq!(t.len(), 64);
        // The saw frame, at the copy for harmonics ≤ 8: its spectrum above 8 is empty.
        let l = 6; // 512 >> 6 = 8
        let n = level_len(l);
        let saw = &t.mip[l][42];
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(n);
        let mut s: Vec<Complex<f32>> = saw.iter().map(|v| Complex::new(*v, 0.0)).collect();
        fft.process(&mut s);
        let mag = |k: usize| s[k].norm() / n as f32;
        assert!(mag(1) > 0.1 && mag(8) > 0.005);
        for k in 9..n / 2 {
            assert!(mag(k) < 1e-4, "harmonic {k}: {}", mag(k));
        }
    }

    #[test]
    fn every_factory_table_is_made_and_normalised() {
        for name in FACTORY {
            let t = table(&TableSource::Factory(name.into()));
            assert!(t.len() >= 2, "{name}");
            let peak = t.frames.iter().flat_map(|f| f.iter()).fold(0.0f32, |m, v| m.max(v.abs()));
            assert!((peak - 1.0).abs() < 0.05 || peak > 0.3, "{name}: peak {peak}");
            assert!(t.sample(0.5, 3.0, 0.25).is_finite());
        }
        assert!(presets().len() >= 15);
    }

    #[test]
    fn audio_is_cut_into_cycles() {
        assert_eq!(frames_from_audio(&vec![0.1; FRAME * 8]).len(), 8);
        assert_eq!(frames_from_audio(&vec![0.1; 900]).len(), 1);
        assert_eq!(frames_from_audio(&vec![0.1; 48000]).len(), 23);
    }
}
