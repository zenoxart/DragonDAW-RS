//! Editing operations on the model, kept free of UI code so they can be tested and reused (playlist, piano
//! roll, comping, slicing, quantizing, chord and arpeggio tools).

use crate::model::*;
use crate::time::TempoMap;

// ---------------------------------------------------------------------------------------------------- clips

/// Splits a clip at `beat` (strictly inside). Returns the id of the new right-hand clip.
pub fn split_clip(p: &mut Project, track: Id, clip: Id, beat: f64) -> Option<Id> {
    let tempo = TempoMap::from_project(p);
    let new_id = p.next_id();
    let bpm = p.bpm;
    let t = p.track_mut(track)?;
    let i = t.clips.iter().position(|c| c.id == clip)?;
    let c = &t.clips[i];
    if beat <= c.start + 1e-6 || beat >= c.end() - 1e-6 {
        return None;
    }
    let mut right = c.clone();
    right.id = new_id;
    right.start = beat;
    right.length = c.end() - beat;
    let cut = beat - c.start;
    if let (ClipKind::Audio(a), ClipKind::Audio(ra)) = (&c.kind, &mut right.kind) {
        // Advance the source offset by the elapsed time (in source seconds).
        let secs = tempo.beat_to_seconds(beat) - tempo.beat_to_seconds(c.start);
        let rate = match a.stretch {
            StretchMode::Resample => a.speed * 2f64.powf(a.pitch as f64 / 12.0),
            StretchMode::Stretch => a.speed * a.source_bpm.map(|b| bpm / b).unwrap_or(1.0),
        };
        ra.offset = a.offset + secs * rate;
        ra.fade_in = 0.0;
        ra.gain_line = a.gain_line.iter().filter(|g| g.beat >= cut).map(|g| GainPoint { beat: g.beat - cut, db: g.db }).collect();
    }
    let left = &mut t.clips[i];
    left.length = cut;
    if let ClipKind::Audio(a) = &mut left.kind {
        a.fade_out = 0.0;
        a.gain_line.retain(|g| g.beat <= cut);
    }
    t.clips.insert(i + 1, right);
    Some(new_id)
}

/// Duplicates clips directly after the selection's end (Ctrl+B). Returns the new ids.
pub fn duplicate_clips(p: &mut Project, ids: &[Id]) -> Vec<Id> {
    let mut out = Vec::new();
    let sel: Vec<(Id, Clip)> = p.tracks.iter().flat_map(|t| t.clips.iter().filter(|c| ids.contains(&c.id)).map(move |c| (t.id, c.clone()))).collect();
    if sel.is_empty() {
        return out;
    }
    let start = sel.iter().map(|(_, c)| c.start).fold(f64::MAX, f64::min);
    let end = sel.iter().map(|(_, c)| c.end()).fold(f64::MIN, f64::max);
    let shift = end - start;
    for (tid, mut c) in sel {
        c.id = p.next_id();
        c.start += shift;
        out.push(c.id);
        if let Some(t) = p.track_mut(tid) {
            t.clips.push(c);
        }
    }
    out
}

/// Copies clips in place (same track and position) and returns the copies' ids in the order of `ids`
/// (the Shift + drag: the copies are then dragged away, the originals stay).
pub fn copy_clips_in_place(p: &mut Project, ids: &[Id]) -> Vec<Id> {
    let mut out = Vec::new();
    for want in ids {
        let Some((tid, c)) = p.tracks.iter().find_map(|t| t.clips.iter().find(|c| c.id == *want).map(|c| (t.id, c.clone()))) else { continue };
        let mut c = c;
        c.id = p.next_id();
        out.push(c.id);
        if let Some(t) = p.track_mut(tid) {
            t.clips.push(c);
        }
    }
    out
}

/// Removes clips by id from every track.
pub fn delete_clips(p: &mut Project, ids: &[Id]) {
    for t in &mut p.tracks {
        t.clips.retain(|c| !ids.contains(&c.id));
    }
}

/// Shuffle edit: moves every clip on the track that starts at or after `from` by `delta` beats.
/// Moves the clips `ids` (all on the track `from`) onto a new track right below it, in an edit group with
/// it and routed to the same mixer channel. The source track joins the group (or brings its group).
/// Returns the new track.
pub fn clips_to_grouped_track(p: &mut Project, from: Id, ids: &[Id]) -> Option<Id> {
    let fi = p.tracks.iter().position(|t| t.id == from)?;
    if !p.tracks[fi].clips.iter().any(|c| ids.contains(&c.id)) {
        return None;
    }
    let group = match p.tracks[fi].group {
        Some(g) => g,
        None => {
            let g = p.next_id();
            p.tracks[fi].group = Some(g);
            g
        }
    };
    let id = p.next_id();
    let src = &p.tracks[fi];
    let n = p.tracks.iter().filter(|t| t.group == Some(group)).count() + 1;
    let mut t = Track {
        id,
        name: format!("{} {n}", src.name.trim_end_matches(|c: char| c.is_ascii_digit() || c == ' ')),
        color: src.color,
        height: src.height,
        muted: src.muted,
        solo: src.solo,
        armed: false,
        input: src.input,
        monitor: src.monitor,
        mixer: src.mixer,
        clips: Vec::new(),
        takes: Vec::new(),
        show_takes: false,
        group: Some(group),
        pinned: false,
        audio: src.audio,
        frozen: None,
    };
    let src = &mut p.tracks[fi];
    let mut k = 0;
    while k < src.clips.len() {
        if ids.contains(&src.clips[k].id) {
            t.clips.push(src.clips.remove(k));
        } else {
            k += 1;
        }
    }
    // Right below the last track of its group.
    let at = p.tracks.iter().rposition(|x| x.group == Some(group)).map(|i| i + 1).unwrap_or(fi + 1);
    p.tracks.insert(at, t);
    Some(id)
}

pub fn shuffle(p: &mut Project, track: Id, from: f64, delta: f64) {
    if let Some(t) = p.track_mut(track) {
        for c in t.clips.iter_mut().filter(|c| c.start >= from - 1e-9) {
            c.start = (c.start + delta).max(0.0);
        }
    }
}

/// Ctrl+Insert: inserts `range.len()` beats of silence at `range.start` on every track (clips across
/// the point are split), moving markers too.
pub fn insert_time(p: &mut Project, range: Range) {
    let tracks: Vec<Id> = p.tracks.iter().map(|t| t.id).collect();
    for t in tracks {
        let ids: Vec<Id> = p.track(t).map(|t| t.clips.iter().map(|c| c.id).collect()).unwrap_or_default();
        for id in ids {
            split_clip(p, t, id, range.start);
        }
        shuffle(p, t, range.start, range.len());
    }
    for m in p.markers.iter_mut().filter(|m| m.beat >= range.start) {
        m.beat += range.len();
    }
}

/// Ctrl+Delete: deletes `range` on every track and closes the gap, moving markers too.
pub fn delete_time(p: &mut Project, range: Range) {
    let tracks: Vec<Id> = p.tracks.iter().map(|t| t.id).collect();
    for t in tracks {
        delete_range_shuffle(p, t, range);
    }
    p.markers.retain(|m| m.beat < range.start || m.beat >= range.end);
    for m in p.markers.iter_mut().filter(|m| m.beat >= range.end) {
        m.beat -= range.len();
    }
}

/// Deletes a time range on a track and closes the gap (shuffle delete).
pub fn delete_range_shuffle(p: &mut Project, track: Id, range: Range) {
    let ids: Vec<Id> = p.track(track).map(|t| t.clips.iter().map(|c| c.id).collect()).unwrap_or_default();
    for id in ids {
        split_clip(p, track, id, range.start);
    }
    let ids: Vec<Id> = p.track(track).map(|t| t.clips.iter().map(|c| c.id).collect()).unwrap_or_default();
    for id in ids {
        split_clip(p, track, id, range.end);
    }
    if let Some(t) = p.track_mut(track) {
        t.clips.retain(|c| !(c.start >= range.start - 1e-9 && c.end() <= range.end + 1e-9));
    }
    shuffle(p, track, range.end, -range.len());
}

