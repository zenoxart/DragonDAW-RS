//! Minimal CLAP and VST3 plugins, written only to test Dragon DAW's plugin host.
//!
//! * CLAP `dragon.test.gain` – stereo gain effect, one parameter (id 0, 0..2, default 1), state = gain.
//! * CLAP `dragon.test.sine` – sine instrument, plays the last held note.
//! * VST3 "Dragon Test Gain" – single-component gain effect, one parameter (id 0, normalized 0..1 → 0..2).
//!
//! One library exports both `clap_entry` and `GetPluginFactory`, so the same file can be installed as a
//! `.clap` file and inside a `.vst3` bundle.

#![allow(non_snake_case, non_upper_case_globals)]
#![allow(clippy::missing_safety_doc)]

pub mod clap_plugins;
pub mod vst3_plugin;
