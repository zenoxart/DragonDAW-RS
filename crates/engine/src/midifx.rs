//! MIDI effects: note processors in front of a channel's instrument – arpeggiator, chord, note repeat and
//! scale / transpose. A channel's chain wraps its instrument ([`MidiChain`] is itself an
//! [`InstrumentVoice`]): notes go through the effects in order, and the effects' generated notes are played
//! sample-accurately – the instrument is rendered in pieces split at their times.
//!
//! Real-time rules: every buffer is fixed-size (no allocation while playing); the chain is (re)configured off
//! the audio thread, under the instrument's lock, when the channel's effects change.

use crate::effects::{ParamInfo, PluginParam};
use crate::instruments::{InstrumentVoice, NoteOn};
use crate::model::Instrument as Model;
use crate::model::MidiFx;
use crate::samples::SampleCache;
use std::collections::BTreeMap;

// ------------------------------------------------------------------------------------------- definitions

/// A MIDI effect type: its id, name and parameters (plain values, like the audio effects').
pub struct MidiFxDef {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub params: &'static [ParamInfo],
}

impl MidiFxDef {
    pub fn defaults(&self) -> BTreeMap<String, f32> {
        self.params.iter().map(|p| (p.id.to_string(), p.default)).collect()
    }
    pub fn value(&self, params: &BTreeMap<String, f32>, id: &str) -> f32 {
        params.get(id).copied().or_else(|| self.params.iter().find(|p| p.id == id).map(|p| p.default)).unwrap_or(0.0)
    }
}

pub const ARP_MODES: &[&str] = &["Up", "Down", "Up-down", "Down-up", "As played", "Random", "Chord"];
pub const RATES: &[&str] = &["1/4", "1/8", "1/8 T", "1/16", "1/16 T", "1/32", "1/64"];
pub const RATE_BEATS: [f64; 7] = [1.0, 0.5, 1.0 / 3.0, 0.25, 1.0 / 6.0, 0.125, 0.0625];
pub const CHORDS: &[&str] = &["Major", "Minor", "7", "Maj7", "Min7", "Sus2", "Sus4", "Power", "Octave", "Add9", "Dim", "Aug"];
const CHORD_NOTES: [&[i8]; 12] = [&[0, 4, 7], &[0, 3, 7], &[0, 4, 7, 10], &[0, 4, 7, 11], &[0, 3, 7, 10], &[0, 2, 7], &[0, 5, 7], &[0, 7], &[0, 12], &[0, 4, 7, 14], &[0, 3, 6], &[0, 4, 8]];
pub const SCALES: &[&str] = &["Chromatic", "Major", "Minor", "Dorian", "Phrygian", "Lydian", "Mixolydian", "Harmonic minor", "Pentatonic major", "Pentatonic minor", "Blues"];
const SCALE_STEPS: [&[u8]; 11] = [
    &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
    &[0, 2, 4, 5, 7, 9, 11],
    &[0, 2, 3, 5, 7, 8, 10],
    &[0, 2, 3, 5, 7, 9, 10],
    &[0, 1, 3, 5, 7, 8, 10],
    &[0, 2, 4, 6, 7, 9, 11],
    &[0, 2, 4, 5, 7, 9, 10],
    &[0, 2, 3, 5, 7, 8, 11],
    &[0, 2, 4, 7, 9],
    &[0, 3, 5, 7, 10],
    &[0, 3, 5, 6, 7, 10],
];
pub const ROOTS: &[&str] = &["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

const ARP_PARAMS: &[ParamInfo] = &[
    ParamInfo::choice("mode", "Mode", ARP_MODES, 0.0),
    ParamInfo::choice("rate", "Rate", RATES, 3.0),
    ParamInfo::lin("octaves", "Octaves", 1.0, 4.0, 1.0, ""),
    ParamInfo::lin("gate", "Gate", 5.0, 100.0, 60.0, "%"),
    ParamInfo::lin("swing", "Swing", 0.0, 75.0, 0.0, "%"),
    ParamInfo::lin("velocity", "Velocity", 0.0, 127.0, 0.0, ""),
    ParamInfo::toggle("latch", "Latch", false),
];
const CHORD_PARAMS: &[ParamInfo] = &[
    ParamInfo::choice("chord", "Chord", CHORDS, 0.0),
    ParamInfo::lin("inversion", "Inversion", 0.0, 3.0, 0.0, ""),
    ParamInfo::toggle("spread", "Open voicing", false),
    ParamInfo::lin("strum", "Strum", 0.0, 120.0, 0.0, "ms"),
    ParamInfo::lin("velocity", "Upper velocity", 20.0, 100.0, 85.0, "%"),
];
const REPEAT_PARAMS: &[ParamInfo] = &[
    ParamInfo::choice("rate", "Rate", RATES, 3.0),
    ParamInfo::lin("gate", "Gate", 5.0, 100.0, 50.0, "%"),
    ParamInfo::lin("decay", "Velocity decay", 0.0, 50.0, 8.0, "%"),
    ParamInfo::lin("step", "Pitch step", -12.0, 12.0, 0.0, "st"),
    ParamInfo::lin("count", "Repeats", 1.0, 64.0, 64.0, ""),
];
const SCALE_PARAMS: &[ParamInfo] = &[ParamInfo::lin("transpose", "Transpose", -24.0, 24.0, 0.0, "st"), ParamInfo::choice("root", "Root", ROOTS, 0.0), ParamInfo::choice("scale", "Scale", SCALES, 0.0)];

pub fn registry() -> &'static [MidiFxDef] {
    const R: &[MidiFxDef] = &[
        MidiFxDef {
            id: "arp",
            name: "Arpeggiator",
            description: "Plays held notes one after another in time with the song: up, down, up-down, as played, random or as repeated chords, over up to four octaves.",
            params: ARP_PARAMS,
        },
        MidiFxDef { id: "chord", name: "Chord", description: "Turns every note into a chord (twelve types, inversions, open voicing), optionally strummed.", params: CHORD_PARAMS },
        MidiFxDef { id: "repeat", name: "Note repeat", description: "Retriggers held notes in time with the song, with fading velocity and a pitch step per repeat.", params: REPEAT_PARAMS },
        MidiFxDef { id: "scale", name: "Scale", description: "Transposes notes and moves them onto a scale (the nearest scale note at or below).", params: SCALE_PARAMS },
    ];
    R
}

