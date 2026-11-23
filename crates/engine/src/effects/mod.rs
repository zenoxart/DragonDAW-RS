//! Built-in effects. Every effect is described by an [`EffectDef`] (id, name, category, parameters) and
//! created through the registry, so the UI, presets, automation and project files all work generically on
//! `id → value` parameter maps.

mod color;
pub use color::{shape as saturation_curve, DragonParticle, INGREDIENTS as CRUCIBLE_INGREDIENTS};
pub mod alter;
pub mod clipper;
pub mod dynamics;
pub mod eq;
pub mod jadefang;
mod modulation;
pub use modulation::DELAY_SYNC_BEATS;
pub mod multisat;
pub mod panner;
pub mod pitch;
pub mod reverb;
pub mod strips;
pub mod voice;

pub use eq::{ParametricEq, EQ_BANDS};

use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParamKind {
    Linear,
    /// Drawn and dragged on a logarithmic scale (frequencies, times).
    Log,
    /// Index into a list of names.
    Choice(&'static [&'static str]),
    Toggle,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub unit: &'static str,
    pub kind: ParamKind,
}

impl ParamInfo {
    pub const fn lin(id: &'static str, name: &'static str, min: f32, max: f32, default: f32, unit: &'static str) -> Self {
        ParamInfo { id, name, min, max, default, unit, kind: ParamKind::Linear }
    }
    pub const fn log(id: &'static str, name: &'static str, min: f32, max: f32, default: f32, unit: &'static str) -> Self {
        ParamInfo { id, name, min, max, default, unit, kind: ParamKind::Log }
    }
    pub const fn choice(id: &'static str, name: &'static str, options: &'static [&'static str], default: f32) -> Self {
        ParamInfo { id, name, min: 0.0, max: (options.len() - 1) as f32, default, unit: "", kind: ParamKind::Choice(options) }
    }
    pub const fn toggle(id: &'static str, name: &'static str, default: bool) -> Self {
        ParamInfo { id, name, min: 0.0, max: 1.0, default: if default { 1.0 } else { 0.0 }, unit: "", kind: ParamKind::Toggle }
    }

    /// Maps a plain value to 0 … 1 (for knobs and automation display).
    pub fn normalize(&self, v: f32) -> f32 {
        let v = v.clamp(self.min, self.max);
        match self.kind {
            ParamKind::Log if self.min > 0.0 => (v / self.min).ln() / (self.max / self.min).ln(),
            _ => {
                if self.max > self.min {
                    (v - self.min) / (self.max - self.min)
                } else {
                    0.0
                }
            }
        }
    }

    pub fn denormalize(&self, n: f32) -> f32 {
        let n = n.clamp(0.0, 1.0);
        let v = match self.kind {
            ParamKind::Log if self.min > 0.0 => self.min * (self.max / self.min).powf(n),
            _ => self.min + n * (self.max - self.min),
        };
        match self.kind {
            ParamKind::Choice(_) | ParamKind::Toggle => v.round(),
            _ => v,
        }
    }

    /// Display text of a value.
    pub fn format(&self, v: f32) -> String {
        match self.kind {
            ParamKind::Choice(names) => names.get(v.round().max(0.0) as usize).copied().unwrap_or("?").to_string(),
            ParamKind::Toggle => if v >= 0.5 { "On" } else { "Off" }.to_string(),
            _ => {
                let a = v.abs();
                let num = if self.unit == "Hz" && a >= 1000.0 {
                    return format!("{:.2} kHz", v / 1000.0);
                } else if a >= 100.0 {
                    format!("{v:.0}")
                } else if a >= 10.0 {
                    format!("{v:.1}")
                } else {
                    format!("{v:.2}")
                };
                if self.unit.is_empty() {
                    num
                } else {
                    format!("{num} {}", self.unit)
                }
            }
        }
    }
}

