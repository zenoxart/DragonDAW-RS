//! Auto-balance: the way a mix engineer sets a static starting balance, as an algorithm.
//!
//! 1. **Measure each source while it plays, before its fader.** Loudness is K-weighted (LUFS) over 400 ms
//!    windows; silence, tails and bleed are left out, and the loud half of what remains is the level the
//!    source has in its busy passages (the chorus, not the average over the song with all its rests).
//! 2. **Know what each source is.** Its role comes from the names of the channel, the tracks and clips
//!    playing into it, its instrument and its samples; with no hint in a name, from its sound (how much of
//!    its energy sits in the low end or the top, how transient it is, how much of the song it plays in).
//!    Every role can be corrected by hand and is remembered on the channel.
//! 3. **Balance by role, relative to the focus.** The lead vocal (or, without one, the kick) is the
//!    reference; every role sits a set number of dB under it: kick and drums just under, snare, bass and
//!    808 close behind, the lead instrument, then chords and backing vocals, then percussion, cymbals,
//!    pads, effects and room sounds furthest back.
//! 4. **Layers share their role's level.** Three pad layers together are as loud as one pad, so each one
//!    is turned down (6 · log10(n) dB).
//! 5. **Keep the low end for the low end.** Anything that is not kick, bass or 808 but carries a lot of
//!    energy under 250 Hz is set 1.5 dB lower so it doesn't cloud the bass.
//! 6. **Leave headroom.** The balanced channels are summed once more (master effects bypassed) and every
//!    fader moves by the same amount so the sum peaks around −6 dBFS and its loud parts sit near −16 LUFS:
//!    room for the master chain, with the balance between the channels unchanged.
//! 7. **Faders, not miracles.** Faders stay between −40 and +10 dB; a source that would need more is
//!    reported, because that gain belongs at the source (clip gain, the instrument's level).
//! 8. **Touch only what's yours to balance.** Muted and silent channels, buses, returns, VCAs and the
//!    master keep their faders; pan, sends and effects are never changed. One undo step.

use crate::dsp::{Biquad, BiquadKind};
use crate::model::{ChannelKind, ClipKind, Id, Instrument, Project};
use serde::{Deserialize, Serialize};

/// What a channel is in the mix: it decides where it sits in the balance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Role {
    LeadVocal,
    Kick,
    /// A whole drum kit or drum loop on one channel.
    Drums,
    Snare,
    Toms,
    Bass,
    Sub808,
    Lead,
    BackingVocal,
    Chords,
    Perc,
    Cymbals,
    Pad,
    Fx,
    Room,
    Other,
}

impl Role {
    pub const ALL: [Role; 16] = [
        Role::LeadVocal,
        Role::Kick,
        Role::Drums,
        Role::Snare,
        Role::Toms,
        Role::Bass,
        Role::Sub808,
        Role::Lead,
        Role::BackingVocal,
        Role::Chords,
        Role::Perc,
        Role::Cymbals,
        Role::Pad,
        Role::Fx,
        Role::Room,
        Role::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Role::LeadVocal => "Lead vocal",
            Role::Kick => "Kick",
            Role::Drums => "Drums (kit / loop)",
            Role::Snare => "Snare / clap",
            Role::Toms => "Toms",
            Role::Bass => "Bass",
            Role::Sub808 => "808 / sub",
            Role::Lead => "Lead instrument",
            Role::BackingVocal => "Backing vocals",
            Role::Chords => "Chords / keys / guitars",
            Role::Perc => "Hats / percussion",
            Role::Cymbals => "Cymbals / overheads",
            Role::Pad => "Pad / strings bed",
            Role::Fx => "FX / risers",
            Role::Room => "Room / ambience",
            Role::Other => "Other",
        }
    }

    /// Where the role sits, in dB of loudness (while playing) under the focus of the mix.
    pub fn target(self) -> f32 {
        match self {
            Role::LeadVocal => 0.0,
            Role::Kick => -1.0,
            Role::Drums => -1.0,
            Role::Snare => -2.0,
            Role::Sub808 => -2.0,
            Role::Bass => -3.0,
            Role::Lead => -4.0,
            Role::Toms => -5.0,
            Role::Chords => -7.0,
            Role::Other => -7.0,
            Role::BackingVocal => -8.0,
            Role::Perc => -10.0,
            Role::Cymbals => -11.0,
            Role::Pad => -12.0,
            Role::Fx => -13.0,
            Role::Room => -14.0,
        }
    }

    /// Kick, bass and 808 own the low end.
    pub fn owns_low_end(self) -> bool {
        matches!(self, Role::Kick | Role::Bass | Role::Sub808 | Role::Drums)
    }
}