pub fn find(id: &str) -> Option<&'static MidiFxDef> {
    registry().iter().find(|d| d.id == id)
}

// ------------------------------------------------------------------------------------------------ events

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ev {
    /// Absolute sample time.
    pub at: u64,
    pub on: bool,
    pub note: u8,
    pub vel: f32,
    pub pan: f32,
    pub detune: f32,
    pub slide: bool,
}

/// A fixed-capacity list (never allocates after creation).
#[derive(Clone)]
pub struct EvBuf {
    v: Vec<Ev>,
}

impl EvBuf {
    fn new(cap: usize) -> EvBuf {
        EvBuf { v: Vec::with_capacity(cap) }
    }
    fn push(&mut self, e: Ev) {
        if self.v.len() < self.v.capacity() {
            self.v.push(e);
        }
    }
    fn clear(&mut self) {
        self.v.clear();
    }
    pub fn as_slice(&self) -> &[Ev] {
        &self.v
    }
}

/// Song time for the generators.
#[derive(Clone, Copy)]
struct Clock {
    rate: f64,
    bpm: f64,
    /// Beat and sample time at the start of the current block.
    beat: f64,
    now: u64,
    playing: bool,
}

impl Clock {
    fn spb(&self) -> f64 {
        self.rate * 60.0 / self.bpm.max(1.0)
    }
    /// The sample time of the first grid line (multiple of `step` beats) at or after `t`.
    fn grid_at_or_after(&self, t: u64, step: f64) -> u64 {
        let beat = self.beat + (t as f64 - self.now as f64) / self.spb();
        let k = (beat / step - 1e-9).ceil();
        let at = self.now as f64 + (k * step - self.beat) * self.spb();
        at.max(t as f64).round() as u64
    }
}

trait Stage: Send {
    fn set(&mut self, id: &str, v: f32);
    /// An incoming event at time `e.at`: may emit events at that same time.
    fn event(&mut self, e: Ev, clock: &Clock, out: &mut EvBuf);
    /// Emits generated events with time `< until`.
    fn advance(&mut self, until: u64, clock: &Clock, out: &mut EvBuf);
    /// Whether it still has notes to play (keeps the instrument running).
    fn busy(&self) -> bool;
    fn panic(&mut self, at: u64, out: &mut EvBuf);
}

const MAX_HELD: usize = 32;

#[derive(Clone, Copy, Default)]
struct Held {
    note: u8,
    vel: f32,
    order: u32,
}

// --------------------------------------------------------------------------------------------- arpeggiator

struct Arp {
    mode: usize,
    rate: usize,
    octaves: usize,
    gate: f64,
    swing: f64,
    velocity: f32,
    latch: bool,
    held: [Held; MAX_HELD],
    n_held: usize,
    /// Keys physically down (for latch: a press after all were released starts a new chord).
    down: usize,
    counter: u32,
    next: Option<u64>,
    step: u64,
    index: usize,
    /// Notes sounding and when they end.
    sounding: [(u8, u64); 16],
    n_sounding: usize,
    rng: crate::dsp::Rng,
    proto: Ev,
}

impl Arp {
    fn new() -> Arp {
        Arp {
            mode: 0,
            rate: 3,
            octaves: 1,
            gate: 0.6,
            swing: 0.0,
            velocity: 0.0,
            latch: false,
            held: [Held::default(); MAX_HELD],
            n_held: 0,
            down: 0,
            counter: 0,
            next: None,
            step: 0,
            index: 0,
            sounding: [(0, 0); 16],
            n_sounding: 0,
            rng: crate::dsp::Rng::new(0xa4b),
            proto: Ev { at: 0, on: true, note: 60, vel: 0.8, pan: 0.0, detune: 0.0, slide: false },
        }
    }