/// A real-time effect processing planar stereo blocks in place.
pub trait Effect: Send {
    fn set_param(&mut self, id: &str, value: f32);
    fn reset(&mut self);
    fn process(&mut self, l: &mut [f32], r: &mut [f32], sidechain: Option<(&[f32], &[f32])>);
    /// Algorithmic latency in samples (compensated by the mixer).
    fn latency(&self) -> usize {
        0
    }
    /// Current gain reduction for meters (dB, ≥ 0).
    fn gain_reduction(&self) -> f32 {
        0.0
    }
    /// Live values for the effect's display (pitch, correlation, envelopes …), read by the UI after each
    /// block; the meaning is effect-specific. Empty for most effects.
    fn telemetry(&self) -> &[f32] {
        &[]
    }
    /// Project tempo (for synced effects).
    fn set_tempo(&mut self, _bpm: f64) {}
    /// The song position (beats) at the start of the next block and whether the transport plays, called
    /// before every `process` (for effects that lock their motion to the bar).
    fn set_position(&mut self, _beat: f64, _playing: bool) {}
    /// Parameters of a hosted third-party plugin (empty for built-ins, which use [`EffectDef::params`]).
    fn plugin_params(&self) -> Vec<PluginParam> {
        Vec::new()
    }
    /// Current normalized value of a plugin parameter.
    fn plugin_param(&self, _id: u32) -> Option<f64> {
        None
    }
    /// Opaque plugin state for saving in the project.
    fn save_state(&mut self) -> Option<Vec<u8>> {
        None
    }
    /// Loads a state saved by [`save_state`](Self::save_state) (a preset) into the running plugin;
    /// returns whether it took it.
    fn load_state(&mut self, _data: &[u8]) -> bool {
        false
    }
    /// Opens (`true`) or closes the plugin's own editor window; returns whether it is open.
    fn editor(&mut self, _open: bool) -> bool {
        false
    }
    /// Services an open plugin editor (window events, plugin timers). Called from the UI thread every frame;
    /// returns `false` when no editor is open (any more).
    fn idle(&mut self) -> bool {
        false
    }
    /// Puts the plugin's own editor window in full screen (or back). Returns whether it is now full screen.
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
}

/// A parameter of a hosted plugin (values are normalized 0 … 1 on the host side).
#[derive(Debug, Clone, PartialEq)]
pub struct PluginParam {
    pub id: u32,
    pub name: String,
    pub default: f64,
    /// Discrete steps (0 = continuous).
    pub steps: u32,
    pub automatable: bool,
}

pub struct EffectDef {
    pub id: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    pub params: &'static [ParamInfo],
    pub uses_sidechain: bool,
    pub create: fn(f32) -> Box<dyn Effect>,
}

impl EffectDef {
    pub fn param(&self, id: &str) -> Option<&ParamInfo> {
        self.params.iter().find(|p| p.id == id)
    }
    pub fn defaults(&self) -> BTreeMap<String, f32> {
        self.params.iter().map(|p| (p.id.to_string(), p.default)).collect()
    }
}

/// Every built-in effect.
pub fn registry() -> &'static [EffectDef] {
    static R: std::sync::OnceLock<Vec<EffectDef>> = std::sync::OnceLock::new();
    R.get_or_init(|| {
        vec![
            eq::def(),
            jadefang::def(),
            dynamics::compressor_def(),
            dynamics::multiband_def(),
            dynamics::gate_def(),
            dynamics::limiter_def(),
            dynamics::deesser_def(),
            dynamics::transient_def(),
            reverb::reverb_def(),
            modulation::delay_def(),
            modulation::chorus_def(),
            modulation::flanger_def(),
            modulation::phaser_def(),
            modulation::tremolo_def(),
            panner::orbit_def(),
            color::saturator_def(),
            multisat::multiband_saturator_def(),
            color::bitcrusher_def(),
            clipper::soft_clipper_def(),
            color::filter_def(),
            color::utility_def(),
            color::imager_def(),
            color::dragon_particle_def(),
            pitch::pitch_correct_def(),
            voice::clean_voice_def(),
            strips::class_a_strip_def(),
            strips::drum_bus_def(),
            alter::alter_def(),
        ]
    })
}

pub fn find(id: &str) -> Option<&'static EffectDef> {
    registry().iter().find(|d| d.id == id)
}

/// Creates an effect and applies a parameter map (missing values keep their defaults). The effect is wrapped
/// in [`Smoothed`], so later parameter changes glide instead of stepping.
pub fn create(id: &str, rate: f32, params: &BTreeMap<String, f32>) -> Option<Box<dyn Effect>> {
    let def = find(id)?;
    let mut e = Smoothed::new(def, (def.create)(rate), rate);
    for p in def.params {
        e.set_param(p.id, params.get(p.id).copied().unwrap_or(p.default));
    }
    Some(Box::new(e))
}

