//! The Transmuter preset library: the hand-made signature presets plus a large bank built from
//! sound-design recipes. Every recipe describes one kind of sound (a reese bass, a pluck, a riser …)
//! and varies it inside ranges that keep it musical; a fixed seed per preset makes the bank identical
//! on every machine and every run. Levels are evened out by [`LEVELS`] (measured, see the tests).

use crate::dsp::Rng;
use crate::model::Envelope;
use crate::wt::*;
use std::collections::HashSet;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    Signature,
    Bass,
    Lead,
    Pad,
    Pluck,
    Keys,
    Bells,
    Arp,
    Stab,
    Atmosphere,
    Fx,
    Drums,
}

impl Category {
    pub const ALL: [Category; 12] =
        [Category::Signature, Category::Bass, Category::Lead, Category::Pad, Category::Pluck, Category::Keys, Category::Bells, Category::Arp, Category::Stab, Category::Atmosphere, Category::Fx, Category::Drums];
    pub fn name(self) -> &'static str {
        match self {
            Category::Signature => "Signature",
            Category::Bass => "Bass",
            Category::Lead => "Lead",
            Category::Pad => "Pad",
            Category::Pluck => "Pluck",
            Category::Keys => "Keys",
            Category::Bells => "Bells",
            Category::Arp => "Arp & Sequence",
            Category::Stab => "Stab & Chord",
            Category::Atmosphere => "Atmosphere",
            Category::Fx => "FX",
            Category::Drums => "Drums & Perc",
        }
    }
    /// Short code shown in lists ("BA Deep Reese").
    pub fn code(self) -> &'static str {
        match self {
            Category::Signature => "SG",
            Category::Bass => "BA",
            Category::Lead => "LD",
            Category::Pad => "PD",
            Category::Pluck => "PL",
            Category::Keys => "KY",
            Category::Bells => "BL",
            Category::Arp => "AR",
            Category::Stab => "ST",
            Category::Atmosphere => "AT",
            Category::Fx => "FX",
            Category::Drums => "DR",
        }
    }
    /// The note the level is measured on (and a good note to audition with).
    pub fn audition_note(self) -> u8 {
        match self {
            Category::Bass => 36,
            Category::Drums => 43,
            Category::Pad | Category::Atmosphere | Category::Stab => 57,
            _ => 60,
        }
    }
    /// How many presets the bank builds for the category.
    fn count(self) -> usize {
        match self {
            Category::Signature => 0,
            Category::Bass => 380,
            Category::Lead => 300,
            Category::Pad => 290,
            Category::Pluck => 200,
            Category::Keys => 180,
            Category::Bells => 110,
            Category::Arp => 150,
            Category::Stab => 150,
            Category::Atmosphere => 140,
            Category::Fx => 150,
            Category::Drums => 100,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LibPreset {
    pub name: String,
    pub category: Category,
    /// The kind of sound inside the category ("Reese", "Supersaw" …), searchable.
    pub kind: &'static str,
    pub params: WavetableParams,
}

/// The whole library, built once.
pub fn library() -> &'static [LibPreset] {
    static LIB: OnceLock<Vec<LibPreset>> = OnceLock::new();
    LIB.get_or_init(build)
}

/// Finds a preset by name.
pub fn find(name: &str) -> Option<&'static LibPreset> {
    library().iter().find(|p| p.name == name)
}

/// Measured level corrections (dB) for the generated presets, in library order after the signature
/// presets. Regenerate with `cargo test --release -p dragon-engine measure_preset_levels -- --ignored`.
pub(crate) const LEVELS: &[f32] = &include!("wt_levels.in");

fn build() -> Vec<LibPreset> {
    let mut out: Vec<LibPreset> = presets().into_iter().map(|(n, p)| LibPreset { name: n.to_string(), category: Category::Signature, kind: "Signature", params: WavetableParams { preset: n.to_string(), ..p } }).collect();
    let mut used: HashSet<String> = out.iter().map(|p| p.name.to_lowercase()).collect();
    let mut gen_index = 0;
    for cat in Category::ALL {
        for i in 0..cat.count() {
            let mut r = R(Rng::new(0x9E37_79B9_7F4A_7C15 ^ ((cat as u64 + 1) << 32) ^ (i as u64 * 0x2545_F491)));
            for _ in 0..3 {
                r.0.next_u64();
            }
            let (kind, nouns, mut p) = recipe(cat, &mut r, i);
            let mut name = format!("{} {}", r.pick(adjectives(cat)), r.pick(nouns));
            if used.contains(&name.to_lowercase()) {
                let base = name.clone();
                for k in 2.. {
                    name = format!("{base} {}", roman(k));
                    if !used.contains(&name.to_lowercase()) {
                        break;
                    }
                }
            }
            used.insert(name.to_lowercase());
            let db = LEVELS.get(gen_index).copied().unwrap_or(0.0);
            p.trim_db = db;
            p.preset = name.clone();
            gen_index += 1;
            out.push(LibPreset { name, category: cat, kind, params: p });
        }
    }
    out
}

fn roman(n: usize) -> &'static str {
    ["", "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI", "XII"].get(n).copied().unwrap_or("XX")
}

// ------------------------------------------------------------------------------------------------ helpers

struct R(Rng);

impl R {
    fn f(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.0.next_f32()
    }
    /// Log-uniform (for frequencies and times).
    fn lg(&mut self, a: f32, b: f32) -> f32 {
        (a.ln() + (b.ln() - a.ln()) * self.0.next_f32()).exp()
    }
    fn i(&mut self, a: i32, b: i32) -> i32 {
        a + (self.0.next_u64() % ((b - a + 1) as u64)) as i32
    }
    fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        xs[(self.0.next_u64() % xs.len() as u64) as usize]
    }
    fn chance(&mut self, p: f32) -> bool {
        self.0.next_f32() < p
    }
}

fn env(a: f32, d: f32, s: f32, r: f32) -> Envelope {
    Envelope { enabled: true, attack: a, decay: d, sustain: s, release: r, ..Envelope::default() }
}

fn osc(table: &str, pos: f32) -> WtOsc {
    WtOsc { on: true, table: TableSource::Factory(table.into()), pos, ..WtOsc::default() }
}

fn filt(kind: FilterKind, cutoff: f32, resonance: f32) -> WtFilter {
    WtFilter { on: true, kind, cutoff, resonance, ..WtFilter::default() }
}

fn slot(source: ModSource, dest: ModDest, amount: f32) -> ModSlot {
    ModSlot { source, dest, amount, on: true }
}

fn fx(id: &str, params: &[(&str, f32)]) -> FxSlot {
    FxSlot { id: id.into(), on: true, params: params.iter().map(|(k, v)| (k.to_string(), *v)).collect() }
}

fn synced(shape: LfoShape, beats: f32) -> WtLfo {
    WtLfo { shape, sync: true, beats, ..WtLfo::default() }
}

fn free(shape: LfoShape, rate: f32) -> WtLfo {
    WtLfo { shape, rate, ..WtLfo::default() }
}

const BEATS_FAST: [f32; 5] = [0.25, 1.0 / 3.0, 0.5, 2.0 / 3.0, 1.0];
const BEATS_SLOW: [f32; 4] = [2.0, 4.0, 8.0, 16.0];
const ALL_TABLES: [&str; 12] = FACTORY;

/// A fresh patch: one saw, filters off, the amp envelope given.
fn base(amp: Envelope) -> WavetableParams {
    let mut p = WavetableParams { osc: [osc("Basic Shapes", 0.66), WtOsc::default(), WtOsc::default()], ..WavetableParams::default() };
    p.env[0] = amp;
    p.filters = [WtFilter::default(), WtFilter::default()];
    p.matrix.clear();
    p.fx.clear();
    p
}

/// Delay and / or reverb, scaled by how roomy the sound should be (0 … 1).
fn space(r: &mut R, p: &mut WavetableParams, amount: f32) {
    if r.chance(0.35 + amount * 0.3) {
        p.fx.push(fx("chorus", &[("rate", r.lg(0.2, 1.5)), ("depth", r.f(0.3, 0.7)), ("mix", r.f(0.25, 0.5))]));
    }
    if r.chance(0.2 + amount * 0.6) {
        p.fx.push(fx("delay", &[("sync", r.pick(&[5.0, 6.0, 7.0, 8.0])), ("feedback", r.f(0.2, 0.55)), ("mix", r.f(0.08, 0.12 + amount * 0.18)), ("highcut", r.lg(3000.0, 9000.0))]));
    }
    if r.chance(0.3 + amount * 0.7) {
        p.fx.push(fx("reverb", &[("mode", r.pick(&[1.0, 2.0, 3.0, 4.0])), ("decay", r.lg(0.8, 1.5 + amount * 7.0)), ("mix", r.f(0.1, 0.15 + amount * 0.3)), ("size", r.f(0.4, 1.0))]));
    }
}

/// Four macros with names that match what they move.
fn macros(p: &mut WavetableParams, names: [&str; 4], slots: [Option<(ModDest, f32)>; 4]) {
    for k in 0..4 {
        p.macro_names[k] = names[k].to_string();
        if let Some((d, a)) = slots[k] {
            p.matrix.push(slot(ModSource::Macro(k as u8), d, a));
        }
    }
}

/// The usual macro set: tone, morph, a third and space-ish width.
fn std_macros(p: &mut WavetableParams, r: &mut R, third: (&str, ModDest)) {
    let tone = if p.filters[0].on { (ModDest::FilterCutoff(0), r.f(0.25, 0.45)) } else { (ModDest::OscPos(0), r.f(0.3, 0.6)) };
    macros(p, ["Tone", "Morph", third.0, "Detune"], [Some(tone), Some((ModDest::OscPos(0), r.f(0.3, 0.7))), Some((third.1, r.f(0.3, 0.6))), Some((ModDest::OscDetune(0), r.f(0.3, 0.6)))]);
}

// ------------------------------------------------------------------------------------------------ names