    /// The note sequence: held notes sorted (or in press order), over the octaves, in the mode's direction.
    fn sequence(&self, out: &mut [(u8, f32); 128]) -> usize {
        let mut base = [(0u8, 0f32, 0u32); MAX_HELD];
        let n = self.n_held;
        for (b, h) in base.iter_mut().zip(&self.held[..n]) {
            *b = (h.note, h.vel, h.order);
        }
        if self.mode == 4 {
            base[..n].sort_by_key(|b| b.2);
        } else {
            base[..n].sort_by_key(|b| b.0);
        }
        let mut k = 0;
        for o in 0..self.octaves {
            for b in &base[..n] {
                let note = b.0 as usize + 12 * o;
                if note < 128 && k < 128 {
                    out[k] = (note as u8, b.1);
                    k += 1;
                }
            }
        }
        k
    }

    fn release_all(&mut self, at: u64, out: &mut EvBuf) {
        for (n, _) in &self.sounding[..self.n_sounding] {
            out.push(Ev { at, on: false, note: *n, ..self.proto });
        }
        self.n_sounding = 0;
    }
}

impl Stage for Arp {
    fn set(&mut self, id: &str, v: f32) {
        match id {
            "mode" => self.mode = (v.round().max(0.0) as usize).min(ARP_MODES.len() - 1),
            "rate" => self.rate = (v.round().max(0.0) as usize).min(RATES.len() - 1),
            "octaves" => self.octaves = v.round().clamp(1.0, 4.0) as usize,
            "gate" => self.gate = v.clamp(5.0, 100.0) as f64 / 100.0,
            "swing" => self.swing = v.clamp(0.0, 75.0) as f64 / 100.0,
            "velocity" => self.velocity = v.clamp(0.0, 127.0),
            "latch" => self.latch = v >= 0.5,
            _ => {}
        }
    }

    fn event(&mut self, e: Ev, _clock: &Clock, _out: &mut EvBuf) {
        self.proto = Ev { at: 0, ..e };
        if e.on {
            if self.latch && self.down == 0 {
                self.n_held = 0;
            }
            self.down += 1;
            if self.n_held < MAX_HELD && !self.held[..self.n_held].iter().any(|h| h.note == e.note) {
                self.counter += 1;
                self.held[self.n_held] = Held { note: e.note, vel: e.vel, order: self.counter };
                self.n_held += 1;
            }
        } else {
            self.down = self.down.saturating_sub(1);
            if !self.latch {
                if let Some(i) = self.held[..self.n_held].iter().position(|h| h.note == e.note) {
                    self.held[i] = self.held[self.n_held - 1];
                    self.n_held -= 1;
                }
            }
        }
    }

    fn advance(&mut self, until: u64, clock: &Clock, out: &mut EvBuf) {
        let step_beats = RATE_BEATS[self.rate];
        let step_len = (step_beats * clock.spb()).max(1.0);
        loop {
            // The earliest of: a note ending, the next step.
            let end = self.sounding[..self.n_sounding].iter().map(|s| s.1).min();
            let step_at = if self.n_held > 0 {
                Some(*self.next.get_or_insert_with(|| if clock.playing { clock.grid_at_or_after(clock.now, step_beats) } else { clock.now }))
            } else {
                self.next = None;
                None
            };
            let t = match (end, step_at) {
                (Some(a), Some(b)) => a.min(b),
                (Some(a), None) => a,
                (None, Some(b)) => b,
                (None, None) => break,
            };
            if t >= until {
                break;
            }
            if end == Some(t) {
                let mut k = 0;
                while k < self.n_sounding {
                    if self.sounding[k].1 <= t {
                        out.push(Ev { at: t, on: false, note: self.sounding[k].0, ..self.proto });
                        self.sounding[k] = self.sounding[self.n_sounding - 1];
                        self.n_sounding -= 1;
                    } else {
                        k += 1;
                    }
                }
                continue;
            }
            // A step.
            let mut seq = [(0u8, 0f32); 128];
            let n = self.sequence(&mut seq);
            if n == 0 {
                self.next = None;
                continue;
            }
            self.release_all(t, out);
            let len = (step_len * self.gate) as u64;
            let picks: &[(u8, f32)] = if self.mode == 6 {
                &seq[..n]
            } else {
                let i = match self.mode {
                    1 => n - 1 - self.index % n,
                    2 | 3 if n > 1 => {
                        let period = 2 * n - 2;
                        let p = self.index % period;
                        let up = if p < n { p } else { period - p };
                        if self.mode == 2 {
                            up
                        } else {
                            n - 1 - up
                        }
                    }
                    5 => (self.rng.next_u64() % n as u64) as usize,
                    _ => self.index % n,
                };
                std::slice::from_ref(&seq[i])
            };
            for (note, vel) in picks {
                let v = if self.velocity > 0.0 { self.velocity / 127.0 } else { *vel };
                out.push(Ev { at: t, on: true, note: *note, vel: v, ..self.proto });
                if self.n_sounding < self.sounding.len() {
                    self.sounding[self.n_sounding] = (*note, t + len.max(1));
                    self.n_sounding += 1;
                }
            }
            self.index += 1;
            self.step += 1;
            // Swing: every second step comes late by the swing share of half a step.
            let base = step_len;
            let shift = if self.step % 2 == 1 { base * self.swing * 0.5 } else { -base * self.swing * 0.5 };
            self.next = Some(t + (base + shift).round().max(1.0) as u64);
        }
    }

