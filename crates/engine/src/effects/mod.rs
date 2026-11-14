//! Built-in effects. Every effect is described by an [`EffectDef`] (id, name, category, parameters) and
//! created through the registry, so the UI, presets, automation and project files all work generically on
//! `id → value` parameter maps.

mod color;
pub use color::DragonParticle;
mod dynamics;
mod eq;
mod modulation;
mod reverb;

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
    /// Project tempo (for synced effects).
    fn set_tempo(&mut self, _bpm: f64) {}
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
    /// Opens (`true`) or closes the plugin's own editor window; returns whether it is open.
    fn editor(&mut self, _open: bool) -> bool {
        false
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
            color::saturator_def(),
            color::bitcrusher_def(),
            color::filter_def(),
            color::utility_def(),
            color::imager_def(),
            color::dragon_particle_def(),
        ]
    })
}

pub fn find(id: &str) -> Option<&'static EffectDef> {
    registry().iter().find(|d| d.id == id)
}

/// Creates an effect and applies a parameter map (missing values keep their defaults).
pub fn create(id: &str, rate: f32, params: &BTreeMap<String, f32>) -> Option<Box<dyn Effect>> {
    let def = find(id)?;
    let mut e = (def.create)(rate);
    for p in def.params {
        e.set_param(p.id, params.get(p.id).copied().unwrap_or(p.default));
    }
    Some(e)
}

/// Reads a parameter from a map with the definition's default.
pub fn value(def: &EffectDef, params: &BTreeMap<String, f32>, id: &str) -> f32 {
    params.get(id).copied().or_else(|| def.param(id).map(|p| p.default)).unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

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