/// Parameter smoothing for built-in effects. Turning a knob changes a parameter in steps (one per UI frame);
/// applied directly, a jump in a filter frequency, gain or delay time is an audible click. The wrapper keeps
/// the requested value as a target and moves the effect's continuous parameters towards it in short slices
/// (32 samples, ~25 ms time constant; logarithmic parameters move on the log scale). Choices and toggles switch
/// at once. Before the first processed block every value is applied directly.
pub struct Smoothed {
    def: &'static EffectDef,
    inner: Box<dyn Effect>,
    current: Vec<f32>,
    target: Vec<f32>,
    coef: f32,
    primed: bool,
}

const SMOOTH_SLICE: usize = 32;

impl Smoothed {
    pub fn new(def: &'static EffectDef, mut inner: Box<dyn Effect>, rate: f32) -> Self {
        let d: Vec<f32> = def.params.iter().map(|p| p.default).collect();
        // The wrapper skips values equal to what it believes is current, so the effect must really be at
        // the declared defaults first (an effect's constructor may start elsewhere – the EQ's bands start
        // switched off while bands 1–7 default to on).
        for p in def.params {
            inner.set_param(p.id, p.default);
        }
        let coef = 1.0 - (-(SMOOTH_SLICE as f32) / (0.025 * rate.max(1.0))).exp();
        Smoothed { def, inner, current: d.clone(), target: d, coef, primed: false }
    }

    fn continuous(p: &ParamInfo) -> bool {
        matches!(p.kind, ParamKind::Linear | ParamKind::Log)
    }

    /// Moves every continuous parameter one slice towards its target; returns whether any is still moving.
    fn step(&mut self) -> bool {
        let mut moving = false;
        for (i, p) in self.def.params.iter().enumerate() {
            let (c, t) = (self.current[i], self.target[i]);
            if c == t {
                continue;
            }
            let span = (p.max - p.min).abs().max(1e-9);
            let next = if matches!(p.kind, ParamKind::Log) && c > 0.0 && t > 0.0 {
                let (lc, lt) = (c.ln(), t.ln());
                let n = lc + (lt - lc) * self.coef;
                if (lt - n).abs() < 1e-4 {
                    t
                } else {
                    n.exp()
                }
            } else {
                let n = c + (t - c) * self.coef;
                if (t - n).abs() < span * 1e-4 {
                    t
                } else {
                    n
                }
            };
            self.current[i] = next;
            self.inner.set_param(p.id, next);
            moving |= next != t;
        }
        moving
    }

    /// `true` while a parameter is still gliding (for tests).
    pub fn is_smoothing(&self) -> bool {
        self.current != self.target
    }
}

impl Effect for Smoothed {
    fn set_param(&mut self, id: &str, value: f32) {
        let Some(i) = self.def.params.iter().position(|p| p.id == id) else {
            // Hidden / internal parameters pass straight through.
            self.inner.set_param(id, value);
            return;
        };
        let p = &self.def.params[i];
        if self.target[i] == value && (self.primed || self.current[i] == value) {
            return;
        }
        self.target[i] = value;
        if !self.primed || !Self::continuous(p) {
            self.current[i] = value;
            self.inner.set_param(id, value);
        }
    }