fn adjectives(cat: Category) -> &'static [&'static str] {
    match cat {
        Category::Bass => &[
            "Deep",
            "Dirty",
            "Fat",
            "Heavy",
            "Rolling",
            "Gritty",
            "Warm",
            "Dark",
            "Wide",
            "Thick",
            "Rough",
            "Low",
            "Punchy",
            "Tight",
            "Filthy",
            "Brutal",
            "Round",
            "Moody",
            "Hollow",
            "Rubber",
            "Liquid",
            "Crushed",
            "Muddy",
            "Steel",
            "Rusty",
            "Velvet",
            "Neon",
            "Night",
            "Underground",
            "Concrete",
            "Molten",
            "Smoky",
            "Bouncy",
            "Snarling",
            "Furious",
            "Colossal",
            "Subtle",
            "Electric",
            "Toxic",
            "Vintage",
            "Modern",
            "Cold",
        ],
        Category::Lead => &[
            "Bright",
            "Soaring",
            "Sharp",
            "Screaming",
            "Shiny",
            "Hyper",
            "Golden",
            "Silver",
            "Crystal",
            "Burning",
            "Laser",
            "Retro",
            "Euphoric",
            "Wild",
            "Singing",
            "Sweet",
            "Piercing",
            "Fierce",
            "Glowing",
            "Starlit",
            "Electric",
            "Smooth",
            "Cutting",
            "Rising",
            "Flying",
            "Neon",
            "Analog",
            "Digital",
            "Cosmic",
            "Blazing",
            "Vivid",
            "Royal",
            "Clean",
            "Raw",
            "Sunny",
            "Turbo",
            "Lucid",
            "Rebel",
            "Phantom",
            "Heroic",
        ],
        Category::Pad => &[
            "Warm",
            "Soft",
            "Dreamy",
            "Floating",
            "Lush",
            "Velvet",
            "Endless",
            "Distant",
            "Calm",
            "Gentle",
            "Hazy",
            "Misty",
            "Glacial",
            "Silken",
            "Ethereal",
            "Slow",
            "Wide",
            "Warm",
            "Faded",
            "Golden",
            "Twilight",
            "Morning",
            "Evening",
            "Northern",
            "Ocean",
            "Cloud",
            "Celestial",
            "Breathing",
            "Shifting",
            "Analog",
            "Glass",
            "Frozen",
            "Amber",
            "Lunar",
            "Solar",
            "Quiet",
            "Hidden",
            "Infinite",
            "Tender",
            "Serene",
            "Deep",
        ],
        Category::Pluck => &[
            "Bright", "Short", "Tiny", "Crisp", "Bouncy", "Glassy", "Wooden", "Soft", "Snappy", "Plucky", "Happy", "Crystal", "Tropical", "Island", "Neon", "Pop", "Clean", "Dusty", "Muted", "Round", "Sparkly", "Quick",
            "Warm", "Digital", "Little", "Candy", "Paper", "Bamboo", "Liquid", "Polished", "Sunset", "Lo-Fi", "Spring", "Twinkle",
        ],
        Category::Keys => &[
            "Vintage",
            "Warm",
            "Smooth",
            "Mellow",
            "Soft",
            "Bright",
            "Dusty",
            "Soulful",
            "Jazzy",
            "Late Night",
            "Lounge",
            "Classic",
            "Toy",
            "Glass",
            "Velvet",
            "Tine Keys",
            "Gospel",
            "Studio",
            "Tape",
            "Golden",
            "Hazy",
            "Clean",
            "Sweet",
            "Cozy",
            "Lo-Fi",
            "Chill",
            "Silky",
            "Honey",
            "Midnight",
            "Sunday",
        ],
        Category::Bells => &[
            "Crystal",
            "Glass",
            "Silver",
            "Tiny",
            "Temple",
            "Ice",
            "Winter",
            "Chime",
            "Fairy",
            "Star",
            "Magic",
            "Distant",
            "Gamelan",
            "Metal",
            "Golden",
            "Clear",
            "Shimmer",
            "Frozen",
            "Bright",
            "Holy",
            "Music Box",
            "Moon",
            "Snow",
            "Prism",
        ],
        Category::Arp => &[
            "Running",
            "Pulsing",
            "Gated",
            "Driving",
            "Ticking",
            "Racing",
            "Bouncing",
            "Rolling",
            "Cycling",
            "Rhythm",
            "Motion",
            "Stepping",
            "Chopped",
            "Stuttering",
            "Hypnotic",
            "Spiral",
            "Looping",
            "Trance",
            "Disco",
            "Clockwork",
            "Morse",
            "Pattern",
            "Kinetic",
            "Synced",
        ],
        Category::Stab => &[
            "House",
            "Rave",
            "Club",
            "Big",
            "Classic",
            "Garage",
            "Funky",
            "Disco",
            "Detroit",
            "Chicago",
            "Deep",
            "Jazzy",
            "Minor",
            "Major",
            "Bright",
            "Dark",
            "Punchy",
            "Retro",
            "Warehouse",
            "Hit",
            "Brass",
            "Soul",
            "Uplift",
            "Euro",
            "Sunset",
            "Velvet",
            "Gold",
        ],
        Category::Atmosphere => &[
            "Dark",
            "Distant",
            "Haunted",
            "Deep Space",
            "Underwater",
            "Abandoned",
            "Frozen",
            "Desert",
            "Forest",
            "Cavern",
            "Nebula",
            "Ghost",
            "Ancient",
            "Midnight",
            "Dust",
            "Echo",
            "Void",
            "Fog",
            "Storm",
            "Arctic",
            "Industrial",
            "Sacred",
            "Alien",
            "Dream",
            "Ruins",
            "Orbit",
        ],
        Category::Fx => &[
            "Rising", "Falling", "Big", "Laser", "Sci-Fi", "Glitch", "Cosmic", "Warp", "Turbo", "Hyper", "Robot", "Alarm", "Noise", "Sweep", "Impact", "Charge", "Power", "Wobble", "Space", "Rocket", "Static", "Digital",
            "Pulse", "Vortex", "Blast", "Zero G",
        ],
        Category::Drums => {
            &["Deep", "Punchy", "Tight", "Short", "Long", "Hard", "Soft", "Boomy", "Clicky", "Dusty", "Analog", "Digital", "Big", "Tiny", "Snappy", "Club", "Trap", "Techno", "Lo-Fi", "Crisp", "Metal", "Wood", "Room"]
        }
        Category::Signature => &["Dragon"],
    }
}

// ------------------------------------------------------------------------------------------------ recipes

