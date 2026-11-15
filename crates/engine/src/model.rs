//! The project model: everything a song is made of, as plain serializable data.
//!
//! The UI edits a [`Project`]; the engine compiles it into a [`crate::render::Song`] for playback and export.
//! Times on the timeline are in **beats** (quarter notes, `f64`); notes inside a pattern are in **ticks**
//! ([`PPQ`] per beat). Volumes are linear gain, pans are −1 … +1.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Ticks per quarter note for pattern notes.
pub const PPQ: u32 = 96;
/// Id of the master mixer channel.
pub const MASTER: Id = 0;

pub type Id = u64;

/// A colour as RGB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Default for Rgb {
    fn default() -> Self {
        Rgb(196, 30, 58)
    }
}

/// The palette new items cycle through.
pub const PALETTE: [Rgb; 10] = [Rgb(229, 57, 53), Rgb(255, 152, 0), Rgb(253, 216, 53), Rgb(124, 179, 66), Rgb(38, 166, 154), Rgb(41, 182, 246), Rgb(92, 107, 192), Rgb(171, 71, 188), Rgb(236, 64, 122), Rgb(141, 110, 99)];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Metadata {
    pub name: String,
    pub artist: String,
    pub genre: String,
    pub description: String,
}

impl Default for Metadata {
    fn default() -> Self {
        Metadata { name: "Untitled".into(), artist: String::new(), genre: String::new(), description: String::new() }
    }
}

/// A tempo change at a beat (the tempo between points is constant – a step map).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TempoPoint {
    pub beat: f64,
    pub bpm: f64,
}

/// A time-signature change at a bar boundary (given in beats).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TimeSignature {
    pub beat: f64,
    pub numerator: u32,
    pub denominator: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    pub id: Id,
    pub beat: f64,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Metronome {
    pub enabled: bool,
    pub volume: f32,
    /// Bars of count-in before recording starts.
    pub count_in_bars: u32,
    /// Click only while recording.
    pub only_when_recording: bool,
}

impl Default for Metronome {
    fn default() -> Self {
        Metronome { enabled: false, volume: 0.6, count_in_bars: 1, only_when_recording: false }
    }
}

/// A beat range `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Range {
    pub start: f64,
    pub end: f64,
}

impl Range {
    pub fn new(start: f64, end: f64) -> Self {
        Range { start: start.min(end), end: start.max(end) }
    }
    pub fn len(&self) -> f64 {
        self.end - self.start
    }
    pub fn is_empty(&self) -> bool {
        self.len() <= 0.0
    }
    pub fn contains(&self, beat: f64) -> bool {
        beat >= self.start && beat < self.end
    }
}

// ---------------------------------------------------------------------------------------------------- playlist

/// How the clip's audio follows pitch and tempo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum StretchMode {
    /// Plays at the original speed; pitch shifting resamples (tape style).
    #[default]
    Resample,
    /// Pitch and length are independent (time-stretch, WSOLA).
    Stretch,
}