    fn reset(&mut self) {
        for (i, p) in self.def.params.iter().enumerate() {
            if self.current[i] != self.target[i] {
                self.current[i] = self.target[i];
                self.inner.set_param(p.id, self.target[i]);
            }
        }
        self.inner.reset();
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], sidechain: Option<(&[f32], &[f32])>) {
        self.primed = true;
        if self.current == self.target {
            self.inner.process(l, r, sidechain);
            return;
        }
        let n = l.len();
        let mut o = 0;
        while o < n {
            let e = (o + SMOOTH_SLICE).min(n);
            let moving = self.step();
            let sc = sidechain.map(|(a, b)| (&a[o.min(a.len())..e.min(a.len())], &b[o.min(b.len())..e.min(b.len())]));
            self.inner.process(&mut l[o..e], &mut r[o..e], sc);
            o = e;
            if !moving {
                break;
            }
        }
        if o < n {
            let sc = sidechain.map(|(a, b)| (&a[o.min(a.len())..], &b[o.min(b.len())..]));
            self.inner.process(&mut l[o..], &mut r[o..], sc);
        }
    }

    fn latency(&self) -> usize {
        self.inner.latency()
    }
    fn gain_reduction(&self) -> f32 {
        self.inner.gain_reduction()
    }
    // The wrapped effect's live values – without this every stock display (Dragonforge's input history and
    // target meter, the EQ curves, the tuner …) would read nothing.
    fn telemetry(&self) -> &[f32] {
        self.inner.telemetry()
    }
    fn editor_fullscreen(&mut self, on: bool) -> bool {
        self.inner.editor_fullscreen(on)
    }
    fn set_position(&mut self, beat: f64, playing: bool) {
        self.inner.set_position(beat, playing)
    }
    fn set_tempo(&mut self, bpm: f64) {
        self.inner.set_tempo(bpm)
    }
    fn plugin_params(&self) -> Vec<PluginParam> {
        self.inner.plugin_params()
    }
    fn plugin_param(&self, id: u32) -> Option<f64> {
        self.inner.plugin_param(id)
    }
    fn save_state(&mut self) -> Option<Vec<u8>> {
        self.inner.save_state()
    }
    fn load_state(&mut self, data: &[u8]) -> bool {
        self.inner.load_state(data)
    }
    fn editor(&mut self, open: bool) -> bool {
        self.inner.editor(open)
    }
    fn idle(&mut self) -> bool {
        self.inner.idle()
    }
    fn editor_error(&self) -> Option<String> {
        self.inner.editor_error()
    }
    fn take_param_edits(&mut self) -> Vec<(u32, f64)> {
        self.inner.take_param_edits()
    }
}