    fn busy(&self) -> bool {
        self.n_held > 0 || self.n_sounding > 0
    }

    fn panic(&mut self, at: u64, out: &mut EvBuf) {
        self.release_all(at, out);
        self.n_held = 0;
        self.down = 0;
        self.next = None;
    }
}

// ------------------------------------------------------------------------------------------------- chord

struct Chord {
    chord: usize,
    inversion: usize,
    spread: bool,
    strum: f64,
    velocity: f32,
    /// Per root: the notes it played.
    active: [(u8, [u8; 6], u8); 16],
    n_active: usize,
    /// Strummed notes still to start: (time, note, velocity, root).
    pending: [(u64, u8, f32, u8); 32],
    n_pending: usize,
    proto: Ev,
}

impl Chord {
    fn notes(&self, root: u8, out: &mut [u8; 6]) -> usize {
        let iv = CHORD_NOTES[self.chord];
        let mut v = [0i16; 6];
        let n = iv.len().min(6);
        for k in 0..n {
            v[k] = root as i16 + iv[k] as i16;
        }
        // Inversions: the lowest note moves up an octave, `inversion` times.
        for _ in 0..self.inversion.min(n.saturating_sub(1)) {
            v[..n].sort_unstable();
            v[0] += 12;
        }
        v[..n].sort_unstable();
        // Open voicing: the second note from the bottom drops an octave.
        if self.spread && n >= 3 {
            v[1] -= 12;
            v[..n].sort_unstable();
        }
        let mut k = 0;
        for x in &v[..n] {
            if (0..128).contains(x) {
                out[k] = *x as u8;
                k += 1;
            }
        }
        k
    }
}

impl Stage for Chord {
    fn set(&mut self, id: &str, v: f32) {
        match id {
            "chord" => self.chord = (v.round().max(0.0) as usize).min(CHORDS.len() - 1),
            "inversion" => self.inversion = v.round().clamp(0.0, 3.0) as usize,
            "spread" => self.spread = v >= 0.5,
            "strum" => self.strum = v.clamp(0.0, 120.0) as f64,
            "velocity" => self.velocity = v.clamp(20.0, 100.0) / 100.0,
            _ => {}
        }
    }

    fn event(&mut self, e: Ev, clock: &Clock, out: &mut EvBuf) {
        self.proto = e;
        if e.on {
            let mut notes = [0u8; 6];
            let n = self.notes(e.note, &mut notes);
            let gap = (self.strum * 0.001 * clock.rate) as u64;
            for (k, nn) in notes[..n].iter().enumerate() {
                let vel = if k == 0 { e.vel } else { e.vel * self.velocity };
                if k == 0 || gap == 0 {
                    out.push(Ev { note: *nn, vel, ..e });
                } else if self.n_pending < self.pending.len() {
                    self.pending[self.n_pending] = (e.at + gap * k as u64, *nn, vel, e.note);
                    self.n_pending += 1;
                }
            }
            if self.n_active < self.active.len() {
                self.active[self.n_active] = (e.note, notes, n as u8);
                self.n_active += 1;
            }
        } else if let Some(i) = self.active[..self.n_active].iter().position(|a| a.0 == e.note) {
            let (_, notes, n) = self.active[i];
            for nn in &notes[..n as usize] {
                out.push(Ev { note: *nn, ..e });
            }
            // Strums not yet started are dropped.
            let mut k = 0;
            while k < self.n_pending {
                if self.pending[k].3 == e.note {
                    self.pending[k] = self.pending[self.n_pending - 1];
                    self.n_pending -= 1;
                } else {
                    k += 1;
                }
            }
            self.active[i] = self.active[self.n_active - 1];
            self.n_active -= 1;
        } else {
            out.push(e);
        }
    }

    fn advance(&mut self, until: u64, _clock: &Clock, out: &mut EvBuf) {
        let mut k = 0;
        while k < self.n_pending {
            let (t, note, vel, _) = self.pending[k];
            if t < until {
                out.push(Ev { at: t, on: true, note, vel, ..self.proto });
                self.pending[k] = self.pending[self.n_pending - 1];
                self.n_pending -= 1;
            } else {
                k += 1;
            }
        }
    }

    fn busy(&self) -> bool {
        self.n_pending > 0
    }

