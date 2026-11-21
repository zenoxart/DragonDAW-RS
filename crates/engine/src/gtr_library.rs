//! The Drakestring preset library: playing styles and instruments built from recipes (a
//! fingerpicked steel-string, a strummed 12-string, a crunchy electric, a slap-free finger bass …),
//! each varied in musical ranges with a fixed seed. Levels come from [`LEVELS`] (measured).

use crate::dsp::Rng;
use crate::gtr::*;
use crate::wt::FxSlot;
use std::collections::HashSet;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    Acoustic,
    Nylon,
    TwelveString,
    Clean,
    Drive,
    Bass,
    Rhythm,
    Banjo,
    Mandolin,
    Ukulele,
    World,
    Harp,
}

impl Category {
    pub const ALL: [Category; 12] = [
        Category::Acoustic,
        Category::Nylon,
        Category::TwelveString,
        Category::Clean,
        Category::Drive,
        Category::Bass,
        Category::Rhythm,
        Category::Banjo,
        Category::Mandolin,
        Category::Ukulele,
        Category::World,
        Category::Harp,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Category::Acoustic => "Steel Acoustic",
            Category::Nylon => "Nylon & Classical",
            Category::TwelveString => "12-String",
            Category::Clean => "Electric Clean",
            Category::Drive => "Electric Drive",
            Category::Bass => "Bass",
            Category::Rhythm => "Rhythm Patterns",
            Category::Banjo => "Banjo",
            Category::Mandolin => "Mandolin",
            Category::Ukulele => "Ukulele",
            Category::World => "Sitar & World",
            Category::Harp => "Harp",
        }
    }
    pub fn code(self) -> &'static str {
        match self {
            Category::Acoustic => "AC",
            Category::Nylon => "NY",
            Category::TwelveString => "12",
            Category::Clean => "EC",
            Category::Drive => "ED",
            Category::Bass => "BS",
            Category::Rhythm => "RH",
            Category::Banjo => "BJ",
            Category::Mandolin => "MD",
            Category::Ukulele => "UK",
            Category::World => "WD",
            Category::Harp => "HP",
        }
    }
    fn count(self) -> usize {
        match self {
            Category::Acoustic => 330,
            Category::Nylon => 190,
            Category::TwelveString => 140,
            Category::Clean => 220,
            Category::Drive => 220,
            Category::Bass => 200,
            Category::Rhythm => 200,
            Category::Banjo => 80,
            Category::Mandolin => 100,
            Category::Ukulele => 130,
            Category::World => 110,
            Category::Harp => 90,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LibPreset {
    pub name: String,
    pub category: Category,
    /// The playing style ("Fingerstyle", "Strummed" …), searchable.
    pub kind: &'static str,
    pub params: GuitarParams,
}

pub fn library() -> &'static [LibPreset] {
    static LIB: OnceLock<Vec<LibPreset>> = OnceLock::new();
    LIB.get_or_init(build)
}

pub fn find(name: &str) -> Option<&'static LibPreset> {
    library().iter().find(|p| p.name == name)
}

/// What a preset is auditioned (and measured) with: notes to hold.
pub fn audition(p: &GuitarParams) -> Vec<u8> {
    let lo = p.tuning.iter().copied().min().unwrap_or(40);
    match p.mode {
        PlayMode::Solo => vec![lo + 12],
        // A chord a little above the lowest string.
        _ => {
            let r = lo + 8;
            vec![r, r + 4, r + 7]
        }
    }
}

/// Measured level corrections (dB), in library order. Regenerate with
/// `cargo test --release -p dragon-engine measure_guitar_levels -- --ignored`.
pub(crate) const LEVELS: &[f32] = &include!("gtr_levels.in");

fn build() -> Vec<LibPreset> {
    let mut out = Vec::new();
    let mut used: HashSet<String> = HashSet::new();
    for cat in Category::ALL {
        for i in 0..cat.count() {
            let mut r = R(Rng::new(0xD1A6_0057_0000_0001 ^ ((cat as u64 + 1) << 40) ^ (i as u64).wrapping_mul(0x9E37_79B9)));
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
            p.trim_db = LEVELS.get(out.len()).copied().unwrap_or(0.0);
            p.preset = name.clone();
            out.push(LibPreset { name, category: cat, kind, params: p });
        }
    }
    out
}