/// Reads a parameter from a map with the definition's default.
pub fn value(def: &EffectDef, params: &BTreeMap<String, f32>, id: &str) -> f32 {
    params.get(id).copied().or_else(|| def.param(id).map(|p| p.default)).unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The smoothing wrapper hands the effect's live values through (Dragonforge's input history and
    /// target meter read them; they stayed flat while it swallowed them).
    #[test]
    fn wrapper_passes_telemetry() {
        let mut e = create("drum_bus", 48_000.0, &BTreeMap::new()).unwrap();
        let mut l: Vec<f32> = (0..512).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        let mut r = l.clone();
        e.process(&mut l, &mut r, None);
        let t = e.telemetry();
        assert!(t.len() >= 6, "telemetry lost: {t:?}");
        assert!(t[0] > 0.3 && t[3] > 0.05, "input peak / RMS missing: {t:?}");
    }

    /// Zipper / click hunt: sweeping any continuous knob of any stock effect over a smooth low tone, the
    /// way the UI does (a new value every ~16 ms, smoothed by the wrapper), must not put sharp
    /// discontinuities into the output (heard as a gritty, "downsampled" sound while turning a knob).
    /// Measured as the largest second difference relative to the same effect at a fixed setting.
    #[test]
    fn knob_sweeps_are_click_free() {
        let rate = 48000.0;
        let n = 48000;
        // Stereo: the two sides differ (side-only processing must be exercised too).
        let tone: Vec<f32> = (0..n).map(|i| (i as f32 * std::f32::consts::TAU * 110.0 / rate).sin() * 0.25).collect();
        let tone_r: Vec<f32> = (0..n).map(|i| (i as f32 * std::f32::consts::TAU * 110.0 / rate + 1.3).sin() * 0.2).collect();
        // Largest second difference relative to the local level (a gain change is not a click).
        let d2 = |v: &[f32]| {
            let peak = v.iter().fold(0.0f32, |m, x| m.max(x.abs())).max(1e-3);
            v.windows(3).map(|w| (w[0] - 2.0 * w[1] + w[2]).abs()).fold(0.0f32, f32::max) / peak
        };
        // Effects whose knobs are meant to step or grind (bitcrusher's sample-rate reduction, …).
        let skip = ["bitcrusher", "tape_stop", "glitch", "scope", "phase_scope", "tuner", "spectrum", "rhythm_gate", "stutter", "gate"];
        let mut bad = Vec::new();
        for def in registry() {
            if skip.contains(&def.id) {
                continue;
            }
            for p in def.params.iter().filter(|p| matches!(p.kind, ParamKind::Linear | ParamKind::Log)) {
                // mode: 0 = held at the start value, 1 = held at the end value, 2 = swept between them.
                let run = |mode: u8| {
                    let mut e = create(def.id, rate, &def.defaults()).unwrap();
                    let (mut l, mut r) = (tone.clone(), tone_r.clone());
                    let mid = (p.min + p.max) * 0.5;
                    let (a, b) = (p.default.clamp(p.min, p.max), if p.default > mid { p.min } else { p.max });
                    let a = if matches!(p.kind, ParamKind::Log) { a.max(p.min.max(1e-3)) } else { a };
                    let b = if matches!(p.kind, ParamKind::Log) { b.max(p.min.max(1e-3)) } else { b };
                    for (k, (x, y)) in l.chunks_mut(768).zip(r.chunks_mut(768)).enumerate() {
                        let t = (k as f32 / 40.0).min(1.0);
                        let v = if mode == 0 {
                            a
                        } else if mode == 1 {
                            b
                        } else if matches!(p.kind, ParamKind::Log) {
                            (a.ln() + (b.ln() - a.ln()) * t).exp()
                        } else {
                            a + (b - a) * t
                        };
                        e.set_param(p.id, v);
                        e.process(x, y, None);
                    }
                    let lat = e.latency();
                    let from = 4800 + lat;
                    let end = (from + 24000).min(n);
                    if from >= end {
                        return 0.0;
                    }
                    d2(&l[from..end]).max(d2(&r[from..end]))
                };
                // The sharpest the effect is when held anywhere on the way is its own character, not a click.
                let fixed = run(0).max(run(1));
                let swept = run(2);
                // A smooth 110 Hz tone has a relative second difference of ~2e-4; a click is a jump of
                // several percent of the level within one sample.
                let limit = (fixed * 6.0).max(0.03);
                if swept > limit {
                    bad.push(format!("{}.{}: sweep {swept:.4} vs fixed {fixed:.4}", def.id, p.id));
                }
            }
        }
        assert!(bad.is_empty(), "knob sweeps that click:\n{}", bad.join("\n"));
    }

    /// Crash hunt: every stock effect at every common sample rate, with each parameter at its minimum,
    /// maximum and middle (one at a time and all together), with odd block sizes, silence and loud
    /// noise, sidechain on and off – it must never panic or put out NaN / infinity.
    #[test]
    fn no_effect_panics_or_blows_up() {
        let mut rng = crate::dsp::Rng::new(11);
        let loud: Vec<f32> = (0..6000).map(|_| rng.bipolar() * 4.0).collect();
        for rate in [22050.0f32, 44100.0, 48000.0, 88200.0, 96000.0, 192000.0] {
            for def in registry() {
                let mut sets: Vec<Vec<(&str, f32)>> = vec![vec![]];
                for p in def.params {
                    sets.push(vec![(p.id, p.min)]);
                    sets.push(vec![(p.id, p.max)]);
                }
                sets.push(def.params.iter().map(|p| (p.id, p.min)).collect());
                sets.push(def.params.iter().map(|p| (p.id, p.max)).collect());
                sets.push(def.params.iter().map(|p| (p.id, (p.min + p.max) * 0.5)).collect());
                for set in &sets {
                    let mut e = create(def.id, rate, &def.defaults()).unwrap();
                    for (k, v) in set {
                        e.set_param(k, *v);
                    }
                    let _ = e.latency();
                    let (mut l, mut r) = (loud.clone(), loud.clone());
                    let sc = (loud.clone(), loud.clone());
                    let mut pos = 0;
                    for (k, n) in [1usize, 7, 64, 2048, 3000].iter().cycle().enumerate() {
                        if pos >= l.len() {
                            break;
                        }
                        let end = (pos + n).min(l.len());
                        let s = if k % 2 == 0 { Some((&sc.0[pos..end], &sc.1[pos..end])) } else { None };
                        e.process(&mut l[pos..end], &mut r[pos..end], s);
                        pos = end;
                    }
                    let bad = l.iter().chain(r.iter()).any(|x| !x.is_finite());
                    assert!(!bad, "{} at {rate} Hz with {set:?}: NaN / inf", def.id);
                    let _ = e.telemetry();
                }
            }
        }
    }

    /// Every stock effect created with its default parameters behaves exactly like the bare effect with
    /// every default set explicitly (the smoothing wrapper must not skip defaults the effect's constructor
    /// does not start at – that left the EQ's default bands switched off).
    #[test]
    fn defaults_reach_every_effect() {
        let mut rng = crate::dsp::Rng::new(7);
        let x: Vec<f32> = (0..8192).map(|_| rng.bipolar() * 0.3).collect();
        for def in registry() {
            let mut a = create(def.id, 48000.0, &def.defaults()).unwrap();
            let mut b = (def.create)(48000.0);
            for p in def.params {
                b.set_param(p.id, p.default);
            }
            let (mut al, mut ar, mut bl, mut br) = (x.clone(), x.clone(), x.clone(), x.clone());
            for k in (0..x.len()).step_by(256) {
                a.process(&mut al[k..k + 256], &mut ar[k..k + 256], None);
                b.process(&mut bl[k..k + 256], &mut br[k..k + 256], None);
            }
            let diff = al.iter().zip(&bl).map(|(p, q)| (p - q).abs()).fold(0.0f32, f32::max);
            assert!(diff < 1e-4, "{}: wrapped and explicit defaults differ by {diff}", def.id);
        }
    }

    fn noise(n: usize) -> (Vec<f32>, Vec<f32>) {
        let mut rng = crate::dsp::Rng::new(7);
        let l: Vec<f32> = (0..n).map(|_| rng.bipolar() * 0.5).collect();
        let r: Vec<f32> = (0..n).map(|_| rng.bipolar() * 0.5).collect();
        (l, r)
    }

    /// Every effect must stay finite and bounded on noise, with defaults and with every parameter at its
    /// extremes, and block sizes must not matter.
    #[test]
    fn all_effects_are_stable() {
        for def in registry() {
            for extreme in [None, Some(0.0f32), Some(1.0)] {
                let mut params = def.defaults();
                if let Some(n) = extreme {
                    for p in def.params {
                        params.insert(p.id.to_string(), p.denormalize(n));
                    }
                }
                let mut e = create(def.id, 48000.0, &params).unwrap();
                e.set_tempo(128.0);
                let (mut l, mut r) = noise(48000);
                let (sl, sr) = noise(48000);
                for (cl, cr) in l.chunks_mut(333).zip(r.chunks_mut(333)) {
                    let n = cl.len();
                    e.process(cl, cr, Some((&sl[..n], &sr[..n])));
                }
                let peak = l.iter().chain(r.iter()).fold(0.0f32, |m, s| m.max(s.abs()));
                assert!(peak.is_finite() && peak < 64.0, "{} {:?} peak {}", def.id, extreme, peak);
            }
        }
    }

    /// A big parameter jump must not step the output: +24 dB on the utility gain glides in.
    #[test]
    fn parameter_jumps_are_smoothed() {
        let def = find("utility").unwrap();
        let mut e = create("utility", 48000.0, &def.defaults()).unwrap();
        let sine = |k: usize| (k as f32 * 2.0 * std::f32::consts::PI * 220.0 / 48000.0).sin() * 0.5;
        let mut l: Vec<f32> = (0..4800).map(sine).collect();
        let mut r = l.clone();
        e.process(&mut l, &mut r, None);
        e.set_param("gain", 24.0);
        let mut l: Vec<f32> = (4800..4800 + 48000).map(sine).collect();
        let mut r = l.clone();
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            e.process(a, b, None);
        }
        let peak = |x: &[f32]| x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak(&l[..64]) < 0.7, "jumped: {}", peak(&l[..64]));
        assert!(peak(&l[40000..]) > 7.0, "target not reached: {}", peak(&l[40000..]));
        let steps = l.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        assert!(steps < 0.3, "discontinuity {steps}");
    }

    #[test]
    fn param_mapping_roundtrips() {
        let p = ParamInfo::log("f", "Freq", 20.0, 20000.0, 1000.0, "Hz");
        let n = p.normalize(1000.0);
        assert!((p.denormalize(n) - 1000.0).abs() < 0.5);
        assert_eq!(p.format(2500.0), "2.50 kHz");
        let c = ParamInfo::choice("m", "Mode", &["A", "B", "C"], 1.0);
        assert_eq!(c.format(2.0), "C");
    }
}