type Recipe = (&'static str, &'static [&'static str], WavetableParams);

fn recipe(cat: Category, r: &mut R, i: usize) -> Recipe {
    match cat {
        Category::Bass => bass(r, i),
        Category::Lead => lead(r, i),
        Category::Pad => pad(r, i),
        Category::Pluck => pluck(r, i),
        Category::Keys => keys(r, i),
        Category::Bells => bells(r, i),
        Category::Arp => arp(r, i),
        Category::Stab => stab(r, i),
        Category::Atmosphere => atmos(r, i),
        Category::Fx => sfx(r, i),
        Category::Drums => drums(r, i),
        Category::Signature => ("Signature", &["Wave"], WavetableParams::default()),
    }
}

fn mono(p: &mut WavetableParams, r: &mut R) {
    p.polyphony = 1;
    p.mono = true;
    p.legato = r.chance(0.6);
    p.glide = if r.chance(0.6) { r.lg(0.015, 0.09) } else { 0.0 };
}

fn bass(r: &mut R, i: usize) -> Recipe {
    let amp = env(r.lg(0.001, 0.006), r.lg(0.2, 0.8), r.f(0.75, 1.0), r.lg(0.05, 0.2));
    let mut p = base(amp);
    mono(&mut p, r);
    p.sub = SubOsc { on: r.chance(0.7), level: r.f(0.3, 0.6), shape: r.pick(&[SubShape::Sine, SubShape::Sine, SubShape::Triangle]), ..SubOsc::default() };
    match i % 9 {
        0 => {
            // Sub
            p.osc[0] = WtOsc { level: r.f(0.15, 0.35), ..osc("Basic Shapes", r.f(0.0, 0.4)) };
            p.sub = SubOsc { on: true, level: r.f(0.7, 0.95), octave: 0, shape: r.pick(&[SubShape::Sine, SubShape::Triangle]), ..SubOsc::default() };
            p.filters[0] = filt(FilterKind::Lp24, r.lg(200.0, 700.0), r.f(0.0, 0.25));
            if r.chance(0.5) {
                p.fx.push(fx("saturator", &[("mode", r.pick(&[0.0, 1.0])), ("drive", r.f(2.0, 9.0)), ("mix", r.f(0.4, 1.0))]));
            }
            macros(&mut p, ["Tone", "Drive", "Sub", "Glide"], [Some((ModDest::FilterCutoff(0), 0.4)), Some((ModDest::FilterDrive(0), 0.6)), Some((ModDest::SubLevel, 0.3)), None]);
            ("Sub", &["Sub", "Subwoofer", "Floor", "Pressure", "Foundation", "808 Sub"], p)
        }
        1 => {
            // Reese
            let pos = r.pick(&[0.66, 0.66, 0.85]);
            p.osc[0] = WtOsc { unison: r.i(2, 4) as u32, detune: r.f(0.1, 0.3), width: r.f(0.2, 0.5), ..osc("Basic Shapes", pos) };
            p.osc[1] = WtOsc { unison: 2, detune: r.f(0.08, 0.2), fine: r.f(5.0, 14.0), level: r.f(0.5, 0.75), ..osc(r.pick(&["Basic Shapes", "Harmonic Sweep", "Odd Even"]), r.f(0.5, 1.0)) };
            p.filters[0] = filt(FilterKind::Lp24, r.lg(350.0, 1500.0), r.f(0.1, 0.35));
            p.lfo[0] = free(LfoShape::Sine, r.lg(0.1, 0.6));
            p.matrix.push(slot(ModSource::Lfo(0), ModDest::FilterCutoff(0), r.f(0.05, 0.18)));
            p.fx.push(fx("saturator", &[("drive", r.f(3.0, 12.0))]));
            p.fx.push(fx("imager", &[("width", r.f(1.1, 1.6)), ("mono_below", r.lg(100.0, 220.0))]));
            std_macros(&mut p, r, ("Drive", ModDest::FilterDrive(0)));
            ("Reese", &["Reese", "Tear", "Rumble", "Drift", "Growler", "Hum"], p)
        }
        2 => {
            // Growl
            p.osc[0] = WtOsc { warp: r.pick(&[Warp::Fm, Warp::Fold, Warp::BendPlus, Warp::Sync]), warp_amount: r.f(0.15, 0.5), ..osc(r.pick(&["Dragon Growl", "Formant Vowels", "Fold", "Sync Sweep"]), r.f(0.1, 0.6)) };
            p.filters[0] = WtFilter { drive: r.f(0.2, 0.6), ..filt(r.pick(&[FilterKind::Formant, FilterKind::Lp12, FilterKind::Bp, FilterKind::CombPlus]), r.lg(400.0, 1500.0), r.f(0.3, 0.6)) };
            p.filters[1] = filt(FilterKind::Lp24, r.lg(3000.0, 9000.0), 0.1);
            p.lfo[0] = synced(r.pick(&[LfoShape::Triangle, LfoShape::Sine, LfoShape::SawDown, LfoShape::Smooth]), r.pick(&BEATS_FAST));
            p.matrix.push(slot(ModSource::Lfo(0), ModDest::OscPos(0), r.f(0.3, 0.6)));
            p.matrix.push(slot(ModSource::Lfo(0), ModDest::FilterCutoff(0), r.f(0.15, 0.35)));
            p.fx.push(fx("saturator", &[("mode", r.pick(&[0.0, 2.0, 4.0, 5.0])), ("drive", r.f(6.0, 18.0)), ("output", -6.0)]));
            p.fx.push(fx("compressor", &[("threshold", -20.0), ("ratio", 6.0), ("makeup", 4.0)]));
            macros(&mut p, ["Growl", "Wobble", "Warp", "Vowel"], [Some((ModDest::OscPos(0), 0.4)), Some((ModDest::LfoRate(0), 0.25)), Some((ModDest::OscWarp(0), 0.5)), Some((ModDest::FilterCutoff(0), 0.4))]);
            ("Growl", &["Growl", "Beast", "Monster", "Roar", "Gnarl", "Talker", "Yoi"], p)
        }
        3 => {
            // Wobble
            p.osc[0] = osc(r.pick(&["Basic Shapes", "Pulse Width", "Harmonic Sweep"]), r.f(0.6, 1.0));
            p.osc[1] = WtOsc { octave: -1, level: r.f(0.3, 0.6), ..osc("Basic Shapes", r.pick(&[0.66, 1.0])) };
            p.filters[0] = filt(FilterKind::Lp24, r.lg(200.0, 600.0), r.f(0.4, 0.7));
            p.lfo[0] = synced(r.pick(&[LfoShape::Sine, LfoShape::Triangle, LfoShape::SawDown]), r.pick(&BEATS_FAST));
            p.matrix.push(slot(ModSource::Lfo(0), ModDest::FilterCutoff(0), r.f(0.3, 0.5)));
            p.fx.push(fx("saturator", &[("drive", r.f(6.0, 15.0))]));
            macros(
                &mut p,
                ["Cutoff", "Wobble Rate", "Resonance", "Depth"],
                [Some((ModDest::FilterCutoff(0), 0.35)), Some((ModDest::LfoRate(0), 0.25)), Some((ModDest::FilterReso(0), 0.3)), Some((ModDest::OscPos(0), 0.3))],
            );
            ("Wobble", &["Wobble", "Wub", "Womp", "Swing", "Wobbler"], p)
        }
        4 => {
            // Pluck bass
            p.env[0] = env(0.001, r.lg(0.25, 0.7), r.f(0.0, 0.3), r.lg(0.05, 0.2));
            p.osc[0] = WtOsc { unison: r.i(1, 3) as u32, detune: r.f(0.05, 0.2), ..osc(r.pick(&["Basic Shapes", "Pulse Width", "Odd Even"]), r.f(0.55, 1.0)) };
            p.filters[0] = filt(FilterKind::Lp24, r.lg(150.0, 500.0), r.f(0.15, 0.45));
            p.env[1] = env(0.001, r.lg(0.08, 0.35), 0.0, 0.1);
            p.matrix.push(slot(ModSource::Env(1), ModDest::FilterCutoff(0), r.f(0.35, 0.6)));
            std_macros(&mut p, r, ("Snap", ModDest::FilterReso(0)));
            ("Pluck Bass", &["Pluck Bass", "Bounce Bass", "Slap", "Thump", "Pluck Low"], p)
        }
        5 => {
            // Acid
            p.osc[0] = osc(r.pick(&["Basic Shapes", "Pulse Width"]), r.pick(&[0.66, 1.0, 0.3]));
            p.sub.on = false;
            p.filters[0] = WtFilter { drive: r.f(0.2, 0.5), ..filt(r.pick(&[FilterKind::Lp24, FilterKind::Lp12]), r.lg(250.0, 800.0), r.f(0.55, 0.8)) };
            p.env[1] = env(0.001, r.lg(0.08, 0.3), 0.0, 0.1);
            p.matrix.push(slot(ModSource::Env(1), ModDest::FilterCutoff(0), r.f(0.3, 0.55)));
            p.matrix.push(slot(ModSource::Velocity, ModDest::FilterCutoff(0), r.f(0.1, 0.25)));
            p.legato = true;
            p.glide = r.lg(0.04, 0.12);
            p.fx.push(fx("saturator", &[("mode", r.pick(&[0.0, 1.0, 2.0])), ("drive", r.f(6.0, 16.0)), ("output", -5.0)]));
            if r.chance(0.5) {
                p.fx.push(fx("delay", &[("sync", 6.0), ("mix", r.f(0.08, 0.18)), ("feedback", 0.35)]));
            }
            macros(
                &mut p,
                ["Cutoff", "Resonance", "Env Mod", "Drive"],
                [Some((ModDest::FilterCutoff(0), 0.4)), Some((ModDest::FilterReso(0), 0.25)), Some((ModDest::FilterCutoff(0), 0.2)), Some((ModDest::FilterDrive(0), 0.5))],
            );
            ("Acid", &["Acid", "Squelch", "303", "Acid Line", "Rubber Acid"], p)
        }
        6 => {
            // FM bass
            p.osc[0] = WtOsc { warp: Warp::Fm, warp_amount: r.f(0.1, 0.4), ..osc(r.pick(&["FM Bells", "Basic Shapes"]), r.f(0.0, 0.3)) };
            p.env[1] = env(0.001, r.lg(0.1, 0.5), r.f(0.0, 0.3), 0.1);
            p.matrix.push(slot(ModSource::Env(1), ModDest::OscWarp(0), r.f(0.2, 0.5)));
            p.matrix.push(slot(ModSource::Env(1), ModDest::OscPos(0), r.f(0.1, 0.4)));
            if r.chance(0.5) {
                p.filters[0] = filt(FilterKind::Lp12, r.lg(1500.0, 6000.0), 0.1);
            }
            std_macros(&mut p, r, ("FM", ModDest::OscWarp(0)));
            ("FM Bass", &["FM Bass", "Operator", "Digi Bass", "Funk Bass", "Electro"], p)
        }
        7 => {
            // Neuro
            p.osc[0] = WtOsc {
                unison: r.i(1, 3) as u32,
                detune: 0.1,
                warp: r.pick(&[Warp::Sync, Warp::Fold, Warp::Crush, Warp::Asym]),
                warp_amount: r.f(0.2, 0.6),
                ..osc(r.pick(&["Sync Sweep", "Fold", "Digital Steps", "Noise Spectra"]), r.f(0.1, 0.8))
            };
            p.filters[0] = filt(r.pick(&[FilterKind::CombPlus, FilterKind::CombMinus, FilterKind::Notch, FilterKind::Bp]), r.lg(200.0, 1200.0), r.f(0.3, 0.6));
            p.lfo[0] = synced(r.pick(&[LfoShape::SawUp, LfoShape::Square, LfoShape::Random, LfoShape::Triangle]), r.pick(&BEATS_FAST));
            p.matrix.push(slot(ModSource::Lfo(0), ModDest::FilterCutoff(0), r.f(0.2, 0.4)));
            p.matrix.push(slot(ModSource::Lfo(0), ModDest::OscWarp(0), r.f(0.2, 0.4)));
            p.fx.push(fx("saturator", &[("mode", r.pick(&[2.0, 3.0, 4.0])), ("drive", r.f(6.0, 14.0)), ("output", -6.0)]));
            if r.chance(0.4) {
                p.fx.push(fx("bitcrusher", &[("bits", r.f(8.0, 12.0)), ("mix", r.f(0.2, 0.5))]));
            }
            p.fx.push(fx("compressor", &[("threshold", -18.0), ("ratio", 5.0)]));
            macros(&mut p, ["Comb", "Warp", "Rate", "Morph"], [Some((ModDest::FilterCutoff(0), 0.45)), Some((ModDest::OscWarp(0), 0.5)), Some((ModDest::LfoRate(0), 0.25)), Some((ModDest::OscPos(0), 0.5))]);
            ("Neuro", &["Neuro", "Machine", "Circuit", "Reactor", "Mutant", "Engine"], p)
        }
        _ => {
            // 808-style boom
            p.env[0] = env(0.001, r.lg(0.6, 2.0), r.f(0.0, 0.4), r.lg(0.1, 0.4));
            p.osc[0] = osc("Basic Shapes", r.f(0.0, 0.2));
            p.sub.on = false;
            p.env[1] = env(0.001, r.lg(0.03, 0.12), 0.0, 0.05);
            p.matrix.push(slot(ModSource::Env(1), ModDest::Pitch, r.f(0.15, 0.4)));
            p.fx.push(fx("saturator", &[("mode", r.pick(&[0.0, 1.0, 2.0])), ("drive", r.f(4.0, 14.0)), ("output", -4.0)]));
            p.legato = false;
            p.glide = r.lg(0.03, 0.1);
            p.osc[0].warp = Warp::Fold;
            p.osc[0].warp_amount = r.f(0.0, 0.2);
            macros(&mut p, ["Drive", "Punch", "Shape", "Length"], [Some((ModDest::OscWarp(0), 0.5)), Some((ModDest::Pitch, 0.05)), Some((ModDest::OscPos(0), 0.4)), None]);
            ("808", &["808", "Boom", "Trap Bass", "Long 808", "Slide 808"], p)
        }
    }
}

fn lead(r: &mut R, i: usize) -> Recipe {
    let amp = env(r.lg(0.001, 0.02), r.lg(0.3, 1.0), r.f(0.7, 1.0), r.lg(0.1, 0.5));
    let mut p = base(amp);
    if r.chance(0.5) {
        mono(&mut p, r);
    } else {
        p.polyphony = 8;
    }
    // A little delayed vibrato on most leads.
    p.lfo[1] = WtLfo { delay: r.f(0.15, 0.5), rise: r.f(0.2, 0.6), ..free(LfoShape::Sine, r.f(4.5, 6.5)) };
    if r.chance(0.6) {
        p.matrix.push(slot(ModSource::Lfo(1), ModDest::Pitch, r.f(0.004, 0.01)));
    }
    let res = match i % 9 {
        0 => {
            p.osc[0] = WtOsc { unison: r.i(5, 11) as u32, detune: r.f(0.2, 0.45), width: 1.0, ..osc("Basic Shapes", 0.66) };
            p.osc[1] = WtOsc { unison: r.i(3, 7) as u32, detune: r.f(0.15, 0.3), octave: r.pick(&[1, -1, 0]), level: r.f(0.25, 0.45), ..osc("Basic Shapes", r.pick(&[0.66, 1.0])) };
            p.filters[0] = filt(FilterKind::Lp12, r.lg(4000.0, 12000.0), 0.15);
            p.filters[1] = filt(FilterKind::Hp12, r.lg(80.0, 250.0), 0.1);
            ("Supersaw", &["Supersaw", "Anthem", "Saw Stack", "Festival", "Wall"][..])
        }
        1 => {
            p.osc[0] = WtOsc { warp: Warp::Sync, warp_amount: r.f(0.2, 0.5), ..osc(r.pick(&["Basic Shapes", "Sync Sweep"]), r.f(0.5, 0.8)) };
            p.env[1] = env(0.001, r.lg(0.2, 0.8), r.f(0.0, 0.4), 0.2);
            p.matrix.push(slot(ModSource::Env(1), ModDest::OscWarp(0), r.f(0.25, 0.5)));
            p.filters[0] = filt(FilterKind::Lp12, r.lg(3000.0, 9000.0), 0.15);
            ("Sync Lead", &["Sync", "Sync Lead", "Hard Sync", "Tear Lead"][..])
        }
        2 => {
            p.osc[0] = WtOsc { unison: r.i(1, 3) as u32, detune: 0.12, ..osc("Basic Shapes", r.f(0.6, 0.75)) };
            p.osc[1] = WtOsc { fine: r.f(-12.0, 12.0), level: 0.6, ..osc("Basic Shapes", r.f(0.6, 1.0)) };
            p.filters[0] = WtFilter { keytrack: 0.5, ..filt(FilterKind::Lp24, r.lg(1200.0, 5000.0), r.f(0.15, 0.4)) };
            p.env[1] = env(0.001, r.lg(0.2, 0.6), r.f(0.2, 0.5), 0.2);
            p.matrix.push(slot(ModSource::Env(1), ModDest::FilterCutoff(0), r.f(0.15, 0.3)));
            ("Mono Lead", &["Lead", "Mono Lead", "Classic Lead", "Solo", "Mini Lead"][..])
        }
        3 => {
            p.osc[0] = osc("Pulse Width", r.f(0.0, 0.8));
            p.lfo[2] = free(LfoShape::Triangle, r.f(0.3, 2.0));
            p.matrix.push(slot(ModSource::Lfo(2), ModDest::OscPos(0), r.f(0.1, 0.3)));
            if r.chance(0.6) {
                p.fx.push(fx("bitcrusher", &[("bits", r.f(5.0, 10.0)), ("downsample", r.f(1.0, 4.0)), ("mix", r.f(0.3, 0.8))]));
            }
            ("Chip Lead", &["Chip", "8-Bit", "Square Lead", "Arcade", "Pulse"][..])
        }
        4 => {
            p.osc[0] = WtOsc { unison: r.i(1, 3) as u32, detune: 0.1, ..osc("Formant Vowels", r.f(0.0, 1.0)) };
            p.filters[0] = filt(FilterKind::Formant, r.lg(400.0, 2000.0), r.f(0.3, 0.6));
            p.lfo[0] = free(LfoShape::Triangle, r.lg(0.2, 1.5));
            p.matrix.push(slot(ModSource::Lfo(0), ModDest::FilterCutoff(0), r.f(0.1, 0.3)));
            p.matrix.push(slot(ModSource::Lfo(0), ModDest::OscPos(0), r.f(0.1, 0.4)));
            ("Vowel Lead", &["Vowel", "Talker", "Voice Lead", "Yoi Lead", "Speak"][..])
        }
        5 => {
            p.osc[0] = WtOsc { warp: r.pick(&[Warp::Fold, Warp::Asym]), warp_amount: r.f(0.2, 0.5), ..osc(r.pick(&["Fold", "Basic Shapes", "Odd Even"]), r.f(0.3, 0.8)) };
            p.osc[1] = WtOsc { octave: 1, level: 0.3, ..osc("Basic Shapes", 0.66) };
            p.filters[0] = filt(FilterKind::Lp12, r.lg(2500.0, 8000.0), 0.2);
            p.fx.push(fx("saturator", &[("mode", r.pick(&[2.0, 3.0, 4.0])), ("drive", r.f(8.0, 18.0)), ("output", -8.0)]));
            ("Screamer", &["Screamer", "Distortion Lead", "Riot", "Shredder"][..])
        }
        6 => {
            p.osc[0] = osc("Basic Shapes", r.f(0.05, 0.35));
            p.osc[1] = WtOsc { octave: 1, level: r.f(0.1, 0.25), ..osc("Basic Shapes", 0.0) };
            p.matrix.retain(|m| m.dest != ModDest::Pitch);
            p.matrix.push(slot(ModSource::Lfo(1), ModDest::Pitch, r.f(0.008, 0.014)));
            ("Soft Lead", &["Whistle", "Flute Lead", "Soft Lead", "Sine Lead", "Ocarina"][..])
        }
        7 => {
            p.osc[0] = WtOsc { unison: r.i(2, 5) as u32, detune: r.f(0.15, 0.3), ..osc(r.pick(&["Harmonic Sweep", "Odd Even", "Basic Shapes"]), r.f(0.4, 0.9)) };
            p.polyphony = 1;
            p.mono = true;
            p.legato = true;
            p.glide = r.lg(0.06, 0.2);
            p.filters[0] = filt(FilterKind::Lp24, r.lg(2000.0, 7000.0), r.f(0.1, 0.3));
            ("Glide Lead", &["Portamento", "Glide", "Slide Lead", "Bender"][..])
        }
        _ => {
            p.osc[0] = WtOsc { unison: r.i(3, 7) as u32, detune: r.f(0.25, 0.4), warp: Warp::Pwm, warp_amount: r.f(0.2, 0.5), ..osc("Pulse Width", r.f(0.0, 0.5)) };
            p.env[1] = env(0.001, r.lg(0.1, 0.3), 0.0, 0.1);
            p.matrix.push(slot(ModSource::Env(1), ModDest::Pitch, r.f(-0.05, -0.02)));
            p.filters[0] = filt(FilterKind::Lp24, r.lg(2500.0, 7000.0), 0.15);
            p.fx.push(fx("chorus", &[("mix", 0.4)]));
            ("Hoover", &["Hoover", "Mentasm", "Rave Lead", "Dentist"][..])
        }
    };
    space(r, &mut p, 0.4);
    std_macros(&mut p, r, ("Warp", ModDest::OscWarp(0)));
    (res.0, res.1, p)
}

fn pad(r: &mut R, i: usize) -> Recipe {
    let amp = env(r.lg(0.3, 2.5), r.lg(0.5, 3.0), r.f(0.6, 1.0), r.lg(0.8, 4.0));
    let mut p = base(amp);
    p.polyphony = 12;
    p.velocity = r.f(0.1, 0.4);
    let res: (&str, &[&str]) = match i % 8 {
        0 => {
            p.osc[0] = WtOsc { unison: r.i(4, 8) as u32, detune: r.f(0.15, 0.35), ..osc("Basic Shapes", r.pick(&[0.66, 0.5, 0.85])) };
            p.osc[1] = WtOsc { unison: r.i(2, 4) as u32, octave: r.pick(&[1, 0, -1]), level: 0.4, ..osc("Basic Shapes", r.f(0.3, 0.7)) };
            p.filters[0] = filt(FilterKind::Lp24, r.lg(800.0, 3500.0), r.f(0.1, 0.3));
            ("Warm Pad", &["Pad", "Warmth", "Blanket", "Glow", "Analog Pad"])
        }
        1 => {
            p.osc[0] = WtOsc { unison: r.i(2, 5) as u32, detune: r.f(0.1, 0.25), ..osc(r.pick(&["Harmonic Sweep", "Noise Spectra", "Odd Even", "Formant Vowels"]), r.f(0.0, 1.0)) };
            p.osc[1] = WtOsc { unison: 2, level: 0.5, semi: r.pick(&[0, 7, 12, -12]), ..osc(r.pick(&ALL_TABLES), r.f(0.0, 1.0)) };
            p.lfo[0] = free(LfoShape::Smooth, r.lg(0.03, 0.2));
            p.lfo[2] = free(LfoShape::Triangle, r.lg(0.05, 0.25));
            p.matrix.push(slot(ModSource::Lfo(0), ModDest::OscPos(0), r.f(0.25, 0.5)));
            p.matrix.push(slot(ModSource::Lfo(2), ModDest::OscPos(1), r.f(0.2, 0.5)));
            p.filters[0] = filt(FilterKind::Lp12, r.lg(1500.0, 8000.0), 0.15);
            ("Evolving Pad", &["Evolution", "Motion Pad", "Morph", "Tides", "Drift"])
        }
        2 => {
            p.osc[0] = WtOsc { unison: r.i(3, 6) as u32, detune: 0.2, octave: -1, ..osc("Basic Shapes", r.f(0.5, 0.8)) };
            p.filters[0] = filt(FilterKind::Lp24, r.lg(300.0, 900.0), r.f(0.2, 0.45));
            p.lfo[0] = free(LfoShape::Sine, r.lg(0.05, 0.2));
            p.matrix.push(slot(ModSource::Lfo(0), ModDest::FilterCutoff(0), r.f(0.1, 0.25)));
            ("Dark Pad", &["Shadow", "Dark Pad", "Abyss", "Night Pad", "Murk"])
        }
        3 => {
            p.osc[0] = WtOsc { unison: r.i(3, 6) as u32, detune: 0.25, ..osc("Basic Shapes", r.f(0.3, 0.66)) };
            p.noise = NoiseOsc { on: true, color: r.f(0.5, 0.9), level: r.f(0.08, 0.2), ..NoiseOsc::default() };
            p.filters[0] = filt(FilterKind::Hp12, r.lg(300.0, 900.0), 0.1);
            p.filters[1] = filt(FilterKind::Lp12, r.lg(5000.0, 12000.0), 0.1);
            ("Airy Pad", &["Air", "Breath", "Wind Pad", "Sky", "Cloud"])
        }
        4 => {
            p.osc[0] = WtOsc { unison: r.i(3, 6) as u32, detune: 0.2, ..osc("Formant Vowels", r.f(0.0, 1.0)) };
            p.filters[0] = filt(FilterKind::Formant, r.lg(400.0, 1800.0), r.f(0.3, 0.55));
            p.lfo[0] = free(LfoShape::Smooth, r.lg(0.05, 0.3));
            p.matrix.push(slot(ModSource::Lfo(0), ModDest::FilterCutoff(0), r.f(0.1, 0.3)));
            ("Choir", &["Choir", "Voices", "Aahs", "Chant", "Vox Pad"])
        }
        5 => {
            p.osc[0] = WtOsc { unison: r.i(2, 4) as u32, ..osc("FM Bells", r.f(0.05, 0.35)) };
            p.osc[1] = WtOsc { octave: 1, level: 0.3, ..osc("Basic Shapes", 0.0) };
            p.filters[0] = filt(FilterKind::Lp12, r.lg(3000.0, 9000.0), 0.1);
            ("Glass Pad", &["Glass", "Crystal Pad", "Shine", "Frost"])
        }
        6 => {
            p.osc[0] = WtOsc { unison: r.i(5, 9) as u32, detune: r.f(0.15, 0.3), ..osc("Basic Shapes", 0.66) };
            p.filters[0] = filt(FilterKind::Lp12, r.lg(1500.0, 5000.0), 0.15);
            p.lfo[1] = WtLfo { delay: 0.4, rise: 0.8, ..free(LfoShape::Sine, r.f(4.5, 6.0)) };
            p.matrix.push(slot(ModSource::Lfo(1), ModDest::Pitch, r.f(0.003, 0.007)));
            p.fx.push(fx("chorus", &[("mix", 0.4)]));
            ("Strings", &["Strings", "Ensemble", "Orchestra", "Violins", "Cellos"])
        }
        _ => {
            p.osc[0] = WtOsc { unison: r.i(3, 6) as u32, detune: 0.2, ..osc(r.pick(&["Harmonic Sweep", "Basic Shapes", "Organ"]), r.f(0.3, 0.8)) };
            p.osc[1] = WtOsc { octave: 1, unison: 3, level: r.f(0.3, 0.5), ..osc("Basic Shapes", r.f(0.0, 0.4)) };
            p.osc[2] = WtOsc { octave: 2, level: r.f(0.1, 0.25), ..osc("Basic Shapes", 0.0) };
            p.filters[0] = filt(FilterKind::Lp12, r.lg(3000.0, 10000.0), 0.1);
            ("Shimmer", &["Shimmer", "Heaven", "Halo", "Aurora", "Radiance"])
        }
    };
    space(r, &mut p, 0.9);
    std_macros(&mut p, r, ("Motion", ModDest::OscPos(1)));
    (res.0, res.1, p)
}

fn pluck(r: &mut R, i: usize) -> Recipe {
    let amp = env(0.001, r.lg(0.15, 0.7), r.f(0.0, 0.15), r.lg(0.1, 0.5));
    let mut p = base(amp);
    p.polyphony = 12;
    p.env[1] = env(0.001, r.lg(0.05, 0.3), 0.0, 0.1);
    p.matrix.push(slot(ModSource::Velocity, ModDest::FilterCutoff(0), r.f(0.05, 0.15)));
    let res: (&str, &[&str]) = match i % 6 {
        0 => {
            p.osc[0] = WtOsc { unison: r.i(1, 5) as u32, detune: r.f(0.1, 0.25), ..osc("Basic Shapes", r.f(0.55, 0.9)) };
            p.filters[0] = filt(FilterKind::Lp24, r.lg(300.0, 900.0), r.f(0.15, 0.35));
            p.matrix.push(slot(ModSource::Env(1), ModDest::FilterCutoff(0), r.f(0.35, 0.6)));
            ("Synth Pluck", &["Pluck", "Pluck Lead", "Plink", "Snap", "Tick"])
        }
        1 => {
            p.osc[0] = osc("FM Bells", r.f(0.3, 0.8));
            p.matrix.push(slot(ModSource::Env(1), ModDest::OscPos(0), r.f(0.3, 0.6)));
            p.filters[0] = filt(FilterKind::Lp12, r.lg(3000.0, 10000.0), 0.1);
            ("Bell Pluck", &["Bell Pluck", "Ding", "Chime Pluck", "Glint"])
        }
        2 => {
            p.osc[0] = osc("Basic Shapes", r.f(0.2, 0.4));
            p.osc[1] = WtOsc { octave: 2, level: r.f(0.1, 0.3), ..osc("Basic Shapes", 0.0) };
            p.env[0] = env(0.001, r.lg(0.2, 0.6), 0.0, r.lg(0.1, 0.3));
            ("Kalimba", &["Kalimba", "Thumb Piano", "Mbira", "Drop"])
        }
        3 => {
            p.osc[0] = osc(r.pick(&["Odd Even", "Basic Shapes"]), r.f(0.0, 0.5));
            p.filters[0] = WtFilter { keytrack: 1.0, ..filt(FilterKind::Lp12, r.lg(700.0, 2500.0), r.f(0.2, 0.4)) };
            p.matrix.push(slot(ModSource::Env(1), ModDest::FilterCutoff(0), r.f(0.2, 0.4)));
            ("Mallet", &["Marimba", "Mallet", "Wood", "Xylo", "Log Drum"])
        }
        4 => {
            p.osc[0] = WtOsc { unison: 2, detune: 0.1, ..osc("Harmonic Sweep", r.f(0.3, 0.9)) };
            p.filters[0] = filt(FilterKind::Lp12, r.lg(1500.0, 5000.0), 0.2);
            p.matrix.push(slot(ModSource::Env(1), ModDest::OscPos(0), r.f(0.3, 0.5)));
            p.env[0].release = r.lg(0.4, 1.2);
            ("Harp", &["Harp", "Strum", "Lyre", "Zither"])
        }
        _ => {
            p.osc[0] = WtOsc { unison: r.i(5, 9) as u32, detune: r.f(0.2, 0.35), ..osc("Basic Shapes", 0.66) };
            p.filters[0] = filt(FilterKind::Lp24, r.lg(500.0, 1500.0), 0.2);
            p.matrix.push(slot(ModSource::Env(1), ModDest::FilterCutoff(0), r.f(0.35, 0.55)));
            ("Future Pluck", &["Future Pluck", "Trance Pluck", "Tropical", "Pluck Stack"])
        }
    };
    space(r, &mut p, 0.6);
    std_macros(&mut p, r, ("Body", ModDest::OscLevel(0)));
    (res.0, res.1, p)
}

fn keys(r: &mut R, i: usize) -> Recipe {
    let amp = env(0.001, r.lg(0.6, 2.5), r.f(0.2, 0.6), r.lg(0.2, 0.6));
    let mut p = base(amp);
    p.polyphony = 16;
    p.velocity = r.f(0.5, 0.8);
    let res: (&str, &[&str]) = match i % 5 {
        0 => {
            p.osc[0] = osc("FM Bells", r.f(0.05, 0.3));
            p.osc[1] = WtOsc { level: 0.5, ..osc("Basic Shapes", 0.0) };
            p.env[1] = env(0.001, r.lg(0.3, 1.0), 0.0, 0.2);
            p.matrix.push(slot(ModSource::Env(1), ModDest::OscPos(0), r.f(0.1, 0.3)));
            p.matrix.push(slot(ModSource::Velocity, ModDest::OscPos(0), r.f(0.1, 0.2)));
            p.lfo[0] = free(LfoShape::Sine, r.f(3.0, 6.0));
            p.matrix.push(slot(ModSource::Lfo(0), ModDest::OscPan(0), r.f(0.2, 0.5)));
            ("Electric Piano", &["E-Piano", "Tine", "Suitcase", "Stage Keys", "EP"])
        }
        1 => {
            p.env[0] = env(0.005, 1.0, 1.0, r.lg(0.05, 0.2));
            p.osc[0] = osc("Organ", r.f(0.0, 1.0));
            p.lfo[0] = free(LfoShape::Sine, r.f(5.0, 7.0));
            p.matrix.push(slot(ModSource::Lfo(0), ModDest::Pitch, r.f(0.002, 0.005)));
            p.fx.push(fx("chorus", &[("rate", r.f(4.0, 7.0)), ("depth", 0.3), ("mix", 0.4)]));
            if r.chance(0.5) {
                p.fx.push(fx("saturator", &[("mode", 0.0), ("drive", r.f(3.0, 8.0))]));
            }
            ("Organ", &["Organ", "Drawbar", "Church", "B3", "Combo Organ"])
        }
        2 => {
            p.osc[0] = osc("Pulse Width", r.f(0.4, 0.9));
            p.filters[0] = filt(FilterKind::Hp12, r.lg(300.0, 900.0), 0.2);
            p.filters[1] = filt(FilterKind::Lp12, r.lg(2000.0, 6000.0), 0.2);
            p.env[0] = env(0.001, r.lg(0.3, 0.8), 0.1, 0.1);
            ("Clav", &["Clav", "Funk Keys", "Clavi", "Wah Keys"])
        }
        3 => {
            p.osc[0] = osc("Basic Shapes", r.f(0.15, 0.4));
            p.osc[1] = WtOsc { octave: 1, semi: r.pick(&[0, 7]), level: 0.25, ..osc("FM Bells", r.f(0.3, 0.6)) };
            p.env[0] = env(0.001, r.lg(0.3, 1.0), 0.0, 0.3);
            ("Toy Piano", &["Toy Piano", "Music Keys", "Celesta", "Tiny Keys"])
        }
        _ => {
            p.osc[0] = WtOsc { unison: 2, detune: 0.1, ..osc("Basic Shapes", r.f(0.5, 0.7)) };
            p.filters[0] = WtFilter { keytrack: 0.6, ..filt(FilterKind::Lp24, r.lg(600.0, 2000.0), 0.15) };
            p.env[1] = env(0.001, r.lg(0.3, 1.2), 0.2, 0.3);
            p.matrix.push(slot(ModSource::Env(1), ModDest::FilterCutoff(0), r.f(0.2, 0.4)));
            p.matrix.push(slot(ModSource::Velocity, ModDest::FilterCutoff(0), r.f(0.15, 0.3)));
            ("Poly Keys", &["Poly Keys", "Synth Piano", "Analog Keys", "Brass Keys"])
        }
    };
    space(r, &mut p, 0.5);
    std_macros(&mut p, r, ("Bark", ModDest::OscPos(0)));
    (res.0, res.1, p)
}

/// Bells: twelve kinds of struck metal and glass, each its own sound – not one bell in different rooms.
fn bells(r: &mut R, i: usize) -> Recipe {
    let sine = |pos: f32| osc("Basic Shapes", pos);
    let mut p;
    let res: (&str, &[&str]) = match i % 12 {
        0 => {
            // Tubular / church bell: deep, inharmonic (frequency modulation at an odd ratio), very long.
            p = base(env(0.001, r.lg(5.0, 8.0), 0.0, r.lg(3.0, 5.0)));
            p.osc[0] = WtOsc { octave: -1, warp: Warp::Fm, warp_amount: r.f(0.35, 0.55), ..osc("FM Bells", r.f(0.6, 0.9)) };
            p.osc[1] = WtOsc { semi: 15, fine: r.f(30.0, 45.0), level: 0.12, ..sine(0.0) };
            p.filters[0] = filt(FilterKind::Lp24, r.lg(2500.0, 4000.0), 0.1);
            p.fx.push(fx("reverb", &[("mode", 4.0), ("decay", r.f(6.0, 9.0)), ("mix", 0.42), ("size", 1.0)]));
            ("Church Bell", &["Church Bell", "Tubular", "Campanile", "Toll", "Belfry"])
        }
        1 => {
            // Glockenspiel: bright, high, short, a sharp mallet.
            p = base(env(0.001, r.lg(0.8, 1.4), 0.0, r.lg(0.6, 1.0)));
            p.osc[0] = WtOsc { octave: 1, ..sine(0.0) };
            p.osc[1] = WtOsc { semi: 31, fine: r.f(-15.0, 0.0), level: 0.3, ..sine(0.0) };
            p.env[1] = env(0.001, 0.08, 0.0, 0.05);
            p.matrix.push(slot(ModSource::Env(1), ModDest::OscLevel(1), 0.6));
            p.fx.push(fx("reverb", &[("mode", 3.0), ("decay", 1.8), ("mix", 0.22), ("size", 0.6)]));
            ("Glockenspiel", &["Glockenspiel", "Glock", "Mallet", "Orchestra Bells", "Lyre"])
        }
        2 => {
            // Music box: tiny, very short, ticking, with an echo.
            p = base(env(0.001, r.lg(0.3, 0.5), 0.0, 0.3));
            p.osc[0] = WtOsc { octave: 1, ..sine(r.f(0.0, 0.1)) };
            p.osc[1] = WtOsc { octave: 2, level: 0.4, ..sine(0.0) };
            p.filters[0] = filt(FilterKind::Hp12, r.lg(500.0, 900.0), 0.1);
            p.fx.push(fx("delay", &[("sync", 3.0), ("feedback", 0.3), ("mix", 0.2)]));
            p.fx.push(fx("reverb", &[("mode", 0.0), ("decay", 0.8), ("mix", 0.2), ("size", 0.3)]));
            ("Music Box", &["Music Box", "Clockwork", "Jewel Box", "Ballerina", "Wind-Up"])
        }
        3 => {
            // Crystal glass: glassy, shimmering, chorused, a bright plate.
            p = base(env(0.001, r.lg(2.5, 4.0), 0.0, r.lg(2.0, 3.0)));
            p.osc[0] = WtOsc { warp: Warp::Fm, warp_amount: r.f(0.6, 0.8), ..osc("FM Bells", r.f(0.2, 0.4)) };
            p.osc[1] = WtOsc { semi: 19, fine: r.f(5.0, 10.0), level: 0.2, ..sine(0.0) };
            p.filters[0] = filt(FilterKind::Hp12, r.lg(700.0, 1200.0), 0.1);
            p.fx.push(fx("chorus", &[("rate", 1.2), ("depth", 0.8), ("mix", 0.5)]));
            p.fx.push(fx("reverb", &[("mode", 3.0), ("decay", r.f(4.0, 6.0)), ("mix", 0.45), ("size", 0.9)]));
            ("Crystal", &["Crystal", "Wine Glass", "Prism", "Ice", "Diamond"])
        }
        4 => {
            // Singing bowl: blooms in, beats slowly, rings for ages.
            p = base(env(r.lg(0.06, 0.15), 8.0, 0.55, r.lg(5.0, 8.0)));
            p.osc[0] = WtOsc { unison: 2, detune: r.f(0.02, 0.04), ..sine(0.0) };
            p.osc[1] = WtOsc { semi: 24, fine: r.f(10.0, 20.0), level: 0.25, ..sine(0.0) };
            p.lfo[0] = free(LfoShape::Sine, r.lg(0.2, 0.5));
            p.matrix.push(slot(ModSource::Lfo(0), ModDest::OscLevel(1), 0.45));
            p.fx.push(fx("reverb", &[("mode", 2.0), ("decay", 5.0), ("mix", 0.35), ("size", 0.9)]));
            ("Singing Bowl", &["Singing Bowl", "Temple Bowl", "Om", "Meditation", "Monastery"])
        }
        5 => {
            // Gamelan: bronze, inharmonic, a little nasal.
            p = base(env(0.001, r.lg(1.6, 2.6), 0.0, r.lg(1.0, 1.8)));
            p.osc[0] = WtOsc { warp: Warp::Fm, warp_amount: r.f(0.4, 0.6), ..osc("Odd Even", r.f(0.4, 0.6)) };
            p.osc[1] = WtOsc { semi: 10, fine: r.f(20.0, 40.0), level: 0.35, ..sine(0.0) };
            p.filters[0] = filt(FilterKind::Bp, r.lg(1000.0, 2000.0), r.f(0.2, 0.35));
            p.fx.push(fx("reverb", &[("mode", 0.0), ("decay", 1.5), ("mix", 0.25), ("size", 0.5)]));
            ("Gamelan", &["Gamelan", "Gong", "Bonang", "Bronze", "Kempul"])
        }
        6 => {
            // Soft celesta: round, a felt mallet, the velocity opens it.
            p = base(env(r.lg(0.003, 0.006), r.lg(1.2, 1.8), 0.0, 1.0));
            p.osc[0] = osc("FM Bells", r.f(0.1, 0.2));
            p.filters[0] = filt(FilterKind::Lp12, r.lg(1800.0, 3000.0), 0.1);
            p.matrix.push(slot(ModSource::Velocity, ModDest::FilterCutoff(0), 0.3));
            p.fx.push(fx("reverb", &[("mode", 1.0), ("decay", 2.0), ("mix", 0.25), ("size", 0.6)]));
            ("Soft Celesta", &["Celesta", "Snow Keys", "Felt Bell", "Lullaby", "Nursery"])
        }
        7 => {
            // Vibraphone: pure, warm, the motor's tremolo.
            p = base(env(0.002, r.lg(2.5, 4.0), 0.0, r.lg(0.8, 1.4)));
            p.osc[0] = sine(0.0);
            p.osc[1] = WtOsc { semi: 24, level: 0.18, ..sine(0.0) };
            p.lfo[0] = free(LfoShape::Sine, r.f(4.5, 6.5));
            p.matrix.push(slot(ModSource::Lfo(0), ModDest::Master, r.f(0.25, 0.4)));
            p.fx.push(fx("reverb", &[("mode", 0.0), ("decay", 1.2), ("mix", 0.22), ("size", 0.5)]));
            ("Vibraphone", &["Vibes", "Vibraphone", "Lounge Vibes", "Motor Vibes", "Cool Vibes"])
        }
        8 => {
            // Wind chimes: sparkling, every strike a little different, echoes far away.
            p = base(env(0.001, r.lg(2.0, 3.0), 0.0, 2.5));
            p.osc[0] = WtOsc { octave: 1, ..osc("FM Bells", r.f(0.8, 1.0)) };
            p.lfo[0] = synced(LfoShape::Random, 0.25);
            p.matrix.push(slot(ModSource::Lfo(0), ModDest::OscPos(0), 0.4));
            p.matrix.push(slot(ModSource::Random, ModDest::OscPan(0), 0.8));
            p.fx.push(fx("delay", &[("sync", 6.0), ("feedback", 0.55), ("mix", 0.35), ("width", 1.0)]));
            p.fx.push(fx("reverb", &[("mode", 4.0), ("decay", 7.0), ("mix", 0.45), ("size", 1.0)]));
            ("Wind Chimes", &["Wind Chimes", "Sparkle", "Fairy Dust", "Starfall", "Breeze"])
        }
        9 => {
            // Dark FM bell: folded, filtered low, a little tape grit.
            p = base(env(0.001, r.lg(3.0, 5.0), 0.0, r.lg(2.0, 3.5)));
            p.osc[0] = WtOsc { warp: Warp::Fold, warp_amount: r.f(0.25, 0.4), ..osc("Fold", r.f(0.3, 0.5)) };
            p.osc[1] = WtOsc { octave: -1, semi: 7, level: 0.3, ..sine(0.0) };
            p.filters[0] = filt(FilterKind::Lp24, r.lg(600.0, 1100.0), 0.2);
            p.env[1] = env(0.001, 0.4, 0.0, 0.3);
            p.matrix.push(slot(ModSource::Env(1), ModDest::FilterCutoff(0), 0.35));
            p.fx.push(fx("saturator", &[("mode", 1.0), ("drive", 6.0), ("mix", 0.5)]));
            p.fx.push(fx("reverb", &[("mode", 2.0), ("decay", 4.0), ("mix", 0.3), ("size", 0.8)]));
            ("Dark Bell", &["Dark Bell", "Cloister", "Iron Bell", "Dungeon", "Requiem"])
        }
        10 => {
            // 8-bit bell: stepped, crushed, a game's pickup sound.
            p = base(env(0.001, r.lg(0.5, 0.9), 0.0, 0.4));
            p.osc[0] = osc("Digital Steps", r.f(0.4, 0.7));
            p.osc[1] = WtOsc { octave: 1, level: 0.35, ..osc("Pulse Width", 0.5) };
            p.fx.push(fx("bitcrusher", &[("bits", r.f(5.0, 7.0)), ("downsample", 2.0), ("mix", 0.8)]));
            p.fx.push(fx("delay", &[("sync", 5.0), ("feedback", 0.35), ("mix", 0.2)]));
            ("8-Bit Bell", &["Chip Bell", "Coin", "Pixel", "Arcade", "Power-Up"])
        }
        _ => {
            // Reverse bell: swells in from nothing and stops.
            p = base(env(r.lg(0.8, 1.5), 0.3, 0.0, 0.25));
            p.osc[0] = WtOsc { warp: Warp::Fm, warp_amount: 0.4, ..osc("FM Bells", r.f(0.5, 0.8)) };
            p.osc[1] = WtOsc { semi: 12, level: 0.3, ..sine(0.0) };
            p.fx.push(fx("reverb", &[("mode", 4.0), ("decay", 6.0), ("mix", 0.5), ("size", 1.0)]));
            ("Reverse Bell", &["Reverse", "Backwards", "Inhale", "Rewind", "Swell"])
        }
    };
    p.polyphony = 16;
    std_macros(&mut p, r, ("Metal", ModDest::OscLevel(1)));
    (res.0, res.1, p)
}

fn arp(r: &mut R, i: usize) -> Recipe {
    let amp = env(0.001, r.lg(0.08, 0.3), r.f(0.0, 0.2), r.lg(0.05, 0.2));
    let mut p = base(amp);
    p.polyphony = 8;
    let res: (&str, &[&str]) = match i % 3 {
        0 => {
            // A gated (trance-gate style) chord sound.
            p.env[0] = env(0.002, 0.5, 1.0, 0.2);
            p.osc[0] = WtOsc { unison: r.i(3, 7) as u32, detune: 0.25, ..osc("Basic Shapes", r.pick(&[0.66, 1.0])) };
            p.filters[0] = filt(FilterKind::Lp24, r.lg(300.0, 900.0), r.f(0.2, 0.4));
            p.lfo[0] = WtLfo { unipolar: true, ..synced(r.pick(&[LfoShape::Square, LfoShape::SawDown]), r.pick(&[0.25, 0.5])) };
            p.matrix.push(slot(ModSource::Lfo(0), ModDest::FilterCutoff(0), r.f(0.35, 0.55)));
            ("Gate", &["Gate", "Trance Gate", "Chopper", "Stutter", "Slicer"])
        }
        1 => {
            p.osc[0] = osc(r.pick(&["Pulse Width", "Digital Steps", "Basic Shapes"]), r.f(0.3, 1.0));
            p.filters[0] = filt(FilterKind::Lp24, r.lg(400.0, 1500.0), r.f(0.2, 0.5));
            p.env[1] = env(0.001, r.lg(0.05, 0.2), 0.0, 0.1);
            p.matrix.push(slot(ModSource::Env(1), ModDest::FilterCutoff(0), r.f(0.3, 0.5)));
            p.lfo[0] = synced(LfoShape::Random, r.pick(&[0.25, 0.5]));
            p.matrix.push(slot(ModSource::Lfo(0), ModDest::OscPos(0), r.f(0.15, 0.35)));
            ("Sequence", &["Sequence", "Seq", "Arp", "Pattern", "Ostinato"])
        }
        _ => {
            p.osc[0] = WtOsc { unison: 2, ..osc(r.pick(&["Sync Sweep", "Harmonic Sweep", "FM Bells"]), r.f(0.2, 0.8)) };
            p.lfo[0] = synced(r.pick(&[LfoShape::SawUp, LfoShape::Triangle, LfoShape::Square]), r.pick(&[0.25, 0.5, 1.0 / 3.0]));
            p.matrix.push(slot(ModSource::Lfo(0), ModDest::OscPos(0), r.f(0.3, 0.6)));
            p.filters[0] = filt(FilterKind::Lp12, r.lg(2000.0, 7000.0), 0.15);
            ("Pulse", &["Pulse", "Bubbles", "Ripple", "Ticker", "Morse"])
        }
    };
    p.fx.push(fx("delay", &[("sync", r.pick(&[5.0, 6.0, 4.0])), ("feedback", r.f(0.3, 0.6)), ("mix", r.f(0.15, 0.3)), ("mode", 1.0)]));
    space(r, &mut p, 0.4);
    std_macros(&mut p, r, ("Rate", ModDest::LfoRate(0)));
    (res.0, res.1, p)
}

fn stab(r: &mut R, i: usize) -> Recipe {
    let amp = env(0.001, r.lg(0.15, 0.6), r.f(0.0, 0.3), r.lg(0.1, 0.4));
    let mut p = base(amp);
    p.polyphony = 12;
    let chord: [i32; 2] = r.pick(&[[3, 7], [4, 7], [5, 7], [3, 10], [4, 11], [7, 12], [3, 8]]);
    let res: (&str, &[&str]) = match i % 4 {
        0 => {
            p.osc[0] = osc(r.pick(&["Organ", "Basic Shapes"]), r.f(0.0, 1.0));
            p.osc[1] = WtOsc { semi: chord[0], level: 0.6, ..p.osc[0].clone() };
            p.osc[2] = WtOsc { semi: chord[1], level: 0.6, ..p.osc[0].clone() };
            ("Chord Stab", &["Chord", "Chord Stab", "Minor Stab", "House Chord", "Deep Chord"])
        }
        1 => {
            p.osc[0] = WtOsc { unison: r.i(3, 7) as u32, detune: 0.2, ..osc("Basic Shapes", 0.66) };
            p.osc[1] = WtOsc { unison: 3, semi: chord[1], level: 0.5, ..osc("Basic Shapes", 0.66) };
            p.filters[0] = filt(FilterKind::Lp24, r.lg(600.0, 1800.0), 0.25);
            p.env[1] = env(r.lg(0.01, 0.06), r.lg(0.1, 0.4), 0.2, 0.1);
            p.matrix.push(slot(ModSource::Env(1), ModDest::FilterCutoff(0), r.f(0.25, 0.45)));
            ("Brass", &["Brass", "Horn", "Brass Stab", "Fanfare", "Section"])
        }
        2 => {
            p.osc[0] = WtOsc { unison: r.i(5, 9) as u32, detune: 0.3, ..osc("Basic Shapes", r.pick(&[0.66, 1.0])) };
            p.osc[1] = WtOsc { unison: 5, semi: chord[0], detune: 0.3, level: 0.6, ..osc("Basic Shapes", 0.66) };
            p.osc[2] = WtOsc { unison: 5, semi: chord[1], detune: 0.3, level: 0.6, ..osc("Basic Shapes", 0.66) };
            p.filters[0] = filt(FilterKind::Lp12, r.lg(2000.0, 8000.0), 0.15);
            ("Rave Stab", &["Rave Stab", "Hoover Stab", "Hit", "Rave Chord", "Euphoria"])
        }
        _ => {
            p.osc[0] = osc(r.pick(&["Pulse Width", "Odd Even", "Digital Steps"]), r.f(0.2, 0.9));
            p.osc[1] = WtOsc { semi: chord[0], level: 0.6, ..p.osc[0].clone() };
            p.osc[2] = WtOsc { semi: chord[1], level: 0.6, ..p.osc[0].clone() };
            p.filters[0] = filt(FilterKind::Bp, r.lg(700.0, 2500.0), 0.3);
            p.filters[0].mix = 0.7;
            ("Garage Stab", &["Garage", "Organ Stab", "Skank", "Dub Chord"])
        }
    };
    space(r, &mut p, 0.6);
    std_macros(&mut p, r, ("Spread", ModDest::OscPan(1)));
    (res.0, res.1, p)
}

fn atmos(r: &mut R, i: usize) -> Recipe {
    let amp = env(r.lg(1.0, 5.0), 2.0, 1.0, r.lg(2.0, 6.0));
    let mut p = base(amp);
    p.polyphony = 8;
    p.velocity = 0.2;
    let res: (&str, &[&str]) = match i % 4 {
        0 => {
            p.osc[0] = WtOsc { octave: -1, unison: 3, ..osc(r.pick(&["Harmonic Sweep", "Noise Spectra", "Fold"]), r.f(0.0, 1.0)) };
            p.lfo[0] = free(LfoShape::Smooth, r.lg(0.02, 0.1));
            p.matrix.push(slot(ModSource::Lfo(0), ModDest::OscPos(0), r.f(0.3, 0.6)));
            p.filters[0] = filt(FilterKind::Lp24, r.lg(400.0, 1500.0), r.f(0.2, 0.4));
            ("Drone", &["Drone", "Hum", "Monolith", "Rumble", "Engine Room"])
        }
        1 => {
            p.osc[0].on = false;
            p.noise = NoiseOsc { on: true, color: r.f(0.2, 0.8), level: r.f(0.4, 0.6), ..NoiseOsc::default() };
            p.filters[0] = filt(r.pick(&[FilterKind::Bp, FilterKind::CombPlus, FilterKind::Formant]), r.lg(300.0, 3000.0), r.f(0.4, 0.7));
            p.filters[0].keytrack = 1.0;
            p.lfo[0] = free(LfoShape::Smooth, r.lg(0.05, 0.3));
            p.matrix.push(slot(ModSource::Lfo(0), ModDest::FilterCutoff(0), r.f(0.2, 0.5)));
            ("Wind", &["Wind", "Ocean", "Breath", "Whisper", "Storm"])
        }
        2 => {
            p.osc[0] = WtOsc { warp: Warp::Crush, warp_amount: r.f(0.2, 0.6), ..osc(r.pick(&["Digital Steps", "Noise Spectra", "Sync Sweep"]), r.f(0.0, 1.0)) };
            p.lfo[0] = free(LfoShape::Random, r.lg(1.0, 8.0));
            p.matrix.push(slot(ModSource::Lfo(0), ModDest::OscPos(0), r.f(0.2, 0.5)));
            p.filters[0] = filt(FilterKind::Bp, r.lg(500.0, 3000.0), r.f(0.2, 0.4));
            p.fx.push(fx("bitcrusher", &[("bits", r.f(6.0, 12.0)), ("mix", 0.4)]));
            ("Texture", &["Texture", "Grain", "Static", "Particles", "Circuitry"])
        }
        _ => {
            p.osc[0] = WtOsc { unison: r.i(3, 6) as u32, detune: r.f(0.3, 0.6), ..osc(r.pick(&ALL_TABLES), r.f(0.0, 1.0)) };
            p.osc[1] = WtOsc { semi: r.pick(&[7, 12, 19, -12]), unison: 3, level: 0.4, ..osc(r.pick(&ALL_TABLES), r.f(0.0, 1.0)) };
            p.lfo[0] = free(LfoShape::Smooth, r.lg(0.03, 0.15));
            p.lfo[2] = free(LfoShape::Sine, r.lg(0.02, 0.1));
            p.matrix.push(slot(ModSource::Lfo(0), ModDest::OscPos(0), 0.4));
            p.matrix.push(slot(ModSource::Lfo(2), ModDest::OscPos(1), 0.4));
            p.filters[0] = filt(FilterKind::Lp12, r.lg(1500.0, 6000.0), 0.15);
            ("Soundscape", &["Soundscape", "Horizon", "Space", "Dreamscape", "Vista"])
        }
    };
    space(r, &mut p, 1.0);
    std_macros(&mut p, r, ("Motion", ModDest::LfoRate(0)));
    (res.0, res.1, p)
}

fn sfx(r: &mut R, i: usize) -> Recipe {
    let mut p = base(env(0.01, 1.0, 1.0, r.lg(0.3, 2.0)));
    p.polyphony = 4;
    let res: (&str, &[&str]) = match i % 6 {
        0 | 1 => {
            // Riser / downlifter: a one-shot LFO as a long ramp.
            let up = i.is_multiple_of(6);
            p.noise = NoiseOsc { on: true, color: 0.6, level: r.f(0.3, 0.5), ..NoiseOsc::default() };
            p.osc[0] = WtOsc { unison: 5, detune: 0.3, level: r.f(0.3, 0.6), ..osc("Basic Shapes", 0.66) };
            p.filters[0] = filt(FilterKind::Lp24, r.lg(300.0, 800.0), r.f(0.3, 0.5));
            p.lfo[0] = WtLfo { mode: LfoMode::OneShot, unipolar: true, ..synced(if up { LfoShape::SawUp } else { LfoShape::SawDown }, r.pick(&BEATS_SLOW)) };
            p.matrix.push(slot(ModSource::Lfo(0), ModDest::FilterCutoff(0), r.f(0.4, 0.6)));
            p.matrix.push(slot(ModSource::Lfo(0), ModDest::Pitch, r.f(0.2, 0.5)));
            if up {
                ("Riser", &["Riser", "Uplifter", "Build", "Ascend", "Takeoff"])
            } else {
                ("Downlifter", &["Downlifter", "Fall", "Drop", "Descent", "Crash Down"])
            }
        }
        2 => {
            p.env[0] = env(0.001, r.lg(0.1, 0.5), 0.0, 0.1);
            p.osc[0] = osc(r.pick(&["Basic Shapes", "Sync Sweep", "FM Bells"]), r.f(0.3, 1.0));
            p.env[1] = env(0.001, r.lg(0.05, 0.3), 0.0, 0.05);
            p.matrix.push(slot(ModSource::Env(1), ModDest::Pitch, r.f(0.6, 1.0)));
            ("Zap", &["Zap", "Laser", "Pew", "Blaster", "Phaser Shot"])
        }
        3 => {
            p.osc[0] = osc(r.pick(&["Basic Shapes", "Pulse Width"]), r.f(0.0, 1.0));
            p.lfo[0] = synced(r.pick(&[LfoShape::Sine, LfoShape::Triangle, LfoShape::Square]), r.pick(&[0.5, 1.0, 2.0]));
            p.matrix.push(slot(ModSource::Lfo(0), ModDest::Pitch, r.f(0.15, 0.3)));
            ("Siren", &["Siren", "Alarm", "Klaxon", "Signal"])
        }
        4 => {
            p.osc[0] = WtOsc { warp: Warp::Crush, warp_amount: r.f(0.3, 0.7), ..osc(r.pick(&["Digital Steps", "Noise Spectra", "Sync Sweep"]), r.f(0.0, 1.0)) };
            p.lfo[0] = synced(LfoShape::Random, r.pick(&[0.125, 0.25]));
            p.matrix.push(slot(ModSource::Lfo(0), ModDest::Pitch, r.f(0.1, 0.4)));
            p.matrix.push(slot(ModSource::Lfo(0), ModDest::OscPos(0), 0.5));
            p.fx.push(fx("bitcrusher", &[("bits", r.f(4.0, 8.0)), ("downsample", r.f(2.0, 8.0)), ("mix", 0.6)]));
            ("Glitch", &["Glitch", "Bleeps", "Computer", "Data", "Bug"])
        }
        _ => {
            p.env[0] = env(0.001, r.lg(0.8, 2.5), 0.0, 0.5);
            p.noise = NoiseOsc { on: true, color: r.f(0.3, 0.7), level: 0.5, ..NoiseOsc::default() };
            p.sub = SubOsc { on: true, level: 0.7, ..SubOsc::default() };
            p.osc[0].on = false;
            p.filters[0] = filt(FilterKind::Lp24, r.lg(2000.0, 6000.0), 0.2);
            p.env[1] = env(0.001, r.lg(0.2, 0.8), 0.0, 0.2);
            p.matrix.push(slot(ModSource::Env(1), ModDest::FilterCutoff(0), 0.4));
            p.matrix.push(slot(ModSource::Env(1), ModDest::Pitch, r.f(0.2, 0.5)));
            ("Impact", &["Impact", "Boom", "Hit", "Slam", "Thunder"])
        }
    };
    space(r, &mut p, 0.8);
    std_macros(&mut p, r, ("Pitch", ModDest::Pitch));
    (res.0, res.1, p)
}

fn drums(r: &mut R, i: usize) -> Recipe {
    let mut p = base(env(0.001, 0.4, 0.0, 0.1));
    p.polyphony = 4;
    p.velocity = 0.8;
    let res: (&str, &[&str]) = match i % 5 {
        0 => {
            p.env[0] = env(0.001, r.lg(0.25, 0.9), 0.0, 0.1);
            p.osc[0] = WtOsc { random_phase: 0.0, ..osc("Basic Shapes", r.f(0.0, 0.15)) };
            p.env[1] = env(0.001, r.lg(0.02, 0.08), 0.0, 0.05);
            p.matrix.push(slot(ModSource::Env(1), ModDest::Pitch, r.f(0.7, 1.0)));
            p.fx.push(fx("saturator", &[("drive", r.f(3.0, 12.0))]));
            ("Kick", &["Kick", "Bass Drum", "Thud", "Kick 909", "Boom Kick"])
        }
        1 => {
            p.env[0] = env(0.001, r.lg(0.2, 0.6), 0.0, 0.1);
            p.osc[0] = WtOsc { random_phase: 0.0, ..osc("Basic Shapes", r.f(0.0, 0.3)) };
            p.env[1] = env(0.001, r.lg(0.05, 0.15), 0.0, 0.05);
            p.matrix.push(slot(ModSource::Env(1), ModDest::Pitch, r.f(0.2, 0.45)));
            ("Tom", &["Tom", "Floor Tom", "Conga", "Tabla", "Bongo"])
        }
        2 => {
            p.env[0] = env(0.001, r.lg(0.12, 0.35), 0.0, 0.1);
            p.osc[0] = WtOsc { level: 0.5, random_phase: 0.0, ..osc("Basic Shapes", 0.33) };
            p.noise = NoiseOsc { on: true, color: r.f(0.5, 0.8), level: 0.6, ..NoiseOsc::default() };
            p.filters[0] = filt(FilterKind::Hp12, r.lg(150.0, 400.0), 0.1);
            p.env[1] = env(0.001, 0.03, 0.0, 0.03);
            p.matrix.push(slot(ModSource::Env(1), ModDest::Pitch, 0.3));
            p.fx.push(fx("reverb", &[("mode", 0.0), ("decay", 0.6), ("mix", r.f(0.05, 0.2))]));
            ("Snare", &["Snare", "Clap", "Rim", "Snap", "Crack"])
        }
        3 => {
            p.env[0] = env(0.001, r.lg(0.04, 0.4), 0.0, r.lg(0.03, 0.2));
            p.osc[0].on = false;
            p.noise = NoiseOsc { on: true, color: r.f(0.7, 1.0), level: 0.7, ..NoiseOsc::default() };
            p.filters[0] = filt(FilterKind::Hp24, r.lg(5000.0, 10000.0), 0.2);
            ("Hat", &["Hat", "Hi-Hat", "Open Hat", "Shaker", "Tick"])
        }
        _ => {
            p.env[0] = env(0.001, r.lg(0.1, 0.6), 0.0, 0.2);
            p.osc[0] = osc("FM Bells", r.f(0.5, 1.0));
            p.osc[1] = WtOsc { semi: r.i(5, 30), level: 0.5, ..osc("FM Bells", r.f(0.5, 1.0)) };
            p.filters[0] = filt(FilterKind::Hp12, r.lg(400.0, 2000.0), 0.2);
            ("Metal Perc", &["Cowbell", "Clank", "Anvil", "Metal Hit", "Perc"])
        }
    };
    macros(&mut p, ["Pitch", "Tone", "Drive", "Snap"], [Some((ModDest::Pitch, 0.25)), Some((ModDest::OscPos(0), 0.4)), Some((ModDest::OscWarp(0), 0.4)), Some((ModDest::NoiseLevel, 0.3))]);
    p.osc[0].warp = if p.osc[0].warp == Warp::Off { Warp::Fold } else { p.osc[0].warp };
    (res.0, res.1, p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_library_has_over_two_thousand_unique_presets() {
        let lib = library();
        assert!(lib.len() > 2000, "{}", lib.len());
        let names: HashSet<String> = lib.iter().map(|p| p.name.to_lowercase()).collect();
        assert_eq!(names.len(), lib.len(), "names are unique");
        for c in Category::ALL {
            assert!(lib.iter().any(|p| p.category == c), "{c:?}");
        }
        // The same library every time.
        assert_eq!(build()[500].name, lib[500].name);
        for p in lib {
            let w = &p.params;
            assert!(w.master.is_finite() && w.master > 0.0, "{}", p.name);
            assert!(w.osc.iter().all(|o| o.unison >= 1 && o.unison <= 16 && o.pos >= 0.0 && o.pos <= 1.0), "{}", p.name);
            assert!(w.filters.iter().all(|f| f.cutoff >= 20.0 && f.cutoff <= 20000.0 && f.resonance <= 1.0), "{}", p.name);
            assert!(w.osc.iter().any(|o| o.on) || w.noise.on || w.sub.on, "{} makes sound", p.name);
            assert_eq!(w.preset, p.name);
        }
    }

    #[test]
    fn categories_cycle_through_distinct_characters() {
        // The first presets of a category are different kinds of sound, not one sound varied.
        let kinds: HashSet<&str> = library().iter().filter(|p| p.category == Category::Bells).take(24).map(|p| p.kind).collect();
        assert!(kinds.len() >= 8, "Bells: only {} kinds: {kinds:?}", kinds.len());
    }
}
