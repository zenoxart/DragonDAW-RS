//! The piano preset library: recipes for each kind of piano sound (a concert grand on a big stage,
//! an intimate felt piano, a lo-fi tape upright, a barking tine electric, arpeggios …), varied within
//! musical ranges with a fixed seed. Levels come from [`LEVELS`] (measured).

use crate::dsp::Rng;
use crate::pno::*;
use crate::wt::FxSlot;
use std::collections::HashSet;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    Concert,
    Studio,
    Upright,
    Felt,
    Cinematic,
    Jazz,
    HonkyTonk,
    LoFi,
    Tine,
    Reed,
    Bells,
    Arp,
}

impl Category {
    pub const ALL: [Category; 12] =
        [Category::Concert, Category::Studio, Category::Upright, Category::Felt, Category::Cinematic, Category::Jazz, Category::HonkyTonk, Category::LoFi, Category::Tine, Category::Reed, Category::Bells, Category::Arp];
    pub fn name(self) -> &'static str {
        match self {
            Category::Concert => "Concert Grand",
            Category::Studio => "Studio & Pop",
            Category::Upright => "Upright & Vintage",
            Category::Felt => "Felt & Intimate",
            Category::Cinematic => "Cinematic & Ambient",
            Category::Jazz => "Jazz & Lounge",
            Category::HonkyTonk => "Honky-Tonk & Saloon",
            Category::LoFi => "Lo-Fi & Tape",
            Category::Tine => "Tine Electric",
            Category::Reed => "Reed Electric",
            Category::Bells => "Toy & Celesta",
            Category::Arp => "Arpeggios",
        }
    }
    pub fn code(self) -> &'static str {
        match self {
            Category::Concert => "CG",
            Category::Studio => "SP",
            Category::Upright => "UP",
            Category::Felt => "FE",
            Category::Cinematic => "CI",
            Category::Jazz => "JZ",
            Category::HonkyTonk => "HT",
            Category::LoFi => "LF",
            Category::Tine => "TN",
            Category::Reed => "RD",
            Category::Bells => "TC",
            Category::Arp => "AR",
        }
    }
    fn count(self) -> usize {
        match self {
            Category::Concert => 250,
            Category::Studio => 220,
            Category::Upright => 180,
            Category::Felt => 180,
            Category::Cinematic => 200,
            Category::Jazz => 150,
            Category::HonkyTonk => 90,
            Category::LoFi => 150,
            Category::Tine => 170,
            Category::Reed => 140,
            Category::Bells => 100,
            Category::Arp => 180,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LibPreset {
    pub name: String,
    pub category: Category,
    pub kind: &'static str,
    pub params: PianoParams,
}

pub fn library() -> &'static [LibPreset] {
    static LIB: OnceLock<Vec<LibPreset>> = OnceLock::new();
    LIB.get_or_init(build)
}

pub fn find(name: &str) -> Option<&'static LibPreset> {
    library().iter().find(|p| p.name == name)
}

/// What a preset is auditioned and measured with: a chord around middle C.
pub fn audition(p: &PianoParams) -> Vec<u8> {
    let (lo, hi) = p.model.range();
    let root = 60u8.clamp(lo, hi.saturating_sub(7));
    vec![root - 12.min(root - lo), root, root + 4, root + 7]
}

/// Measured level corrections (dB), in library order. Regenerate with
/// `cargo test --release -p dragon-engine measure_piano_levels -- --ignored`.
pub(crate) const LEVELS: &[f32] = &include!("pno_levels.in");

