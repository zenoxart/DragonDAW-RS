//! MIDI: message parsing for live input, MIDI learn mappings, standard MIDI file import/export and a
//! retrospective capture buffer ("I played something good while not recording").

use crate::model::{Note, Pattern, Target, PPQ};
use crate::{EngineError, Result};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MidiMessage {
    NoteOn { channel: u8, note: u8, velocity: u8 },
    NoteOff { channel: u8, note: u8 },
    Control { channel: u8, controller: u8, value: u8 },
    PitchBend { channel: u8, value: i16 },
    Program { channel: u8, program: u8 },
    ChannelPressure { channel: u8, value: u8 },
    Clock,
    Start,
    Continue,
    Stop,
    Other,
}

pub fn parse(bytes: &[u8]) -> MidiMessage {
    let Some(&status) = bytes.first() else { return MidiMessage::Other };
    let ch = status & 0x0F;
    let d1 = bytes.get(1).copied().unwrap_or(0);
    let d2 = bytes.get(2).copied().unwrap_or(0);
    match status & 0xF0 {
        0x90 if d2 > 0 => MidiMessage::NoteOn { channel: ch, note: d1, velocity: d2 },
        0x90 | 0x80 => MidiMessage::NoteOff { channel: ch, note: d1 },
        0xB0 => MidiMessage::Control { channel: ch, controller: d1, value: d2 },
        0xE0 => MidiMessage::PitchBend { channel: ch, value: ((d2 as i16) << 7 | d1 as i16) - 8192 },
        0xC0 => MidiMessage::Program { channel: ch, program: d1 },
        0xD0 => MidiMessage::ChannelPressure { channel: ch, value: d1 },
        0xF0 => match status {
            0xF8 => MidiMessage::Clock,
            0xFA => MidiMessage::Start,
            0xFB => MidiMessage::Continue,
            0xFC => MidiMessage::Stop,
            _ => MidiMessage::Other,
        },
        _ => MidiMessage::Other,
    }
}

/// Maps a 0–127 controller value into a mapping's range.
pub fn map_cc(value: u8, min: f32, max: f32) -> f32 {
    min + (value as f32 / 127.0) * (max - min)
}

/// Sensible value range of a target for MIDI learn.
pub fn target_range(t: &Target) -> (f32, f32) {
    match t {
        Target::MixerVolume(_) | Target::ChannelVolume(_) | Target::SendLevel { .. } => (0.0, 1.0),
        Target::MixerPan(_) | Target::ChannelPan(_) => (-1.0, 1.0),
        Target::MixerWidth(_) => (0.0, 2.0),
        Target::MixerMute(_) | Target::InsertMix { .. } => (0.0, 1.0),
        Target::ChannelPitch(_) => (-12.0, 12.0),
        Target::SynthCutoff(_) => (40.0, 16000.0),
        Target::SynthResonance(_) => (0.0, 0.95),
        Target::Tempo => (60.0, 200.0),
        Target::InsertParam { .. } | Target::PluginParam { .. } => (0.0, 1.0),
    }
}

/// Notes per MIDI channel of an imported file.
#[derive(Debug, Clone, Default)]
pub struct ImportedMidi {
    pub tracks: Vec<(String, Vec<Note>)>,
    pub bpm: Option<f64>,
}

/// Reads a standard MIDI file. Ticks are converted to [`PPQ`].
pub fn import(path: &Path) -> Result<ImportedMidi> {
    let data = std::fs::read(path).map_err(|e| EngineError::Io(e.to_string()))?;
    import_bytes(&data)
}