fn roman(n: usize) -> &'static str {
    ["", "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI", "XII"].get(n).copied().unwrap_or("XX")
}

struct R(Rng);

impl R {
    fn f(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.0.next_f32()
    }
    fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        xs[(self.0.next_u64() % xs.len() as u64) as usize]
    }
    fn chance(&mut self, p: f32) -> bool {
        self.0.next_f32() < p
    }
}

fn fx(id: &str, params: &[(&str, f32)]) -> FxSlot {
    FxSlot { id: id.into(), on: true, params: params.iter().map(|(k, v)| (k.to_string(), *v)).collect() }
}

/// Room, delay and chorus to taste (`amount` 0 … 1 = how roomy).
fn space(r: &mut R, p: &mut GuitarParams, amount: f32, chorus: f32) {
    if r.chance(chorus) {
        p.fx.push(fx("chorus", &[("rate", r.f(0.3, 1.2)), ("depth", r.f(0.2, 0.5)), ("mix", r.f(0.2, 0.45))]));
    }
    if r.chance(0.15 + amount * 0.5) {
        p.fx.push(fx("delay", &[("sync", r.pick(&[5.0, 6.0, 7.0])), ("feedback", r.f(0.15, 0.45)), ("mix", r.f(0.06, 0.1 + amount * 0.15)), ("highcut", r.f(2500.0, 7000.0))]));
    }
    if r.chance(0.35 + amount * 0.6) {
        p.fx.push(fx("reverb", &[("mode", r.pick(&[0.0, 1.0, 2.0, 3.0])), ("decay", r.f(0.6, 1.2 + amount * 3.5)), ("mix", r.f(0.08, 0.12 + amount * 0.22)), ("size", r.f(0.3, 0.9))]));
    }
}

fn tuning_for(r: &mut R, model: GuitarModel, alt_chance: f32) -> (Vec<u8>, &'static str) {
    if matches!(model, GuitarModel::SteelAcoustic | GuitarModel::TwelveString | GuitarModel::Electric | GuitarModel::Nylon) && r.chance(alt_chance) {
        let (name, t) = r.pick(&TUNINGS[1..8]);
        (t.to_vec(), name)
    } else {
        (model.default_tuning(), "")
    }
}

fn adjectives(cat: Category) -> &'static [&'static str] {
    match cat {
        Category::Acoustic => &[
            "Warm",
            "Bright",
            "Sunset",
            "Campfire",
            "Folk",
            "Spruce",
            "Dusty",
            "Golden",
            "Morning",
            "Porch",
            "Studio",
            "Vintage",
            "Open",
            "Gentle",
            "Crisp",
            "Mellow",
            "Road",
            "Prairie",
            "Coffeehouse",
            "Rustic",
            "Big",
            "Sweet",
            "Honest",
            "Wooden",
            "Late Night",
            "Highway",
            "Cabin",
            "Summer",
            "Autumn",
            "Delta",
            "Rainy",
            "Quiet",
        ],
        Category::Nylon => {
            &["Spanish", "Classical", "Soft", "Velvet", "Bossa", "Tender", "Concert", "Moonlit", "Warm", "Gut", "Old World", "Serenade", "Romance", "Plaza", "Andalusian", "Silk", "Night", "Candle", "Sevilla", "Cedar"]
        }
        Category::TwelveString => &["Shimmering", "Jangly", "Big", "Chiming", "Ringing", "Bright", "Sixties", "Wide", "Silver", "Choral", "Cathedral", "Sparkling", "Rich", "Lush", "Golden", "Sunny"],
        Category::Clean => {
            &["Glassy", "Clean", "Funky", "Surf", "Jazzy", "Spanky", "Sparkle", "Neo Soul", "Velvet", "Twangy", "Chorus", "Dreamy", "Indie", "Motown", "Smooth", "Liquid", "Pristine", "Bell", "Chicken", "Lounge"]
        }
        Category::Drive => {
            &["Crunchy", "Gritty", "Fuzzy", "Screaming", "Blues", "Overdriven", "Hot", "Stack", "Garage", "Grunge", "Arena", "Vintage", "Raw", "Heavy", "Searing", "Burning", "Smoky", "Rebel", "Thick", "Saturated"]
        }
        Category::Bass => &["Deep", "Round", "Punchy", "Motown", "Fat", "Warm", "Funky", "Tight", "Dub", "Growling", "Finger", "Pick", "Muted", "Vintage", "Modern", "Rumbling", "Solid", "Walking", "Bouncy", "Low"],
        Category::Rhythm => &["Strumming", "Driving", "Campfire", "Pop", "Folk", "Island", "Country", "Busy", "Lazy", "Sunday", "Road Trip", "Happy", "Steady", "Gallop", "Swinging", "Feelgood", "Summer", "Uplifting"],
        Category::Banjo => &["Bluegrass", "Mountain", "Clawhammer", "Bright", "Old Time", "Front Porch", "Twangy", "Hillside", "Rolling", "Plucky"],
        Category::Mandolin => &["Italian", "Bluegrass", "Celtic", "Tremolo", "Bright", "Sweet", "Folk", "Napoli", "Irish", "Chiming", "Mountain"],
        Category::Ukulele => &["Island", "Happy", "Sunny", "Tiny", "Beach", "Hawaiian", "Soft", "Bright", "Tropical", "Cheerful", "Summer", "Pocket", "Breezy"],
        Category::World => &["Raga", "Temple", "Desert", "Silk Road", "Mystic", "Ancient", "Eastern", "Bazaar", "Monsoon", "Meditative", "Drone", "Spice"],
        Category::Harp => &["Celestial", "Concert", "Angelic", "Celtic", "Fairy", "Crystal", "Gentle", "Dream", "Golden", "Moonlight", "Elven"],
    }
}