    fn panic(&mut self, at: u64, out: &mut EvBuf) {
        for (_, notes, n) in &self.active[..self.n_active] {
            for nn in &notes[..*n as usize] {
                out.push(Ev { at, on: false, note: *nn, ..self.proto });
            }
        }
        self.n_active = 0;
        self.n_pending = 0;
    }
}

// -------------------------------------------------------------------------------------------- note repeat

struct Repeat {
    rate: usize,
    gate: f64,
    decay: f32,
    step: f32,
    count: u32,
    /// Held: (note, velocity, next time, repeats done, sounding note, its end).
    held: [(u8, f32, u64, u32, u8, u64); 16],
    n_held: usize,
    proto: Ev,
}

impl Stage for Repeat {
    fn set(&mut self, id: &str, v: f32) {
        match id {
            "rate" => self.rate = (v.round().max(0.0) as usize).min(RATES.len() - 1),
            "gate" => self.gate = v.clamp(5.0, 100.0) as f64 / 100.0,
            "decay" => self.decay = v.clamp(0.0, 50.0) / 100.0,
            "step" => self.step = v.clamp(-12.0, 12.0),
            "count" => self.count = v.round().clamp(1.0, 64.0) as u32,
            _ => {}
        }
    }

    fn event(&mut self, e: Ev, clock: &Clock, out: &mut EvBuf) {
        self.proto = e;
        let len = RATE_BEATS[self.rate] * clock.spb();
        if e.on {
            out.push(e);
            if self.n_held < self.held.len() {
                self.held[self.n_held] = (e.note, e.vel, e.at + len.round() as u64, 0, e.note, e.at + (len * self.gate) as u64);
                self.n_held += 1;
            }
        } else if let Some(i) = self.held[..self.n_held].iter().position(|h| h.0 == e.note) {
            let h = self.held[i];
            // The sounding repeat ends now if it has not already.
            if h.5 != u64::MAX && h.5 > e.at {
                out.push(Ev { note: h.4, ..e });
            }
            self.held[i] = self.held[self.n_held - 1];
            self.n_held -= 1;
        } else {
            out.push(e);
        }
    }

    fn advance(&mut self, until: u64, clock: &Clock, out: &mut EvBuf) {
        let len = (RATE_BEATS[self.rate] * clock.spb()).max(1.0);
        for h in self.held[..self.n_held].iter_mut() {
            loop {
                let (end, next) = (h.5, h.2);
                if end != u64::MAX && end < until && end <= next {
                    out.push(Ev { at: end, on: false, note: h.4, ..self.proto });
                    h.5 = u64::MAX;
                    continue;
                }
                if next < until && h.3 + 1 < self.count {
                    h.3 += 1;
                    let note = (h.0 as f32 + self.step * h.3 as f32).round().clamp(0.0, 127.0) as u8;
                    let vel = (h.1 * (1.0 - self.decay).powi(h.3 as i32)).max(0.02);
                    if h.5 != u64::MAX {
                        out.push(Ev { at: next, on: false, note: h.4, ..self.proto });
                    }
                    out.push(Ev { at: next, on: true, note, vel, ..self.proto });
                    h.4 = note;
                    h.5 = next + (len * self.gate) as u64;
                    h.2 = next + len.round() as u64;
                    continue;
                }
                break;
            }
        }
    }

    fn busy(&self) -> bool {
        self.n_held > 0
    }

    fn panic(&mut self, at: u64, out: &mut EvBuf) {
        for h in &self.held[..self.n_held] {
            out.push(Ev { at, on: false, note: h.4, ..self.proto });
        }
        self.n_held = 0;
    }
}

// -------------------------------------------------------------------------------------------------- scale

struct Scale {
    transpose: i32,
    root: u8,
    scale: usize,
    /// What each incoming note was mapped to (so its note-off matches).
    map: [u8; 128],
}

impl Scale {
    fn map(&self, n: u8) -> u8 {
        let t = (n as i32 + self.transpose).clamp(0, 127);
        let steps = SCALE_STEPS[self.scale];
        // The nearest scale note at or below.
        let mut x = t;
        while x > 0 {
            let pc = ((x - self.root as i32).rem_euclid(12)) as u8;
            if steps.contains(&pc) {
                break;
            }
            x -= 1;
        }
        x as u8
    }
}

impl Stage for Scale {
    fn set(&mut self, id: &str, v: f32) {
        match id {
            "transpose" => self.transpose = v.round() as i32,
            "root" => self.root = (v.round().max(0.0) as u8).min(11),
            "scale" => self.scale = (v.round().max(0.0) as usize).min(SCALES.len() - 1),
            _ => {}
        }
    }
    fn event(&mut self, e: Ev, _clock: &Clock, out: &mut EvBuf) {
        let n = if e.on {
            let m = self.map(e.note);
            self.map[e.note as usize] = m;
            m
        } else {
            self.map[e.note as usize]
        };
        out.push(Ev { note: n, ..e });
    }
    fn advance(&mut self, _until: u64, _clock: &Clock, _out: &mut EvBuf) {}
    fn busy(&self) -> bool {
        false
    }
    fn panic(&mut self, _at: u64, _out: &mut EvBuf) {}
}

