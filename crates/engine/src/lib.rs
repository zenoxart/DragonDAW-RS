//! # Dragon engine
//!
//! The audio engine of Dragon DAW: a plain-data [`model`] of a song, a sample-accurate [`render`]er that
//! drives both real-time playback and export, DSP [`effects`] and [`instruments`], and the I/O around them.
//!
//! | Module | Content |
//! |---|---|
//! | [`model`] | Project, tracks, clips, takes, Channel Rack, patterns, notes, mixer, automation |
//! | [`time`] | Tempo map (beats ↔ seconds ↔ frames), bar/beat and timecode formatting |
//! | [`samples`] | Decoding (WAV/FLAC/MP3/OGG/AIFF/M4A), resampling, cache, waveform peaks, WAV writing |
//! | [`dsp`] | Biquads, SVF, envelopes, LFOs, delay lines, loudness (LUFS / true peak), time-stretch |
//! | [`effects`] | 19 built-in effects behind one parameter registry |
//! | [`instruments`] | Sampler (+ slicer), subtractive synth, drum pads |
//! | [`render`] | Song compilation, mixer graph with sends/sidechain/VCA/PDC, automation, metronome |
//! | [`edit`] | Editing operations: split, duplicate, quantize, chords, arpeggios, comping, … |
//! | [`project_io`] | Save/load (`.dragon`), collect-and-save, autosave and crash recovery |
//! | [`history`] | Undo/redo |
//! | [`export`] | Offline bounce (WAV/FLAC), stems, normalisation, dither |
//! | [`midi`] | MIDI file import/export, MIDI messages, MIDI learn mapping |
//! | [`analysis`] | Tempo and key detection |
//! | [`device`] | Audio/MIDI devices, real-time engine, recording (feature `device`) |
//! | [`plugins`] | CLAP and VST3 hosting (feature `plugins`) |

pub mod analysis;
pub mod dsp;
pub mod edit;
pub mod effects;
pub mod export;
pub mod history;
pub mod instruments;
pub mod midi;
pub mod model;
pub mod project_io;
pub mod render;
pub mod samples;
pub mod time;

#[cfg(feature = "device")]
pub mod device;
#[cfg(feature = "plugins")]
pub mod plugins;

#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum EngineError {
    #[error("I/O: {0}")]
    Io(String),
    #[error("cannot decode audio: {0}")]
    Decode(String),
    #[error("invalid project: {0}")]
    Project(String),
    #[error("audio device: {0}")]
    Device(String),
    #[error("plugin: {0}")]
    Plugin(String),
    #[error("MIDI: {0}")]
    Midi(String),
}

pub type Result<T> = std::result::Result<T, EngineError>;