#[cfg(test)]
mod fuzz {
    use super::*;

    /// Every effect with random settings (all parameters at once), random block sizes and random input
    /// levels – silence, quiet, loud and clipping – never panics and never outputs NaN or infinity.
    #[test]
    fn random_settings_stay_finite() {
        let rounds: usize = std::env::var("DRAGON_FUZZ").ok().and_then(|v| v.parse().ok()).unwrap_or(if cfg!(debug_assertions) { 6 } else { 60 });
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut bad = Vec::new();
        for def in registry() {
            'rounds: for round in 0..rounds {
                let rate = [44100.0, 48000.0, 96000.0][(rnd() % 3) as usize];
                let params: BTreeMap<String, f32> = def.params.iter().map(|p| (p.id.to_string(), p.denormalize((rnd() % 1001) as f32 / 1000.0))).collect();
                let mut e = create(def.id, rate, &params).unwrap();
                e.set_tempo(60.0 + (rnd() % 140) as f64);
                let (mut l, mut r) = (vec![0.0f32; 2048], vec![0.0f32; 2048]);
                for b in 0..30 {
                    let n = 1 + (rnd() % 2048) as usize;
                    let level = [0.0, 1e-4, 0.3, 1.0, 4.0][(rnd() % 5) as usize];
                    for k in 0..n {
                        l[k] = ((rnd() % 2001) as f32 / 1000.0 - 1.0) * level;
                        r[k] = ((rnd() % 2001) as f32 / 1000.0 - 1.0) * level;
                    }
                    // Knob moves while it runs.
                    if b % 7 == 3 {
                        let p = &def.params[(rnd() % def.params.len() as u64) as usize];
                        e.set_param(p.id, p.denormalize((rnd() % 1001) as f32 / 1000.0));
                    }
                    e.set_position((b * 2) as f64 * 0.37, true);
                    let (scl, scr) = (l[..n].to_vec(), r[..n].to_vec());
                    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| e.process(&mut l[..n], &mut r[..n], Some((&scl, &scr)))));
                    let why = match res {
                        Err(p) => Some(format!("panic {}", crate::fault::message(&*p))),
                        Ok(()) => l[..n].iter().chain(r[..n].iter()).find(|x| !x.is_finite()).map(|x| format!("output {x}")),
                    };
                    if let Some(w) = why {
                        let set: Vec<String> = def.params.iter().map(|p| format!("{}={}", p.id, params[p.id])).collect();
                        bad.push(format!("{} round {round} block {b} ({n} @ {rate}, level {level}): {w}\n   {}", def.id, set.join(" ")));
                        break 'rounds;
                    }
                }
            }
        }
        assert!(bad.is_empty(), "{}", bad.join("\n"));
    }
}