type Recipe = (&'static str, &'static [&'static str], GuitarParams);

fn recipe(cat: Category, r: &mut R, i: usize) -> Recipe {
    match cat {
        Category::Acoustic => acoustic(r, i, GuitarModel::SteelAcoustic),
        Category::Nylon => nylon(r, i),
        Category::TwelveString => acoustic(r, i, GuitarModel::TwelveString),
        Category::Clean => electric(r, i, false),
        Category::Drive => electric(r, i, true),
        Category::Bass => bass(r, i),
        Category::Rhythm => rhythm(r, i),
        Category::Banjo => small(r, i, GuitarModel::Banjo),
        Category::Mandolin => small(r, i, GuitarModel::Mandolin),
        Category::Ukulele => small(r, i, GuitarModel::Ukulele),
        Category::World => world(r, i),
        Category::Harp => harp(r),
    }
}

fn common(r: &mut R, p: &mut GuitarParams) {
    p.humanize = r.f(0.15, 0.5);
    p.noise = r.f(0.1, 0.5);
    p.width = r.f(0.4, 0.9);
    p.velocity = r.f(0.4, 0.8);
}

fn acoustic(r: &mut R, i: usize, model: GuitarModel) -> Recipe {
    let mut p = GuitarParams::for_model(model);
    common(r, &mut p);
    let (t, _) = tuning_for(r, model, 0.18);
    p.tuning = t;
    if r.chance(0.2) {
        p.capo = r.pick(&[2, 3, 4, 5, 7]);
    }
    p.body = r.f(0.5, 0.95);
    p.body_size = r.f(0.75, 1.25);
    p.sustain = r.f(4.0, 9.0);
    if model == GuitarModel::TwelveString {
        p.doubling = r.f(0.55, 0.85);
        p.doubling_detune = r.f(2.0, 7.0);
    }
    let res: (&str, &[&str]) = match i % 6 {
        0 => {
            p.mode = PlayMode::Solo;
            p.pick_hardness = r.f(0.15, 0.4);
            p.pick_position = r.f(0.15, 0.3);
            p.brightness = r.f(0.4, 0.65);
            ("Fingerstyle", &["Fingerstyle", "Fingerpick", "Ballad", "Lullaby", "Travis"])
        }
        1 => {
            p.mode = PlayMode::Strum;
            p.strum_time = r.f(8.0, 22.0);
            p.pick_hardness = r.f(0.55, 0.9);
            p.brightness = r.f(0.55, 0.8);
            ("Strummed", &["Strum", "Strummer", "Chords", "Rhythm", "Jangle"])
        }
        2 => {
            p.mode = PlayMode::Chord;
            p.strum_time = r.f(10.0, 30.0);
            p.strum_dir = r.pick(&[StrumDir::Down, StrumDir::Alternate]);
            p.pick_hardness = r.f(0.5, 0.85);
            ("Chord Strummer", &["Chord Strum", "Easy Chords", "One Finger", "Chord Player"])
        }
        3 => {
            p.mode = PlayMode::Solo;
            p.articulation = Articulation::PalmMute;
            p.pick_hardness = r.f(0.6, 0.9);
            p.brightness = r.f(0.5, 0.7);
            ("Palm Muted", &["Palm Mute", "Chug", "Muted", "Pulse"])
        }
        4 => {
            p.mode = PlayMode::Solo;
            p.pick_hardness = r.f(0.7, 1.0);
            p.pick_position = r.f(0.08, 0.15);
            p.brightness = r.f(0.7, 0.92);
            ("Flatpick", &["Flatpick", "Lead", "Picker", "Bright Pick", "Solo"])
        }
        _ => {
            p.mode = PlayMode::Solo;
            p.articulation = r.pick(&[Articulation::Harmonic, Articulation::Slide, Articulation::Sustain]);
            p.brightness = r.f(0.45, 0.8);
            p.pick_hardness = r.f(0.3, 0.7);
            p.slide_time = r.f(0.05, 0.2);
            ("Expressive", &["Harmonics", "Slide", "Glide", "Echoes", "Shimmer"])
        }
    };
    let amt = r.f(0.2, 0.7);
    space(r, &mut p, amt, 0.15);
    (res.0, res.1, p)
}

fn nylon(r: &mut R, i: usize) -> Recipe {
    let mut p = GuitarParams::for_model(GuitarModel::Nylon);
    common(r, &mut p);
    p.body = r.f(0.6, 0.95);
    p.body_size = r.f(0.85, 1.2);
    p.brightness = r.f(0.25, 0.5);
    p.pick_hardness = r.f(0.1, 0.35);
    p.sustain = r.f(3.5, 7.0);
    let res: (&str, &[&str]) = match i % 4 {
        0 => ("Fingerstyle", &["Etude", "Prelude", "Fingerstyle", "Nocturne", "Arpeggio"]),
        1 => {
            p.mode = PlayMode::Strum;
            p.strum_time = r.f(10.0, 25.0);
            ("Strummed", &["Rasgueado", "Strum", "Flamenco", "Chords"])
        }
        2 => {
            p.mode = PlayMode::Pattern;
            p.pattern = r.pick(&[9, 2, 8, 15]);
            p.strum_time = r.f(6.0, 16.0);
            ("Bossa & Patterns", &["Bossa", "Samba", "Waltz", "Pattern"])
        }
        _ => {
            p.brightness = r.f(0.45, 0.65);
            p.pick_position = r.f(0.08, 0.14);
            ("Ponticello", &["Ponticello", "Bright Nylon", "Tango", "Spanish Lead"])
        }
    };
    let amt = r.f(0.3, 0.8);
    space(r, &mut p, amt, 0.05);
    (res.0, res.1, p)
}

fn electric(r: &mut R, i: usize, driven: bool) -> Recipe {
    let mut p = GuitarParams::for_model(GuitarModel::Electric);
    common(r, &mut p);
    let (t, _) = tuning_for(r, GuitarModel::Electric, if driven { 0.25 } else { 0.08 });
    p.tuning = t;
    p.pickup = r.f(0.0, 1.0);
    p.sustain = r.f(5.0, 12.0);
    p.tone = r.f(0.3, 0.9);
    p.pick_hardness = r.f(0.6, 1.0);
    if driven {
        p.drive = r.f(0.35, 0.95);
        p.brightness = r.f(0.55, 0.85);
        let res: (&str, &[&str]) = match i % 4 {
            0 => ("Lead", &["Lead", "Solo", "Wail", "Bend", "Sustainer"]),
            1 => {
                p.mode = PlayMode::Strum;
                p.strum_time = r.f(4.0, 12.0);
                ("Power Chords", &["Power Chords", "Riff", "Stack", "Rhythm"])
            }
            2 => {
                p.articulation = Articulation::PalmMute;
                ("Palm Mute Chug", &["Chug", "Gallop", "Mute Riff", "Chugger"])
            }
            _ => {
                p.drive = r.f(0.2, 0.45);
                ("Crunch", &["Crunch", "Blues", "Edge", "Bite", "Rock"])
            }
        };
        p.fx.push(fx("compressor", &[("threshold", -18.0), ("ratio", 3.0), ("makeup", 3.0)]));
        let amt = r.f(0.2, 0.6);
        space(r, &mut p, amt, 0.1);
        (res.0, res.1, p)
    } else {
        p.drive = r.f(0.0, 0.12);
        p.brightness = r.f(0.5, 0.85);
        let res: (&str, &[&str]) = match i % 4 {
            0 => ("Clean", &["Clean", "Glass", "Single Coil", "Neck Tone", "Bridge Tone"]),
            1 => {
                p.mode = PlayMode::Strum;
                p.strum_time = r.f(5.0, 15.0);
                ("Clean Chords", &["Chords", "Strum", "Comp", "Shimmer"])
            }
            2 => {
                p.articulation = r.pick(&[Articulation::PalmMute, Articulation::Dead]);
                p.mode = PlayMode::Strum;
                p.strum_time = r.f(2.0, 7.0);
                ("Funk", &["Funk", "Scratch", "Chank", "Skank"])
            }
            _ => {
                p.tone = r.f(0.2, 0.45);
                p.pickup = r.f(0.7, 1.0);
                ("Jazz", &["Jazz Box", "Archtop", "Warm Jazz", "Comping"])
            }
        };
        let amt = r.f(0.2, 0.8);
        space(r, &mut p, amt, 0.45);
        (res.0, res.1, p)
    }
}

fn bass(r: &mut R, i: usize) -> Recipe {
    let mut p = GuitarParams::for_model(GuitarModel::Bass);
    common(r, &mut p);
    p.width = r.f(0.0, 0.2);
    p.pickup = r.f(0.1, 0.9);
    p.tone = r.f(0.2, 0.7);
    p.sustain = r.f(4.0, 9.0);
    if r.chance(0.2) {
        p.tuning = vec![23, 28, 33, 38, 43];
    }
    let res: (&str, &[&str]) = match i % 5 {
        0 => {
            p.pick_hardness = r.f(0.1, 0.35);
            p.brightness = r.f(0.3, 0.5);
            ("Finger", &["Finger Bass", "Fingers", "Groove", "Pocket"])
        }
        1 => {
            p.pick_hardness = r.f(0.75, 1.0);
            p.brightness = r.f(0.55, 0.8);
            p.pick_position = r.f(0.06, 0.12);
            ("Pick", &["Pick Bass", "Rock Bass", "Punk Bass", "Driver"])
        }
        2 => {
            p.articulation = Articulation::PalmMute;
            p.brightness = r.f(0.2, 0.4);
            ("Muted", &["Muted Bass", "Thump", "Dub", "Motown"])
        }
        3 => {
            p.drive = r.f(0.25, 0.6);
            p.brightness = r.f(0.5, 0.75);
            ("Drive", &["Growl", "Fuzz Bass", "Grit", "Snarl"])
        }
        _ => {
            p.articulation = Articulation::Slide;
            p.slide_time = r.f(0.06, 0.18);
            p.brightness = r.f(0.25, 0.45);
            p.pick_hardness = r.f(0.05, 0.2);
            ("Fretless", &["Fretless", "Slide Bass", "Singing Bass", "Mwah"])
        }
    };
    if r.chance(0.6) {
        p.fx.push(fx("compressor", &[("threshold", -20.0), ("ratio", 4.0), ("makeup", 4.0)]));
    }
    (res.0, res.1, p)
}

fn rhythm(r: &mut R, i: usize) -> Recipe {
    let model = r.pick(&[GuitarModel::SteelAcoustic, GuitarModel::SteelAcoustic, GuitarModel::TwelveString, GuitarModel::Nylon, GuitarModel::Ukulele, GuitarModel::Electric, GuitarModel::Mandolin]);
    let mut p = GuitarParams::for_model(model);
    common(r, &mut p);
    p.mode = PlayMode::Pattern;
    p.pattern = i % PATTERNS.len();
    p.strum_time = r.f(5.0, 14.0);
    p.pick_hardness = r.f(0.5, 0.9);
    if model == GuitarModel::Electric {
        p.drive = r.f(0.0, 0.4);
    }
    if model == GuitarModel::TwelveString {
        p.doubling = r.f(0.55, 0.8);
    }
    let amt = r.f(0.2, 0.6);
    space(r, &mut p, amt, 0.1);
    let kind = PATTERNS[p.pattern].0;
    (kind, &["Strum", "Groove", "Rhythm", "Pattern", "Strummer", "Beat"], p)
}

fn small(r: &mut R, i: usize, model: GuitarModel) -> Recipe {
    let mut p = GuitarParams::for_model(model);
    common(r, &mut p);
    p.body = r.f(0.6, 0.95);
    p.body_size *= r.f(0.85, 1.15);
    let res: (&str, &[&str]) = match i % 3 {
        0 => ("Picked", &["Picking", "Melody", "Roll", "Tune"]),
        1 => {
            p.mode = PlayMode::Strum;
            p.strum_time = r.f(6.0, 18.0);
            ("Strummed", &["Strum", "Chords", "Chop", "Rhythm"])
        }
        _ => {
            p.mode = PlayMode::Pattern;
            p.pattern = r.pick(&[0, 1, 3, 7, 10, 13]);
            p.strum_time = r.f(4.0, 12.0);
            ("Pattern", &["Groove", "Pattern", "Train", "Shuffle"])
        }
    };
    p.brightness = (p.brightness + r.f(-0.15, 0.1)).clamp(0.2, 0.95);
    let amt = r.f(0.15, 0.6);
    space(r, &mut p, amt, 0.05);
    (res.0, res.1, p)
}

fn world(r: &mut R, i: usize) -> Recipe {
    let res: (&str, &[&str], GuitarModel) = match i % 3 {
        0 => ("Sitar", &["Sitar", "Raga", "Jawari", "Drone"], GuitarModel::Sitar),
        1 => ("Koto-like", &["Koto", "Zither", "Pluck", "Garden"], GuitarModel::Nylon),
        _ => ("Oud-like", &["Oud", "Lute", "Saz", "Caravan"], GuitarModel::Nylon),
    };
    let mut p = GuitarParams::for_model(res.2);
    common(r, &mut p);
    match i % 3 {
        0 => {
            p.buzz = r.f(0.4, 0.9);
            p.brightness = r.f(0.6, 0.9);
        }
        1 => {
            p.tuning = vec![50, 55, 57, 62, 64, 67];
            p.pick_hardness = r.f(0.5, 0.8);
            p.pick_position = r.f(0.06, 0.12);
            p.brightness = r.f(0.55, 0.8);
            p.body_size = r.f(0.6, 0.8);
            p.buzz = r.f(0.0, 0.2);
        }
        _ => {
            p.tuning = vec![36, 41, 46, 51, 55, 60];
            p.pick_hardness = r.f(0.6, 0.9);
            p.body_size = r.f(1.1, 1.4);
            p.brightness = r.f(0.4, 0.65);
            p.articulation = r.pick(&[Articulation::Sustain, Articulation::Slide]);
        }
    }
    let amt = r.f(0.4, 0.9);
    space(r, &mut p, amt, 0.0);
    (res.0, res.1, p)
}

fn harp(r: &mut R) -> Recipe {
    let mut p = GuitarParams::for_model(GuitarModel::Harp);
    common(r, &mut p);
    p.sustain = r.f(5.0, 10.0);
    p.brightness = r.f(0.3, 0.6);
    p.body = r.f(0.3, 0.7);
    if r.chance(0.4) {
        p.mode = PlayMode::Strum;
        p.strum_time = r.f(25.0, 70.0);
        p.strum_dir = r.pick(&[StrumDir::Up, StrumDir::Down]);
    }
    let amt = r.f(0.6, 1.0);
    space(r, &mut p, amt, 0.0);
    ("Harp", &["Harp", "Glissando", "Lyre", "Strings", "Arpa"], p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_library_has_two_thousand_unique_presets() {
        let lib = library();
        assert!(lib.len() >= 2000, "{}", lib.len());
        let names: HashSet<String> = lib.iter().map(|p| p.name.to_lowercase()).collect();
        assert_eq!(names.len(), lib.len());
        for c in Category::ALL {
            assert!(lib.iter().filter(|p| p.category == c).count() >= 50, "{c:?}");
        }
        assert_eq!(build()[700].name, lib[700].name);
        for p in lib {
            let g = &p.params;
            assert!(!g.tuning.is_empty() && g.tuning.len() <= 8, "{}", p.name);
            assert!(g.pattern < PATTERNS.len() && g.sustain > 0.0 && g.master > 0.0, "{}", p.name);
            assert_eq!(g.preset, p.name);
        }
    }
}