/// Sets clip gain so the clip's peak reaches `target_db` (Normalize).
pub fn normalize_clip(c: &mut Clip, peak: f32, target_db: f32) {
    if let ClipKind::Audio(a) = &mut c.kind {
        if peak > 1e-6 {
            a.gain_db = target_db - crate::dsp::gain_to_db(peak);
        }
    }
}

// ---------------------------------------------------------------------------------------------------- comping

/// Moves the current main-lane clips into a new take (used before recording another pass).
pub fn push_take(p: &mut Project, track: Id, name: &str) -> Option<Id> {
    let id = p.next_id();
    let t = p.track_mut(track)?;
    let clips = std::mem::take(&mut t.clips);
    t.takes.push(Take { id, name: name.into(), clips });
    Some(id)
}

/// Comping: copies `range` of a take onto the main lane, replacing whatever was there.
pub fn comp_promote(p: &mut Project, track: Id, take: Id, range: Range) -> bool {
    // Cut the main lane at the range edges and remove what lies inside.
    let ids: Vec<Id> = p.track(track).map(|t| t.clips.iter().map(|c| c.id).collect()).unwrap_or_default();
    for id in &ids {
        split_clip(p, track, *id, range.start);
    }
    let ids: Vec<Id> = p.track(track).map(|t| t.clips.iter().map(|c| c.id).collect()).unwrap_or_default();
    for id in &ids {
        split_clip(p, track, *id, range.end);
    }
    let Some(take_clips) = p.track(track).and_then(|t| t.takes.iter().find(|k| k.id == take)).map(|k| k.clips.clone()) else { return false };
    if let Some(t) = p.track_mut(track) {
        t.clips.retain(|c| !(c.start >= range.start - 1e-9 && c.end() <= range.end + 1e-9));
    }
    // Copy the overlapping parts of the take's clips into a scratch track, trim them to the range.
    let scratch_id = p.next_id();
    let Some(mut scratch) = p.track(track).cloned() else { return false };
    scratch.id = scratch_id;
    scratch.clips = take_clips.into_iter().filter(|c| c.end() > range.start && c.start < range.end).collect();
    for c in &mut scratch.clips {
        c.id = p.next_id();
    }
    p.tracks.push(scratch);
    let ids: Vec<Id> = p.track(scratch_id).unwrap().clips.iter().map(|c| c.id).collect();
    for id in ids {
        split_clip(p, scratch_id, id, range.start);
    }
    let ids: Vec<Id> = p.track(scratch_id).unwrap().clips.iter().map(|c| c.id).collect();
    for id in ids {
        split_clip(p, scratch_id, id, range.end);
    }
    let scratch = p.tracks.pop().unwrap();
    let mut pieces: Vec<Clip> = scratch.clips.into_iter().filter(|c| c.start >= range.start - 1e-9 && c.end() <= range.end + 1e-9).collect();
    for c in &mut pieces {
        if let ClipKind::Audio(a) = &mut c.kind {
            a.take = Some(take);
        }
    }
    if let Some(t) = p.track_mut(track) {
        t.clips.extend(pieces);
        t.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
    }
    true
}

// ---------------------------------------------------------------------------------------------------- notes

/// Musical scales (intervals from the root).
pub const SCALES: &[(&str, &[u8])] = &[
    ("Major", &[0, 2, 4, 5, 7, 9, 11]),
    ("Minor", &[0, 2, 3, 5, 7, 8, 10]),
    ("Harmonic minor", &[0, 2, 3, 5, 7, 8, 11]),
    ("Dorian", &[0, 2, 3, 5, 7, 9, 10]),
    ("Phrygian", &[0, 1, 3, 5, 7, 8, 10]),
    ("Mixolydian", &[0, 2, 4, 5, 7, 9, 10]),
    ("Pentatonic major", &[0, 2, 4, 7, 9]),
    ("Pentatonic minor", &[0, 3, 5, 7, 10]),
    ("Blues", &[0, 3, 5, 6, 7, 10]),
    ("Chromatic", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]),
    ("Lydian", &[0, 2, 4, 6, 7, 9, 11]),
    ("Locrian", &[0, 1, 3, 5, 6, 8, 10]),
    ("Melodic minor", &[0, 2, 3, 5, 7, 9, 11]),
    ("Harmonic major", &[0, 2, 4, 5, 7, 8, 11]),
    ("Phrygian dominant", &[0, 1, 4, 5, 7, 8, 10]),
    ("Hungarian minor", &[0, 2, 3, 6, 7, 8, 11]),
    ("Double harmonic", &[0, 1, 4, 5, 7, 8, 11]),
    ("Neapolitan minor", &[0, 1, 3, 5, 7, 8, 11]),
    ("Whole tone", &[0, 2, 4, 6, 8, 10]),
    ("Diminished (H-W)", &[0, 1, 3, 4, 6, 7, 9, 10]),
    ("Diminished (W-H)", &[0, 2, 3, 5, 6, 8, 9, 11]),
    ("Major blues", &[0, 2, 3, 4, 7, 9]),
    ("Bebop dominant", &[0, 2, 4, 5, 7, 9, 10, 11]),
    ("Hirajoshi", &[0, 2, 3, 7, 8]),
    ("In (Japanese)", &[0, 1, 5, 7, 8]),
    ("Egyptian", &[0, 2, 5, 7, 10]),
];

pub fn in_scale(pitch: u8, root: u8, scale: &[u8]) -> bool {
    scale.contains(&((pitch as i32 - root as i32).rem_euclid(12) as u8))
}

/// Snaps a pitch to the nearest note of the scale.
pub fn snap_to_scale(pitch: u8, root: u8, scale: &[u8]) -> u8 {
    for d in 0..12i32 {
        for s in [-d, d] {
            let q = (pitch as i32 + s).clamp(0, 127) as u8;
            if in_scale(q, root, scale) {
                return q;
            }
        }
    }
    pitch
}

/// Quantizes start (and optionally end) of notes to a grid in ticks with a strength 0 … 1.
pub fn quantize(notes: &mut [Note], grid: u32, strength: f32, ends: bool) {
    let g = grid.max(1) as f32;
    let s = strength.clamp(0.0, 1.0);
    for n in notes {
        let target = (n.start as f32 / g).round() * g;
        let new_start = (n.start as f32 + (target - n.start as f32) * s).round().max(0.0) as u32;
        let end = n.end();
        n.start = new_start;
        if ends {
            let te = (end as f32 / g).round() * g;
            let ne = (end as f32 + (te - end as f32) * s).round() as u32;
            n.length = ne.saturating_sub(n.start).max(1);
        }
    }
}

/// Extends every note to the next note's start (legato).
pub fn legato(notes: &mut [Note]) {
    let mut order: Vec<usize> = (0..notes.len()).collect();
    order.sort_by_key(|&i| notes[i].start);
    for w in 0..order.len() {
        let i = order[w];
        let next = order[w + 1..].iter().map(|&j| notes[j].start).find(|&s| s > notes[i].start);
        if let Some(ns) = next {
            notes[i].length = ns - notes[i].start;
        }
    }
}

pub fn transpose(notes: &mut [Note], semitones: i32) {
    for n in notes {
        n.pitch = (n.pitch as i32 + semitones).clamp(0, 127) as u8;
    }
}

/// Chord types for the chord stamp: name and intervals.
pub const CHORDS: &[(&str, &[i32])] = &[
    ("Major", &[0, 4, 7]),
    ("Minor", &[0, 3, 7]),
    ("Diminished", &[0, 3, 6]),
    ("Augmented", &[0, 4, 8]),
    ("Sus2", &[0, 2, 7]),
    ("Sus4", &[0, 5, 7]),
    ("Major 7", &[0, 4, 7, 11]),
    ("Minor 7", &[0, 3, 7, 10]),
    ("Dominant 7", &[0, 4, 7, 10]),
    ("Minor 9", &[0, 3, 7, 10, 14]),
    ("Major 9", &[0, 4, 7, 11, 14]),
    ("Power", &[0, 7, 12]),
];

/// Chord stamp: notes of `chord` at `root`. `top_down` voices the chord downwards from the root.
pub fn chord(root: u8, chord: &[i32], start: u32, length: u32, velocity: f32, top_down: bool) -> Vec<Note> {
    let top = *chord.iter().max().unwrap_or(&0);
    chord
        .iter()
        .map(|i| {
            let p = if top_down { root as i32 - top + i } else { root as i32 + i };
            Note { pitch: p.clamp(0, 127) as u8, start, length, velocity, ..Note::default() }
        })
        .collect()
}

