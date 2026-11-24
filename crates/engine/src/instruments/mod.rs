//! Instruments of the Channel Rack: sampler (with slices), subtractive synth and drum pads. Plugin
//! instruments implement the same trait in [`crate::plugins`].

#[cfg(test)]
pub(crate) mod diversity;
mod drums;
mod guitar;
pub mod kit;
pub mod native;
mod piano;
mod sampler;
mod synth;
mod wavetable;

pub use drums::{default_pads, DrumPads};
pub use guitar::Guitar;
pub use piano::Piano;
pub use sampler::{detect_slices as sampler_slices, Sampler};
pub use synth::Synth;
pub use wavetable::Wavetable;

use crate::model::Instrument as Model;
use crate::samples::SampleCache;

/// A playing note's parameters.
#[derive(Debug, Clone, Copy)]
pub struct NoteOn {
    pub note: u8,
    pub velocity: f32,
    pub pan: f32,
    /// Extra pitch in semitones (step pitch, channel pitch).
    pub detune: f32,
    /// Glide from the previous note instead of retriggering.
    pub slide: bool,
}

pub trait InstrumentVoice: Send {
    fn note_on(&mut self, n: NoteOn);
    fn note_off(&mut self, note: u8);
    /// Releases every voice quickly (`seconds` fade).
    fn choke(&mut self, seconds: f32);
    /// Adds the next block to `l`/`r`.
    fn process(&mut self, l: &mut [f32], r: &mut [f32]);
    /// Updates parameters from the model without dropping playing voices.
    fn update(&mut self, model: &Model, cache: &SampleCache);
    fn active_voices(&self) -> usize;
    /// Sets a modulated parameter (automation) by name.
    fn set_param(&mut self, _id: &str, _value: f32) {}
    /// What a fretted instrument's strings are playing (fret per string, −1 = silent), for its editor.
    fn display(&self) -> Option<[i8; 8]> {
        None
    }
    /// Parameters of a hosted plugin instrument.
    fn plugin_params(&self) -> Vec<crate::effects::PluginParam> {
        Vec::new()
    }
    fn plugin_param(&self, _id: u32) -> Option<f64> {
        None
    }
    fn save_state(&mut self) -> Option<Vec<u8>> {
        None
    }
    /// Loads a state saved by [`save_state`](Self::save_state) (a preset) into the running plugin;
    /// returns whether it took it.
    fn load_state(&mut self, _data: &[u8]) -> bool {
        false
    }
    fn editor(&mut self, _open: bool) -> bool {
        false
    }
    /// See [`crate::effects::Effect::idle`].
    fn idle(&mut self) -> bool {
        false
    }
    /// Full screen for the plugin's own editor window.
    fn editor_fullscreen(&mut self, _on: bool) -> bool {
        false
    }
    /// Why the last `editor(true)` failed, if it did (shown to the user).
    fn editor_error(&self) -> Option<String> {
        None
    }
    /// Parameter edits made in the plugin's own GUI since the last call: (parameter id, normalised value).
    fn take_param_edits(&mut self) -> Vec<(u32, f64)> {
        Vec::new()
    }
    /// The channel's MIDI effects (only the chain in front of the instrument uses them). Called off the
    /// audio thread when they change.
    fn configure_midi(&mut self, _fx: &[crate::model::MidiFx]) {}
    /// Tempo and song position at the start of the next block, and whether the transport plays.
    fn set_transport(&mut self, _bpm: f64, _beat: f64, _playing: bool) {}
}

/// Creates the engine instrument for a model (plugins are created by the plugin host).
pub fn create(model: &Model, rate: f32, cache: &SampleCache) -> Option<Box<dyn InstrumentVoice>> {
    let mut i: Box<dyn InstrumentVoice> = match model {
        Model::Sampler(_) => Box::new(Sampler::new(rate)),
        Model::Synth(_) => Box::new(Synth::new(rate)),
        Model::DrumPads(_) => Box::new(DrumPads::new(rate)),
        Model::Wavetable(_) => Box::new(Wavetable::new(rate)),
        Model::Guitar(_) => Box::new(Guitar::new(rate)),
        Model::Piano(_) => Box::new(Piano::new(rate)),
        Model::Native(p) => native::create(p, rate)?,
        Model::Plugin(_) => return None,
    };
    i.update(model, cache);
    Some(i)
}

#[inline]
pub fn note_ratio(semitones: f32) -> f64 {
    2f64.powf(semitones as f64 / 12.0)
}

pub fn midi_to_hz(note: f32) -> f32 {
    440.0 * 2f32.powf((note - 69.0) / 12.0)
}

/// Note name (`C4` = 60).
pub fn note_name(n: u8) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    format!("{}{}", NAMES[(n % 12) as usize], n as i32 / 12 - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Instrument as M, SynthParams};

    #[test]
    fn note_names() {
        assert_eq!(note_name(60), "C4");
        assert_eq!(note_name(69), "A4");
        assert!((midi_to_hz(69.0) - 440.0).abs() < 1e-3);
    }

    #[test]
    fn synth_plays_and_releases() {
        let cache = SampleCache::default();
        let mut s = create(&M::Synth(SynthParams::default()), 48000.0, &cache).unwrap();
        s.note_on(NoteOn { note: 57, velocity: 1.0, pan: 0.0, detune: 0.0, slide: false });
        let mut l = vec![0.0; 4800];
        let mut r = vec![0.0; 4800];
        s.process(&mut l, &mut r);
        assert!(l.iter().any(|x| x.abs() > 0.01));
        s.note_off(57);
        let mut l2 = vec![0.0; 48000];
        let mut r2 = vec![0.0; 48000];
        s.process(&mut l2, &mut r2);
        assert_eq!(s.active_voices(), 0);
        assert!(l2[47000..].iter().all(|x| x.abs() < 1e-4));
    }
}
