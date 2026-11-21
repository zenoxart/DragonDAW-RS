//! Emberkeys: the model of the modelled piano (grands, upright, felt, honky-tonk) and its electric
//! and percussive cousins (tine and reed electric pianos, toy piano, celesta). The sound is modelled,
//! not sampled (see `instruments::piano`). This module holds the parameters and the arpeggiator.

use crate::wt::FxSlot;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum PianoModel {
    #[default]
    ConcertGrand,
    StudioGrand,
    BrightGrand,
    Upright,
    Felt,
    HonkyTonk,
    Tine,
    Reed,
    Toy,
    Celesta,
}

impl PianoModel {
    pub const ALL: [PianoModel; 10] = [
        PianoModel::ConcertGrand,
        PianoModel::StudioGrand,
        PianoModel::BrightGrand,
        PianoModel::Upright,
        PianoModel::Felt,
        PianoModel::HonkyTonk,
        PianoModel::Tine,
        PianoModel::Reed,
        PianoModel::Toy,
        PianoModel::Celesta,
    ];
    pub fn name(self) -> &'static str {
        match self {
            PianoModel::ConcertGrand => "Concert grand",
            PianoModel::StudioGrand => "Studio grand",
            PianoModel::BrightGrand => "Bright grand",
            PianoModel::Upright => "Upright",
            PianoModel::Felt => "Felt piano",
            PianoModel::HonkyTonk => "Honky-tonk",
            PianoModel::Tine => "Tine electric piano",
            PianoModel::Reed => "Reed electric piano",
            PianoModel::Toy => "Toy piano",
            PianoModel::Celesta => "Celesta",
        }
    }
    /// Struck strings (the piano family) rather than struck tines, reeds or bars.
    pub fn strings(self) -> bool {
        !matches!(self, PianoModel::Tine | PianoModel::Reed | PianoModel::Toy | PianoModel::Celesta)
    }
    /// The playable range (lowest, highest key).
    pub fn range(self) -> (u8, u8) {
        match self {
            PianoModel::Tine => (28, 103),
            PianoModel::Reed => (33, 96),
            PianoModel::Toy => (53, 96),
            PianoModel::Celesta => (60, 108),
            _ => (21, 108),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ArpMode {
    #[default]
    Up,
    Down,
    UpDown,
    Random,
    AsPlayed,
    Chord,
}

impl ArpMode {
    pub const ALL: [ArpMode; 6] = [ArpMode::Up, ArpMode::Down, ArpMode::UpDown, ArpMode::Random, ArpMode::AsPlayed, ArpMode::Chord];
    pub fn name(self) -> &'static str {
        match self {
            ArpMode::Up => "Up",
            ArpMode::Down => "Down",
            ArpMode::UpDown => "Up & down",
            ArpMode::Random => "Random",
            ArpMode::AsPlayed => "As played",
            ArpMode::Chord => "Chord repeat",
        }
    }
}

/// Step lengths the arpeggiator offers (beats, name).
pub const ARP_RATES: [(f32, &str); 7] = [(1.0, "1/4"), (0.5, "1/8"), (1.0 / 3.0, "1/8 T"), (0.25, "1/16"), (1.0 / 6.0, "1/16 T"), (0.125, "1/32"), (2.0, "1/2")];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Arp {
    pub on: bool,
    pub mode: ArpMode,
    /// Beats per step.
    pub rate: f32,
    pub octaves: u8,
    /// Note length as a share of the step.
    pub gate: f32,
    /// Delays every second step (0 … 0.5 of a step).
    pub swing: f32,
    /// Accents the first step of every beat.
    pub accent: f32,
}

impl Default for Arp {
    fn default() -> Self {
        Arp { on: false, mode: ArpMode::Up, rate: 0.25, octaves: 2, gate: 0.8, swing: 0.0, accent: 0.3 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PianoParams {
    pub preset: String,
    pub model: PianoModel,
    /// Hammer hardness / brightness (0 soft … 1 hard).
    pub tone: f32,
    /// Soundboard and case resonance.
    pub body: f32,
    /// The hammer's knock and felt noise.
    pub hammer: f32,
    /// How quickly the dampers stop a note (seconds).
    pub release: f32,
    /// Damper and key-release noise.
    pub release_noise: f32,
    /// How closely the two or three strings of a note agree (1 tight … 0 loose, honky-tonk).
    pub tightness: f32,
    /// Sympathetic resonance of the other strings (stronger with the sustain pedal down).
    pub resonance: f32,
    /// String stiffness: how much the upper partials are stretched (the piano "shimmer").
    pub stiffness: f32,
    /// Stretch tuning (bass flat, treble sharp, like a tuned piano).
    pub stretch: f32,
    /// How long the strings ring (× the natural ring time).
    pub sustain: f32,
    /// Darkens the top end.
    pub high_damp: f32,
    /// Pedal mechanism noise.
    pub pedal_noise: f32,
    /// Velocity range the keys are mapped into (low, high).
    pub velocity_low: f32,
    pub velocity_high: f32,
    /// How much velocity changes the level (0 none … 1 full concert dynamics).
    pub dynamics: f32,
    /// A slow swell instead of a strike (seconds), for pads and film scores.
    pub swell: f32,
    /// The sustain pedal held down permanently.
    pub pedal_latch: bool,
    /// Electric pianos: pickup drive (bark) and stereo tremolo.
    pub drive: f32,
    pub tremolo_depth: f32,
    pub tremolo_rate: f32,
    pub width: f32,
    pub arp: Arp,
    pub fx: Vec<FxSlot>,
    pub master: f32,
    pub trim_db: f32,
}

impl Default for PianoParams {
    fn default() -> Self {
        PianoParams {
            preset: "Init".into(),
            model: PianoModel::ConcertGrand,
            tone: 0.55,
            body: 0.6,
            hammer: 0.4,
            release: 0.18,
            release_noise: 0.3,
            tightness: 0.8,
            resonance: 0.35,
            stiffness: 0.5,
            stretch: 0.6,
            sustain: 1.0,
            high_damp: 0.15,
            pedal_noise: 0.3,
            velocity_low: 0.0,
            velocity_high: 1.0,
            dynamics: 0.75,
            swell: 0.0,
            pedal_latch: false,
            drive: 0.2,
            tremolo_depth: 0.0,
            tremolo_rate: 4.5,
            width: 0.7,
            arp: Arp::default(),
            fx: Vec::new(),
            master: 0.8,
            trim_db: 0.0,
        }
    }
}

impl PianoParams {
    pub fn for_model(model: PianoModel) -> Self {
        let mut p = PianoParams { model, ..Default::default() };
        match model {
            PianoModel::StudioGrand => {
                p.tone = 0.6;
                p.body = 0.5;
            }
            PianoModel::BrightGrand => {
                p.tone = 0.8;
                p.hammer = 0.5;
                p.body = 0.45;
            }
            PianoModel::Upright => {
                p.tone = 0.5;
                p.body = 0.75;
                p.sustain = 0.7;
                p.stiffness = 0.65;
                p.tightness = 0.7;
            }
            PianoModel::Felt => {
                p.tone = 0.15;
                p.hammer = 0.7;
                p.release_noise = 0.6;
                p.pedal_noise = 0.5;
                p.sustain = 0.8;
                p.high_damp = 0.45;
            }
            PianoModel::HonkyTonk => {
                p.tightness = 0.1;
                p.tone = 0.7;
                p.sustain = 0.6;
                p.body = 0.7;
            }
            PianoModel::Tine => {
                p.tone = 0.5;
                p.drive = 0.25;
                p.tremolo_depth = 0.3;
                p.release = 0.12;
            }
            PianoModel::Reed => {
                p.tone = 0.55;
                p.drive = 0.45;
                p.tremolo_depth = 0.25;
                p.tremolo_rate = 5.5;
                p.release = 0.1;
            }
            PianoModel::Toy => {
                p.tone = 0.7;
                p.release = 0.15;
                p.sustain = 0.6;
            }
            PianoModel::Celesta => {
                p.tone = 0.55;
                p.release = 0.3;
            }
            PianoModel::ConcertGrand => {}
        }
        p
    }
}
