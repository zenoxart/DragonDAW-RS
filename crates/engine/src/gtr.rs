//! Dragon Strings: the model of the plucked-string instrument (acoustic, nylon, 12-string, electric,
//! bass, banjo, mandolin, ukulele, sitar, harp). The sound is modelled, not sampled: each string is a
//! tuned feedback loop excited by a pluck (see `instruments::guitar`). This module holds the
//! parameters, tunings, the chord voicer (works for any tuning) and the strum patterns.

use crate::wt::FxSlot;
use serde::{Deserialize, Serialize};

/// The kind of instrument: sets the string material, body and pickup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum GuitarModel {
    #[default]
    SteelAcoustic,
    Nylon,
    TwelveString,
    Electric,
    Bass,
    Banjo,
    Mandolin,
    Ukulele,
    Sitar,
    Harp,
}

impl GuitarModel {
    pub const ALL: [GuitarModel; 10] = [
        GuitarModel::SteelAcoustic,
        GuitarModel::Nylon,
        GuitarModel::TwelveString,
        GuitarModel::Electric,
        GuitarModel::Bass,
        GuitarModel::Banjo,
        GuitarModel::Mandolin,
        GuitarModel::Ukulele,
        GuitarModel::Sitar,
        GuitarModel::Harp,
    ];
    pub fn name(self) -> &'static str {
        match self {
            GuitarModel::SteelAcoustic => "Steel acoustic",
            GuitarModel::Nylon => "Nylon classical",
            GuitarModel::TwelveString => "12-string",
            GuitarModel::Electric => "Electric",
            GuitarModel::Bass => "Bass",
            GuitarModel::Banjo => "Banjo",
            GuitarModel::Mandolin => "Mandolin",
            GuitarModel::Ukulele => "Ukulele",
            GuitarModel::Sitar => "Sitar",
            GuitarModel::Harp => "Harp",
        }
    }
    /// The usual tuning (open strings, lowest first).
    pub fn default_tuning(self) -> Vec<u8> {
        match self {
            GuitarModel::Bass => vec![28, 33, 38, 43],
            GuitarModel::Banjo => vec![50, 55, 59, 62],
            GuitarModel::Mandolin => vec![55, 62, 69, 76],
            GuitarModel::Ukulele => vec![67, 60, 64, 69],
            GuitarModel::Sitar => vec![48, 55, 60, 62, 67, 72],
            GuitarModel::Harp => vec![36, 43, 48, 55, 60, 67, 72],
            _ => vec![40, 45, 50, 55, 59, 64],
        }
    }
    /// Frets on the neck (how far up a string can be played).
    pub fn frets(self) -> u8 {
        match self {
            GuitarModel::Banjo | GuitarModel::Ukulele => 19,
            GuitarModel::Harp => 12,
            GuitarModel::Electric => 24,
            _ => 20,
        }
    }
}

/// How a note is played.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Articulation {
    #[default]
    Sustain,
    PalmMute,
    Harmonic,
    Dead,
    Slide,
}

impl Articulation {
    pub const ALL: [Articulation; 5] = [Articulation::Sustain, Articulation::PalmMute, Articulation::Harmonic, Articulation::Dead, Articulation::Slide];
    pub fn name(self) -> &'static str {
        match self {
            Articulation::Sustain => "Sustain",
            Articulation::PalmMute => "Palm mute",
            Articulation::Harmonic => "Harmonic",
            Articulation::Dead => "Dead note",
            Articulation::Slide => "Legato slide",
        }
    }
    /// The key switch (C0 = 12 …) that selects it while playing.
    pub fn key(self) -> u8 {
        12 + Articulation::ALL.iter().position(|a| *a == self).unwrap_or(0) as u8
    }
}

/// What the keys do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PlayMode {
    /// One key, one string.
    #[default]
    Solo,
    /// Notes played together are strummed across the strings.
    Strum,
    /// Play any chord (or one key for a major chord): it is voiced like a guitarist would and strummed.
    Chord,
    /// Hold a chord: it is strummed in a rhythm pattern, in time with the song.
    Pattern,
}