fn new_stage(id: &str) -> Option<Box<dyn Stage>> {
    Some(match id {
        "arp" => Box::new(Arp::new()),
        "chord" => Box::new(Chord {
            chord: 0,
            inversion: 0,
            spread: false,
            strum: 0.0,
            velocity: 0.85,
            active: [(0, [0; 6], 0); 16],
            n_active: 0,
            pending: [(0, 0, 0.0, 0); 32],
            n_pending: 0,
            proto: Ev { at: 0, on: true, note: 60, vel: 0.8, pan: 0.0, detune: 0.0, slide: false },
        }),
        "repeat" => {
            Box::new(Repeat { rate: 3, gate: 0.5, decay: 0.08, step: 0.0, count: 64, held: [(0, 0.0, 0, 0, 0, 0); 16], n_held: 0, proto: Ev { at: 0, on: true, note: 60, vel: 0.8, pan: 0.0, detune: 0.0, slide: false } })
        }
        "scale" => {
            let mut map = [0u8; 128];
            for (k, m) in map.iter_mut().enumerate() {
                *m = k as u8;
            }
            Box::new(Scale { transpose: 0, root: 0, scale: 0, map })
        }
        _ => return None,
    })
}

// ------------------------------------------------------------------------------------------------- chain

/// A channel's instrument behind its MIDI effects.
pub struct MidiChain {
    inner: Box<dyn InstrumentVoice>,
    stages: Vec<(String, bool, Box<dyn Stage>)>,
    clock: Clock,
    /// Event buffers: what goes into a stage, what comes out, and the block's final events.
    a: EvBuf,
    b: EvBuf,
    fin: EvBuf,
}

impl MidiChain {
    pub fn new(inner: Box<dyn InstrumentVoice>, rate: f32) -> MidiChain {
        MidiChain { inner, stages: Vec::new(), clock: Clock { rate: rate as f64, bpm: 120.0, beat: 0.0, now: 0, playing: false }, a: EvBuf::new(512), b: EvBuf::new(512), fin: EvBuf::new(1024) }
    }

    fn active(&self) -> bool {
        self.stages.iter().any(|s| s.1)
    }

    /// Runs `ev` (and what each stage makes of it) through the stages from `from` on; the results land in
    /// `fin` (events reaching the instrument).
    fn feed(&mut self, from: usize, ev: Ev) {
        self.a.clear();
        self.a.push(ev);
        for k in from..self.stages.len() {
            if !self.stages[k].1 {
                continue;
            }
            self.b.clear();
            let clock = self.clock;
            for e in self.a.v.iter() {
                self.stages[k].2.event(*e, &clock, &mut self.b);
            }
            std::mem::swap(&mut self.a, &mut self.b);
        }
        for e in self.a.v.iter() {
            self.fin.push(*e);
        }
    }

    fn deliver(&mut self, e: Ev) {
        if e.on {
            self.inner.note_on(NoteOn { note: e.note, velocity: e.vel, pan: e.pan, detune: e.detune, slide: e.slide });
        } else {
            self.inner.note_off(e.note);
        }
    }

    /// Sets the transport for the next block.
    pub fn set_transport(&mut self, bpm: f64, beat: f64, playing: bool) {
        self.clock.bpm = bpm.max(1.0);
        self.clock.beat = beat;
        self.clock.playing = playing;
    }
}

impl InstrumentVoice for MidiChain {
    fn note_on(&mut self, n: NoteOn) {
        if !self.active() {
            return self.inner.note_on(n);
        }
        self.fin.clear();
        let e = Ev { at: self.clock.now, on: true, note: n.note, vel: n.velocity, pan: n.pan, detune: n.detune, slide: n.slide };
        self.feed(0, e);
        for k in 0..self.fin.v.len() {
            let e = self.fin.v[k];
            self.deliver(e);
        }
        self.fin.clear();
    }

    fn note_off(&mut self, note: u8) {
        if !self.active() {
            return self.inner.note_off(note);
        }
        self.fin.clear();
        let e = Ev { at: self.clock.now, on: false, note, vel: 0.0, pan: 0.0, detune: 0.0, slide: false };
        self.feed(0, e);
        for k in 0..self.fin.v.len() {
            let e = self.fin.v[k];
            self.deliver(e);
        }
        self.fin.clear();
    }