/// Diatonic triad on a scale degree (0-based) of `root` in `scale` (for the chord progression tool).
pub fn diatonic_chord(root: u8, scale: &[u8], degree: usize, sevenths: bool) -> Vec<u8> {
    let n = scale.len();
    let at = |k: usize| -> u8 {
        let octave = (k / n) as i32;
        (root as i32 + scale[k % n] as i32 + 12 * octave).clamp(0, 127) as u8
    };
    let mut v = vec![at(degree), at(degree + 2), at(degree + 4)];
    if sevenths {
        v.push(at(degree + 6));
    }
    v
}

/// Builds a progression (scale degrees, 0-based) as notes, one chord per `bar_ticks`, plus an optional
/// bass line (root an octave below).
pub fn progression(root: u8, scale: &[u8], degrees: &[usize], bar_ticks: u32, bass: bool) -> Vec<Note> {
    let mut out = Vec::new();
    for (i, d) in degrees.iter().enumerate() {
        let start = i as u32 * bar_ticks;
        let pitches = diatonic_chord(root, scale, *d, false);
        for &p in &pitches {
            out.push(Note { pitch: p, start, length: bar_ticks, velocity: 0.75, ..Note::default() });
        }
        if bass {
            out.push(Note { pitch: pitches[0].saturating_sub(24), start, length: bar_ticks, velocity: 0.85, color: 1, ..Note::default() });
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArpMode {
    Up,
    Down,
    UpDown,
    Random,
    Chord,
}

/// Arpeggiates the notes that sound together: each chord becomes a run of `step` ticks with `gate` 0 … 1.
pub fn arpeggiate(notes: &[Note], mode: ArpMode, step: u32, gate: f32, octaves: u32) -> Vec<Note> {
    let mut chords: std::collections::BTreeMap<(u32, u32), Vec<Note>> = std::collections::BTreeMap::new();
    for n in notes {
        chords.entry((n.start, n.length)).or_default().push(*n);
    }
    let mut rng = crate::dsp::Rng::new(42);
    let mut out = Vec::new();
    for ((start, length), mut ns) in chords {
        ns.sort_by_key(|n| n.pitch);
        let mut seq: Vec<Note> = Vec::new();
        for o in 0..octaves.max(1) {
            for n in &ns {
                seq.push(Note { pitch: (n.pitch as u32 + 12 * o).min(127) as u8, ..*n });
            }
        }
        let order: Vec<Note> = match mode {
            ArpMode::Up | ArpMode::Chord | ArpMode::Random => seq.clone(),
            ArpMode::Down => seq.iter().rev().copied().collect(),
            ArpMode::UpDown => {
                let mut v = seq.clone();
                if seq.len() > 2 {
                    v.extend(seq[1..seq.len() - 1].iter().rev().copied());
                }
                v
            }
        };
        let steps = (length / step.max(1)).max(1);
        for k in 0..steps {
            let t = start + k * step;
            let len = ((step as f32 * gate).round() as u32).max(1);
            if mode == ArpMode::Chord {
                for n in &ns {
                    out.push(Note { start: t, length: len, ..*n });
                }
            } else {
                let n = if mode == ArpMode::Random { order[(rng.next_u64() as usize) % order.len()] } else { order[k as usize % order.len()] };
                out.push(Note { start: t, length: len, ..n });
            }
        }
    }
    out
}

/// Strum: offsets the notes of each chord by `ticks` per voice (upwards).
pub fn strum(notes: &mut [Note], ticks: u32) {
    let mut by_start: std::collections::BTreeMap<u32, Vec<usize>> = std::collections::BTreeMap::new();
    for (i, n) in notes.iter().enumerate() {
        by_start.entry(n.start).or_default().push(i);
    }
    for (_, mut idx) in by_start {
        idx.sort_by_key(|&i| notes[i].pitch);
        for (k, &i) in idx.iter().enumerate() {
            let shift = k as u32 * ticks;
            notes[i].start += shift;
            notes[i].length = notes[i].length.saturating_sub(shift).max(1);
        }
    }
}

/// Flam: adds a quieter grace note `ticks` before every note.
pub fn flam(notes: &[Note], ticks: u32) -> Vec<Note> {
    let mut out = notes.to_vec();
    for n in notes {
        if n.start >= ticks {
            out.push(Note { start: n.start - ticks, length: ticks.max(1), velocity: n.velocity * 0.6, ..*n });
        }
    }
    out
}

/// Humanize: random timing (± ticks) and velocity (± amount).
pub fn humanize(notes: &mut [Note], ticks: u32, velocity: f32, seed: u64) {
    let mut rng = crate::dsp::Rng::new(seed);
    for n in notes {
        let dt = (rng.bipolar() * ticks as f32).round() as i64;
        n.start = (n.start as i64 + dt).max(0) as u32;
        n.velocity = (n.velocity + rng.bipolar() * velocity).clamp(0.05, 1.0);
    }
}

/// Splits a note at a tick (slice tool). Returns the right part.
pub fn split_note(n: &mut Note, at: u32) -> Option<Note> {
    if at <= n.start || at >= n.end() {
        return None;
    }
    let right = Note { start: at, length: n.end() - at, ..*n };
    n.length = at - n.start;
    Some(right)
}

/// Converts the steps of a pattern channel into piano-roll notes (and clears the steps).
pub fn steps_to_notes(pc: &mut PatternChannel, root: u8) {
    let step_ticks = PPQ / 4;
    for (i, s) in pc.steps.iter_mut().enumerate() {
        if s.on {
            pc.notes.push(Note { pitch: (root as i32 + s.pitch as i32).clamp(0, 127) as u8, start: i as u32 * step_ticks, length: step_ticks, velocity: s.velocity, pan: s.pan, ..Note::default() });
            s.on = false;
        }
    }
}

// ---------------------------------------------------------------------------------------------------- slicing

/// Turns an audio file into a sliced sampler channel plus a pattern that plays the slices in order
/// (Slicex / Fruity Slicer). Returns (channel id, pattern id).
pub fn slice_to_rack(p: &mut Project, path: &std::path::Path, data: &crate::samples::AudioData, sensitivity: f32) -> (Id, Id) {
    let slices = crate::instruments::sampler_slices(data, sensitivity, 64);
    let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Slices".into());
    let ch = p.add_channel(&name, Instrument::Sampler(SamplerParams { path: path.to_path_buf(), root_note: 60, slices: slices.clone(), ..Default::default() }));
    let pat = p.add_pattern();
    let secs = data.duration();
    let beats = secs * p.bpm / 60.0;
    let Some(pattern) = p.pattern_mut(pat) else { return (ch, pat) };
    pattern.name = format!("{name} slices");
    pattern.steps = ((beats * 4.0).ceil() as u32).clamp(4, 256);
    let mut notes = Vec::new();
    for (i, s) in slices.iter().enumerate() {
        let start = (*s as f64 * beats * PPQ as f64).round() as u32;
        let end = slices.get(i + 1).map(|e| (*e as f64 * beats * PPQ as f64).round() as u32).unwrap_or((beats * PPQ as f64) as u32);
        notes.push(Note { pitch: (60 + i).min(127) as u8, start, length: end.saturating_sub(start).max(1), ..Note::default() });
    }
    pattern.channels.insert(ch, PatternChannel { steps: Vec::new(), notes });
    (ch, pat)
}

// ---------------------------------------------------------------------------------------------- patterns

/// A free pattern name: `base #2`, `base #3`, … (Studio's naming for clones and unique copies).
fn unique_pattern_name(p: &Project, base: &str) -> String {
    let root = match base.rfind(" #") {
        Some(i) if base[i + 2..].parse::<u32>().is_ok() => &base[..i],
        _ => base,
    };
    (2..).map(|n| format!("{root} #{n}")).find(|n| !p.patterns.iter().any(|x| &x.name == n)).unwrap()
}

/// Copies a pattern (inserted right after it) and returns the copy's id.
pub fn clone_pattern(p: &mut Project, pat: Id) -> Option<Id> {
    let k = p.patterns.iter().position(|x| x.id == pat)?;
    let mut c = p.patterns[k].clone();
    c.id = p.next_id();
    c.name = unique_pattern_name(p, &c.name);
    let id = c.id;
    p.patterns.insert(k + 1, c);
    Some(id)
}

fn pattern_clips_mut(p: &mut Project) -> impl Iterator<Item = &mut Clip> {
    p.tracks.iter_mut().flat_map(|t| t.clips.iter_mut()).filter(|c| matches!(c.kind, ClipKind::Pattern { .. }))
}

fn clip_pattern(c: &Clip) -> Option<Id> {
    match c.kind {
        ClipKind::Pattern { pattern } => Some(pattern),
        _ => None,
    }
}

/// "Make unique": every given pattern clip gets its own copy of its pattern, so editing it no longer changes
/// the other clips. Returns the new pattern ids (in clip order).
pub fn make_unique(p: &mut Project, clips: &[Id]) -> Vec<Id> {
    let targets: Vec<(Id, Id)> = p.tracks.iter().flat_map(|t| t.clips.iter()).filter(|c| clips.contains(&c.id)).filter_map(|c| clip_pattern(c).map(|pat| (c.id, pat))).collect();
    let mut out = Vec::new();
    for (cid, pat) in targets {
        let Some(new) = clone_pattern(p, pat) else { continue };
        let name = p.pattern(new).map(|x| x.name.clone()).unwrap_or_default();
        if let Some(c) = pattern_clips_mut(p).find(|c| c.id == cid) {
            c.kind = ClipKind::Pattern { pattern: new };
            c.name = name;
        }
        out.push(new);
    }
    out
}

/// Points pattern clips at another pattern ("select source pattern").
pub fn set_clip_pattern(p: &mut Project, clips: &[Id], pat: Id) {
    let name = p.pattern(pat).map(|x| x.name.clone()).unwrap_or_default();
    for c in pattern_clips_mut(p).filter(|c| clips.contains(&c.id)) {
        c.kind = ClipKind::Pattern { pattern: pat };
        c.name = name.clone();
    }
}

/// "Split by channel": one pattern per channel that has steps or notes in `pat`, named and coloured after the
/// channel. Every playlist clip of `pat` is replaced by one clip per new pattern, stacked on that track and the
/// tracks below it (tracks are added when needed); the original pattern is removed. Returns the new patterns.
pub fn split_by_channel(p: &mut Project, pat: Id) -> Vec<Id> {
    let Some(k) = p.patterns.iter().position(|x| x.id == pat) else { return Vec::new() };
    let src = p.patterns[k].clone();
    let used: Vec<(Id, PatternChannel)> = src.channels.iter().filter(|(_, c)| c.steps.iter().any(|s| s.on) || !c.notes.is_empty()).map(|(id, c)| (*id, c.clone())).collect();
    if used.len() < 2 {
        return Vec::new();
    }
    let len = src.length_beats();
    let mut new_ids = Vec::new();
    let mut new_patterns = Vec::new();
    for (ch, content) in &used {
        let id = p.next_id();
        let (name, color) = p.channel(*ch).map(|c| (c.name.clone(), c.color)).unwrap_or((format!("{} part", src.name), src.color));
        // Keep the original length so every part loops like the whole did.
        let mut part = Pattern { id, name: format!("{} – {}", src.name, name), color, steps: src.steps, swing: src.swing, channels: [(*ch, content.clone())].into_iter().collect() };
        if part.length_beats() < len {
            part.steps = (len * 4.0).round() as u32;
        }
        new_ids.push(id);
        new_patterns.push(part);
    }
    p.patterns.splice(k..=k, new_patterns);
    // Replace the clips: the first part where the clip was, the others on the tracks below – an empty one,
    // or a new track inserted right there when the next one already holds other material (the parts stay
    // together and never land in someone else's track).
    let mut placements: Vec<(Id, Clip)> = Vec::new();
    for t in &p.tracks {
        for c in t.clips.iter().filter(|c| clip_pattern(c) == Some(pat)) {
            placements.push((t.id, c.clone()));
        }
    }
    for t in p.tracks.iter_mut() {
        t.clips.retain(|c| clip_pattern(c) != Some(pat));
    }
    // Tracks this split filled (they may take further parts of other clips at other times).
    let mut ours: Vec<Id> = Vec::new();
    for (tid, c) in placements {
        let Some(mut ti) = p.tracks.iter().position(|t| t.id == tid) else { continue };
        for (j, id) in new_ids.iter().enumerate() {
            if j > 0 {
                ti += 1;
                let free = p.tracks.get(ti).is_some_and(|t| t.clips.is_empty() || ours.contains(&t.id) && !t.clips.iter().any(|x| x.start < c.end() && x.end() > c.start));
                if !free {
                    let n = p.tracks.len() + 1;
                    p.add_track(&format!("Track {n}"));
                    let t = p.tracks.pop().expect("just added");
                    p.tracks.insert(ti, t);
                }
            }
            let cid = p.next_id();
            let (name, color) = p.pattern(*id).map(|x| (x.name.clone(), x.color)).unwrap_or_default();
            ours.push(p.tracks[ti].id);
            p.tracks[ti].clips.push(Clip { id: cid, name, start: c.start, length: c.length, muted: c.muted, color, kind: ClipKind::Pattern { pattern: *id } });
        }
    }
    new_ids
}

/// "Split pads into channels": a drum-pads channel becomes one channel per pad that is played anywhere
/// (notes in any pattern, or steps – they play note 60). Each new channel is a drum-pads instrument with
/// just that pad (its layers, gain, pan, pitch), plays into the same mixer channel with the source's
/// volume, pan, pitch and envelope, and gets that pad's notes in every pattern – the song sounds the same,
/// but every sound has its own row, notes and settings. The new channels take the source's place in the
/// rack; the source is removed. Returns the new channels (empty when it's not a drum-pads channel or
/// fewer than two pads are used).
pub fn split_drum_pads(p: &mut Project, ch: Id) -> Vec<Id> {
    let Some(ci) = p.channels.iter().position(|c| c.id == ch) else { return Vec::new() };
    let src = p.channels[ci].clone();
    let Instrument::DrumPads(pads) = &src.instrument else { return Vec::new() };
    let pads = pads.clone();
    let pad_of = |note: u8| pad_for_note(&pads, note);
    // Which pads are played.
    let mut used: Vec<usize> = Vec::new();
    for pt in &p.patterns {
        if let Some(pc) = pt.channels.get(&ch) {
            for n in &pc.notes {
                if let Some(k) = pad_of(n.pitch) {
                    if !used.contains(&k) {
                        used.push(k);
                    }
                }
            }
            if pc.steps.iter().any(|s| s.on) {
                if let Some(k) = pad_of(60) {
                    if !used.contains(&k) {
                        used.push(k);
                    }
                }
            }
        }
    }
    if used.len() < 2 {
        return Vec::new();
    }
    used.sort_by_key(|&k| pads[k].note);
    let mut new_ids = Vec::new();
    let mut new_channels = Vec::new();
    for (j, &k) in used.iter().enumerate() {
        let id = p.next_id();
        let pad = &pads[k];
        let name = if pad.name.is_empty() { format!("{} {}", src.name, j + 1) } else { pad.name.clone() };
        let mut c = src.clone();
        c.id = id;
        c.name = name;
        c.color = PALETTE[(ci + j + 3) % PALETTE.len()];
        c.instrument = Instrument::DrumPads(vec![pad.clone()]);
        c.group = src.group.clone();
        new_ids.push((k, id));
        new_channels.push(c);
    }
    p.channels.splice(ci..=ci, new_channels);
    for pt in p.patterns.iter_mut() {
        let Some(pc) = pt.channels.remove(&ch) else { continue };
        for &(k, id) in &new_ids {
            let notes: Vec<Note> = pc.notes.iter().filter(|n| pad_of(n.pitch) == Some(k)).copied().collect();
            let steps = if pad_of(60) == Some(k) { pc.steps.clone() } else { Vec::new() };
            if !notes.is_empty() || steps.iter().any(|s| s.on) {
                pt.channels.insert(id, PatternChannel { steps, notes });
            }
        }
    }
    new_ids.into_iter().map(|(_, id)| id).collect()
}

/// Cuts a pattern in two at `at` beats (on the step grid; on a bar line when it has notes, because note
/// patterns round their length to bars). Notes crossing the cut are split. Playlist clips of the pattern are
/// replaced by clips of the two halves that play exactly what was heard before (also across loop repeats).
/// Returns the two new patterns.
pub fn split_pattern_at(p: &mut Project, pat: Id, at: f64) -> Option<(Id, Id)> {
    let k = p.patterns.iter().position(|x| x.id == pat)?;
    let src = p.patterns[k].clone();
    let len = src.length_beats();
    let has_notes = src.channels.values().any(|c| !c.notes.is_empty());
    let grid = if has_notes { 4.0 } else { 0.25 };
    if at <= 0.0 || at >= len || ((at / grid).round() * grid - at).abs() > 1e-9 {
        return None;
    }
    let cut_step = (at * 4.0).round() as usize;
    let cut_tick = (at * PPQ as f64).round() as u32;
    let total_steps = (len * 4.0).round() as usize;
    let (a_id, b_id) = (p.next_id(), p.next_id());
    let mut a = Pattern { id: a_id, name: format!("{} A", src.name), color: src.color, steps: cut_step as u32, swing: src.swing, channels: std::collections::BTreeMap::new() };
    let mut b = Pattern { id: b_id, name: format!("{} B", src.name), color: src.color, steps: (total_steps - cut_step) as u32, swing: src.swing, channels: std::collections::BTreeMap::new() };
    for (ch, c) in &src.channels {
        let mut steps = c.steps.clone();
        steps.resize(total_steps.max(steps.len()), Step::default());
        let (mut na, mut nb) = (Vec::new(), Vec::new());
        for n in &c.notes {
            if n.end() <= cut_tick {
                na.push(*n);
            } else if n.start >= cut_tick {
                nb.push(Note { start: n.start - cut_tick, ..*n });
            } else {
                na.push(Note { length: cut_tick - n.start, ..*n });
                nb.push(Note { start: 0, length: n.end() - cut_tick, ..*n });
            }
        }
        a.channels.insert(*ch, PatternChannel { steps: steps[..cut_step].to_vec(), notes: na });
        b.channels.insert(*ch, PatternChannel { steps: steps[cut_step..total_steps].to_vec(), notes: nb });
    }
    p.patterns.splice(k..=k, [a, b]);
    // Rewrite the clips: each loop repeat of the old pattern becomes an A clip and a B clip.
    for t in p.tracks.iter_mut() {
        let old: Vec<Clip> = t.clips.iter().filter(|c| clip_pattern(c) == Some(pat)).cloned().collect();
        t.clips.retain(|c| clip_pattern(c) != Some(pat));
        for c in old {
            let mut s = c.start;
            while s < c.end() - 1e-9 {
                for (off, l, id, suffix) in [(0.0, at, a_id, "A"), (at, len - at, b_id, "B")] {
                    let from = s + off;
                    let to = (from + l).min(c.end());
                    if to > from + 1e-9 {
                        t.clips.push(Clip { id: 0, name: format!("{} {suffix}", src.name), start: from, length: to - from, muted: c.muted, color: c.color, kind: ClipKind::Pattern { pattern: id } });
                    }
                }
                s += len;
            }
        }
    }
    // Fresh ids for the new clips.
    let mut fix: Vec<(usize, usize)> = Vec::new();
    for (ti, t) in p.tracks.iter().enumerate() {
        for (ci, c) in t.clips.iter().enumerate() {
            if c.id == 0 {
                fix.push((ti, ci));
            }
        }
    }
    for (ti, ci) in fix {
        let id = p.next_id();
        p.tracks[ti].clips[ci].id = id;
    }
    Some((a_id, b_id))
}

/// Deletes a pattern and its playlist clips. The last pattern is never deleted. Returns the pattern to show.
pub fn delete_pattern(p: &mut Project, pat: Id) -> Option<Id> {
    if p.patterns.len() <= 1 {
        return None;
    }
    let k = p.patterns.iter().position(|x| x.id == pat)?;
    p.patterns.remove(k);
    for t in p.tracks.iter_mut() {
        t.clips.retain(|c| clip_pattern(c) != Some(pat));
    }
    Some(p.patterns[k.min(p.patterns.len() - 1)].id)
}

/// Replaces the sound of a channel-rack channel with an audio file (Studio: drop a sample onto a channel).
/// A sampler keeps its settings (root note, envelope, loop, …) except the region and slices, which belong to
/// the old file; any other instrument becomes a sampler. The channel takes the file's name when it was still
/// named after its old sample (or the instrument type), so names given by hand are kept. Returns `false` when
/// the channel does not exist.
pub fn replace_channel_sample(p: &mut Project, channel: Id, path: &std::path::Path) -> bool {
    let Some(ch) = p.channel_mut(channel) else { return false };
    let stem = |q: &std::path::Path| q.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let auto_named = match &ch.instrument {
        Instrument::Sampler(s) => ch.name == stem(&s.path) || ch.name.is_empty() || ch.name == "Sampler",
        // The type's name, or what the stock instruments were called before they had their own names.
        other => ch.name == other.kind_name() || ch.name.is_empty() || matches!(ch.name.as_str(), "Synth" | "Drum Pads" | "Drums" | "Dragon Wave" | "Dragon Strings"),
    };
    match &mut ch.instrument {
        Instrument::Sampler(s) => {
            s.path = path.to_path_buf();
            s.start = 0.0;
            s.end = 1.0;
            s.slices.clear();
        }
        other => *other = Instrument::Sampler(SamplerParams { path: path.to_path_buf(), ..Default::default() }),
    }
    if auto_named {
        ch.name = stem(path);
    }
    true
}

/// File types the sampler can load.
pub fn is_audio_file(path: &std::path::Path) -> bool {
    matches!(path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(), Some("wav" | "flac" | "mp3" | "ogg" | "aif" | "aiff" | "m4a"))
}

#[cfg(test)]
mod tests {
    fn beat_project() -> (super::Project, super::Id, super::Id, super::Id) {
        use super::*;
        let mut p = Project::new_default();
        let kick = p.add_channel("Kick", Instrument::Synth(Default::default()));
        let bass = p.add_channel("Bass", Instrument::Synth(Default::default()));
        let pat = p.patterns[0].id;
        let pt = p.pattern_mut(pat).unwrap();
        pt.steps = 32; // two bars
        pt.channels.insert(kick, PatternChannel { steps: (0..32).map(|i| Step { on: i % 4 == 0, ..Default::default() }).collect(), notes: vec![] });
        pt.channels.insert(
            bass,
            PatternChannel {
                steps: vec![],
                notes: vec![Note { pitch: 36, start: 0, length: 2 * PPQ, velocity: 0.8, ..Default::default() }, Note { pitch: 38, start: 3 * PPQ, length: 2 * PPQ, velocity: 0.8, ..Default::default() }],
            },
        );
        let id = p.next_id();
        p.tracks[0].clips.push(Clip { id, name: "Pattern 1".into(), start: 0.0, length: 16.0, muted: false, color: Rgb::default(), kind: ClipKind::Pattern { pattern: pat } });
        (p, pat, kick, bass)
    }

    /// Note-on beats heard from the playlist (compiled song events) – the reference for "sounds the same".
    fn heard(p: &super::Project) -> Vec<(i64, usize, u8)> {
        let cache = crate::samples::SampleCache::default();
        let stretch = crate::render::StretchCache::default();
        let mut pool = crate::render::Pool::new();
        let song = crate::render::Song::compile(p, &mut crate::render::CompileContext { cache: &cache, stretch: &stretch, pool: &mut pool, rate: 44100.0 });
        let mut v: Vec<(i64, usize, u8)> = song.note_ons().into_iter().map(|(b, c, n)| ((b * 1000.0).round() as i64, c, n)).collect();
        v.sort();
        v
    }

    #[test]
    fn make_unique_and_clone_patterns() {
        use super::*;
        let (mut p, pat, kick, _) = beat_project();
        let first = p.tracks[0].clips[0].id;
        let id2 = p.next_id();
        p.tracks[1].clips.push(Clip { id: id2, name: "Pattern 1".into(), start: 16.0, length: 8.0, muted: false, color: Rgb::default(), kind: ClipKind::Pattern { pattern: pat } });
        let new = make_unique(&mut p, &[id2]);
        assert_eq!(new.len(), 1);
        assert_eq!(p.patterns.len(), 2);
        assert_eq!(p.pattern(new[0]).unwrap().name, "Pattern 1 #2");
        // Editing the unique copy leaves the original alone.
        p.pattern_mut(new[0]).unwrap().channels.get_mut(&kick).unwrap().steps[1].on = true;
        assert!(!p.pattern(pat).unwrap().channels[&kick].steps[1].on);
        assert!(matches!(p.tracks[0].clips[0].kind, ClipKind::Pattern { pattern } if pattern == pat) && p.tracks[0].clips[0].id == first);
        let c = clone_pattern(&mut p, new[0]).unwrap();
        assert_eq!(p.pattern(c).unwrap().name, "Pattern 1 #3");
        assert_eq!(p.patterns[2].id, c, "inserted after its source");
    }

    #[test]
    fn split_by_channel_keeps_the_sound() {
        use super::*;
        let (mut p, pat, kick, bass) = beat_project();
        let before = heard(&p);
        let parts = split_by_channel(&mut p, pat);
        assert_eq!(parts.len(), 2);
        assert!(p.pattern(pat).is_none(), "original replaced");
        assert!(p.pattern(parts[0]).unwrap().channels.contains_key(&kick) && p.pattern(parts[1]).unwrap().channels.contains_key(&bass));
        assert_eq!(p.tracks[0].clips.len(), 1);
        assert_eq!(p.tracks[1].clips.len(), 1, "second part on the next track");
        assert_eq!(heard(&p), before, "the song plays the same notes");
    }

    /// A drum-pads channel splits into one channel per played pad, each with its pad and notes; the song
    /// plays the same notes at the same times.
    #[test]
    fn split_drum_pads_gives_each_sound_its_channel() {
        use super::*;
        let mut p = Project::new_default();
        let pad = |name: &str, note: u8| Pad { name: name.into(), note, ..Default::default() };
        let drums = p.add_channel("Drums", Instrument::DrumPads(vec![pad("Kick", 36), pad("Snare", 38), pad("Hat", 42), pad("Unused", 49)]));
        let pat = p.patterns[0].id;
        let n = |pitch: u8, beat: u32| Note { pitch, start: beat * PPQ, length: PPQ / 2, velocity: 0.8, ..Default::default() };
        p.pattern_mut(pat).unwrap().channels.insert(drums, PatternChannel { steps: vec![], notes: vec![n(36, 0), n(42, 0), n(38, 1), n(42, 1), n(36, 2), n(38, 3)] });
        let id = p.next_id();
        p.tracks[0].clips.push(Clip { id, name: "P".into(), start: 0.0, length: 4.0, muted: false, color: Rgb::default(), kind: ClipKind::Pattern { pattern: pat } });
        let notes_heard = |p: &Project| heard(p).into_iter().map(|(b, _, n)| (b, n)).collect::<Vec<_>>();
        let before = notes_heard(&p);
        let parts = split_drum_pads(&mut p, drums);
        assert_eq!(parts.len(), 3, "kick, snare, hat – the unused pad gets no channel");
        assert!(p.channel(drums).is_none(), "the source is replaced");
        let names: Vec<&str> = parts.iter().map(|c| p.channel(*c).unwrap().name.as_str()).collect();
        assert_eq!(names, ["Kick", "Snare", "Hat"]);
        let pt = p.pattern(pat).unwrap();
        assert_eq!(pt.channels[&parts[0]].notes.len(), 2);
        assert_eq!(pt.channels[&parts[2]].notes.len(), 2);
        assert_eq!(notes_heard(&p), before, "same notes at the same times");
    }

    /// The parts never land on a track that already holds other material: a new track is inserted there.
    #[test]
    fn split_by_channel_makes_new_tracks_when_the_next_is_taken() {
        use super::*;
        let (mut p, pat, _, _) = beat_project();
        while p.tracks.len() < 2 {
            p.add_track("Track");
        }
        let other = p.next_id();
        let audio_track = p.tracks[1].id;
        p.tracks[1].clips.push(Clip { id: other, name: "vocal".into(), start: 40.0, length: 4.0, muted: false, color: Rgb::default(), kind: ClipKind::Audio(AudioClip::default()) });
        let before = heard(&p);
        let n = p.tracks.len();
        let parts = split_by_channel(&mut p, pat);
        assert_eq!(parts.len(), 2);
        assert_eq!(p.tracks.len(), n + 1, "a track inserted");
        assert_eq!(p.tracks[1].clips.len(), 1, "the second part on the new track right below");
        assert!(matches!(p.tracks[1].clips[0].kind, ClipKind::Pattern { .. }));
        assert_eq!(p.tracks[2].id, audio_track, "the vocal track moved down, untouched");
        assert_eq!(p.tracks[2].clips.len(), 1);
        assert_eq!(heard(&p), before);
    }

    #[test]
    fn split_pattern_at_a_bar_keeps_the_sound() {
        use super::*;
        let (mut p, pat, _, bass) = beat_project();
        let before = heard(&p);
        assert!(split_pattern_at(&mut p, pat, 2.0).is_none(), "note patterns split on bar lines");
        let (a, b) = split_pattern_at(&mut p, pat, 4.0).unwrap();
        assert_eq!(p.pattern(a).unwrap().length_beats(), 4.0);
        assert_eq!(p.pattern(b).unwrap().length_beats(), 4.0);
        // The bass note crossing the cut (beat 3 → 5) became two notes.
        assert_eq!(p.pattern(a).unwrap().channels[&bass].notes.len(), 2);
        assert_eq!(p.pattern(b).unwrap().channels[&bass].notes[0].start, 0);
        // The 16-beat clip (two loops of 8 beats) became A B A B.
        let kinds: Vec<Id> = p.tracks[0]
            .clips
            .iter()
            .map(|c| match c.kind {
                ClipKind::Pattern { pattern } => pattern,
                _ => 0,
            })
            .collect();
        assert_eq!(kinds, vec![a, b, a, b]);
        let after = heard(&p);
        // Split notes retrigger at the cut; everything heard before is still heard at the same time.
        assert!(before.iter().all(|e| after.contains(e)), "{before:?} vs {after:?}");
        assert_eq!(after.len(), before.len() + 2, "the crossing note now also starts at the cut (twice: two loops)");
        assert!(delete_pattern(&mut p, a).is_some());
        assert!(p.tracks[0].clips.iter().all(|c| !matches!(c.kind, ClipKind::Pattern { pattern } if pattern == a)));
    }
    #[test]
    fn dropping_a_sample_replaces_the_channel_sound() {
        use super::*;
        let mut p = Project::new_default();
        let kick = p.add_channel("kick_01", Instrument::Sampler(SamplerParams { path: "kits/kick_01.wav".into(), root_note: 48, start: 0.2, slices: vec![0.5], ..Default::default() }));
        assert!(replace_channel_sample(&mut p, kick, std::path::Path::new("kits/kick_02.wav")));
        let c = p.channel(kick).unwrap();
        let Instrument::Sampler(s) = &c.instrument else { panic!() };
        assert_eq!(s.path, std::path::PathBuf::from("kits/kick_02.wav"));
        assert_eq!(s.root_note, 48, "settings kept");
        assert_eq!((s.start, s.end), (0.0, 1.0), "region of the old file reset");
        assert!(s.slices.is_empty());
        assert_eq!(c.name, "kick_02", "auto name follows the sample");
        // A hand-given name is kept; a synth becomes a sampler.
        let lead = p.add_channel("My lead", Instrument::Synth(Default::default()));
        replace_channel_sample(&mut p, lead, std::path::Path::new("vox.wav"));
        let c = p.channel(lead).unwrap();
        assert!(matches!(c.instrument, Instrument::Sampler(_)));
        assert_eq!(c.name, "My lead");
        assert!(!replace_channel_sample(&mut p, 999_999, std::path::Path::new("x.wav")));
        assert!(is_audio_file(std::path::Path::new("A.WAV")) && !is_audio_file(std::path::Path::new("song.dragon")));
    }

    use super::*;

    fn audio_clip(p: &mut Project, start: f64, len: f64) -> (Id, Id) {
        let id = p.next_id();
        let t = p.tracks[0].id;
        p.tracks[0].clips.push(Clip { id, name: "c".into(), start, length: len, muted: false, color: Rgb::default(), kind: ClipKind::Audio(AudioClip { source: "x.wav".into(), ..Default::default() }) });
        (t, id)
    }

    #[test]
    fn split_keeps_audio_continuous() {
        let mut p = Project::new_default();
        p.bpm = 120.0;
        let (t, c) = audio_clip(&mut p, 1.0, 4.0);
        let r = split_clip(&mut p, t, c, 3.0).unwrap();
        let tr = p.track(t).unwrap();
        assert_eq!(tr.clips.len(), 2);
        assert_eq!(tr.clips[0].length, 2.0);
        let right = tr.clips.iter().find(|x| x.id == r).unwrap();
        assert_eq!(right.start, 3.0);
        assert!((right.audio().unwrap().offset - 1.0).abs() < 1e-9); // 2 beats at 120 BPM = 1 s
        assert!(split_clip(&mut p, t, r, 3.0).is_none());
    }

    #[test]
    fn duplicate_places_after_selection() {
        let mut p = Project::new_default();
        let (t, c) = audio_clip(&mut p, 2.0, 2.0);
        let ids = duplicate_clips(&mut p, &[c]);
        let d = p.track(t).unwrap().clips.iter().find(|x| x.id == ids[0]).unwrap();
        assert_eq!(d.start, 4.0);
    }

    #[test]
    fn comping_replaces_range_from_take() {
        let mut p = Project::new_default();
        let (t, _) = audio_clip(&mut p, 0.0, 8.0);
        let take = push_take(&mut p, t, "Take 1").unwrap();
        audio_clip(&mut p, 0.0, 8.0);
        assert!(comp_promote(&mut p, t, take, Range::new(2.0, 4.0)));
        let tr = p.track(t).unwrap();
        let from_take: Vec<&Clip> = tr.clips.iter().filter(|c| c.audio().unwrap().take == Some(take)).collect();
        assert_eq!(from_take.len(), 1);
        assert_eq!((from_take[0].start, from_take[0].length), (2.0, 2.0));
        let total: f64 = tr.clips.iter().map(|c| c.length).sum();
        assert!((total - 8.0).abs() < 1e-9);
    }

    #[test]
    fn shuffle_delete_closes_gap() {
        let mut p = Project::new_default();
        let (t, _) = audio_clip(&mut p, 0.0, 2.0);
        audio_clip(&mut p, 4.0, 2.0);
        delete_range_shuffle(&mut p, t, Range::new(2.0, 4.0));
        let starts: Vec<f64> = p.track(t).unwrap().clips.iter().map(|c| c.start).collect();
        assert_eq!(starts, vec![0.0, 2.0]);
    }

    #[test]
    fn note_tools() {
        let mut n = vec![Note { start: 26, length: 20, ..Note::default() }];
        quantize(&mut n, 24, 1.0, false);
        assert_eq!(n[0].start, 24);
        let c = chord(60, CHORDS[0].1, 0, 96, 0.8, false);
        assert_eq!(c.iter().map(|x| x.pitch).collect::<Vec<_>>(), vec![60, 64, 67]);
        let td = chord(60, CHORDS[0].1, 0, 96, 0.8, true);
        assert_eq!(td.iter().map(|x| x.pitch).collect::<Vec<_>>(), vec![53, 57, 60]);
        let a = arpeggiate(&c, ArpMode::Up, 24, 0.5, 1);
        assert_eq!(a.len(), 4);
        assert_eq!(a.iter().map(|x| x.pitch).collect::<Vec<_>>(), vec![60, 64, 67, 60]);
        assert_eq!(snap_to_scale(61, 0, SCALES[0].1), 60);
        let prog = progression(60, SCALES[0].1, &[0, 4, 5, 3], 384, true);
        assert_eq!(prog.len(), 16);
        let mut s = c.clone();
        strum(&mut s, 5);
        assert_eq!(s.iter().map(|x| x.start).collect::<Vec<_>>(), vec![0, 5, 10]);
        let mut l = vec![Note { start: 0, length: 10, ..Note::default() }, Note { start: 48, length: 10, ..Note::default() }];
        legato(&mut l);
        assert_eq!(l[0].length, 48);
        let mut nn = Note { start: 0, length: 96, ..Note::default() };
        let r = split_note(&mut nn, 48).unwrap();
        assert_eq!((nn.length, r.start, r.length), (48, 48, 48));
    }
}

// ------------------------------------------------------------------------------------------------
// Piano-roll tools (Limit, Articulate, LFO, Claw machine, Scale levels, Score flipper,
// Randomize, Riff machine). Each works on the given notes and returns the result.

/// Limit (Alt+K): moves notes outside `lo ..= hi` into the range by whole octaves.
pub fn limit(notes: &mut [Note], lo: u8, hi: u8) {
    let (lo, hi) = (lo.min(hi), hi.max(lo));
    for n in notes {
        let mut p = n.pitch as i32;
        while p < lo as i32 && p + 12 <= 127 {
            p += 12;
        }
        while p > hi as i32 && p - 12 >= 0 {
            p -= 12;
        }
        n.pitch = p.clamp(lo as i32, hi as i32) as u8;
    }
}

/// Articulate (Alt+L): scales note lengths (`scale`, 1 = unchanged) and optionally ends every note
/// `gap` ticks before the next start.
pub fn articulate(notes: &mut [Note], scale: f32, gap: Option<u32>) {
    let starts: Vec<u32> = notes.iter().map(|n| n.start).collect();
    for n in notes.iter_mut() {
        n.length = ((n.length as f32 * scale).round() as u32).max(1);
        if let Some(g) = gap {
            if let Some(next) = starts.iter().copied().filter(|s| *s > n.start).min() {
                n.length = n.length.min((next - n.start).saturating_sub(g).max(1));
            }
        }
    }
}

/// Which note property a tool writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteProp {
    Velocity,
    Pan,
    Release,
}

fn prop(n: &mut Note, p: NoteProp) -> &mut f32 {
    match p {
        NoteProp::Velocity => &mut n.velocity,
        NoteProp::Pan => &mut n.pan,
        NoteProp::Release => &mut n.release,
    }
}

/// LFO (Alt+O): a sine of `cycles_per_bar` (bar = `bar_ticks`) shaping a property around `center`
/// with depth `amount`.
pub fn lfo(notes: &mut [Note], p: NoteProp, bar_ticks: u32, cycles_per_bar: f32, center: f32, amount: f32) {
    let (lo, hi) = if p == NoteProp::Pan { (-1.0, 1.0) } else { (0.0, 1.0) };
    for n in notes {
        let ph = n.start as f32 / bar_ticks.max(1) as f32 * cycles_per_bar * std::f32::consts::TAU;
        *prop(n, p) = (center + amount * ph.sin()).clamp(lo, hi);
    }
}

/// Claw machine (Alt+W): walks the notes in time order and keeps `keep`, then drops `skip`, repeatedly.
pub fn claw(notes: &[Note], keep: usize, skip: usize) -> Vec<Note> {
    let mut v = notes.to_vec();
    v.sort_by_key(|n| (n.start, n.pitch));
    let period = (keep + skip).max(1);
    v.into_iter().enumerate().filter(|(i, _)| i % period < keep.max(1)).map(|(_, n)| n).collect()
}

/// Scale levels (Alt+X): property = property × `scale` + `offset`.
pub fn scale_levels(notes: &mut [Note], p: NoteProp, scale: f32, offset: f32) {
    let (lo, hi) = if p == NoteProp::Pan { (-1.0, 1.0) } else { (0.0, 1.0) };
    for n in notes {
        let v = prop(n, p);
        *v = (*v * scale + offset).clamp(lo, hi);
    }
}

/// Score flipper (Alt+Y): mirrors pitches around their middle (vertical) and/or time (horizontal).
pub fn flip(notes: &mut [Note], vertical: bool, horizontal: bool) {
    if notes.is_empty() {
        return;
    }
    if vertical {
        let (lo, hi) = notes.iter().fold((127u8, 0u8), |(a, b), n| (a.min(n.pitch), b.max(n.pitch)));
        for n in notes.iter_mut() {
            n.pitch = (lo as i32 + hi as i32 - n.pitch as i32).clamp(0, 127) as u8;
        }
    }
    if horizontal {
        let (s, e) = notes.iter().fold((u32::MAX, 0u32), |(a, b), n| (a.min(n.start), b.max(n.end())));
        for n in notes.iter_mut() {
            n.start = s + (e - n.end());
        }
    }
}

/// Randomize (Alt+R): random timing (± `ticks`), velocity (± `velocity`), pitch (± `pitch` semitones,
/// kept in `scale` when given, relative to `root`).
pub fn randomize(notes: &mut [Note], ticks: u32, velocity: f32, pitch: u8, scale: Option<(u8, &[u8])>, seed: u64) {
    let mut rng = crate::dsp::Rng::new(seed);
    for n in notes {
        if ticks > 0 {
            n.start = (n.start as i64 + (rng.bipolar() * ticks as f32).round() as i64).max(0) as u32;
        }
        n.velocity = (n.velocity + rng.bipolar() * velocity).clamp(0.05, 1.0);
        if pitch > 0 {
            let p = (n.pitch as i32 + (rng.bipolar() * pitch as f32).round() as i32).clamp(0, 127) as u8;
            n.pitch = match scale {
                Some((root, s)) => snap_to_scale(p, root, s),
                None => p,
            };
        }
    }
}

/// Riff machine (Alt+E): a seeded riff in a scale – `bars` long on a `step` grid, `density` 0 … 1 of
/// the steps sounding, moving mostly stepwise around `root` (MIDI note).
pub fn riff(root: u8, scale: &[u8], bars: u32, step: u32, density: f32, seed: u64) -> Vec<Note> {
    let mut rng = crate::dsp::Rng::new(seed);
    let steps = bars * crate::model::PPQ * 4 / step.max(1);
    let mut degree: i32 = 0;
    let mut out: Vec<Note> = Vec::new();
    let pitch_of = |d: i32| -> u8 {
        let n = scale.len().max(1) as i32;
        let oct = d.div_euclid(n);
        let off = scale.get(d.rem_euclid(n) as usize).copied().unwrap_or(0) as i32;
        (root as i32 + oct * 12 + off).clamp(0, 127) as u8
    };
    for s in 0..steps {
        // Strong beats sound more often.
        let accent = if s % 4 == 0 { 0.25 } else { 0.0 };
        if rng.next_f32() < density + accent {
            let mv = match (rng.next_f32() * 10.0) as i32 {
                0..=3 => 1,
                4..=7 => -1,
                8 => 2,
                _ => -2,
            };
            degree = (degree + mv).clamp(-5, 9);
            // Sometimes hold for two steps.
            let len = if rng.next_f32() < 0.25 { step * 2 } else { step };
            let start = s * step;
            if out.last().is_some_and(|l: &Note| l.end() > start) {
                continue;
            }
            out.push(Note { pitch: pitch_of(degree), start, length: len, velocity: if s % 4 == 0 { 0.9 } else { 0.7 }, ..Default::default() });
        }
    }
    out
}

#[cfg(test)]
mod fl_tool_tests {
    use super::*;
    use crate::model::PPQ;

    fn n(p: u8, s: u32) -> Note {
        Note { pitch: p, start: s, length: PPQ, ..Default::default() }
    }

    #[test]
    fn piano_roll_tools() {
        let mut v = vec![n(30, 0), n(100, PPQ)];
        limit(&mut v, 48, 72);
        assert!(v.iter().all(|x| (48..=72).contains(&x.pitch)), "{v:?}");
        assert_eq!(v[0].pitch % 12, 30 % 12, "octave moves keep the note name");

        let mut v = vec![n(60, 0), n(62, PPQ)];
        articulate(&mut v, 0.5, None);
        assert_eq!(v[0].length, PPQ / 2);

        let v: Vec<Note> = (0..8).map(|i| n(60, i * PPQ)).collect();
        assert_eq!(claw(&v, 1, 1).len(), 4);

        let mut v = vec![n(60, 0), n(67, PPQ)];
        flip(&mut v, true, false);
        assert_eq!((v[0].pitch, v[1].pitch), (67, 60));
        flip(&mut v, false, true);
        assert_eq!((v[0].start, v[1].start), (PPQ, 0));

        let mut v = vec![n(60, 0)];
        scale_levels(&mut v, NoteProp::Velocity, 0.5, 0.0);
        assert!((v[0].velocity - 0.4).abs() < 1e-4);

        let mut v: Vec<Note> = (0..16).map(|i| n(60, i * PPQ / 4)).collect();
        lfo(&mut v, NoteProp::Velocity, PPQ * 4, 1.0, 0.5, 0.4);
        assert!(v.iter().any(|x| x.velocity > 0.85) && v.iter().any(|x| x.velocity < 0.15));

        let major = &[0u8, 2, 4, 5, 7, 9, 11];
        let r = riff(60, major, 2, PPQ / 4, 0.5, 9);
        assert!(!r.is_empty());
        assert!(r.iter().all(|x| in_scale(x.pitch, 0, major)));
        assert_eq!(r, riff(60, major, 2, PPQ / 4, 0.5, 9), "seeded");
        let mut v: Vec<Note> = (0..8).map(|i| n(60, i * PPQ)).collect();
        randomize(&mut v, 0, 0.0, 3, Some((0, major)), 4);
        assert!(v.iter().all(|x| in_scale(x.pitch, 0, major)));
    }
}

/// The tracks that play into mixer channel `mixer`.
pub fn tracks_into(p: &Project, mixer: Id) -> Vec<Id> {
    p.tracks.iter().filter(|t| t.mixer == mixer).map(|t| t.id).collect()
}

/// Freeze: mixer channel `mixer` was rendered to `file` (from beat `start`, `length` beats). Its tracks'
/// clips are put aside and the first track plays the render; the channel's switched-on inserts are
/// switched off (they are in the render) – instruments and effects cost no CPU until Unfreeze.
pub fn freeze_channel(p: &mut Project, mixer: Id, file: &std::path::Path, start: f64, length: f64) -> bool {
    let tracks = tracks_into(p, mixer);
    if tracks.is_empty() || tracks.iter().any(|t| p.track(*t).is_some_and(|t| t.frozen.is_some())) {
        return false;
    }
    let inserts_on: Vec<Id> = p.mixer.get(mixer).map(|m| m.inserts.iter().filter(|i| i.enabled).map(|i| i.id).collect()).unwrap_or_default();
    if let Some(m) = p.mixer.get_mut(mixer) {
        for i in m.inserts.iter_mut() {
            i.enabled = false;
        }
    }
    let id = p.next_id();
    for (k, tid) in tracks.iter().enumerate() {
        let Some(t) = p.track_mut(*tid) else { continue };
        let clips = std::mem::take(&mut t.clips);
        t.frozen = Some(Frozen { clips, inserts_on: if k == 0 { inserts_on.clone() } else { Vec::new() }, file: file.to_path_buf() });
        if k == 0 {
            let name = format!("{} (frozen)", t.name);
            t.clips.push(Clip { id, name, start, length: length.max(0.01), muted: false, color: t.color, kind: ClipKind::Audio(AudioClip { source: file.to_path_buf(), ..Default::default() }) });
        }
    }
    true
}

/// Unfreeze: the tracks get their clips back and the channel's inserts are switched on again.
pub fn unfreeze_channel(p: &mut Project, mixer: Id) -> bool {
    let tracks = tracks_into(p, mixer);
    let mut on = Vec::new();
    let mut any = false;
    for tid in tracks {
        let Some(t) = p.track_mut(tid) else { continue };
        if let Some(f) = t.frozen.take() {
            t.clips = f.clips;
            on.extend(f.inserts_on);
            any = true;
        }
    }
    if let Some(m) = p.mixer.get_mut(mixer) {
        for i in m.inserts.iter_mut().filter(|i| on.contains(&i.id)) {
            i.enabled = true;
        }
    }
    any
}