#[cfg(test)]
mod runaway {
    use super::*;

    /// No setting makes an effect's feedback run away: after loud input then a long silence, the output
    /// stays bounded (well under +60 dB over the input) – a slow runaway the short fuzz cannot see.
    #[test]
    fn no_feedback_runaway() {
        let rounds: usize = std::env::var("DRAGON_FUZZ").ok().and_then(|v| v.parse().ok()).unwrap_or(if cfg!(debug_assertions) { 1 } else { 4 });
        let mut seed = 0x1234_5678_9abc_def1u64;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut bad = Vec::new();
        for def in registry() {
            for round in 0..rounds {
                let rate = 48000.0;
                let mut params: BTreeMap<String, f32> = def.params.iter().map(|p| (p.id.to_string(), p.denormalize((rnd() % 1001) as f32 / 1000.0))).collect();
                // Push every feedback-like control to its maximum half of the time.
                for p in def.params {
                    if (p.id.contains("feedback") || p.id.contains("reso") || p.id == "fb" || p.id.ends_with("_fb")) && rnd() % 2 == 0 {
                        params.insert(p.id.to_string(), p.max);
                    }
                }
                let mut e = create(def.id, rate, &params).unwrap();
                let (mut l, mut r) = (vec![0.0f32; 1024], vec![0.0f32; 1024]);
                let mut peak = 0.0f32;
                // 1 s of loud noise, then 8 s of silence.
                for b in 0..((9.0 * rate) as usize / 1024) {
                    let loud = b < (rate as usize / 1024);
                    for k in 0..1024 {
                        l[k] = if loud { ((rnd() % 2001) as f32 / 1000.0 - 1.0) * 0.9 } else { 0.0 };
                        r[k] = if loud { ((rnd() % 2001) as f32 / 1000.0 - 1.0) * 0.9 } else { 0.0 };
                    }
                    e.process(&mut l, &mut r, None);
                    peak = l.iter().chain(r.iter()).fold(peak, |m, x| if x.is_finite() { m.max(x.abs()) } else { f32::INFINITY });
                }
                if peak > 1000.0 {
                    let set: Vec<String> = def.params.iter().map(|p| format!("{}={}", p.id, params[p.id])).collect();
                    bad.push(format!("{} round {round}: peak {peak}\n   {}", def.id, set.join(" ")));
                    break;
                }
            }
        }
        assert!(bad.is_empty(), "{}", bad.join("\n"));
    }
}