/// How a source sounds, measured before its fader.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Profile {
    /// Loudness of its busy passages (LUFS): the loud half of the 400 ms windows in which it plays.
    pub active: f32,
    /// Gated integrated loudness over the song (LUFS).
    pub integrated: f32,
    /// Sample peak (dBFS).
    pub peak_db: f32,
    /// Share of the measured time in which it plays (0 … 1).
    pub presence: f32,
    /// Energy shares below 100 Hz, 100–250 Hz, 250 Hz–2 kHz, 2–6 kHz and above 6 kHz (sum 1).
    pub bands: [f32; 5],
}

impl Default for Profile {
    fn default() -> Self {
        Profile { active: -70.0, integrated: -70.0, peak_db: -120.0, presence: 0.0, bands: [0.0; 5] }
    }
}

impl Profile {
    pub fn silent(&self) -> bool {
        self.active <= -60.0
    }
    /// Energy share under 250 Hz.
    pub fn low_share(&self) -> f32 {
        self.bands[0] + self.bands[1]
    }
    /// Energy share above 2 kHz.
    pub fn high_share(&self) -> f32 {
        self.bands[3] + self.bands[4]
    }
    /// Peak over loudness while playing (dB): high for drums, low for pads.
    pub fn crest(&self) -> f32 {
        self.peak_db - self.active
    }
}

fn lufs(e: f64) -> f32 {
    if e <= 1e-12 {
        -70.0
    } else {
        (-0.691 + 10.0 * e.log10()) as f32
    }
}

/// The measurement of one channel (offline): K-weighted loudness, peak and the spectral balance.
pub struct Analyzer {
    meter: crate::dsp::loudness::LoudnessMeter,
    lp: [Biquad; 4],
    energy: [f64; 5],
    peak: f32,
}

impl Analyzer {
    pub fn new(rate: f32) -> Self {
        let lp = [100.0, 250.0, 2000.0, 6000.0].map(|f| Biquad::new(BiquadKind::LowPass, f, 0.707, 0.0, rate));
        Analyzer { meter: crate::dsp::loudness::LoudnessMeter::exact(rate), lp, energy: [0.0; 5], peak: 0.0 }
    }

    pub fn process(&mut self, l: &[f32], r: &[f32]) {
        self.meter.process(l, r);
        for (a, b) in l.iter().zip(r) {
            self.peak = self.peak.max(a.abs()).max(b.abs());
            let m = (a + b) * 0.5;
            for (k, f) in self.lp.iter_mut().enumerate() {
                let y = f.process(m) as f64;
                self.energy[k] += y * y;
            }
            self.energy[4] += (m * m) as f64;
        }
    }

    pub fn profile(&self) -> Profile {
        let blocks = self.meter.block_energies();
        let windows: Vec<f64> = blocks.windows(4).map(|w| w.iter().sum::<f64>() / 4.0).collect();
        let mut playing: Vec<f64> = windows.iter().copied().filter(|&e| lufs(e) > -50.0).collect();
        let loudest = playing.iter().copied().fold(0.0f64, f64::max);
        // Tails, bleed and fades 30 LU under the loudest moment don't count as playing.
        playing.retain(|&e| lufs(e) > lufs(loudest) - 30.0);
        if playing.is_empty() {
            return Profile::default();
        }
        playing.sort_by(|a, b| a.total_cmp(b));
        let upper = &playing[playing.len() / 2..];
        let active = lufs(upper.iter().sum::<f64>() / upper.len() as f64);
        let e = self.energy;
        let parts = [e[0], (e[1] - e[0]).max(0.0), (e[2] - e[1]).max(0.0), (e[3] - e[2]).max(0.0), (e[4] - e[3]).max(0.0)];
        let sum: f64 = parts.iter().sum::<f64>().max(1e-20);
        Profile { active, integrated: self.meter.integrated(), peak_db: crate::dsp::gain_to_db(self.peak), presence: playing.len() as f32 / windows.len().max(1) as f32, bands: parts.map(|x| (x / sum) as f32) }
    }
}

