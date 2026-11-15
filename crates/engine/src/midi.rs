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
                                notes.push(Note {
                                    pitch: key.as_int(),
                                    start: (start as f64 * scale).round() as u32,
                                    length: (((t - start) as f64 * scale).round() as u32).max(1),
                                    velocity: vel as f32 / 127.0,
                                    color: ch,
                                    ..Note::default()
                                });
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
    let tempo = [
        TrackEvent { delta: u28::new(0), kind: TrackEventKind::Meta(MetaMessage::Tempo(u24::new((60_000_000.0 / bpm.max(1.0)) as u32))) },
        TrackEvent { delta: u28::new(0), kind: TrackEventKind::Meta(MetaMessage::EndOfTrack) },
    ];
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

// ---------------------------------------------------------------------------------------------- Mackie Control

/// An action decoded from a Mackie Control Universal (MCU) compatible control surface (Behringer X-Touch,
/// iCON, PreSonus FaderPort in MCU mode, …). Strips are 0–7 within the current bank.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum McuAction {
    /// Fader position 0 … 1 (strip 8 = master).
    Fader {
        strip: u8,
        value: f32,
    },
    /// V-pot turned by `delta` detents (negative = left).
    Pan {
        strip: u8,
        delta: i8,
    },
    Mute(u8),
    Solo(u8),
    Arm(u8),
    Select(u8),
    Play,
    Stop,
    Record,
    Rewind,
    Forward,
    Loop,
    Click,
    BankLeft,
    BankRight,
    ChannelLeft,
    ChannelRight,
}

/// Decodes one MCU message (button presses only; releases are ignored).
pub fn mackie(m: MidiMessage) -> Option<McuAction> {
    use McuAction::*;
    match m {
        MidiMessage::PitchBend { channel, value } if channel <= 8 => Some(Fader { strip: channel, value: ((value as f32 + 8192.0) / 16383.0).clamp(0.0, 1.0) }),
        MidiMessage::Control { controller, value, .. } if (0x10..=0x17).contains(&controller) => {
            let mag = (value & 0x3F) as i8;
            Some(Pan { strip: controller - 0x10, delta: if value & 0x40 != 0 { -mag } else { mag } })
        }
        MidiMessage::NoteOn { note, velocity, .. } if velocity > 0 => Some(match note {
            0x00..=0x07 => Arm(note),
            0x08..=0x0F => Solo(note - 0x08),
            0x10..=0x17 => Mute(note - 0x10),
            0x18..=0x1F => Select(note - 0x18),
            0x2E => BankLeft,
            0x2F => BankRight,
            0x30 => ChannelLeft,
            0x31 => ChannelRight,
            0x56 => Loop,
            0x59 => Click,
            0x5B => Rewind,
            0x5C => Forward,
            0x5D => Stop,
            0x5E => Play,
            0x5F => Record,
            _ => return None,
        }),
        _ => None,
    }
}

/// MCU fader curve: fader travel 0 … 1 → linear gain (unity at about 75 %, +6 dB at the top).
pub fn mcu_fader_to_gain(v: f32) -> f32 {
    if v <= 0.0 {
        0.0
    } else {
        // −60 dB … +6 dB with unity at 0.75.
        let db = if v >= 0.75 { (v - 0.75) / 0.25 * 6.0 } else { -60.0 + v / 0.75 * 60.0 };
        crate::dsp::db_to_gain(db)
    }
}

/// Inverse of [`mcu_fader_to_gain`] (for motor-fader feedback).
pub fn mcu_gain_to_fader(g: f32) -> f32 {
    if g <= 0.001 {
        return 0.0;
    }
    let db = crate::dsp::gain_to_db(g).clamp(-60.0, 6.0);
    if db >= 0.0 {
        0.75 + db / 6.0 * 0.25
    } else {
        (db + 60.0) / 60.0 * 0.75
    }
}

/// MIDI bytes that move motor fader `strip` to `fader` (0 … 1).
pub fn mcu_fader_message(strip: u8, fader: f32) -> [u8; 3] {
    let v = (fader.clamp(0.0, 1.0) * 16383.0).round() as u16;
    [0xE0 | (strip & 0x0F), (v & 0x7F) as u8, (v >> 7) as u8]
}

/// MIDI bytes that light (or clear) a button LED.
pub fn mcu_led_message(note: u8, on: bool) -> [u8; 3] {
    [0x90, note, if on { 0x7F } else { 0 }]
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

    #[test]
    fn mackie_control_decoding() {
        use super::*;
        assert_eq!(mackie(parse(&[0x90, 0x5E, 0x7F])), Some(McuAction::Play));
        assert_eq!(mackie(parse(&[0x90, 0x5E, 0x00])), None, "release ignored");
        assert_eq!(mackie(parse(&[0x90, 0x12, 0x7F])), Some(McuAction::Mute(2)));
        assert_eq!(mackie(parse(&[0x90, 0x09, 0x7F])), Some(McuAction::Solo(1)));
        assert_eq!(mackie(parse(&[0x90, 0x03, 0x7F])), Some(McuAction::Arm(3)));
        assert_eq!(mackie(parse(&[0xB0, 0x11, 0x41])), Some(McuAction::Pan { strip: 1, delta: -1 }));
        assert_eq!(mackie(parse(&[0xB0, 0x11, 0x03])), Some(McuAction::Pan { strip: 1, delta: 3 }));
        match mackie(parse(&[0xE3, 0x7F, 0x7F])) {
            Some(McuAction::Fader { strip: 3, value }) => assert!((value - 1.0).abs() < 1e-3),
            other => panic!("{other:?}"),
        }
        // Fader curve round trip and unity at 75 %.
        assert!((mcu_fader_to_gain(0.75) - 1.0).abs() < 1e-4);
        for g in [0.01f32, 0.25, 0.5, 1.0, 1.8] {
            assert!((mcu_fader_to_gain(mcu_gain_to_fader(g)) - g).abs() / g < 0.01, "{g}");
        }
        assert_eq!(mcu_fader_message(0, 1.0), [0xE0, 0x7F, 0x7F]);
        let back = parse(&mcu_fader_message(2, 0.5));
        assert!(matches!(mackie(back), Some(McuAction::Fader { strip: 2, value }) if (value - 0.5).abs() < 1e-3));
    }
}