fn build() -> Vec<LibPreset> {
    let mut out = Vec::new();
    let mut used: HashSet<String> = HashSet::new();
    for cat in Category::ALL {
        for i in 0..cat.count() {
            let mut r = R(Rng::new(0x9A10_0000_C0DE_0001 ^ ((cat as u64 + 1) << 36) ^ (i as u64).wrapping_mul(0x2545_F491_4F6C_DD1D)));
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

/// The room: a hall, a studio or a small room, scaled by `size` (0 close … 1 cathedral).
fn room(r: &mut R, p: &mut PianoParams, size: f32) {
    let decay = 0.6 + size * size * 7.0 * r.f(0.7, 1.3);
    let mix = (0.08 + size * 0.3 * r.f(0.7, 1.2)).min(0.55);
    p.fx.push(fx("reverb", &[("mode", if size > 0.6 { r.pick(&[2.0, 3.0, 4.0]) } else { r.pick(&[0.0, 1.0, 2.0]) }), ("decay", decay), ("mix", mix), ("size", (0.3 + size * 0.7).min(1.0))]));
}

fn adjectives(cat: Category) -> &'static [&'static str] {
    match cat {
        Category::Concert => &[
            "Grand",
            "Royal",
            "Concert",
            "Majestic",
            "Noble",
            "Velvet",
            "Golden",
            "Imperial",
            "Recital",
            "Sonata",
            "Opera",
            "Crystal",
            "Classic",
            "Romantic",
            "Viennese",
            "Symphonic",
            "Nocturne",
            "Ivory",
            "Ebony",
            "Pristine",
            "Elegant",
            "Philharmonic",
            "Salon",
            "Cathedral",
            "Silver",
            "Marble",
        ],
        Category::Studio => {
            &["Pop", "Studio", "Bright", "Radio", "Session", "Modern", "Punchy", "Clean", "Shiny", "Ballad", "Chart", "Stadium", "Anthem", "Crisp", "Polished", "Uplifting", "Summer", "City", "Sunrise", "Neon"]
        }
        Category::Upright => &["Old", "Parlour", "Vintage", "Living Room", "School", "Church Hall", "Grandma's", "Dusty", "Wooden", "Attic", "Antique", "Victorian", "Cottage", "Warm", "Family", "Chapel", "Village"],
        Category::Felt => &["Soft", "Hushed", "Muted", "Whisper", "Velvet", "Felted", "Close", "Intimate", "Late Night", "Sleepy", "Gentle", "Fragile", "Tender", "Quiet", "Cotton", "Midnight", "Paper", "Lullaby"],
        Category::Cinematic => {
            &["Epic", "Distant", "Endless", "Floating", "Ethereal", "Frozen", "Dream", "Ocean", "Mountain", "Starlit", "Aurora", "Infinite", "Weightless", "Ghost", "Twilight", "Horizon", "Nebula", "Cloud", "Elegy"]
        }
        Category::Jazz => &["Smoky", "Blue", "Late Club", "Lounge", "Bebop", "Swing", "Velvet", "Cool", "Midnight", "Martini", "Basement", "Uptown", "Bossa", "Silk", "After Hours", "Ballroom"],
        Category::HonkyTonk => &["Saloon", "Western", "Ragtime", "Dusty", "Barroom", "Rusty", "Tack", "Old West", "Bourbon", "Whiskey", "Frontier", "Boomtown"],
        Category::LoFi => &["Lo-Fi", "Tape", "Cassette", "Vinyl", "Wobbly", "Faded", "Hazy", "Bedroom", "Chill", "Rainy Day", "Sunday", "Memory", "Polaroid", "Warped", "Dusty", "Nostalgic", "VHS", "Study"],
        Category::Tine => &["Suitcase", "Stage", "Mellow", "Barking", "Seventies", "Soul", "Smooth", "Glassy", "Funky", "Velvet", "Tremolo", "Dreamy", "Neo Soul", "Gospel", "Silky", "Vintage", "Warm"],
        Category::Reed => &["Gritty", "Classroom", "Rock", "Barking", "Seventies", "Buzzy", "Soulful", "Wobbly", "Funky", "Raw", "Dirty", "Groovy", "Vintage", "Bluesy"],
        Category::Bells => &["Music Box", "Toy", "Celesta", "Tiny", "Sugar Plum", "Snow Globe", "Fairy", "Nursery", "Twinkle", "Clockwork", "Moonlit", "Winter", "Lullaby"],
        Category::Arp => &["Rippling", "Cascading", "Flowing", "Running", "Spiraling", "Falling", "Rising", "Glittering", "Shimmering", "Pulsing", "Racing", "Dancing", "Rolling", "Winding"],
    }
}

type Recipe = (&'static str, &'static [&'static str], PianoParams);

fn grand_base(r: &mut R, model: PianoModel) -> PianoParams {
    let mut p = PianoParams::for_model(model);
    p.tone = (p.tone + r.f(-0.15, 0.15)).clamp(0.05, 1.0);
    p.body = (p.body + r.f(-0.15, 0.15)).clamp(0.1, 1.0);
    p.hammer = r.f(0.2, 0.6);
    p.release = r.f(0.12, 0.3);
    p.release_noise = r.f(0.1, 0.5);
    p.tightness = r.f(0.65, 0.95);
    p.resonance = r.f(0.2, 0.6);
    p.stiffness = r.f(0.35, 0.7);
    p.stretch = r.f(0.4, 0.9);
    p.high_damp = r.f(0.0, 0.3);
    p.dynamics = r.f(0.6, 0.9);
    p.width = r.f(0.5, 0.9);
    p
}

fn recipe(cat: Category, r: &mut R, i: usize) -> Recipe {
    match cat {
        Category::Concert => {
            let mut p = grand_base(r, PianoModel::ConcertGrand);
            let size = r.f(0.35, 0.85);
            room(r, &mut p, size);
            let kinds: [(&str, &[&str]); 3] =
                [("Hall", &["Grand", "Hall", "Stage", "Recital", "Concert"]), ("Close", &["Close Mic", "Player View", "Keys", "Front Row"]), ("Classical", &["Etude", "Sonata", "Prelude", "Nocturne", "Waltz"])];
            let k = kinds[i % 3];
            if i % 3 == 1 {
                p.width = r.f(0.8, 1.0);
                p.fx.clear();
                let size = r.f(0.15, 0.35);
                room(r, &mut p, size);
            }
            (k.0, k.1, p)
        }
        Category::Studio => {
            let model = if r.chance(0.6) { PianoModel::BrightGrand } else { PianoModel::StudioGrand };
            let mut p = grand_base(r, model);
            p.tone = r.f(0.6, 0.95);
            p.dynamics = r.f(0.4, 0.7);
            p.fx.push(fx("compressor", &[("threshold", r.f(-24.0, -14.0)), ("ratio", r.f(2.0, 4.0)), ("makeup", r.f(2.0, 5.0))]));
            if r.chance(0.5) {
                p.fx.push(fx("eq", &[]));
            }
            let size = r.f(0.15, 0.5);
            room(r, &mut p, size);
            if i.is_multiple_of(4) {
                p.fx.push(fx("chorus", &[("mix", r.f(0.15, 0.3)), ("depth", r.f(0.2, 0.4))]));
                ("Layered", &["Shimmer Keys", "Pop Piano", "Ballad", "Radio Keys"], p)
            } else {
                ("Pop", &["Pop Grand", "Studio Grand", "Session Piano", "Hit Piano", "Keys"], p)
            }
        }
        Category::Upright => {
            let mut p = grand_base(r, PianoModel::Upright);
            p.sustain = r.f(0.5, 0.85);
            p.tightness = r.f(0.45, 0.8);
            p.body = r.f(0.6, 0.95);
            let size = r.f(0.1, 0.4);
            room(r, &mut p, size);
            if r.chance(0.4) {
                p.fx.push(fx("saturator", &[("drive", r.f(1.0, 4.0)), ("mix", r.f(0.3, 0.6))]));
            }
            ("Upright", &["Upright", "Parlour Piano", "Home Piano", "Spinet", "Old Piano"], p)
        }
        Category::Felt => {
            let mut p = grand_base(r, PianoModel::Felt);
            p.tone = r.f(0.02, 0.3);
            p.hammer = r.f(0.5, 0.9);
            p.release_noise = r.f(0.4, 0.9);
            p.pedal_noise = r.f(0.3, 0.8);
            p.high_damp = r.f(0.3, 0.7);
            p.dynamics = r.f(0.4, 0.7);
            p.velocity_high = r.f(0.6, 0.9);
            p.width = r.f(0.3, 0.7);
            let size = r.f(0.05, 0.45);
            room(r, &mut p, size);
            ("Felt", &["Felt", "Felt Piano", "Muted Keys", "Una Corda", "Whisper Keys"], p)
        }
        Category::Cinematic => {
            let model = r.pick(&[PianoModel::ConcertGrand, PianoModel::Felt, PianoModel::StudioGrand]);
            let mut p = grand_base(r, model);
            p.pedal_latch = r.chance(0.5);
            p.resonance = r.f(0.5, 0.95);
            p.high_damp = r.f(0.2, 0.6);
            if i % 2 == 1 {
                // A swell short enough that the strings still ring when it arrives.
                p.swell = r.f(0.15, 0.6);
                p.sustain = r.f(1.3, 1.7);
            }
            let size = r.f(0.75, 1.0);
            room(r, &mut p, size);
            if r.chance(0.5) {
                p.fx.push(fx("delay", &[("sync", r.pick(&[5.0, 6.0, 7.0])), ("feedback", r.f(0.3, 0.6)), ("mix", r.f(0.1, 0.25)), ("highcut", r.f(2000.0, 5000.0))]));
            }
            if i % 2 == 1 {
                ("Swell", &["Swell", "Reverse Piano", "Bowed Keys", "Pad Piano", "Rising Keys"], p)
            } else {
                ("Ambient", &["Ambient Keys", "Score", "Theme", "Elegy", "Memory"], p)
            }
        }
        Category::Jazz => {
            let model = r.pick(&[PianoModel::StudioGrand, PianoModel::ConcertGrand, PianoModel::Upright]);
            let mut p = grand_base(r, model);
            p.tone = r.f(0.35, 0.6);
            p.high_damp = r.f(0.15, 0.4);
            p.dynamics = r.f(0.7, 0.95);
            let size = r.f(0.15, 0.4);
            room(r, &mut p, size);
            ("Jazz", &["Jazz Piano", "Trio Piano", "Club Keys", "Comping Piano", "Ballad Keys"], p)
        }
        Category::HonkyTonk => {
            let mut p = grand_base(r, PianoModel::HonkyTonk);
            p.tightness = r.f(0.0, 0.3);
            p.sustain = r.f(0.4, 0.7);
            p.tone = r.f(0.6, 0.9);
            let size = r.f(0.1, 0.3);
            room(r, &mut p, size);
            ("Honky-Tonk", &["Honky-Tonk", "Saloon Piano", "Tack Piano", "Ragtime", "Barrelhouse"], p)
        }
        Category::LoFi => {
            let model = r.pick(&[PianoModel::Upright, PianoModel::Felt, PianoModel::StudioGrand, PianoModel::Tine]);
            let mut p = grand_base(r, model);
            p.high_damp = r.f(0.4, 0.8);
            p.tightness = r.f(0.4, 0.8);
            p.fx.push(fx("chorus", &[("rate", r.f(0.2, 0.8)), ("depth", r.f(0.3, 0.7)), ("mix", r.f(0.2, 0.45))]));
            p.fx.push(fx("saturator", &[("drive", r.f(2.0, 6.0)), ("mix", r.f(0.4, 0.8))]));
            if r.chance(0.5) {
                p.fx.push(fx("bitcrusher", &[("bits", r.f(9.0, 13.0)), ("mix", r.f(0.2, 0.5))]));
            }
            let size = r.f(0.1, 0.5);
            room(r, &mut p, size);
            ("Lo-Fi", &["Tape Piano", "Lo-Fi Keys", "Cassette Keys", "Beat Piano", "Dusty Keys"], p)
        }
        Category::Tine | Category::Reed => {
            let model = if cat == Category::Tine { PianoModel::Tine } else { PianoModel::Reed };
            let mut p = PianoParams::for_model(model);
            p.tone = r.f(0.3, 0.8);
            p.drive = r.f(0.1, 0.8);
            p.tremolo_depth = if r.chance(0.6) { r.f(0.15, 0.6) } else { 0.0 };
            p.tremolo_rate = r.f(3.0, 7.0);
            p.dynamics = r.f(0.5, 0.85);
            p.release = r.f(0.08, 0.2);
            p.width = r.f(0.4, 0.9);
            if r.chance(0.4) {
                p.fx.push(fx("chorus", &[("mix", r.f(0.2, 0.4))]));
            }
            if r.chance(0.3) {
                p.fx.push(fx("phaser", &[]));
            }
            let size = r.f(0.1, 0.45);
            room(r, &mut p, size);
            if cat == Category::Tine {
                ("Tine EP", &["Tine EP", "Electric Piano", "Stage EP", "Suitcase EP", "Soul EP"], p)
            } else {
                ("Reed EP", &["Reed EP", "Electric Piano", "Rock EP", "Funk EP", "Wurly"], p)
            }
        }
        Category::Bells => {
            let mut p = PianoParams::for_model(if i.is_multiple_of(2) { PianoModel::Celesta } else { PianoModel::Toy });
            p.tone = r.f(0.4, 0.8);
            p.width = r.f(0.4, 0.9);
            let size = r.f(0.3, 0.8);
            room(r, &mut p, size);
            if p.model == PianoModel::Celesta {
                ("Celesta", &["Celesta", "Glockenkeys", "Snow Keys", "Sparkle"], p)
            } else {
                ("Toy Piano", &["Toy Piano", "Tinkle", "Plink", "Music Box"], p)
            }
        }
        Category::Arp => {
            let model = r.pick(&[PianoModel::ConcertGrand, PianoModel::StudioGrand, PianoModel::Felt, PianoModel::Tine, PianoModel::Celesta, PianoModel::BrightGrand]);
            let mut p = if model.strings() { grand_base(r, model) } else { PianoParams::for_model(model) };
            p.arp = Arp {
                on: true,
                mode: ArpMode::ALL[i % ArpMode::ALL.len()],
                rate: r.pick(&[0.5, 0.25, 0.25, 1.0 / 3.0, 1.0 / 6.0]),
                octaves: r.pick(&[1, 2, 2, 3]),
                gate: r.f(0.4, 1.0),
                swing: if r.chance(0.3) { r.f(0.1, 0.3) } else { 0.0 },
                accent: r.f(0.1, 0.5),
            };
            p.pedal_latch = r.chance(0.3);
            let size = r.f(0.3, 0.8);
            room(r, &mut p, size);
            if r.chance(0.4) {
                p.fx.push(fx("delay", &[("sync", r.pick(&[5.0, 6.0])), ("feedback", r.f(0.2, 0.45)), ("mix", r.f(0.08, 0.2))]));
            }
            ("Arpeggio", &["Arpeggio", "Cascade", "Ripple", "Arp Piano", "Waterfall"], p)
        }
    }
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
        assert_eq!(build()[1200].name, lib[1200].name);
        for p in lib {
            assert_eq!(p.params.preset, p.name);
            assert!(p.params.velocity_low < p.params.velocity_high, "{}", p.name);
        }
    }
}