/// Lower-case words of a name ("Kick_01 (Layer)" → kick, 01, layer).
fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(|w| w.to_lowercase()).collect()
}

/// A key matches a word it starts (≥ 3 letters: "vocal" in "vocals") or equals (shorter: "bd", "hh").
fn hit(ws: &[String], keys: &[&str]) -> bool {
    ws.iter().any(|w| keys.iter().any(|k| if k.len() >= 3 { w.starts_with(k) } else { w == k }))
}

/// The role a set of names points to (most specific first), if any.
pub fn role_from_names(names: &[String]) -> Option<Role> {
    let ws: Vec<String> = names.iter().flat_map(|n| words(n)).collect();
    let joined = names.iter().map(|n| n.to_lowercase()).collect::<Vec<_>>().join(" | ");
    let phrase = |ps: &[&str]| ps.iter().any(|p| joined.contains(p));
    let rules: [(Role, bool); 15] = [
        (
            Role::BackingVocal,
            phrase(&["backing", "back vox", "bgv", "harmony", "harmonies", "choir", "adlib", "ad-lib", "ad lib", "double", "dbl"]) || hit(&ws, &["bv", "bvs", "bgv", "bgvs", "backing", "harmon", "choir", "adlib", "dbl"]),
        ),
        (Role::LeadVocal, hit(&ws, &["vocal", "vox", "voice", "rap", "singer", "acapella", "acappella", "lv"])),
        (Role::Sub808, hit(&ws, &["808", "808s"])),
        (Role::Drums, phrase(&["drum loop", "drum bus", "full drums", "drum kit", "breakbeat"]) || hit(&ws, &["drums", "drumloop", "break", "breaks", "kit", "beat"])),
        (Role::Kick, hit(&ws, &["kick", "kik", "bd", "bassdrum"]) || phrase(&["bass drum"])),
        (Role::Snare, hit(&ws, &["snare", "snr", "clap", "rim", "rimshot", "sd", "snap"])),
        (Role::Toms, hit(&ws, &["tom", "toms", "floortom"])),
        (Role::Room, hit(&ws, &["room", "rooms", "ambience"])),
        (Role::Cymbals, hit(&ws, &["crash", "ride", "cymbal", "overhead", "overheads", "oh", "china", "splash"])),
        (Role::Perc, hit(&ws, &["hat", "hats", "hihat", "hh", "shaker", "tamb", "perc", "conga", "bongo", "cowbell", "clave", "triangle", "top", "tops", "click"])),
        (Role::Bass, hit(&ws, &["bass", "sub", "reese", "bassline"])),
        (Role::Fx, hit(&ws, &["fx", "sfx", "riser", "sweep", "impact", "noise", "uplifter", "downlifter", "whoosh", "reverse", "rev", "transition", "foley", "vinyl"])),
        (Role::Pad, hit(&ws, &["pad", "pads", "atmo", "atmos", "drone", "ambient", "texture", "bed", "strings", "string", "choir"])),
        (Role::Lead, hit(&ws, &["lead", "solo", "melody", "pluck", "arp", "hookline", "topline", "flute", "sax", "whistle"])),
        (Role::Chords, hit(&ws, &["chord", "chords", "piano", "keys", "epiano", "organ", "guitar", "gtr", "acoustic", "synth", "brass", "horn", "horns", "bell", "bells", "mallet", "marimba", "stab", "rhythm"])),
    ];
    rules.iter().find(|(_, m)| *m).map(|(r, _)| *r)
}

/// The role a source's sound points to, when no name says what it is.
pub fn role_from_sound(p: &Profile) -> Role {
    let (low, high, crest) = (p.low_share(), p.high_share(), p.crest());
    if low > 0.65 {
        if crest > 12.0 && p.presence < 0.7 {
            Role::Kick
        } else {
            Role::Bass
        }
    } else if p.presence < 0.15 {
        Role::Fx
    } else if high > 0.55 {
        if crest > 12.0 {
            Role::Perc
        } else {
            Role::Cymbals
        }
    } else if crest > 16.0 {
        Role::Snare
    } else if crest < 9.0 && p.presence > 0.6 {
        Role::Pad
    } else {
        Role::Other
    }
}

