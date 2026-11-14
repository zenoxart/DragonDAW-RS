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

/// Removes clips by id from every track.
pub fn delete_clips(p: &mut Project, ids: &[Id]) {
    for t in &mut p.tracks {
        t.clips.retain(|c| !ids.contains(&c.id));
    }
}

/// Shuffle edit: moves every clip on the track that starts at or after `from` by `delta` beats.
pub fn shuffle(p: &mut Project, track: Id, from: f64, delta: f64) {
    if let Some(t) = p.track_mut(track) {
        for c in t.clips.iter_mut().filter(|c| c.start >= from - 1e-9) {
            c.start = (c.start + delta).max(0.0);
        }
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
    let mut scratch = p.track(track).cloned().unwrap();
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
    let pattern = p.pattern_mut(pat).unwrap();
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

#[cfg(test)]
mod tests {
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