    fn choke(&mut self, seconds: f32) {
        // The stages forget their notes; the instrument's own choke silences what sounds.
        for s in self.stages.iter_mut() {
            self.b.clear();
            s.2.panic(self.clock.now, &mut self.b);
        }
        self.inner.choke(seconds);
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len();
        if !self.active() {
            self.inner.process(l, r);
            self.clock.now += n as u64;
            return;
        }
        let until = self.clock.now + n as u64;
        // Generated events of this block, stage by stage (later stages also see what earlier ones made).
        self.fin.clear();
        for k in 0..self.stages.len() {
            if !self.stages[k].1 {
                continue;
            }
            self.b.clear();
            let clock = self.clock;
            self.stages[k].2.advance(until, &clock, &mut self.b);
            let made: usize = self.b.v.len();
            for j in 0..made {
                let e = self.b.v[j];
                // Through the following stages.
                let mut cur = EvBuf { v: std::mem::take(&mut self.a.v) };
                cur.clear();
                cur.push(e);
                for m in k + 1..self.stages.len() {
                    if !self.stages[m].1 {
                        continue;
                    }
                    let mut next = EvBuf { v: std::mem::take(&mut self.fin.v) };
                    let keep = next.v.len();
                    for ce in cur.v.iter() {
                        self.stages[m].2.event(*ce, &clock, &mut next);
                    }
                    // `next` holds the block's results so far plus this stage's output after `keep`.
                    cur.clear();
                    for x in next.v.drain(keep..) {
                        cur.push(x);
                    }
                    self.fin.v = next.v;
                }
                for x in cur.v.iter() {
                    self.fin.push(*x);
                }
                self.a.v = cur.v;
            }
        }
        // In time order (stable, in place).
        let evs = &mut self.fin.v;
        for i in 1..evs.len() {
            let mut j = i;
            while j > 0 && evs[j - 1].at > evs[j].at {
                evs.swap(j - 1, j);
                j -= 1;
            }
        }
        // Render in pieces between the events.
        let mut pos = 0usize;
        let mut i = 0;
        while pos < n {
            let next = self.fin.v.get(i).map(|e| (e.at.saturating_sub(self.clock.now) as usize).min(n)).unwrap_or(n).max(pos);
            if next > pos {
                self.inner.process(&mut l[pos..next], &mut r[pos..next]);
                pos = next;
            }
            while let Some(e) = self.fin.v.get(i).copied() {
                if (e.at.saturating_sub(self.clock.now) as usize) > pos {
                    break;
                }
                self.deliver(e);
                i += 1;
            }
            if i >= self.fin.v.len() && pos >= n {
                break;
            }
        }
        self.fin.clear();
        self.clock.now = until;
        // Keep the transport beat moving for the next block if nobody sets it.
        self.clock.beat += n as f64 / self.clock.spb();
    }

    fn update(&mut self, model: &Model, cache: &SampleCache) {
        self.inner.update(model, cache)
    }

    fn active_voices(&self) -> usize {
        self.inner.active_voices() + usize::from(self.stages.iter().any(|s| s.1 && s.2.busy()))
    }

    fn set_param(&mut self, id: &str, value: f32) {
        if id == "__bpm" {
            self.clock.bpm = value.max(1.0) as f64;
        }
        self.inner.set_param(id, value)
    }

    fn configure_midi(&mut self, fx: &[MidiFx]) {
        // Rebuild only when the list of types changed; otherwise keep the stages (and their held notes).
        let same = fx.len() == self.stages.len() && fx.iter().zip(&self.stages).all(|(f, s)| f.id == s.0);
        if !same {
            let mut out = EvBuf::new(256);
            for s in self.stages.iter_mut() {
                s.2.panic(self.clock.now, &mut out);
            }
            for e in out.v.clone() {
                self.deliver(e);
            }
            self.stages = fx.iter().filter_map(|f| new_stage(&f.id).map(|s| (f.id.clone(), f.enabled, s))).collect();
        }
        for (f, s) in fx.iter().zip(self.stages.iter_mut()) {
            s.1 = f.enabled;
            if let Some(def) = find(&f.id) {
                for p in def.params {
                    s.2.set(p.id, def.value(&f.params, p.id));
                }
            }
        }
    }

    fn set_transport(&mut self, bpm: f64, beat: f64, playing: bool) {
        MidiChain::set_transport(self, bpm, beat, playing);
        self.inner.set_transport(bpm, beat, playing);
    }

