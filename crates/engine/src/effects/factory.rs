//! Factory presets for every stock effect: 8 characters × 25 variations = 200 per effect.
//!
//! They are made from the effect's own parameter descriptions, deterministically (the same names and
//! values on every machine and in every version that keeps the parameter list), and always safe: level,
//! routing, monitoring and analysis switches stay at their defaults, feedback stays well below run-away,
//! and gains move only within a quarter of their range. Each character has a direction of its own (Warm and
//! Dark lower the frequencies, Bright raises them, Wide opens width, Heavy pushes harder and wetter) and
//! every variation moves from it by a little more or less, so neighbouring presets differ audibly.

use super::{EffectDef, ParamInfo, ParamKind};
use std::collections::BTreeMap;

/// One factory preset.
#[derive(Debug, Clone, PartialEq)]
pub struct Factory {
    /// The character group ("Warm", "Bright" …), shown as a submenu.
    pub group: &'static str,
    pub name: String,
    pub params: BTreeMap<String, f32>,
}

/// Presets per effect.
pub const PER_EFFECT: usize = GROUPS.len() * NOUNS.len();

struct Group {
    name: &'static str,
    /// Intensity range across the 25 variations.
    t: (f32, f32),
    /// Wet amount range for mix parameters.
    mix: (f32, f32),
    /// Pull of frequencies: −1 down … +1 up (0 = each preset its own way).
    freq: f32,
    /// Pull of width parameters.
    width: f32,
    /// Chance that a choice switches away from its default.
    choice: f32,
}

const GROUPS: [Group; 8] = [
    Group { name: "Subtle", t: (0.08, 0.32), mix: (0.2, 0.5), freq: 0.0, width: 0.0, choice: 0.1 },
    Group { name: "Warm", t: (0.25, 0.6), mix: (0.4, 0.8), freq: -0.6, width: 0.0, choice: 0.25 },
    Group { name: "Bright", t: (0.25, 0.6), mix: (0.4, 0.8), freq: 0.6, width: 0.2, choice: 0.25 },
    Group { name: "Punchy", t: (0.35, 0.75), mix: (0.6, 1.0), freq: 0.15, width: 0.0, choice: 0.3 },
    Group { name: "Wide", t: (0.3, 0.7), mix: (0.5, 0.9), freq: 0.0, width: 0.8, choice: 0.3 },
    Group { name: "Dark", t: (0.3, 0.7), mix: (0.5, 0.9), freq: -0.85, width: 0.0, choice: 0.3 },
    Group { name: "Heavy", t: (0.6, 1.0), mix: (0.75, 1.0), freq: 0.0, width: 0.1, choice: 0.4 },
    Group { name: "Creative", t: (0.5, 1.0), mix: (0.5, 1.0), freq: 0.0, width: 0.3, choice: 0.7 },
];

const NOUNS: [&str; 25] = [
    "Ember", "Scale", "Wing", "Claw", "Hoard", "Lair", "Cinder", "Fang", "Ash", "Spire", "Rune", "Talon", "Smoke", "Crown", "Obsidian", "Comet", "Tide", "Forge", "Glyph", "Storm", "Dusk", "Venom", "Halo", "Shard",
    "Wyrm",
];

/// Choices a preset never picks on its own: the gate's upward expander can lift quiet input by its whole
/// range (tens of dB) and is only safe set by hand.
const AVOID: [(&str, &str, f32); 1] = [("gate", "style", 4.0)];

/// Parameters (by suffix) a preset of that effect leaves alone: the equaliser keeps each band's filter
/// type, slope, stereo placement and dynamics, so its presets are tone curves, not filter experiments.
const KEEP_FOR: [(&str, &str); 5] = [("eq", "_type"), ("eq", "_stereo"), ("eq", "_slope"), ("eq", "_dyn"), ("eq", "_thr")];

#[derive(Clone, Copy, PartialEq)]
enum Role {
    /// Kept at the default (levels, routing, monitoring, solo/bypass, analysis switches).
    Keep,
    Mix,
    Feedback,
    Width,
    Freq,
    /// A gain or threshold in dB (moves within a third of its range).
    Db,
    Shape,
}

fn role(p: &ParamInfo) -> Role {
    let id = p.id;
    let last = id.rsplit('_').next().unwrap_or(id);
    const KEEP: [&str; 31] = [
        "output",
        "trim",
        "post",
        "ceiling",
        "gain",
        "input",
        "level",
        "makeup",
        "comp_gain",
        "auto_makeup",
        "unity",
        "solo",
        "bypass",
        "delta",
        "listen",
        "sidechain",
        "sc",
        "freeze",
        "invert_l",
        "invert_r",
        "swap",
        "mono",
        "dc",
        "oversampling",
        "true_peak",
        "lookahead",
        "phase",
        "resolution",
        "link",
        "auto_gain",
        "bands",
    ];
    // Band gains of an equaliser are tone, not level.
    let band_gain = id.contains('_') && last == "gain" && id != "comp_gain";
    if !band_gain && (KEEP.contains(&id) || KEEP.contains(&last) && matches!(p.kind, ParamKind::Toggle)) {
        return Role::Keep;
    }
    if matches!(last, "solo" | "bypass" | "sc" | "listen") {
        return Role::Keep;
    }
    if id == "mix" || last == "mix" {
        return Role::Mix;
    }
    if matches!(last, "feedback" | "cross" | "feed" | "fb" | "reso" | "resonance") {
        return Role::Feedback;
    }
    if last == "width" {
        return Role::Width;
    }
    if p.unit == "Hz" {
        return Role::Freq;
    }
    if p.unit.starts_with("dB") {
        return Role::Db;
    }
    Role::Shape
}