/// Every name that says what plays into mixer channel `ch`: the channel, its tracks and their clips (and
/// audio files), the rack channels routed to it, their instruments, samples and pads.
pub fn names_for(p: &Project, ch: Id) -> Vec<String> {
    let mut out = Vec::new();
    let stem = |path: &std::path::Path| path.file_stem().map(|s| s.to_string_lossy().into_owned());
    if let Some(c) = p.mixer.get(ch) {
        out.push(c.name.clone());
    }
    for t in p.tracks.iter().filter(|t| t.mixer == ch) {
        out.push(t.name.clone());
        for c in &t.clips {
            if let ClipKind::Audio(a) = &c.kind {
                out.push(c.name.clone());
                out.extend(stem(&a.source));
            }
        }
    }
    for rc in p.channels.iter().filter(|c| c.mixer == ch) {
        out.push(rc.name.clone());
        match &rc.instrument {
            Instrument::Sampler(s) => out.extend(stem(&s.path)),
            Instrument::DrumPads(_) => out.push("drum kit".into()),
            Instrument::Plugin(pr) => out.push(pr.name.clone()),
            Instrument::Guitar(_) | Instrument::Piano(_) => out.push("chords".into()),
            _ => {}
        }
    }
    out
}

/// One channel's balance.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub id: Id,
    pub name: String,
    pub role: Role,
    /// Where the role came from: a name, the sound, or the user.
    pub source: RoleSource,
    pub profile: Profile,
    /// Fader before (dB).
    pub before_db: f32,
    /// Fader relative to the headroom offset (dB): role target − measured + layer / low-end corrections.
    pub relative_db: f32,
    /// The final fader (dB), within −40 … +10.
    pub fader_db: f32,
    /// The fader hit its limit: the source itself is too quiet (+) or too loud (−) by this much.
    pub short_db: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleSource {
    Name,
    Sound,
    User,
}

pub const FADER_MIN_DB: f32 = -40.0;
pub const FADER_MAX_DB: f32 = 10.0;
/// The sum of the balanced channels, into the master chain: its loud parts near −16 LUFS …
pub const SUM_TARGET_LUFS: f32 = -16.0;
/// … and its peaks no higher than −6 dBFS.
pub const SUM_PEAK_DB: f32 = -6.0;

/// The channels auto-balance may move: those carrying their own signal (not the master, a VCA, a bus or
/// return fed by other channels) that are not muted.
pub fn balanceable(p: &Project) -> Vec<Id> {
    let fed: std::collections::HashSet<Id> = p.mixer.channels.iter().flat_map(|c| c.output.into_iter().chain(c.sends.iter().map(|s| s.target))).collect();
    p.mixer.channels.iter().filter(|c| c.kind != ChannelKind::Vca && !fed.contains(&c.id) && !c.muted).map(|c| c.id).collect()
}

/// The balance (before the headroom offset) from every channel's profile; `roles` are the user's
/// corrections (they win over names and sound).
pub fn plan(p: &Project, profiles: &[(Id, Profile)], roles: &[(Id, Role)]) -> Vec<Entry> {
    let mut out = Vec::new();
    for id in balanceable(p) {
        let Some(&(_, prof)) = profiles.iter().find(|(i, _)| *i == id) else { continue };
        if prof.silent() {
            continue;
        }
        let Some(c) = p.mixer.get(id) else { continue };
        let (role, source) = if let Some(&(_, r)) = roles.iter().find(|(i, _)| *i == id) {
            (r, RoleSource::User)
        } else if let Some(r) = c.balance_role {
            (r, RoleSource::User)
        } else if let Some(r) = role_from_names(std::slice::from_ref(&c.name)).or_else(|| role_from_names(&names_for(p, id))) {
            // The channel's own name first: a channel "Kick" on a track called "Drums" is a kick.
            (r, RoleSource::Name)
        } else {
            (role_from_sound(&prof), RoleSource::Sound)
        };
        let mut rel = role.target() - prof.active;
        if !role.owns_low_end() && prof.low_share() > 0.45 {
            rel -= 1.5;
        }
        out.push(Entry { id, name: c.name.clone(), role, source, profile: prof, before_db: crate::dsp::gain_to_db(c.volume), relative_db: rel, fader_db: rel, short_db: 0.0 });
    }
    // Layers of one role share its level.
    let roles: Vec<Role> = out.iter().map(|e| e.role).collect();
    for e in &mut out {
        let n = roles.iter().filter(|&&r| r == e.role).count();
        if n > 1 {
            e.relative_db -= 6.0 * (n as f32).log10();
        }
    }
    finish(&mut out, 0.0);
    out
}