    fn display(&self) -> Option<[i8; 8]> {
        self.inner.display()
    }
    fn plugin_params(&self) -> Vec<PluginParam> {
        self.inner.plugin_params()
    }
    fn plugin_param(&self, id: u32) -> Option<f64> {
        self.inner.plugin_param(id)
    }
    fn save_state(&mut self) -> Option<Vec<u8>> {
        self.inner.save_state()
    }
    fn load_state(&mut self, data: &[u8]) -> bool {
        self.inner.load_state(data)
    }
    fn editor(&mut self, open: bool) -> bool {
        self.inner.editor(open)
    }
    fn idle(&mut self) -> bool {
        self.inner.idle()
    }
    fn editor_fullscreen(&mut self, on: bool) -> bool {
        self.inner.editor_fullscreen(on)
    }
    fn editor_error(&self) -> Option<String> {
        self.inner.editor_error()
    }
    fn take_param_edits(&mut self) -> Vec<(u32, f64)> {
        self.inner.take_param_edits()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records what reaches the instrument (note, on, sample time).
    struct Rec {
        log: std::sync::Arc<std::sync::Mutex<Vec<(u8, bool, u64)>>>,
        t: u64,
    }
    impl InstrumentVoice for Rec {
        fn note_on(&mut self, n: NoteOn) {
            self.log.lock().unwrap().push((n.note, true, self.t));
        }
        fn note_off(&mut self, note: u8) {
            self.log.lock().unwrap().push((note, false, self.t));
        }
        fn choke(&mut self, _s: f32) {}
        fn process(&mut self, l: &mut [f32], _r: &mut [f32]) {
            self.t += l.len() as u64;
        }
        fn update(&mut self, _m: &Model, _c: &SampleCache) {}
        fn active_voices(&self) -> usize {
            0
        }
    }

    fn chain(fx: &[(&str, &[(&str, f32)])]) -> (MidiChain, std::sync::Arc<std::sync::Mutex<Vec<(u8, bool, u64)>>>) {
        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut c = MidiChain::new(Box::new(Rec { log: log.clone(), t: 0 }), 48000.0);
        let list: Vec<MidiFx> = fx
            .iter()
            .map(|(id, ps)| {
                let mut params = find(id).unwrap().defaults();
                for (k, v) in ps.iter() {
                    params.insert(k.to_string(), *v);
                }
                MidiFx { id: id.to_string(), enabled: true, params }
            })
            .collect();
        c.configure_midi(&list);
        c.set_transport(120.0, 0.0, true);
        (c, log)
    }

    fn run(c: &mut MidiChain, blocks: usize) {
        let mut l = [0.0f32; 128];
        let mut r = [0.0f32; 128];
        for _ in 0..blocks {
            c.process(&mut l, &mut r);
        }
    }

    #[test]
    fn arpeggiator_plays_up_in_sixteenths() {
        let (mut c, log) = chain(&[("arp", &[])]);
        for n in [64, 60, 67] {
            c.note_on(NoteOn { note: n, velocity: 0.8, pan: 0.0, detune: 0.0, slide: false });
        }
        run(&mut c, 375); // two beats at 120 BPM (48 000 samples)
        let ons: Vec<(u8, u64)> = log.lock().unwrap().iter().filter(|e| e.1).map(|e| (e.0, e.2)).collect();
        assert_eq!(&ons[..4].iter().map(|e| e.0).collect::<Vec<_>>(), &[60, 64, 67, 60]);
        // A sixteenth at 120 BPM = 6000 samples, sample-accurate.
        assert_eq!(ons[1].1 - ons[0].1, 6000);
        assert_eq!(ons.len(), 8);
    }

    #[test]
    fn chord_turns_a_note_into_a_triad_and_releases_it() {
        let (mut c, log) = chain(&[("chord", &[("chord", 1.0)])]);
        c.note_on(NoteOn { note: 60, velocity: 0.8, pan: 0.0, detune: 0.0, slide: false });
        c.note_off(60);
        let l = log.lock().unwrap();
        let ons: Vec<u8> = l.iter().filter(|e| e.1).map(|e| e.0).collect();
        let offs: Vec<u8> = l.iter().filter(|e| !e.1).map(|e| e.0).collect();
        assert_eq!(ons, vec![60, 63, 67]);
        assert_eq!(offs, vec![60, 63, 67]);
    }

    #[test]
    fn scale_then_chord_and_repeat_chain() {
        let (mut c, log) = chain(&[("scale", &[("scale", 1.0)]), ("repeat", &[("rate", 3.0), ("count", 3.0)])]);
        // C# is not in C major: it becomes C, then repeats twice more.
        c.note_on(NoteOn { note: 61, velocity: 0.8, pan: 0.0, detune: 0.0, slide: false });
        run(&mut c, 375);
        c.note_off(61);
        let l = log.lock().unwrap();
        let ons: Vec<u8> = l.iter().filter(|e| e.1).map(|e| e.0).collect();
        assert_eq!(ons, vec![60, 60, 60]);
        let on_count = l.iter().filter(|e| e.1).count();
        let off_count = l.iter().filter(|e| !e.1).count();
        assert_eq!(on_count, off_count, "every note ends: {l:?}");
    }

    /// No allocation while playing: the chain's buffers keep their capacity.
    #[test]
    fn buffers_do_not_grow() {
        let (mut c, _log) = chain(&[("arp", &[("octaves", 4.0), ("rate", 6.0)]), ("chord", &[("chord", 9.0)])]);
        let caps = (c.a.v.capacity(), c.b.v.capacity(), c.fin.v.capacity());
        for n in 40..70 {
            c.note_on(NoteOn { note: n, velocity: 0.8, pan: 0.0, detune: 0.0, slide: false });
        }
        run(&mut c, 2000);
        assert_eq!(caps, (c.a.v.capacity(), c.b.v.capacity(), c.fin.v.capacity()));
    }
}