/// A repeatable hash → uniform in −1 … 1.
fn noise(parts: &[u64]) -> f32 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        h ^= *p;
        h = h.wrapping_mul(0x0100_0000_01b3);
        h ^= h >> 29;
    }
    h = h.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    ((h >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
}

fn hash_str(s: &str) -> u64 {
    s.bytes().fold(1469598103934665603u64, |h, b| (h ^ b as u64).wrapping_mul(1099511628211))
}

/// Moves `p` from its default by `amount` (−1 … 1) of its usable span.
fn moved(p: &ParamInfo, amount: f32) -> f32 {
    let (lo, hi) = (p.min.min(p.max), p.max.max(p.min));
    let amount = amount.clamp(-1.0, 1.0);
    let v = match p.kind {
        ParamKind::Log if p.min > 0.0 => {
            // Half the logarithmic range either way.
            let span = (hi / lo).ln() * 0.5;
            p.default.max(lo) * (span * amount).exp()
        }
        _ => {
            let up = hi - p.default;
            let down = p.default - lo;
            p.default + if amount >= 0.0 { up } else { down } * amount
        }
    };
    v.clamp(lo, hi)
}

/// The factory presets of one effect.
pub fn presets(def: &EffectDef) -> Vec<Factory> {
    let eh = hash_str(def.id);
    let mut out = Vec::with_capacity(PER_EFFECT);
    for (gi, g) in GROUPS.iter().enumerate() {
        for (k, noun) in NOUNS.iter().enumerate() {
            let f = k as f32 / (NOUNS.len() - 1) as f32;
            let t = g.t.0 + (g.t.1 - g.t.0) * f;
            let mut params = def.defaults();
            for (pi, p) in def.params.iter().enumerate() {
                let r = if KEEP_FOR.iter().any(|(e, suf)| *e == def.id && p.id.ends_with(suf)) { Role::Keep } else { role(p) };
                // The group's direction for this parameter, varied per preset.
                let base = noise(&[eh, gi as u64, pi as u64, 1]);
                let var = noise(&[eh, gi as u64, k as u64, pi as u64, 2]);
                let dir = (base * 0.6 + var * 0.4).clamp(-1.0, 1.0);
                let v = match (r, p.kind) {
                    (Role::Keep, _) => continue,
                    (_, ParamKind::Toggle) => {
                        if noise(&[eh, gi as u64, k as u64, pi as u64, 3]).abs() < g.choice * 0.5 {
                            if p.default > 0.5 {
                                0.0
                            } else {
                                1.0
                            }
                        } else {
                            continue;
                        }
                    }
                    (_, ParamKind::Choice(opts)) => {
                        if (noise(&[eh, gi as u64, k as u64, pi as u64, 4]) * 0.5 + 0.5) < g.choice {
                            let n = opts.len().max(1);
                            let c = ((noise(&[eh, gi as u64, k as u64, pi as u64, 5]) * 0.5 + 0.5) * n as f32).floor().min((n - 1) as f32);
                            if AVOID.iter().any(|(e, id, x)| *e == def.id && *id == p.id && *x == c) {
                                continue;
                            }
                            c
                        } else {
                            continue;
                        }
                    }
                    (Role::Mix, _) => {
                        let (lo, hi) = (p.min.min(p.max), p.max.max(p.min));
                        let m = g.mix.0 + (g.mix.1 - g.mix.0) * (0.5 + 0.5 * var);
                        lo + (hi - lo) * m
                    }
                    // At most 70 % of the way from the default to either end: no run-away echoes or
                    // screaming resonance.
                    (Role::Feedback, _) => moved(p, dir * t * 0.7),
                    (Role::Width, _) => moved(p, (g.width + dir * 0.4) * t),
                    (Role::Freq, _) => {
                        let pull = if g.freq == 0.0 { dir } else { g.freq + dir * 0.35 };
                        moved(p, pull * t)
                    }
                    (Role::Db, _) => moved(p, dir * t / 4.0),
                    (Role::Shape, _) => moved(p, dir * t),
                };
                params.insert(p.id.to_string(), v);
            }
            out.push(Factory { group: g.name, name: format!("{} {}", g.name, noun), params });
        }
    }
    out
}

/// The factory presets of one effect, made once and kept.
pub fn cached(def: &'static EffectDef) -> &'static [Factory] {
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<std::collections::HashMap<&'static str, &'static [Factory]>>> = OnceLock::new();
    let mut map = CACHE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    map.entry(def.id).or_insert_with(|| Box::leak(presets(def).into_boxed_slice()))
}

/// The group names in menu order.
pub fn groups() -> impl Iterator<Item = &'static str> {
    GROUPS.iter().map(|g| g.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stock_effect_has_200_distinct_presets() {
        for def in super::super::registry() {
            let v = presets(def);
            assert!(v.len() >= 200, "{}: {}", def.id, v.len());
            let mut names: Vec<&str> = v.iter().map(|p| p.name.as_str()).collect();
            names.sort();
            names.dedup();
            assert_eq!(names.len(), v.len(), "{}: names repeat", def.id);
            let mut distinct = v.iter().map(|p| format!("{:?}", p.params)).collect::<Vec<_>>();
            distinct.sort();
            distinct.dedup();
            assert!(distinct.len() >= 190, "{}: only {} distinct settings", def.id, distinct.len());
            for p in &v {
                for prm in def.params {
                    let x = p.params[prm.id];
                    assert!(x.is_finite() && x >= prm.min.min(prm.max) - 1e-4 && x <= prm.max.max(prm.min) + 1e-4, "{} {} {}={x}", def.id, p.name, prm.id);
                }
            }
        }
    }

    #[test]
    fn presets_are_stable() {
        let d = super::super::find("chorus").unwrap();
        assert_eq!(presets(d), presets(d));
    }
}