impl PlayMode {
    pub const ALL: [PlayMode; 4] = [PlayMode::Solo, PlayMode::Strum, PlayMode::Chord, PlayMode::Pattern];
    pub fn name(self) -> &'static str {
        match self {
            PlayMode::Solo => "Solo",
            PlayMode::Strum => "Strum",
            PlayMode::Chord => "Chord",
            PlayMode::Pattern => "Pattern",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum StrumDir {
    #[default]
    Down,
    Up,
    Alternate,
}

impl StrumDir {
    pub const ALL: [StrumDir; 3] = [StrumDir::Down, StrumDir::Up, StrumDir::Alternate];
    pub fn name(self) -> &'static str {
        match self {
            StrumDir::Down => "Down",
            StrumDir::Up => "Up",
            StrumDir::Alternate => "Alternate",
        }
    }
}

/// A rhythm for pattern mode: one character per step.
/// `D` down strum, `d` light down (top strings), `U` up strum, `u` light up, `X` muted chuck,
/// `B` bass note only, `-` let ring, `.` stop the strings.
pub const PATTERNS: [(&str, &str, f32); 16] = [
    ("Folk 8ths", "D-DU-UDU", 0.5),
    ("Pop 16ths", "D-D-DUDUD-DUDUDU", 0.25),
    ("Ballad", "B-d-U-d-", 0.5),
    ("Country", "B-DUB-DU", 0.5),
    ("Reggae skank", "-X-X-X-X", 0.5),
    ("Funk chucks", "DXUXDUXUDXUXDUXU", 0.25),
    ("Rock 8ths", "DDDDDDDD", 0.5),
    ("Shuffle", "D-UD-UD-UD-U", 1.0 / 3.0),
    ("Waltz", "B-D-D-", 0.5),
    ("Bossa", "B-dUB-dU-dUB-dU-", 0.25),
    ("Island", "D-DU-UDU-UDU-UDU", 0.25),
    ("Gallop", "DXUD-XUD", 0.25),
    ("Half time", "D---X---", 0.5),
    ("Train", "DuDuXuDu", 0.25),
    ("Stops", "D.D.DUD.", 0.5),
    ("Arpeggio", "B-d-u-d-", 0.25),
];

/// Chord kinds the voicer knows (intervals from the root).
pub const CHORDS: [(&str, &[u8]); 12] = [
    ("Major", &[0, 4, 7]),
    ("Minor", &[0, 3, 7]),
    ("7", &[0, 4, 7, 10]),
    ("m7", &[0, 3, 7, 10]),
    ("maj7", &[0, 4, 7, 11]),
    ("sus4", &[0, 5, 7]),
    ("sus2", &[0, 2, 7]),
    ("5", &[0, 7]),
    ("add9", &[0, 2, 4, 7]),
    ("6", &[0, 4, 7, 9]),
    ("dim", &[0, 3, 6]),
    ("aug", &[0, 4, 8]),
];

/// Recognises a chord from the keys held: (root note, chord index). One key = a major chord.
pub fn detect_chord(notes: &[u8]) -> Option<(u8, usize)> {
    let bass = *notes.iter().min()?;
    let mut pcs: Vec<u8> = notes.iter().map(|n| n % 12).collect();
    pcs.sort_unstable();
    pcs.dedup();
    if pcs.len() == 1 {
        return Some((bass, 0));
    }
    // Try every held pitch class as the root, the bass first; the best full match wins.
    let mut roots: Vec<u8> = vec![bass % 12];
    roots.extend(pcs.iter().copied().filter(|p| *p != bass % 12));
    let mut best: Option<(usize, u8, usize)> = None;
    for root in roots {
        for (k, (_, iv)) in CHORDS.iter().enumerate() {
            let set: Vec<u8> = iv.iter().map(|i| (root + i) % 12).collect();
            if pcs.iter().all(|p| set.contains(p)) {
                let missing = set.len() - pcs.len();
                let score = missing * 4 + usize::from(root != bass % 12) + k / 4;
                if best.is_none_or(|b| score < b.0) {
                    best = Some((score, root, k));
                }
            }
        }
    }
    let (_, root, k) = best?;
    // The root in the octave of the lowest key.
    let r = bass - ((bass % 12 + 12 - root) % 12);
    Some((r, k))
}

/// Voices a chord on the strings like a guitarist would: one note per string within a four-fret
/// hand position, the root in the bass, open strings where they fit. `None` = the string is not played.
pub fn voice_chord(tuning: &[u8], root: u8, intervals: &[u8], frets: u8) -> Vec<Option<u8>> {
    let pcs: Vec<u8> = intervals.iter().map(|i| (root + i) % 12).collect();
    let mut best: Option<(i32, Vec<Option<u8>>)> = None;
    for w in 0..=frets.saturating_sub(4).min(12) {
        let mut v: Vec<Option<u8>> = Vec::with_capacity(tuning.len());
        for &open in tuning {
            let mut pick = None;
            let mut candidates: Vec<u8> = Vec::new();
            if w <= 3 {
                candidates.push(0);
            }
            candidates.extend((w.max(1)..=w + 3).filter(|f| *f <= frets));
            for f in candidates {
                if pcs.contains(&((open + f) % 12)) {
                    pick = Some(open + f);
                    break;
                }
            }
            v.push(pick);
        }
        // The bass (lowest sounding string) must be the root: mute strings below the lowest root.
        // Strings are listed lowest first, but re-entrant tunings (ukulele) may not be sorted.
        let order = {
            let mut o: Vec<usize> = (0..tuning.len()).collect();
            o.sort_by_key(|&i| tuning[i]);
            o
        };
        let lowest_root = order.iter().position(|&i| v[i].is_some_and(|n| n % 12 == root % 12));
        let Some(lr) = lowest_root else { continue };
        for &i in &order[..lr] {
            v[i] = None;
        }
        let sounding = v.iter().flatten().count() as i32;
        let has_all = pcs.iter().filter(|p| v.iter().flatten().any(|n| n % 12 == **p)).count() as i32;
        // Prefer full chords, then positions near the nut with ringing open strings (how most
        // players reach for a chord), then the bass on a low string.
        let opens = (0..tuning.len()).filter(|&i| v[i] == Some(tuning[i])).count() as i32;
        let score = has_all * 100 + sounding * 10 + opens * 3 - w as i32 * 6 - lr as i32 * 12;
        if best.as_ref().is_none_or(|b| score > b.0) {
            best = Some((score, v));
        }
    }
    best.map(|b| b.1).unwrap_or_else(|| tuning.iter().map(|_| None).collect())
}

/// Picks a string for `note`: the lowest position (fret) on a free string, else the string
/// that could play it lowest. Returns (string, fret).
pub fn string_for(tuning: &[u8], note: u8, frets: u8, busy: &[bool]) -> Option<(usize, u8)> {
    let fits = |i: usize| note >= tuning[i] && note - tuning[i] <= frets;
    let mut opts: Vec<usize> = (0..tuning.len()).filter(|&i| fits(i)).collect();
    // Prefer low frets (the way a player stays near the nut), free strings first.
    opts.sort_by_key(|&i| (busy.get(i).copied().unwrap_or(false), (note - tuning[i]) as i32 / 5, (note - tuning[i]) as i32));
    opts.first().map(|&i| (i, note - tuning[i]))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GuitarParams {
    pub preset: String,
    pub model: GuitarModel,
    /// Open strings, lowest first (re-entrant tunings allowed).
    pub tuning: Vec<u8>,
    pub capo: u8,
    // The string.
    /// How long a note rings (seconds, low notes; high notes ring shorter).
    pub sustain: f32,
    /// String brightness (0 dull … 1 bright).
    pub brightness: f32,
    /// Where the string is plucked (0 at the bridge … 0.5 mid-string).
    pub pick_position: f32,
    /// Pick (1) or soft finger (0).
    pub pick_hardness: f32,
    /// Fret, finger and string noises.
    pub noise: f32,
    /// How much velocity changes level and brightness.
    pub velocity: f32,
    /// Second string per course (12-string, mandolin): 0 = off, level of the second string.
    pub doubling: f32,
    pub doubling_detune: f32,
    /// Release time when a key is let go (seconds).
    pub release: f32,
    /// Small random variation of timing, level and tone per note.
    pub humanize: f32,
    // Body / pickup / amp.
    /// The wooden body's resonance (acoustic models).
    pub body: f32,
    /// Body size: smaller = higher, boxier resonances.
    pub body_size: f32,
    /// Electric: pickup position (0 bridge … 1 neck).
    pub pickup: f32,
    /// Amp drive (0 clean … 1 heavy).
    pub drive: f32,
    /// Amp tone (0 dark … 1 bright).
    pub tone: f32,
    /// Sitar-style bridge buzz.
    pub buzz: f32,
    // Playing.
    pub articulation: Articulation,
    pub mode: PlayMode,
    /// Milliseconds between strings in a strum.
    pub strum_time: f32,
    pub strum_dir: StrumDir,
    /// Index into [`PATTERNS`].
    pub pattern: usize,
    /// Chord used when one key is held in chord / pattern mode ([`CHORDS`] index).
    pub single_key_chord: usize,
    /// Glide time of legato slides (seconds).
    pub slide_time: f32,
    pub width: f32,
    pub fx: Vec<FxSlot>,
    pub master: f32,
    /// Level trim in dB (evens out the presets), after the FX.
    pub trim_db: f32,
}

impl Default for GuitarParams {
    fn default() -> Self {
        GuitarParams {
            preset: "Init".into(),
            model: GuitarModel::SteelAcoustic,
            tuning: GuitarModel::SteelAcoustic.default_tuning(),
            capo: 0,
            sustain: 6.0,
            brightness: 0.6,
            pick_position: 0.18,
            pick_hardness: 0.6,
            noise: 0.3,
            velocity: 0.6,
            doubling: 0.0,
            doubling_detune: 4.0,
            release: 0.25,
            humanize: 0.3,
            body: 0.7,
            body_size: 1.0,
            pickup: 0.3,
            drive: 0.0,
            tone: 0.5,
            buzz: 0.0,
            articulation: Articulation::Sustain,
            mode: PlayMode::Solo,
            strum_time: 14.0,
            strum_dir: StrumDir::Alternate,
            pattern: 0,
            single_key_chord: 0,
            slide_time: 0.08,
            width: 0.6,
            fx: Vec::new(),
            master: 0.8,
            trim_db: 0.0,
        }
    }
}

impl GuitarParams {
    /// A fresh patch for `model` with its usual tuning and character.
    pub fn for_model(model: GuitarModel) -> Self {
        let mut p = GuitarParams { model, tuning: model.default_tuning(), ..Default::default() };
        match model {
            GuitarModel::Nylon => {
                p.brightness = 0.35;
                p.pick_hardness = 0.25;
                p.sustain = 5.0;
                p.body = 0.8;
            }
            GuitarModel::TwelveString => {
                p.doubling = 0.7;
                p.brightness = 0.7;
            }
            GuitarModel::Electric => {
                p.body = 0.0;
                p.sustain = 9.0;
                p.drive = 0.15;
                p.brightness = 0.7;
            }
            GuitarModel::Bass => {
                p.body = 0.0;
                p.sustain = 7.0;
                p.brightness = 0.4;
                p.pick_hardness = 0.3;
                p.pickup = 0.4;
            }
            GuitarModel::Banjo => {
                p.sustain = 2.0;
                p.brightness = 0.85;
                p.body = 0.9;
                p.body_size = 0.55;
            }
            GuitarModel::Mandolin => {
                p.doubling = 0.9;
                p.doubling_detune = 3.0;
                p.sustain = 3.0;
                p.brightness = 0.75;
                p.body_size = 0.6;
            }
            GuitarModel::Ukulele => {
                p.brightness = 0.45;
                p.pick_hardness = 0.25;
                p.sustain = 2.5;
                p.body_size = 0.55;
            }
            GuitarModel::Sitar => {
                p.buzz = 0.6;
                p.sustain = 8.0;
                p.brightness = 0.8;
                p.body_size = 1.3;
            }
            GuitarModel::Harp => {
                p.pick_hardness = 0.2;
                p.pick_position = 0.3;
                p.sustain = 8.0;
                p.brightness = 0.45;
                p.body = 0.5;
                p.body_size = 1.4;
            }
            GuitarModel::SteelAcoustic => {}
        }
        p
    }

    /// The tuning with the capo applied.
    pub fn open_strings(&self) -> Vec<u8> {
        self.tuning.iter().map(|n| n + self.capo).collect()
    }
}

/// Named tunings for the editor.
pub const TUNINGS: [(&str, &[u8]); 10] = [
    ("Standard", &[40, 45, 50, 55, 59, 64]),
    ("Drop D", &[38, 45, 50, 55, 59, 64]),
    ("DADGAD", &[38, 45, 50, 55, 57, 62]),
    ("Open G", &[38, 43, 50, 55, 59, 62]),
    ("Open D", &[38, 45, 50, 54, 57, 62]),
    ("Open E", &[40, 47, 52, 56, 59, 64]),
    ("Half step down", &[39, 44, 49, 54, 58, 63]),
    ("Drop C", &[36, 43, 48, 53, 57, 62]),
    ("7-string", &[35, 40, 45, 50, 55, 59, 64]),
    ("Baritone", &[35, 40, 45, 50, 54, 59]),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_are_voiced_with_the_root_in_the_bass() {
        let std = GuitarModel::SteelAcoustic.default_tuning();
        for root in 40..52u8 {
            for (name, iv) in CHORDS {
                let v = voice_chord(&std, root, iv, 20);
                let notes: Vec<u8> = v.iter().flatten().copied().collect();
                assert!(notes.len() >= 3.min(iv.len() + 1), "{name} on {root}: {v:?}");
                let lowest = *notes.iter().min().unwrap();
                assert_eq!(lowest % 12, root % 12, "{name} on {root}: bass {lowest}");
                for n in &notes {
                    assert!(iv.iter().any(|i| (root + i) % 12 == n % 12), "{name}: {n} is not in the chord");
                }
            }
        }
        // Open C major: x 3 2 0 1 0.
        assert_eq!(voice_chord(&std, 48, &[0, 4, 7], 20), vec![None, Some(48), Some(52), Some(55), Some(60), Some(64)]);
        // Open E minor: all six strings.
        assert_eq!(voice_chord(&std, 40, &[0, 3, 7], 20).iter().flatten().count(), 6);
    }

    #[test]
    fn chords_are_recognised_from_the_keys() {
        assert_eq!(detect_chord(&[60, 64, 67]), Some((60, 0)));
        assert_eq!(detect_chord(&[57, 60, 64]), Some((57, 1)));
        assert_eq!(detect_chord(&[55, 59, 62, 65]), Some((55, 2)));
        // An inversion still finds its root.
        assert_eq!(detect_chord(&[52, 55, 60]).map(|c| (c.0 % 12, c.1)), Some((0, 0)));
        assert_eq!(detect_chord(&[50]), Some((50, 0)));
    }

    #[test]
    fn notes_go_to_a_string_low_on_the_neck() {
        let std = GuitarModel::SteelAcoustic.default_tuning();
        assert_eq!(string_for(&std, 40, 20, &[false; 6]), Some((0, 0)));
        assert_eq!(string_for(&std, 64, 20, &[false; 6]), Some((5, 0)));
        // A busy string sends the note to the next one.
        assert_eq!(string_for(&std, 45, 20, &[false, true, false, false, false, false]), Some((0, 5)));
        assert_eq!(string_for(&std, 30, 20, &[false; 6]), None);
    }
}