pub fn import_bytes(data: &[u8]) -> Result<ImportedMidi> {
    let smf = midly::Smf::parse(data).map_err(|e| EngineError::Midi(e.to_string()))?;
    let tpq = match smf.header.timing {
        midly::Timing::Metrical(t) => t.as_int() as f64,
        midly::Timing::Timecode(..) => 480.0,
    };
    let scale = PPQ as f64 / tpq;
    let mut out = ImportedMidi::default();
    for (ti, track) in smf.tracks.iter().enumerate() {
        let mut name = format!("Track {}", ti + 1);
        let mut t = 0u64;
        let mut open: std::collections::HashMap<(u8, u8), (u64, u8)> = std::collections::HashMap::new();
        let mut notes = Vec::new();
        for ev in track {
            t += ev.delta.as_int() as u64;
            match ev.kind {
                midly::TrackEventKind::Meta(midly::MetaMessage::TrackName(n)) => name = String::from_utf8_lossy(n).to_string(),
                midly::TrackEventKind::Meta(midly::MetaMessage::Tempo(us)) if out.bpm.is_none() => out.bpm = Some(60_000_000.0 / us.as_int() as f64),
                midly::TrackEventKind::Midi { channel, message } => {
                    let ch = channel.as_int();
                    match message {
                        midly::MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => {
                            open.insert((ch, key.as_int()), (t, vel.as_int()));
                        }
                        midly::MidiMessage::NoteOn { key, .. } | midly::MidiMessage::NoteOff { key, .. } => {
                            if let Some((start, vel)) = open.remove(&(ch, key.as_int())) {
                                notes.push(Note { pitch: key.as_int(), start: (start as f64 * scale).round() as u32, length: (((t - start) as f64 * scale).round() as u32).max(1), velocity: vel as f32 / 127.0, color: ch, ..Note::default() });
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        if !notes.is_empty() {
            notes.sort_by_key(|n| n.start);
            out.tracks.push((name, notes));
        }
    }
    Ok(out)
}

/// Writes a pattern (one MIDI track per channel) as a type-1 standard MIDI file.
pub fn export_pattern(path: &Path, pattern: &Pattern, channel_names: &dyn Fn(u64) -> String, bpm: f64) -> Result<()> {
    let bytes = export_bytes(pattern, channel_names, bpm)?;
    std::fs::write(path, bytes).map_err(|e| EngineError::Io(e.to_string()))
}

pub fn export_bytes(pattern: &Pattern, channel_names: &dyn Fn(u64) -> String, bpm: f64) -> Result<Vec<u8>> {
    use midly::num::{u15, u24, u28, u4, u7};
    use midly::{Format, Header, MetaMessage, MidiMessage as M, Smf, Timing, TrackEvent, TrackEventKind};
    let names: Vec<(u64, Vec<u8>)> = pattern.channels.keys().map(|id| (*id, channel_names(*id).into_bytes())).collect();
    let mut tracks: Vec<Vec<TrackEvent>> = Vec::new();
    let tempo = [TrackEvent { delta: u28::new(0), kind: TrackEventKind::Meta(MetaMessage::Tempo(u24::new((60_000_000.0 / bpm.max(1.0)) as u32))) }, TrackEvent { delta: u28::new(0), kind: TrackEventKind::Meta(MetaMessage::EndOfTrack) }];
    tracks.push(tempo.to_vec());
    for (i, (id, name)) in names.iter().enumerate() {
        let pc = &pattern.channels[id];
        let mut notes: Vec<Note> = pc.notes.clone();
        for (k, s) in pc.steps.iter().enumerate() {
            if s.on {
                notes.push(Note { pitch: (60 + s.pitch as i32).clamp(0, 127) as u8, start: k as u32 * PPQ / 4, length: PPQ / 4, velocity: s.velocity, ..Note::default() });
            }
        }
        let mut evs: Vec<(u32, bool, u8, u8)> = Vec::new();
        for n in notes.iter().filter(|n| !n.muted) {
            let v = (n.velocity * 127.0).round().clamp(1.0, 127.0) as u8;
            evs.push((n.start, true, n.pitch, v));
            evs.push((n.end(), false, n.pitch, 0));
        }
        evs.sort_by_key(|e| (e.0, e.1));
        let ch = u4::new((i % 16) as u8);
        let mut track = vec![TrackEvent { delta: u28::new(0), kind: TrackEventKind::Meta(MetaMessage::TrackName(name)) }];
        let mut last = 0;
        for (t, on, key, vel) in evs {
            let msg = if on { M::NoteOn { key: u7::new(key.min(127)), vel: u7::new(vel) } } else { M::NoteOff { key: u7::new(key.min(127)), vel: u7::new(0) } };
            track.push(TrackEvent { delta: u28::new(t - last), kind: TrackEventKind::Midi { channel: ch, message: msg } });
            last = t;
        }
        track.push(TrackEvent { delta: u28::new(0), kind: TrackEventKind::Meta(MetaMessage::EndOfTrack) });
        tracks.push(track);
    }
    let smf = Smf { header: Header { format: Format::Parallel, timing: Timing::Metrical(u15::new(PPQ as u16)) }, tracks };
    let mut out = Vec::new();
    smf.write_std(&mut out).map_err(|e| EngineError::Midi(e.to_string()))?;
    Ok(out)
}

/// Keeps the last minutes of incoming notes so they can be captured after the fact.
#[derive(Debug, Default)]
pub struct RetroBuffer {
    /// (seconds since start, note, velocity, on)
    events: std::collections::VecDeque<(f64, u8, f32, bool)>,
    pub keep_seconds: f64,
}

impl RetroBuffer {
    pub fn new(keep_seconds: f64) -> Self {
        RetroBuffer { events: Default::default(), keep_seconds }
    }

    pub fn push(&mut self, t: f64, note: u8, velocity: f32, on: bool) {
        self.events.push_back((t, note, velocity, on));
        while self.events.front().is_some_and(|e| t - e.0 > self.keep_seconds) {
            self.events.pop_front();
        }
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Turns the buffer into notes (time 0 = first note) at `bpm`, then clears it.
    pub fn capture(&mut self, bpm: f64) -> Vec<Note> {
        let Some(t0) = self.events.iter().find(|e| e.3).map(|e| e.0) else { return Vec::new() };
        let ticks = |t: f64| ((t - t0) * bpm / 60.0 * PPQ as f64).round().max(0.0) as u32;
        let mut open: std::collections::HashMap<u8, (u32, f32)> = Default::default();
        let mut out = Vec::new();
        let last_t = self.events.back().map(|e| e.0).unwrap_or(t0);
        for &(t, n, v, on) in &self.events {
            if t < t0 {
                continue;
            }
            if on {
                open.insert(n, (ticks(t), v));
            } else if let Some((s, vel)) = open.remove(&n) {
                out.push(Note { pitch: n, start: s, length: (ticks(t) - s).max(1), velocity: vel, ..Note::default() });
            }
        }
        for (n, (s, vel)) in open {
            out.push(Note { pitch: n, start: s, length: (ticks(last_t) - s).max(PPQ / 4), velocity: vel, ..Note::default() });
        }
        out.sort_by_key(|n| n.start);
        self.events.clear();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PatternChannel, Rgb};

    #[test]
    fn parse_messages() {
        assert_eq!(parse(&[0x91, 60, 100]), MidiMessage::NoteOn { channel: 1, note: 60, velocity: 100 });
        assert_eq!(parse(&[0x90, 60, 0]), MidiMessage::NoteOff { channel: 0, note: 60 });
        assert_eq!(parse(&[0xB0, 7, 127]), MidiMessage::Control { channel: 0, controller: 7, value: 127 });
        assert_eq!(parse(&[0xE0, 0, 64]), MidiMessage::PitchBend { channel: 0, value: 0 });
        assert_eq!(map_cc(127, -1.0, 1.0), 1.0);
    }

    #[test]
    fn smf_roundtrip() {
        let mut pat = Pattern { id: 1, name: "p".into(), color: Rgb::default(), steps: 16, swing: 0.0, channels: Default::default() };
        pat.channels.insert(7, PatternChannel { steps: vec![], notes: vec![Note { pitch: 64, start: 96, length: 48, velocity: 0.5, ..Note::default() }] });
        let bytes = export_bytes(&pat, &|_| "Lead".into(), 128.0).unwrap();
        let m = import_bytes(&bytes).unwrap();
        assert_eq!(m.tracks.len(), 1);
        assert_eq!(m.tracks[0].0, "Lead");
        let n = m.tracks[0].1[0];
        assert_eq!((n.pitch, n.start, n.length), (64, 96, 48));
        assert!((m.bpm.unwrap() - 128.0).abs() < 0.01);
    }

    #[test]
    fn retro_capture() {
        let mut r = RetroBuffer::new(60.0);
        r.push(10.0, 60, 0.8, true);
        r.push(10.5, 60, 0.0, false);
        r.push(11.0, 62, 0.8, true);
        r.push(11.25, 62, 0.0, false);
        let n = r.capture(120.0);
        assert_eq!(n.len(), 2);
        assert_eq!((n[0].start, n[0].length), (0, 96));
        assert_eq!(n[1].start, 192);
        assert!(r.is_empty());
    }
}