/// A point on a clip's gain line (beat relative to the clip start, gain in dB).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GainPoint {
    pub beat: f64,
    pub db: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FadeCurve {
    Linear,
    #[default]
    EqualPower,
    Exponential,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioClip {
    /// Audio file (relative to the project folder when inside it).
    pub source: PathBuf,
    /// Seconds into the source where the clip starts.
    pub offset: f64,
    pub gain_db: f32,
    pub pan: f32,
    /// Fade lengths in beats.
    pub fade_in: f64,
    pub fade_out: f64,
    pub fade_curve: FadeCurve,
    pub reverse: bool,
    /// Pitch shift in semitones (−24 … +24) plus cents.
    pub pitch: f32,
    pub stretch: StretchMode,
    /// Playback speed (`Stretch`: tempo factor; `Resample`: rate). 1 = original.
    pub speed: f64,
    /// When set, the clip is warped from this tempo to the project tempo (Stretch mode).
    pub source_bpm: Option<f64>,
    /// Clip gain line (Pro Tools style); empty = flat.
    pub gain_line: Vec<GainPoint>,
    /// Take this clip came from (comping).
    pub take: Option<Id>,
}

impl Default for AudioClip {
    fn default() -> Self {
        AudioClip {
            source: PathBuf::new(),
            offset: 0.0,
            gain_db: 0.0,
            pan: 0.0,
            fade_in: 0.0,
            fade_out: 0.0,
            fade_curve: FadeCurve::EqualPower,
            reverse: false,
            pitch: 0.0,
            stretch: StretchMode::Resample,
            speed: 1.0,
            source_bpm: None,
            gain_line: Vec::new(),
            take: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClipKind {
    Audio(AudioClip),
    /// Plays a Channel-Rack pattern.
    Pattern {
        pattern: Id,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub id: Id,
    pub name: String,
    /// Start on the timeline in beats.
    pub start: f64,
    /// Length in beats.
    pub length: f64,
    pub muted: bool,
    pub color: Rgb,
    pub kind: ClipKind,
}

impl Clip {
    pub fn end(&self) -> f64 {
        self.start + self.length
    }
    pub fn audio(&self) -> Option<&AudioClip> {
        match &self.kind {
            ClipKind::Audio(a) => Some(a),
            _ => None,
        }
    }
    pub fn audio_mut(&mut self) -> Option<&mut AudioClip> {
        match &mut self.kind {
            ClipKind::Audio(a) => Some(a),
            _ => None,
        }
    }
}

/// A recorded alternative (loop recording / playlists) kept below the track for comping.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Take {
    pub id: Id,
    pub name: String,
    pub clips: Vec<Clip>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MonitorMode {
    /// Hear the input while armed.
    #[default]
    Auto,
    /// Always hear the input.
    Input,
    Off,
}

/// Where a recording comes from: one or two device input channels (0-based).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputSource {
    pub left: u16,
    /// `None` = mono input.
    pub right: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: Id,
    pub name: String,
    pub color: Rgb,
    pub height: f32,
    pub muted: bool,
    pub solo: bool,
    /// Armed for audio recording.
    pub armed: bool,
    pub input: Option<InputSource>,
    pub monitor: MonitorMode,
    /// Mixer channel the track's audio goes to.
    pub mixer: Id,
    pub clips: Vec<Clip>,
    pub takes: Vec<Take>,
    pub show_takes: bool,
    /// Edit group (tracks in a group move/mute/solo together).
    pub group: Option<Id>,
    /// Pinned to the top of the playlist (Track Pin).
    pub pinned: bool,
}

// ---------------------------------------------------------------------------------------------------- channel rack

/// Delay → Attack → Hold → Decay → Sustain → Release amplitude envelope (seconds, sustain 0 … 1).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Envelope {
    pub enabled: bool,
    pub delay: f32,
    pub attack: f32,
    pub hold: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    /// Curve bend −1 (log) … 0 (linear) … +1 (exp).
    pub attack_tension: f32,
    pub release_tension: f32,
}

impl Default for Envelope {
    fn default() -> Self {
        Envelope { enabled: false, delay: 0.0, attack: 0.001, hold: 0.0, decay: 0.3, sustain: 1.0, release: 0.05, attack_tension: 0.0, release_tension: 0.0 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SamplerParams {
    pub path: PathBuf,
    /// MIDI note that plays the sample at its own pitch.
    pub root_note: u8,
    /// Start / end as fractions of the sample.
    pub start: f32,
    pub end: f32,
    pub looped: bool,
    pub reverse: bool,
    /// Fine tune in cents.
    pub fine: f32,
    /// Slice points (fractions); when set, note `root + i` plays slice `i` (beat slicer).
    pub slices: Vec<f32>,
    /// Glide time for slide notes in seconds.
    pub glide: f32,
}

impl Default for SamplerParams {
    fn default() -> Self {
        SamplerParams { path: PathBuf::new(), root_note: 60, start: 0.0, end: 1.0, looped: false, reverse: false, fine: 0.0, slices: Vec::new(), glide: 0.05 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Waveform {
    #[default]
    Saw,
    Square,
    Triangle,
    Sine,
    Noise,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FilterMode {
    #[default]
    LowPass,
    HighPass,
    BandPass,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Oscillator {
    pub wave: Waveform,
    pub level: f32,
    /// Coarse in semitones, fine in cents.
    pub coarse: i32,
    pub fine: f32,
    /// Pulse width for square (0.05 … 0.95).
    pub pulse_width: f32,
}

impl Default for Oscillator {
    fn default() -> Self {
        Oscillator { wave: Waveform::Saw, level: 0.8, coarse: 0, fine: 0.0, pulse_width: 0.5 }
    }
}

/// The built-in subtractive synthesizer (3 oscillators → filter → amp, like 3x OSC).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SynthParams {
    pub osc: [Oscillator; 3],
    pub unison: u32,
    pub detune: f32,
    pub filter: FilterMode,
    /// Cutoff in Hz.
    pub cutoff: f32,
    pub resonance: f32,
    /// Filter envelope amount in octaves.
    pub filter_env: f32,
    pub filter_adsr: Envelope,
    pub amp_adsr: Envelope,
    /// LFO rate in Hz and depths (pitch in semitones, filter in octaves).
    pub lfo_rate: f32,
    pub lfo_pitch: f32,
    pub lfo_filter: f32,
    pub glide: f32,
    pub polyphony: u32,
    pub gain: f32,
}

impl Default for SynthParams {
    fn default() -> Self {
        let mut osc = [Oscillator::default(); 3];
        osc[1] = Oscillator { wave: Waveform::Square, level: 0.5, coarse: -12, ..Oscillator::default() };
        osc[2] = Oscillator { wave: Waveform::Saw, level: 0.0, fine: 7.0, ..Oscillator::default() };
        SynthParams {
            osc,
            unison: 1,
            detune: 10.0,
            filter: FilterMode::LowPass,
            cutoff: 4000.0,
            resonance: 0.2,
            filter_env: 2.0,
            filter_adsr: Envelope { enabled: true, attack: 0.005, decay: 0.4, sustain: 0.3, release: 0.3, ..Envelope::default() },
            amp_adsr: Envelope { enabled: true, attack: 0.005, decay: 0.2, sustain: 0.8, release: 0.25, ..Envelope::default() },
            lfo_rate: 5.0,
            lfo_pitch: 0.0,
            lfo_filter: 0.0,
            glide: 0.08,
            polyphony: 16,
            gain: 0.5,
        }
    }
}

/// One sample of a drum pad, played for a velocity range (velocity layers).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PadLayer {
    pub path: PathBuf,
    pub velocity_low: f32,
    pub velocity_high: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pad {
    pub name: String,
    pub note: u8,
    pub layers: Vec<PadLayer>,
    pub gain: f32,
    pub pan: f32,
    pub pitch: f32,
    /// Pads of the same choke group cut each other (0 = none).
    pub choke: u8,
}

impl Default for Pad {
    fn default() -> Self {
        Pad { name: String::new(), note: 36, layers: Vec::new(), gain: 1.0, pan: 0.0, pitch: 0.0, choke: 0 }
    }
}

/// Plugin formats the host can load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum PluginFormat {
    Clap,
    Vst3,
}

/// A third-party plugin instance (instrument or effect).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginRef {
    pub format: PluginFormat,
    /// Library / bundle path.
    pub path: PathBuf,
    /// Plugin id inside the library (CLAP id or VST3 class id as hex).
    pub id: String,
    pub name: String,
    /// Saved plugin state (opaque).
    #[serde(default)]
    pub state: Vec<u8>,
    /// Parameter values by parameter id (plain values).
    #[serde(default)]
    pub params: BTreeMap<u32, f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Instrument {
    Sampler(SamplerParams),
    Synth(SynthParams),
    DrumPads(Vec<Pad>),
    Plugin(PluginRef),
}

impl Instrument {
    pub fn kind_name(&self) -> &'static str {
        match self {
            Instrument::Sampler(s) if !s.slices.is_empty() => "Slicer",
            Instrument::Sampler(_) => "Sampler",
            Instrument::Synth(_) => "Synth",
            Instrument::DrumPads(_) => "Drum Pads",
            Instrument::Plugin(_) => "Plugin",
        }
    }
}

/// A Channel-Rack channel (global, like FL Studio; patterns hold its steps and notes).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RackChannel {
    pub id: Id,
    pub name: String,
    pub color: Rgb,
    pub instrument: Instrument,
    pub volume: f32,
    pub pan: f32,
    /// Pitch in semitones.
    pub pitch: f32,
    pub muted: bool,
    pub solo: bool,
    /// Mixer channel it plays into.
    pub mixer: Id,
    pub envelope: Envelope,
    /// Voice-cut groups (FL "Cut" / "By"): 0 = none.
    pub cut_group: u8,
    pub cut_by: u8,
    pub cut_self: bool,
    /// Rack display group (filter).
    #[serde(default)]
    pub group: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Step {
    pub on: bool,
    pub velocity: f32,
    pub pan: f32,
    /// Semitones.
    pub pitch: f32,
    /// Fine shift (fraction of a step, −0.5 … 0.5).
    pub shift: f32,
}

impl Default for Step {
    fn default() -> Self {
        Step { on: false, velocity: 0.8, pan: 0.0, pitch: 0.0, shift: 0.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Note {
    pub pitch: u8,
    /// Start and length in ticks ([`PPQ`] per beat).
    pub start: u32,
    pub length: u32,
    pub velocity: f32,
    pub pan: f32,
    pub release: f32,
    /// Slide (portamento) note: glides from the previous note.
    pub slide: bool,
    pub muted: bool,
    /// Colour group / MIDI channel.
    pub color: u8,
    pub selected: bool,
}

impl Default for Note {
    fn default() -> Self {
        Note { pitch: 60, start: 0, length: PPQ / 4, velocity: 0.8, pan: 0.0, release: 0.5, slide: false, muted: false, color: 0, selected: false }
    }
}

impl Note {
    pub fn end(&self) -> u32 {
        self.start + self.length
    }
}

/// A pattern's data for one channel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PatternChannel {
    pub steps: Vec<Step>,
    pub notes: Vec<Note>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pattern {
    pub id: Id,
    pub name: String,
    pub color: Rgb,
    /// Steps (16ths) per pattern.
    pub steps: u32,
    /// Swing 0 … 1 (every second 16th is delayed by up to half a step).
    pub swing: f32,
    pub channels: BTreeMap<Id, PatternChannel>,
}

impl Pattern {
    /// Length in beats (at least the step grid, longer when notes reach further).
    pub fn length_beats(&self) -> f64 {
        let grid = self.steps as f64 / 4.0;
        let notes = self.channels.values().flat_map(|c| c.notes.iter()).map(|n| n.end() as f64 / PPQ as f64).fold(0.0, f64::max);
        let notes_bars = (notes / 4.0).ceil() * 4.0;
        grid.max(notes_bars)
    }
}

// ---------------------------------------------------------------------------------------------------- mixer

/// One insert effect.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Insert {
    pub id: Id,
    /// Built-in effect id (`eq`, `compressor`, …) or `plugin`.
    pub effect: String,
    pub enabled: bool,
    /// Dry/wet 0 … 1.
    pub mix: f32,
    pub params: BTreeMap<String, f32>,
    /// Mixer channel feeding the effect's sidechain input.
    pub sidechain: Option<Id>,
    pub plugin: Option<PluginRef>,
    /// Name of the loaded preset (UI only).
    #[serde(default)]
    pub preset: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MixerSend {
    pub target: Id,
    /// Linear gain.
    pub level: f32,
    /// Taken before the fader (and pan).
    pub pre_fader: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ChannelKind {
    /// Carries audio.
    #[default]
    Audio,
    /// A VCA master: carries no audio, scales the faders of its members.
    Vca,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MixerChannel {
    pub id: Id,
    pub name: String,
    pub color: Rgb,
    pub kind: ChannelKind,
    pub volume: f32,
    pub pan: f32,
    pub muted: bool,
    pub solo: bool,
    pub solo_safe: bool,
    pub phase_invert: bool,
    pub swap_lr: bool,
    /// Stereo width 0 (mono) … 1 … 2.
    pub width: f32,
    pub inserts: Vec<Insert>,
    pub sends: Vec<MixerSend>,
    /// Output target; `None` = master (ignored on the master).
    pub output: Option<Id>,
    /// VCA master controlling this channel.
    pub vca: Option<Id>,
    /// Audio input for monitoring / recording straight into the mixer.
    pub input: Option<InputSource>,
    /// Position in the surround field (used when the project has a surround format).
    #[serde(default)]
    pub surround: SurroundPan,
    /// Mix group (linked faders).
    pub group: Option<Id>,
}

impl MixerChannel {
    pub fn new(id: Id, name: &str, color: Rgb) -> Self {
        MixerChannel {
            id,
            name: name.into(),
            color,
            kind: ChannelKind::Audio,
            volume: 0.8,
            pan: 0.0,
            muted: false,
            solo: false,
            solo_safe: false,
            phase_invert: false,
            swap_lr: false,
            width: 1.0,
            inserts: Vec::new(),
            sends: Vec::new(),
            output: None,
            vca: None,
            input: None,
            group: None,
            surround: SurroundPan::default(),
        }
    }
}

/// Surround position of a mixer channel: `x` −1 (left) … 1 (right), `y` −1 (rear) … 1 (front), `z` 0 (ear level)
/// … 1 (overhead), the spread of its two stereo sides in degrees, and how much of it goes to the LFE.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SurroundPan {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub spread: f32,
    pub lfe: f32,
}

impl Default for SurroundPan {
    fn default() -> Self {
        SurroundPan { x: 0.0, y: 1.0, z: 0.0, spread: 30.0, lfe: 0.0 }
    }
}

/// Speaker layouts for surround mixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SurroundFormat {
    /// L R C LFE Ls Rs (SMPTE / WAVE channel order).
    S5_1,
    /// L R C LFE Lb Rb Ls Rs.
    S7_1,
    /// 7.1 plus four height speakers: L R C LFE Lb Rb Ls Rs Ltf Rtf Ltr Rtr (the Dolby Atmos 7.1.4 bed).
    S7_1_4,
}

/// Insert slots per channel.
pub const MAX_INSERTS: usize = 10;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mixer {
    pub master: MixerChannel,
    pub channels: Vec<MixerChannel>,
}

impl Default for Mixer {
    fn default() -> Self {
        let mut master = MixerChannel::new(MASTER, "Master", Rgb(196, 30, 58));
        master.volume = 1.0;
        Mixer { master, channels: Vec::new() }
    }
}

impl Mixer {
    pub fn get(&self, id: Id) -> Option<&MixerChannel> {
        if id == MASTER {
            Some(&self.master)
        } else {
            self.channels.iter().find(|c| c.id == id)
        }
    }
    pub fn get_mut(&mut self, id: Id) -> Option<&mut MixerChannel> {
        if id == MASTER {
            Some(&mut self.master)
        } else {
            self.channels.iter_mut().find(|c| c.id == id)
        }
    }
    pub fn all(&self) -> impl Iterator<Item = &MixerChannel> {
        std::iter::once(&self.master).chain(self.channels.iter())
    }
}

// ---------------------------------------------------------------------------------------------------- automation

/// What an automation lane controls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash, PartialOrd, Ord)]
pub enum Target {
    MixerVolume(Id),
    MixerPan(Id),
    MixerWidth(Id),
    MixerMute(Id),
    SendLevel { channel: Id, target: Id },
    InsertParam { channel: Id, insert: Id, param: String },
    InsertMix { channel: Id, insert: Id },
    ChannelVolume(Id),
    ChannelPan(Id),
    ChannelPitch(Id),
    SynthCutoff(Id),
    SynthResonance(Id),
    PluginParam { channel: Id, insert: Option<Id>, param: u32 },
    Tempo,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AutoPoint {
    pub beat: f64,
    /// Plain value of the target (gain, pan, Hz, …).
    pub value: f32,
    /// Curve tension to the next point: 0 linear, ±1 bent.
    #[serde(default)]
    pub tension: f32,
}

/// Console-style automation modes (Pro Tools).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AutomationMode {
    Off,
    #[default]
    Read,
    /// Overwrites everything while playing.
    Write,
    /// Writes while a control is held, returns to the lane afterwards.
    Touch,
    /// Writes while held and keeps the last value until stop.
    Latch,
    /// Writes relative changes (dB/offset) on top of the lane.
    Trim,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationLane {
    pub id: Id,
    pub target: Target,
    pub points: Vec<AutoPoint>,
    pub mode: AutomationMode,
    pub visible: bool,
}

impl AutomationLane {
    /// Value at a beat (`None` without points).
    pub fn value_at(&self, beat: f64) -> Option<f32> {
        let p = &self.points;
        if p.is_empty() {
            return None;
        }
        if beat <= p[0].beat {
            return Some(p[0].value);
        }
        let i = p.partition_point(|x| x.beat <= beat);
        if i >= p.len() {
            return Some(p[p.len() - 1].value);
        }
        let (a, b) = (p[i - 1], p[i]);
        let span = (b.beat - a.beat).max(1e-9);
        let mut t = ((beat - a.beat) / span) as f32;
        if a.tension.abs() > 1e-3 {
            // Exponential bend: tension > 0 → slow start, < 0 → fast start.
            let k = a.tension * 6.0;
            t = ((k * t).exp() - 1.0) / (k.exp() - 1.0);
        }
        Some(a.value + (b.value - a.value) * t)
    }
}

/// A MIDI controller mapping created with MIDI learn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MidiMapping {
    pub channel: u8,
    pub controller: u8,
    pub target: Target,
    pub min: f32,
    pub max: f32,
}

// ---------------------------------------------------------------------------------------------------- video

/// A video file placed on the timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoRef {
    pub path: PathBuf,
    /// Beat where the video's first frame sits.
    pub start_beat: f64,
    /// Seconds skipped at the start of the file.
    pub offset: f64,
    pub duration: f64,
    pub fps: f64,
    pub width: u32,
    pub height: u32,
}

impl Default for VideoRef {
    fn default() -> Self {
        VideoRef { path: PathBuf::new(), start_beat: 0.0, offset: 0.0, duration: 0.0, fps: 25.0, width: 0, height: 0 }
    }
}

// ---------------------------------------------------------------------------------------------------- project

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Project {
    pub format_version: u32,
    pub meta: Metadata,
    /// Base tempo (before tempo points).
    pub bpm: f64,
    pub tempo_points: Vec<TempoPoint>,
    pub time_signatures: Vec<TimeSignature>,
    pub markers: Vec<Marker>,
    pub loop_range: Option<Range>,
    pub loop_enabled: bool,
    pub punch: Option<Range>,
    pub punch_enabled: bool,
    pub metronome: Metronome,
    pub tracks: Vec<Track>,
    pub channels: Vec<RackChannel>,
    pub patterns: Vec<Pattern>,
    pub mixer: Mixer,
    pub automation: Vec<AutomationLane>,
    pub midi_mappings: Vec<MidiMapping>,
    /// Loudness target for the master meter / export normalisation (LUFS).
    pub loudness_target: f32,
    pub next_id: Id,
    /// Video for scoring / post: shown in sync with the playhead, can be exported with the mix.
    pub video: Option<VideoRef>,
    /// Surround format of the mix (`None` = stereo).
    pub surround: Option<SurroundFormat>,
}

impl Default for Project {
    fn default() -> Self {
        Project {
            format_version: 1,
            meta: Metadata::default(),
            bpm: 140.0,
            tempo_points: Vec::new(),
            time_signatures: vec![TimeSignature { beat: 0.0, numerator: 4, denominator: 4 }],
            markers: Vec::new(),
            loop_range: None,
            loop_enabled: false,
            punch: None,
            punch_enabled: false,
            metronome: Metronome::default(),
            tracks: Vec::new(),
            channels: Vec::new(),
            patterns: Vec::new(),
            mixer: Mixer::default(),
            automation: Vec::new(),
            midi_mappings: Vec::new(),
            loudness_target: -14.0,
            next_id: 1,
            video: None,
            surround: None,
        }
    }
}

impl Project {
    /// A new project with one pattern, a few tracks and matching mixer channels (like a fresh FL session).
    pub fn new_default() -> Self {
        let mut p = Project::default();
        let pat = p.next_id();
        p.patterns.push(Pattern { id: pat, name: "Pattern 1".into(), color: PALETTE[2], steps: 16, swing: 0.0, channels: BTreeMap::new() });
        for i in 0..4 {
            p.add_track(&format!("Track {}", i + 1));
        }
        p
    }

    pub fn next_id(&mut self) -> Id {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        id
    }

    pub fn track(&self, id: Id) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }
    pub fn track_mut(&mut self, id: Id) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }
    pub fn channel(&self, id: Id) -> Option<&RackChannel> {
        self.channels.iter().find(|c| c.id == id)
    }
    pub fn channel_mut(&mut self, id: Id) -> Option<&mut RackChannel> {
        self.channels.iter_mut().find(|c| c.id == id)
    }
    pub fn pattern(&self, id: Id) -> Option<&Pattern> {
        self.patterns.iter().find(|p| p.id == id)
    }
    pub fn pattern_mut(&mut self, id: Id) -> Option<&mut Pattern> {
        self.patterns.iter_mut().find(|p| p.id == id)
    }

    /// Adds a mixer channel and returns its id.
    pub fn add_mixer_channel(&mut self, name: &str) -> Id {
        let id = self.next_id();
        let color = PALETTE[self.mixer.channels.len() % PALETTE.len()];
        self.mixer.channels.push(MixerChannel::new(id, name, color));
        id
    }

    /// Adds a playlist track with its own mixer channel.
    pub fn add_track(&mut self, name: &str) -> Id {
        let mixer = self.add_mixer_channel(name);
        let id = self.next_id();
        let color = PALETTE[self.tracks.len() % PALETTE.len()];
        self.tracks.push(Track {
            id,
            name: name.into(),
            color,
            height: 64.0,
            muted: false,
            solo: false,
            armed: false,
            input: Some(InputSource { left: 0, right: Some(1) }),
            monitor: MonitorMode::Auto,
            mixer,
            clips: Vec::new(),
            takes: Vec::new(),
            show_takes: false,
            group: None,
            pinned: false,
        });
        id
    }

    /// Adds a Channel-Rack channel with its own mixer channel.
    pub fn add_channel(&mut self, name: &str, instrument: Instrument) -> Id {
        let mixer = self.add_mixer_channel(name);
        let id = self.next_id();
        let color = PALETTE[(self.channels.len() + 3) % PALETTE.len()];
        self.channels.push(RackChannel {
            id,
            name: name.into(),
            color,
            instrument,
            volume: 0.8,
            pan: 0.0,
            pitch: 0.0,
            muted: false,
            solo: false,
            mixer,
            envelope: Envelope::default(),
            cut_group: 0,
            cut_by: 0,
            cut_self: false,
            group: String::new(),
        });
        id
    }

    pub fn add_pattern(&mut self) -> Id {
        let id = self.next_id();
        let n = self.patterns.len() + 1;
        self.patterns.push(Pattern { id, name: format!("Pattern {n}"), color: PALETTE[(n + 1) % PALETTE.len()], steps: 16, swing: 0.0, channels: BTreeMap::new() });
        id
    }

    /// Base tempo at beat 0.
    pub fn tempo_at(&self, beat: f64) -> f64 {
        let mut bpm = self.bpm;
        for p in &self.tempo_points {
            if p.beat <= beat {
                bpm = p.bpm;
            } else {
                break;
            }
        }
        bpm.clamp(10.0, 999.0)
    }

    /// Time signature in effect at a beat.
    pub fn signature_at(&self, beat: f64) -> (u32, u32) {
        let mut s = (4, 4);
        for t in &self.time_signatures {
            if t.beat <= beat {
                s = (t.numerator.max(1), t.denominator.max(1));
            }
        }
        s
    }

    /// Beats per bar at a beat (quarter-note beats).
    pub fn beats_per_bar(&self, beat: f64) -> f64 {
        let (n, d) = self.signature_at(beat);
        n as f64 * 4.0 / d as f64
    }

    /// Song length in beats: end of the last clip (at least one bar).
    pub fn song_length(&self) -> f64 {
        let clips = self.tracks.iter().flat_map(|t| t.clips.iter()).map(|c| c.end()).fold(0.0, f64::max);
        clips.max(4.0)
    }

    /// Every file the project references.
    pub fn referenced_files(&self) -> Vec<PathBuf> {
        let mut v = Vec::new();
        if let Some(vid) = &self.video {
            v.push(vid.path.clone());
        }
        for t in &self.tracks {
            for c in t.clips.iter().chain(t.takes.iter().flat_map(|k| k.clips.iter())) {
                if let Some(a) = c.audio() {
                    v.push(a.source.clone());
                }
            }
        }
        for c in &self.channels {
            match &c.instrument {
                Instrument::Sampler(s) => v.push(s.path.clone()),
                Instrument::DrumPads(pads) => v.extend(pads.iter().flat_map(|p| p.layers.iter().map(|l| l.path.clone()))),
                _ => {}
            }
        }
        v.retain(|p| !p.as_os_str().is_empty());
        v.sort();
        v.dedup();
        v
    }

    /// Ensures the internal invariants after loading or a structural edit: ids unique and below `next_id`,
    /// references to deleted mixer channels point to the master.
    pub fn repair(&mut self) {
        let mut max = 0;
        for t in &self.tracks {
            max = max.max(t.id);
            for c in &t.clips {
                max = max.max(c.id);
            }
        }
        for c in &self.channels {
            max = max.max(c.id);
        }
        for p in &self.patterns {
            max = max.max(p.id);
        }
        for m in &self.mixer.channels {
            max = max.max(m.id);
            for i in &m.inserts {
                max = max.max(i.id);
            }
        }
        for a in &self.automation {
            max = max.max(a.id);
        }
        self.next_id = self.next_id.max(max + 1);
        let ids: Vec<Id> = self.mixer.all().map(|c| c.id).collect();
        let fix = |id: &mut Id| {
            if !ids.contains(id) {
                *id = MASTER;
            }
        };
        for t in &mut self.tracks {
            fix(&mut t.mixer);
        }
        for c in &mut self.channels {
            fix(&mut c.mixer);
        }
        for m in &mut self.mixer.channels {
            if let Some(o) = m.output {
                if !ids.contains(&o) || o == m.id {
                    m.output = None;
                }
            }
            m.sends.retain(|s| ids.contains(&s.target) && s.target != m.id);
        }
        if self.time_signatures.is_empty() {
            self.time_signatures.push(TimeSignature { beat: 0.0, numerator: 4, denominator: 4 });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automation_interpolates() {
        let lane =
            AutomationLane { id: 1, target: Target::Tempo, points: vec![AutoPoint { beat: 0.0, value: 0.0, tension: 0.0 }, AutoPoint { beat: 4.0, value: 1.0, tension: 0.0 }], mode: AutomationMode::Read, visible: true };
        assert_eq!(lane.value_at(-1.0), Some(0.0));
        assert!((lane.value_at(2.0).unwrap() - 0.5).abs() < 1e-6);
        assert_eq!(lane.value_at(10.0), Some(1.0));
    }

    #[test]
    fn default_project_is_consistent() {
        let mut p = Project::new_default();
        assert_eq!(p.tracks.len(), 4);
        assert_eq!(p.mixer.channels.len(), 4);
        let before = p.clone();
        p.repair();
        assert_eq!(p, before);
        let json = serde_json::to_string(&p).unwrap();
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn pattern_length_grows_with_notes() {
        let mut p = Pattern { id: 1, name: "p".into(), color: Rgb::default(), steps: 16, swing: 0.0, channels: BTreeMap::new() };
        assert_eq!(p.length_beats(), 4.0);
        p.channels.entry(5).or_default().notes.push(Note { start: PPQ * 5, ..Note::default() });
        assert_eq!(p.length_beats(), 8.0);
    }
}