/// Applies the headroom offset to every fader and keeps them within their range.
pub fn finish(entries: &mut [Entry], offset_db: f32) {
    for e in entries {
        let want = e.relative_db + offset_db;
        e.fader_db = want.clamp(FADER_MIN_DB, FADER_MAX_DB);
        e.short_db = want - e.fader_db;
    }
}

/// The offset that brings the balanced sum (measured with every fader at `relative_db`, master effects
/// bypassed) to the headroom target: loud parts near −16 LUFS, peaks at most −6 dBFS.
pub fn headroom_offset(sum: &Profile) -> f32 {
    if sum.silent() {
        return 0.0;
    }
    (SUM_TARGET_LUFS - sum.active).min(SUM_PEAK_DB - sum.peak_db)
}

/// The project with the planned faders set (before the offset) and the master's effects bypassed: what
/// the second measurement sums.
pub fn trial_project(p: &Project, entries: &[Entry]) -> Project {
    let mut q = p.clone();
    for e in entries {
        if let Some(c) = q.mixer.get_mut(e.id) {
            c.volume = crate::dsp::db_to_gain(e.relative_db);
        }
    }
    for i in &mut q.mixer.master.inserts {
        i.enabled = false;
    }
    q.mixer.master.volume = 1.0;
    q
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prof(active: f32, bands: [f32; 5], peak: f32, presence: f32) -> Profile {
        Profile { active, integrated: active - 2.0, peak_db: peak, presence, bands }
    }
    const MID: [f32; 5] = [0.05, 0.1, 0.6, 0.2, 0.05];

    #[test]
    fn names_say_the_role() {
        let r = |s: &str| role_from_names(&[s.to_string()]);
        assert_eq!(r("Lead Vox"), Some(Role::LeadVocal));
        assert_eq!(r("BV Harmony L"), Some(Role::BackingVocal));
        assert_eq!(r("Kick_01"), Some(Role::Kick));
        assert_eq!(r("808 Glide"), Some(Role::Sub808));
        assert_eq!(r("Hi-Hat closed"), Some(Role::Perc));
        assert_eq!(r("HH"), Some(Role::Perc));
        assert_eq!(r("Snare Top"), Some(Role::Snare));
        assert_eq!(r("Warm Pad"), Some(Role::Pad));
        assert_eq!(r("Riser 8 bars"), Some(Role::Fx));
        assert_eq!(r("Piano chords"), Some(Role::Chords));
        assert_eq!(r("Bass DI"), Some(Role::Bass));
        assert_eq!(r("Subtle thing"), Some(Role::Bass), "prefix match is documented behaviour");
        assert_eq!(r("Insert 3"), None);
    }

    #[test]
    fn sound_says_the_role_without_a_name() {
        assert_eq!(role_from_sound(&prof(-14.0, [0.6, 0.2, 0.15, 0.04, 0.01], 0.0, 0.5)), Role::Kick);
        assert_eq!(role_from_sound(&prof(-14.0, [0.6, 0.2, 0.15, 0.04, 0.01], -8.0, 0.9)), Role::Bass);
        assert_eq!(role_from_sound(&prof(-24.0, [0.0, 0.02, 0.18, 0.4, 0.4], -6.0, 0.8)), Role::Perc);
        assert_eq!(role_from_sound(&prof(-20.0, MID, -14.0, 0.9)), Role::Pad);
        assert_eq!(role_from_sound(&prof(-20.0, MID, -10.0, 0.05)), Role::Fx);
    }

    #[test]
    fn balance_follows_roles_layers_and_low_end() {
        let mut p = Project::new_default();
        let vox = p.add_mixer_channel("Lead Vocal");
        let kick = p.add_mixer_channel("Kick");
        let pad1 = p.add_mixer_channel("Pad A");
        let pad2 = p.add_mixer_channel("Pad B");
        let muddy = p.add_mixer_channel("Keys");
        let anon = p.add_mixer_channel("Insert 9");
        let bus = p.add_mixer_channel("Drum bus");
        let muted = p.add_mixer_channel("Guitar");
        p.mixer.get_mut(kick).unwrap().output = Some(bus);
        p.mixer.get_mut(muted).unwrap().muted = true;
        let profiles = vec![
            (vox, prof(-20.0, MID, -6.0, 0.6)),
            (kick, prof(-12.0, [0.6, 0.2, 0.15, 0.04, 0.01], 0.0, 0.5)),
            (pad1, prof(-30.0, MID, -20.0, 0.9)),
            (pad2, prof(-30.0, MID, -20.0, 0.9)),
            (muddy, prof(-20.0, [0.3, 0.25, 0.35, 0.08, 0.02], -8.0, 0.9)),
            (anon, prof(-20.0, MID, -6.0, 0.6)),
            (bus, prof(-10.0, MID, 0.0, 0.6)),
            (muted, prof(-10.0, MID, 0.0, 0.6)),
        ];
        let plan = plan(&p, &profiles, &[]);
        let get = |id| plan.iter().find(|e| e.id == id);
        let rel = |id| get(id).unwrap().relative_db;
        assert!((rel(vox) - 20.0).abs() < 1e-3, "vocal: 0 − (−20)");
        assert!((rel(kick) - 11.0).abs() < 1e-3, "kick: −1 − (−12)");
        // The kick ends up 1 dB under the vocal in loudness: −12 + 11 = −1 vs −20 + 20 = 0.
        let pad_each = -12.0 + 30.0 - 6.0 * 2f32.log10();
        assert!((rel(pad1) - pad_each).abs() < 1e-3 && (rel(pad2) - pad_each).abs() < 1e-3, "two pad layers share");
        assert!((rel(muddy) - (-7.0 + 20.0 - 1.5)).abs() < 1e-3, "low-heavy keys give way to the bass");
        assert_eq!(get(anon).unwrap().source, RoleSource::Sound);
        assert!(get(bus).is_none() && get(muted).is_none(), "buses and muted channels keep their faders");

        // The user's correction wins.
        let plan2 = super::plan(&p, &profiles, &[(anon, Role::Fx)]);
        let a = plan2.iter().find(|e| e.id == anon).unwrap();
        assert_eq!((a.role, a.source), (Role::Fx, RoleSource::User));
    }

    #[test]
    fn headroom_moves_every_fader_alike_and_limits_report() {
        let sum = prof(-10.0, MID, -2.0, 0.8);
        // Loudness wants −6 dB, the peak −4 dB: the stricter one wins.
        assert!((headroom_offset(&sum) + 6.0).abs() < 1e-3);
        let peaky = prof(-20.0, MID, -1.0, 0.8);
        assert!((headroom_offset(&peaky) + 5.0).abs() < 1e-3);
        let mk = |rel| Entry { id: 1, name: String::new(), role: Role::Other, source: RoleSource::Name, profile: Profile::default(), before_db: 0.0, relative_db: rel, fader_db: 0.0, short_db: 0.0 };
        let mut es = vec![mk(4.0), mk(20.0), mk(-50.0)];
        finish(&mut es, -6.0);
        assert_eq!(es[0].fader_db, -2.0);
        assert_eq!((es[1].fader_db, es[1].short_db), (10.0, 4.0), "needs 4 dB more at the source");
        assert_eq!((es[2].fader_db, es[2].short_db), (-40.0, -16.0));
    }

    #[test]
    fn analyzer_reads_busy_passages_not_the_song_average() {
        let rate = 48000.0;
        // 2 s of a −20 dBFS 1 kHz tone, then 8 s of silence.
        let a = crate::dsp::db_to_gain(-20.0);
        let mut s: Vec<f32> = (0..(rate as usize * 2)).map(|i| (i as f32 * 1000.0 * std::f32::consts::TAU / rate).sin() * a).collect();
        s.extend(std::iter::repeat_n(0.0, rate as usize * 8));
        let mut an = Analyzer::new(rate);
        for c in s.chunks(512) {
            an.process(c, c);
        }
        let pr = an.profile();
        assert!((pr.active + 20.0).abs() < 1.0, "loud while playing: {}", pr.active);
        assert!(pr.presence > 0.1 && pr.presence < 0.3, "{}", pr.presence);
        assert!(pr.bands[2] > 0.8, "a 1 kHz tone is mid: {:?}", pr.bands);
        assert!((pr.peak_db + 20.0).abs() < 0.2);
    }
}
