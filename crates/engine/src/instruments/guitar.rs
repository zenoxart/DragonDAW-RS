//! Dragon Strings: a modelled plucked-string instrument.
//!
//! Every sounding string is a feedback loop the length of one period (an extended Karplus–Strong
//! string): a delay line read with 4-point Lagrange interpolation (exact tuning), a one-pole loss
//! filter (brightness) and a loss gain set from the ring time. A pluck fills the loop with shaped
//! noise: lowpassed by the pick's hardness and comb-filtered by where the string is picked. The
//! strings then go through the instrument: a resonating wooden body (acoustic models) or a pickup
//! and amp (electric, bass), then the FX rack.
//!
//! Playing follows a guitarist: one note per string (a new note on a busy string cuts the old one),
//! strums across the strings, chords voiced on the neck, rhythm patterns in time with the song.

use super::{midi_to_hz, InstrumentVoice, NoteOn};
use crate::dsp::Rng;
use crate::effects::{self, Effect};
use crate::gtr::*;
use crate::model::Instrument as Model;
use crate::samples::SampleCache;

const MAX_STRINGS: usize = 32;
const MAX_EVENTS: usize = 128;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Normal,
    Light,
    Bass,
    Chuck,
}

/// One pluck waiting for its moment (strums, patterns, doubled courses).
#[derive(Clone, Copy, Debug)]
struct Pluck {
    at: u64,
    string: usize,
    note: u8,
    vel: f32,
    key: u8,
    art: Articulation,
    second: bool,
}

struct Str {
    on: bool,
    string: usize,
    note: u8,
    key: u8,
    buf: Vec<f32>,
    w: usize,
    /// Read delay (samples) and its target (slides glide towards it).
    d: f32,
    d_target: f32,
    d_step: f32,
    lp_s: f32,
    lp_y: f32,
    loss: f32,
    gl: f32,
    gr: f32,
    fade: f32,
    fade_mul: f32,
    released: bool,
    buzz_thr: f32,
    pickup_tap: f32,
    quiet: u32,
    age: u64,
    second: bool,
    /// DC blocker.
    dc_x: f32,
    dc_y: f32,
}

impl Str {
    fn new(len: usize) -> Self {
        Str {
            on: false,
            string: 0,
            note: 0,
            key: 0,
            buf: vec![0.0; len],
            w: 0,
            d: 100.0,
            d_target: 100.0,
            d_step: 0.0,
            lp_s: 0.3,
            lp_y: 0.0,
            loss: 0.999,
            gl: 0.7,
            gr: 0.7,
            fade: 1.0,
            fade_mul: 1.0,
            released: false,
            buzz_thr: 0.0,
            pickup_tap: 0.0,
            quiet: 0,
            age: 0,
            second: false,
            dc_x: 0.0,
            dc_y: 0.0,
        }
    }

    #[inline]
    fn read(&self, delay: f32) -> f32 {
        // 4-point Lagrange interpolation around `delay` samples back.
        let len = self.buf.len();
        let di = delay.floor();
        let f = delay - di;
        let i = di as usize;
        let at = |k: usize| self.buf[(self.w + len * 2 - k) % len];
        // Delays are kept ≥ 2.5, so i − 1 ≥ 1 (never the sample being written).
        let (ym1, y0, y1, y2) = (at(i.max(2) - 1), at(i), at(i + 1), at(i + 2));
        // Points at delays i-1, i, i+1, i+2; fraction f between i and i+1.
        let c0 = -f * (f - 1.0) * (f - 2.0) / 6.0;
        let c1 = (f + 1.0) * (f - 1.0) * (f - 2.0) / 2.0;
        let c2 = -(f + 1.0) * f * (f - 2.0) / 2.0;
        let c3 = (f + 1.0) * f * (f - 1.0) / 6.0;
        c0 * ym1 + c1 * y0 + c2 * y1 + c3 * y2
    }
}

/// A two-pole resonator (constant peak gain) for the body.
#[derive(Clone, Copy, Default)]
struct Reso {
    b0: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
    gain: f32,
}

impl Reso {
    fn set(&mut self, rate: f32, hz: f32, q: f32, gain: f32) {
        let w = std::f32::consts::TAU * (hz / rate).min(0.45);
        let alpha = w.sin() / (2.0 * q);
        let a0 = 1.0 + alpha;
        self.b0 = alpha / a0;
        self.a1 = -2.0 * w.cos() / a0;
        self.a2 = (1.0 - alpha) / a0;
        self.gain = gain;
    }
    #[inline]
    fn run(&mut self, x: f32) -> f32 {
        let y = self.b0 * (x - self.x2) - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y * self.gain
    }
}

/// Body modes per model: (Hz, Q, gain) at body size 1.
fn body_modes(m: GuitarModel) -> &'static [(f32, f32, f32)] {
    match m {
        GuitarModel::Nylon => &[(95.0, 9.0, 1.1), (190.0, 8.0, 0.9), (370.0, 7.0, 0.5), (620.0, 6.0, 0.35), (1100.0, 5.0, 0.2), (2400.0, 3.0, 0.12)],
        GuitarModel::Banjo => &[(290.0, 6.0, 0.6), (900.0, 5.0, 0.7), (1800.0, 6.0, 0.9), (3200.0, 5.0, 0.8), (5200.0, 4.0, 0.4)],
        GuitarModel::Harp => &[(70.0, 7.0, 1.0), (150.0, 7.0, 0.8), (320.0, 6.0, 0.5), (700.0, 5.0, 0.3), (1600.0, 4.0, 0.15)],
        GuitarModel::Sitar => &[(110.0, 8.0, 0.8), (230.0, 8.0, 0.7), (480.0, 7.0, 0.5), (900.0, 6.0, 0.4), (2600.0, 4.0, 0.35)],
        _ => &[(98.0, 10.0, 1.0), (200.0, 9.0, 1.0), (390.0, 8.0, 0.55), (560.0, 7.0, 0.4), (830.0, 7.0, 0.3), (1450.0, 5.0, 0.22), (2900.0, 3.0, 0.15)],
    }
}

/// One-pole lowpass / highpass state.
#[derive(Clone, Copy, Default)]
struct OnePole {
    y: f32,
}

impl OnePole {
    #[inline]
    fn lp(&mut self, x: f32, a: f32) -> f32 {
        self.y += a * (x - self.y);
        self.y
    }
}

fn onepole_coef(hz: f32, rate: f32) -> f32 {
    1.0 - (-std::f32::consts::TAU * hz / rate).exp()
}

pub struct Guitar {
    rate: f32,
    p: GuitarParams,
    strs: Vec<Str>,
    events: Vec<Pluck>,
    /// Keys pressed since the last block (strummed together).
    pending: Vec<(u8, f32)>,
    held: Vec<u8>,
    art: Articulation,
    clock: u64,
    rng: Rng,
    down_next: bool,
    /// The current chord (chord / pattern mode): note per string.
    chord: Vec<Option<u8>>,
    chord_key: u8,
    chord_vel: f32,
    pattern_step: usize,
    next_step_at: u64,
    bpm: f32,
    body: [[Reso; 8]; 2],
    body_n: usize,
    hp: [OnePole; 2],
    tone_lp: [OnePole; 2],
    cab1: [OnePole; 2],
    cab2: [OnePole; 2],
    fx: Vec<(String, Box<dyn Effect>)>,
    scratch: (Vec<f32>, Vec<f32>),
    tail: usize,
    /// Ready-made excitation (avoids allocating on the audio thread).
    exc: Vec<f32>,
}

impl Guitar {
    pub fn new(rate: f32) -> Self {
        let len = ((rate / 18.0) as usize + 8).next_power_of_two();
        let mut g = Guitar {
            rate,
            p: GuitarParams::default(),
            strs: (0..MAX_STRINGS).map(|_| Str::new(len)).collect(),
            events: Vec::with_capacity(MAX_EVENTS),
            pending: Vec::with_capacity(32),
            held: Vec::with_capacity(32),
            art: Articulation::Sustain,
            clock: 0,
            rng: Rng::new(0x5EED_6017),
            down_next: true,
            chord: Vec::with_capacity(8),
            chord_key: 0,
            chord_vel: 0.8,
            pattern_step: 0,
            next_step_at: 0,
            bpm: 120.0,
            body: [[Reso::default(); 8]; 2],
            body_n: 0,
            hp: [OnePole::default(); 2],
            tone_lp: [OnePole::default(); 2],
            cab1: [OnePole::default(); 2],
            cab2: [OnePole::default(); 2],
            fx: Vec::new(),
            scratch: (vec![0.0; 4096], vec![0.0; 4096]),
            tail: 0,
            exc: vec![0.0; len],
        };
        g.set_body();
        g
    }

    fn set_body(&mut self) {
        let modes = body_modes(self.p.model);
        self.body_n = modes.len().min(8);
        let size = self.p.body_size.clamp(0.3, 2.0);
        for (k, (hz, q, gain)) in modes.iter().take(8).enumerate() {
            for ch in 0..2 {
                // The two channels a touch apart: a wider, more natural body.
                let spread = if ch == 0 { 0.985 } else { 1.015 };
                self.body[ch][k].set(self.rate, hz / size * spread, *q, *gain);
            }
        }
    }

    fn open(&self) -> Vec<u8> {
        self.p.open_strings()
    }

    fn frets(&self) -> u8 {
        self.p.model.frets()
    }

    /// Fades out whatever plays on `string` (a string plays one note at a time).
    fn choke_string(&mut self, string: usize, secs: f32) {
        let mul = (-6.9 / (secs.max(0.002) * self.rate)).exp();
        for s in self.strs.iter_mut().filter(|s| s.on && s.string == string) {
            s.fade_mul = s.fade_mul.min(mul);
            s.released = true;
        }
    }

    fn free_slot(&mut self) -> usize {
        if let Some(i) = self.strs.iter().position(|s| !s.on) {
            return i;
        }
        // Steal the oldest.
        let mut best = 0;
        for (i, s) in self.strs.iter().enumerate() {
            if s.age < self.strs[best].age {
                best = i;
            }
        }
        best
    }

    /// Starts a string now.
    fn pluck(&mut self, pk: Pluck) {
        let art = pk.art;
        if !pk.second {
            self.choke_string(pk.string, if art == Articulation::Dead { 0.004 } else { 0.012 });
        }
        let p = &self.p;
        let n_str = p.tuning.len().max(1);
        let hum = p.humanize;
        let r1 = self.rng.bipolar();
        let r2 = self.rng.bipolar();
        let mut note = pk.note as f32;
        if art == Articulation::Harmonic {
            note += 12.0;
        }
        if pk.second {
            note += if p.model == GuitarModel::TwelveString && pk.string < 4.min(n_str) { 12.0 } else { 0.0 };
            note += p.doubling_detune / 100.0 * if pk.string.is_multiple_of(2) { 1.0 } else { -1.0 };
        }
        let f0 = midi_to_hz(note);
        let period = (self.rate / f0).clamp(4.0, (self.strs[0].buf.len() - 8) as f32);
        // Brightness: the loss filter. Harder, louder plucks are brighter.
        let vel = pk.vel.clamp(0.0, 1.0);
        let mut bright = (p.brightness + (vel - 0.6) * 0.3 * p.velocity + r1 * 0.04 * hum).clamp(0.0, 1.0);
        let mut ring = p.sustain.max(0.05) * (82.0 / f0).powf(0.55).clamp(0.08, 1.6);
        match art {
            Articulation::PalmMute => {
                ring = ring.min(0.18 + 0.1 * p.sustain.min(1.0));
                bright *= 0.45;
            }
            Articulation::Dead => {
                ring = 0.035;
                bright *= 0.5;
            }
            Articulation::Harmonic => {
                ring *= 1.4;
                bright = (bright + 0.25).min(1.0);
            }
            _ => {}
        }
        let lp_s = (0.02 + (1.0 - bright) * 0.72).clamp(0.0, 0.95);
        // The loss filter's delay at f0 is taken off the read delay, so the loop stays in tune.
        let w = std::f32::consts::TAU * f0 / self.rate;
        let tau = (lp_s * w.sin()).atan2(1.0 - lp_s * w.cos()) / w;
        let h_mag = (1.0 - lp_s) / (1.0 - 2.0 * lp_s * w.cos() + lp_s * lp_s).sqrt();
        let loss = (10f32.powf(-3.0 / (f0 * ring)) / h_mag).min(0.99995);
        let d = (period - tau).max(2.5);
        // The pluck: noise, softened by a finger or pick, combed by where the string is picked.
        let len = (period.round() as usize).clamp(4, self.exc.len());
        let mut soft = (0.05 + (1.0 - p.pick_hardness) * 0.8 - vel * 0.2 * p.velocity).clamp(0.02, 0.95);
        if art == Articulation::Harmonic {
            soft = 0.97;
        }
        let a = 1.0 - soft;
        let (mut y1, mut y2) = (0.0f32, 0.0f32);
        for k in 0..len {
            let x = self.rng.bipolar();
            y1 += a * (x - y1);
            y2 += a * (y1 - y2);
            self.exc[k] = y2;
        }
        let pos = (p.pick_position * (1.0 + r2 * 0.15 * hum)).clamp(0.03, 0.5);
        let tap = ((pos * len as f32).round() as usize).clamp(1, len - 1);
        let mut mean = 0.0;
        // Comb in place, from the end so earlier values are still the originals.
        for k in (0..len).rev() {
            let back = self.exc[(k + len - tap) % len];
            self.exc[k] -= back;
        }
        for k in 0..len {
            mean += self.exc[k];
        }
        mean /= len as f32;
        let mut peak = 1e-6f32;
        for k in 0..len {
            self.exc[k] -= mean;
            peak = peak.max(self.exc[k].abs());
        }
        let level = ((1.0 - p.velocity) + p.velocity * vel.powf(1.4)) * (1.0 + r1 * 0.12 * hum) * if pk.second { p.doubling } else { 1.0 };
        let scale = 1.5 * level / peak;
        let pan = if n_str > 1 { (pk.string as f32 / (n_str - 1) as f32 - 0.5) * p.width } else { 0.0 } + if pk.second { 0.15 * p.width } else { 0.0 };
        let gl = (0.5 * (1.0 - pan)).sqrt();
        let gr = (0.5 * (1.0 + pan)).sqrt();
        let buzz_thr = if p.buzz > 0.0 { 0.35 * (1.0 - p.buzz * 0.85) } else { 0.0 };
        let pickup_tap = if matches!(p.model, GuitarModel::Electric | GuitarModel::Bass) { period * (0.06 + 0.2 * p.pickup) } else { 0.0 };
        let i = self.free_slot();
        let clock = self.clock;
        let s = &mut self.strs[i];
        s.buf.iter_mut().for_each(|x| *x = 0.0);
        let blen = s.buf.len();
        for k in 0..len {
            s.buf[(s.w + blen * 2 - len + k) % blen] = self.exc[k] * scale;
        }
        s.on = true;
        s.string = pk.string;
        s.note = pk.note;
        s.key = pk.key;
        s.d = d;
        s.d_target = d;
        s.d_step = 0.0;
        s.lp_s = lp_s;
        s.lp_y = 0.0;
        s.loss = loss;
        s.gl = gl;
        s.gr = gr;
        s.fade = 1.0;
        s.fade_mul = 1.0;
        s.released = false;
        s.buzz_thr = buzz_thr;
        s.pickup_tap = pickup_tap;
        s.quiet = 0;
        s.age = clock;
        s.second = pk.second;
        s.dc_x = 0.0;
        s.dc_y = 0.0;
    }

    /// Queues a pluck (and its second string on doubled courses).
    fn queue(&mut self, pk: Pluck) {
        if self.events.len() + 2 > MAX_EVENTS {
            return;
        }
        self.events.push(pk);
        if self.p.doubling > 0.0 && self.p.model != GuitarModel::Harp {
            let lag = (self.rate * (0.002 + 0.004 * self.rng.next_f32())) as u64;
            self.events.push(Pluck { at: pk.at + lag, second: true, ..pk });
        }
    }

    /// Strums `notes` (note per string, `None` = not played) starting now.
    fn strum(&mut self, notes: &[Option<u8>], vel: f32, key: u8, kind: Kind, art: Articulation) {
        let down = match self.p.strum_dir {
            StrumDir::Down => true,
            StrumDir::Up => false,
            StrumDir::Alternate => {
                let d = self.down_next;
                self.down_next = !d;
                d
            }
        };
        self.strum_dir(notes, vel, key, kind, art, down);
    }

    fn strum_dir(&mut self, notes: &[Option<u8>], vel: f32, key: u8, kind: Kind, art: Articulation, down: bool) {
        let open = self.open();
        let mut order: Vec<usize> = (0..notes.len().min(open.len())).filter(|&i| notes[i].is_some()).collect();
        order.sort_by_key(|&i| notes[i].unwrap_or(0));
        match kind {
            Kind::Light => {
                let skip = order.len().saturating_sub(4);
                order.drain(..skip);
            }
            Kind::Bass => order.truncate(1),
            _ => {}
        }
        if !down {
            order.reverse();
        }
        let gap = self.p.strum_time.max(0.0) / 1000.0 * self.rate;
        let art = if kind == Kind::Chuck { Articulation::Dead } else { art };
        let base_vel = if kind == Kind::Light { vel * 0.7 } else { vel };
        for (k, &i) in order.iter().enumerate() {
            let jitter = self.rng.bipolar() * self.p.humanize * 0.003 * self.rate;
            let at = self.clock + (k as f32 * gap + if k > 0 { jitter } else { 0.0 }).max(0.0) as u64;
            let v = base_vel * (1.0 - 0.15 * k as f32 / order.len().max(1) as f32);
            if let Some(n) = notes[i] {
                self.queue(Pluck { at, string: i, note: n, vel: v, key, art, second: false });
            }
        }
    }

    fn play_pending(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let pending = std::mem::take(&mut self.pending);
        let art = self.art;
        match self.p.mode {
            PlayMode::Solo => {
                for &(note, vel) in &pending {
                    self.play_solo(note, vel, art);
                }
            }
            PlayMode::Strum => {
                // Each note on its own string, then across the strings.
                let open = self.open();
                let mut notes: Vec<Option<u8>> = vec![None; open.len()];
                let mut busy = vec![false; open.len()];
                let mut sorted = pending.clone();
                sorted.sort_by_key(|p| p.0);
                let vel = sorted.iter().map(|p| p.1).fold(0.0, f32::max);
                let mut leftovers = Vec::new();
                for &(note, _) in &sorted {
                    match string_for(&open, note, self.frets(), &busy) {
                        Some((s, _)) if !busy[s] => {
                            busy[s] = true;
                            notes[s] = Some(note);
                        }
                        _ => leftovers.push(note),
                    }
                }
                if sorted.len() == 1 {
                    self.play_solo(sorted[0].0, sorted[0].1, art);
                } else {
                    let key = sorted[0].0;
                    self.strum(&notes, vel, key, Kind::Normal, art);
                }
                for n in leftovers {
                    self.play_solo(n, vel, art);
                }
            }
            PlayMode::Chord | PlayMode::Pattern => {
                let vel = pending.iter().map(|p| p.1).fold(0.0, f32::max);
                let Some((root, k)) = detect_chord(&self.held).or_else(|| detect_chord(&pending.iter().map(|p| p.0).collect::<Vec<_>>())) else { return };
                let k = if self.held.len() <= 1 { self.p.single_key_chord.min(CHORDS.len() - 1) } else { k };
                let open = self.open();
                self.chord = voice_chord(&open, root, CHORDS[k].1, self.frets());
                self.chord_key = root;
                self.chord_vel = vel;
                if self.p.mode == PlayMode::Chord {
                    // Strings not in the new chord stop.
                    for s in 0..open.len() {
                        if self.chord.get(s).copied().flatten().is_none() {
                            self.choke_string(s, 0.03);
                        }
                    }
                    let chord = self.chord.clone();
                    self.strum(&chord, vel, root, Kind::Normal, art);
                } else {
                    self.pattern_step = 0;
                    self.next_step_at = self.clock;
                }
            }
        }
    }

    fn play_solo(&mut self, note: u8, vel: f32, art: Articulation) {
        let open = self.open();
        // Legato slide: the newest held string glides to the new note.
        if art == Articulation::Slide {
            let rate = self.rate;
            let frets = self.frets();
            let slide = self.p.slide_time.max(0.005);
            if let Some(s) = self.strs.iter_mut().filter(|s| s.on && !s.released && !s.second).max_by_key(|s| s.age) {
                let st = s.string;
                if note >= open[st] && note - open[st] <= frets {
                    let f0 = midi_to_hz(note as f32);
                    let period = rate / f0;
                    let w = std::f32::consts::TAU * f0 / rate;
                    let tau = (s.lp_s * w.sin()).atan2(1.0 - s.lp_s * w.cos()) / w;
                    s.d_target = (period - tau).max(2.5);
                    s.d_step = (s.d_target - s.d) / (slide * rate);
                    s.note = note;
                    s.key = note;
                    return;
                }
            }
        }
        let busy: Vec<bool> = (0..open.len()).map(|i| self.strs.iter().any(|s| s.on && !s.released && s.string == i)).collect();
        let Some((string, _)) = string_for(&open, note, self.frets(), &busy) else {
            // Out of the neck's range: the nearest string plays it anyway.
            let string = if note < open.iter().copied().min().unwrap_or(40) { 0 } else { open.len().saturating_sub(1) };
            let at = self.clock;
            self.queue(Pluck { at, string, note, vel, key: note, art, second: false });
            return;
        };
        let at = self.clock;
        self.queue(Pluck { at, string, note, vel, key: note, art, second: false });
    }

    /// Pattern mode: the next step when it is due.
    fn pattern_tick(&mut self) {
        if self.p.mode != PlayMode::Pattern || self.held.is_empty() || self.chord.is_empty() {
            return;
        }
        while self.next_step_at <= self.clock {
            let (_, steps, beats) = PATTERNS[self.p.pattern.min(PATTERNS.len() - 1)];
            let c = steps.as_bytes()[self.pattern_step % steps.len()] as char;
            let chord = self.chord.clone();
            let vel = self.chord_vel;
            let key = self.chord_key;
            let art = self.art;
            match c {
                'D' => self.strum_dir(&chord, vel, key, Kind::Normal, art, true),
                'd' => self.strum_dir(&chord, vel, key, Kind::Light, art, true),
                'U' => self.strum_dir(&chord, vel * 0.85, key, Kind::Normal, art, false),
                'u' => self.strum_dir(&chord, vel * 0.85, key, Kind::Light, art, false),
                'X' => self.strum_dir(&chord, vel, key, Kind::Chuck, art, true),
                'B' => self.strum_dir(&chord, vel, key, Kind::Bass, art, true),
                '.' => {
                    for s in 0..self.open().len() {
                        self.choke_string(s, 0.03);
                    }
                }
                _ => {}
            }
            self.pattern_step += 1;
            let step = (beats * 60.0 / self.bpm.max(20.0) * self.rate) as u64;
            self.next_step_at += step.max(1);
        }
    }

    /// The fret each string is playing (−1 = silent), for the editor's fretboard.
    pub fn display(&self) -> [i8; 8] {
        let open = self.open();
        let mut out = [-1i8; 8];
        let mut age = [0u64; 8];
        for s in self.strs.iter().filter(|s| s.on && !s.second && s.fade > 0.05 && s.string < 8) {
            if out[s.string] < 0 || s.age >= age[s.string] {
                let fret = s.note as i32 - open.get(s.string).copied().unwrap_or(0) as i32 + self.p.capo as i32;
                out[s.string] = fret.clamp(0, 30) as i8;
                age[s.string] = s.age;
            }
        }
        out
    }
}

impl InstrumentVoice for Guitar {
    fn note_on(&mut self, n: NoteOn) {
        // Key switches: articulations.
        if let Some(a) = Articulation::ALL.iter().find(|a| a.key() == n.note) {
            self.art = *a;
            return;
        }
        // F#0 / G0: strum the current chord down / up again.
        if n.note == 18 || n.note == 19 {
            let chord = self.chord.clone();
            let (vel, key, art) = (n.velocity, self.chord_key, self.art);
            self.strum_dir(&chord, vel, key, Kind::Normal, art, n.note == 18);
            return;
        }
        if n.note < 24 {
            return;
        }
        if !self.held.contains(&n.note) {
            self.held.push(n.note);
        }
        if self.pending.len() < 32 {
            self.pending.push((n.note, n.velocity));
        }
    }

    fn note_off(&mut self, note: u8) {
        if note < 24 {
            return;
        }
        self.held.retain(|k| *k != note);
        let release = self.p.release.max(0.01);
        let mul = (-6.9 / (release * self.rate)).exp();
        match self.p.mode {
            PlayMode::Solo | PlayMode::Strum => {
                for s in self.strs.iter_mut().filter(|s| s.on && s.key == note && !s.released) {
                    s.released = true;
                    s.fade_mul = mul;
                }
                self.events.retain(|e| e.key != note);
            }
            PlayMode::Chord | PlayMode::Pattern => {
                if self.held.is_empty() {
                    for s in self.strs.iter_mut().filter(|s| s.on && !s.released) {
                        s.released = true;
                        s.fade_mul = mul;
                    }
                    self.events.clear();
                    self.chord.clear();
                }
            }
        }
    }

    fn choke(&mut self, seconds: f32) {
        let mul = (-6.9 / (seconds.max(0.002) * self.rate)).exp();
        for s in self.strs.iter_mut().filter(|s| s.on) {
            s.released = true;
            s.fade_mul = s.fade_mul.min(mul);
        }
        self.events.clear();
        self.held.clear();
        self.pending.clear();
        self.chord.clear();
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len().min(r.len());
        if self.scratch.0.len() < n {
            self.scratch = (vec![0.0; n], vec![0.0; n]);
        }
        self.play_pending();
        let (mut sl, mut sr) = std::mem::take(&mut self.scratch);
        sl[..n].iter_mut().for_each(|x| *x = 0.0);
        sr[..n].iter_mut().for_each(|x| *x = 0.0);
        let block_start = self.clock;
        let mut k0 = 0;
        // Sub-blocks between events, so strums are sample-accurate enough (32-sample grid).
        while k0 < n {
            let k1 = (k0 + 32).min(n);
            self.clock = block_start + k0 as u64;
            self.pattern_tick();
            let mut i = 0;
            while i < self.events.len() {
                if self.events[i].at <= self.clock + (k1 - k0) as u64 {
                    let e = self.events.swap_remove(i);
                    self.pluck(e);
                } else {
                    i += 1;
                }
            }
            for s in self.strs.iter_mut().filter(|s| s.on) {
                let blen = s.buf.len();
                let mut energy = 0.0f32;
                for k in k0..k1 {
                    if s.d_step != 0.0 {
                        s.d += s.d_step;
                        if (s.d_step > 0.0 && s.d >= s.d_target) || (s.d_step < 0.0 && s.d <= s.d_target) {
                            s.d = s.d_target;
                            s.d_step = 0.0;
                        }
                    }
                    let x = s.read(s.d);
                    s.lp_y += (1.0 - s.lp_s) * (x - s.lp_y);
                    let mut y = s.lp_y * s.loss;
                    if s.buzz_thr > 0.0 && y.abs() > s.buzz_thr {
                        // The sitar's bridge: the string buzzes against it above a threshold.
                        y = y.signum() * (s.buzz_thr + (y.abs() - s.buzz_thr) * 0.6);
                    }
                    s.buf[s.w] = y;
                    s.w = (s.w + 1) % blen;
                    let mut o = y;
                    if s.pickup_tap > 0.0 {
                        o -= 0.85 * s.read(s.pickup_tap + 1.0);
                    }
                    // DC blocker.
                    let dc = o - s.dc_x + 0.995 * s.dc_y;
                    s.dc_x = o;
                    s.dc_y = dc;
                    let o = dc * s.fade;
                    s.fade *= s.fade_mul;
                    sl[k] += o * s.gl;
                    sr[k] += o * s.gr;
                    energy = energy.max(o.abs());
                }
                if energy < 1e-5 || s.fade < 1e-4 {
                    s.quiet += 1;
                    if s.quiet > 3 || s.fade < 1e-4 {
                        s.on = false;
                    }
                } else {
                    s.quiet = 0;
                }
            }
            k0 = k1;
        }
        self.clock = block_start + n as u64;
        // The body or the amp.
        let p = &self.p;
        let hp_a = onepole_coef(if p.model == GuitarModel::Bass { 30.0 } else { 70.0 }, self.rate);
        let electric = matches!(p.model, GuitarModel::Electric | GuitarModel::Bass);
        let body = if electric { 0.0 } else { p.body.clamp(0.0, 1.0) };
        let drive = if electric { p.drive.clamp(0.0, 1.0) } else { 0.0 };
        let pre = 1.0 + drive * drive * 40.0;
        let post = 1.0 / (1.0 + drive * 3.0);
        let tone_a = onepole_coef(if electric { 1200.0 + p.tone * 9000.0 } else { 2500.0 + p.tone * 16000.0 }, self.rate);
        let cab_a = onepole_coef(if p.model == GuitarModel::Bass { 4500.0 } else { 5200.0 - drive * 1400.0 }, self.rate);
        let bn = self.body_n;
        for (ch, buf) in [&mut sl, &mut sr].into_iter().enumerate() {
            for x in buf[..n].iter_mut() {
                let mut y = *x;
                if body > 0.0 {
                    let mut b = 0.0;
                    for m in self.body[ch][..bn].iter_mut() {
                        b += m.run(y);
                    }
                    y = y * (1.0 - 0.45 * body) + b * body * 1.6;
                }
                if electric {
                    if drive > 0.01 {
                        y = (y * pre).tanh() * post;
                    }
                    let c1 = self.cab1[ch].lp(y, cab_a);
                    y = if drive > 0.01 { self.cab2[ch].lp(c1, cab_a) } else { y };
                }
                y = self.tone_lp[ch].lp(y, tone_a);
                // Highpass: the input minus its lowpass.
                let low = self.hp[ch].lp(y, hp_a);
                *x = y - low;
            }
        }
        let busy = self.strs.iter().any(|s| s.on) || !self.events.is_empty();
        if busy {
            self.tail = (self.rate * 5.0) as usize;
        } else {
            self.tail = self.tail.saturating_sub(n);
        }
        for (_, fx) in self.fx.iter_mut() {
            fx.process(&mut sl[..n], &mut sr[..n], None);
        }
        let gain = self.p.master * 10f32.powf(self.p.trim_db.clamp(-36.0, 36.0) / 20.0);
        for k in 0..n {
            l[k] += sl[k] * gain;
            r[k] += sr[k] * gain;
        }
        self.scratch = (sl, sr);
    }

    fn update(&mut self, model: &Model, _cache: &SampleCache) {
        let Model::Guitar(p) = model else { return };
        let ids: Vec<&str> = p.fx.iter().filter(|f| f.on).map(|f| f.id.as_str()).collect();
        let have: Vec<&str> = self.fx.iter().map(|f| f.0.as_str()).collect();
        if ids != have {
            self.fx = p.fx.iter().filter(|f| f.on).filter_map(|f| effects::create(&f.id, self.rate, &f.params).map(|e| (f.id.clone(), e))).collect();
        } else {
            for ((_, e), f) in self.fx.iter_mut().zip(p.fx.iter().filter(|f| f.on)) {
                for (k, v) in &f.params {
                    e.set_param(k, *v);
                }
            }
        }
        let body_changed = p.model != self.p.model || p.body_size != self.p.body_size;
        if p.articulation != self.p.articulation {
            self.art = p.articulation;
        }
        self.p = (**p).clone();
        if self.p.tuning.is_empty() {
            self.p.tuning = self.p.model.default_tuning();
        }
        self.p.tuning.truncate(8);
        if body_changed {
            self.set_body();
        }
    }

    fn active_voices(&self) -> usize {
        let v = self.strs.iter().filter(|s| s.on).count() + self.events.len() + self.pending.len();
        let pattern = usize::from(self.p.mode == PlayMode::Pattern && !self.held.is_empty());
        let tail = usize::from(v == 0 && self.tail > 0 && !self.fx.is_empty());
        v + pattern + tail
    }

    fn set_param(&mut self, id: &str, value: f32) {
        match id {
            "__bpm" => self.bpm = value,
            "brightness" => self.p.brightness = value.clamp(0.0, 1.0),
            "body" => self.p.body = value.clamp(0.0, 1.0),
            "drive" => self.p.drive = value.clamp(0.0, 1.0),
            "tone" => self.p.tone = value.clamp(0.0, 1.0),
            "master" => self.p.master = value.clamp(0.0, 1.5),
            "strum" => self.p.strum_time = value.clamp(0.0, 120.0),
            _ => {}
        }
    }

    fn display(&self) -> Option<[i8; 8]> {
        Some(Guitar::display(self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(p: GuitarParams, notes: &[(u8, f32)], secs: f32) -> (Vec<f32>, Vec<f32>, Guitar) {
        let rate = 48000.0;
        let mut g = Guitar::new(rate);
        g.update(&Model::Guitar(Box::new(p)), &SampleCache::default());
        for &(n, v) in notes {
            g.note_on(NoteOn { note: n, velocity: v, pan: 0.0, detune: 0.0, slide: false });
        }
        let len = (secs * rate) as usize;
        let (mut l, mut r) = (vec![0.0; len], vec![0.0; len]);
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            g.process(a, b);
        }
        (l, r, g)
    }

    /// Pitch by autocorrelation (Hz).
    fn pitch(x: &[f32], rate: f32) -> f32 {
        let (lo, hi) = ((rate / 1500.0) as usize, (rate / 30.0) as usize);
        let mut best = (0usize, f32::MIN);
        for lag in lo..hi {
            let c: f32 = x.iter().zip(&x[lag..]).map(|(a, b)| a * b).sum();
            if c > best.1 {
                best = (lag, c);
            }
        }
        // Parabolic refinement.
        let c = |lag: usize| -> f32 { x.iter().zip(&x[lag..]).map(|(a, b)| a * b).sum() };
        let (a, b, cc) = (c(best.0 - 1), best.1, c(best.0 + 1));
        let off = 0.5 * (a - cc) / (a - 2.0 * b + cc);
        rate / (best.0 as f32 + off)
    }

    /// The loudest 50 ms (dBFS RMS), the peak and the biggest jump of a preset's audition.
    fn loudness(p: GuitarParams, secs: f32) -> (f32, f32, f32) {
        let notes = crate::gtr_library::audition(&p);
        let rate = 48000.0;
        let mut g = Guitar::new(rate);
        g.update(&Model::Guitar(Box::new(p)), &SampleCache::default());
        g.set_param("__bpm", 120.0);
        for n in notes {
            g.note_on(NoteOn { note: n, velocity: 0.8, pan: 0.0, detune: 0.0, slide: false });
        }
        let len = (secs * rate) as usize;
        let (mut l, mut r) = (vec![0.0; len], vec![0.0; len]);
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            g.process(a, b);
        }
        let win = 2400;
        let mut best = 0.0f32;
        for k in (0..len.saturating_sub(win)).step_by(win / 2) {
            let e: f32 = l[k..k + win].iter().chain(&r[k..k + win]).map(|v| v * v).sum::<f32>() / (2 * win) as f32;
            best = best.max(e);
        }
        let peak = l.iter().chain(&r).fold(0.0f32, |m, v| m.max(v.abs()));
        let jump = l.windows(2).chain(r.windows(2)).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        (10.0 * best.max(1e-12).log10(), peak, jump)
    }

    const TARGET_DB: f32 = -17.0;

    #[test]
    fn library_presets_play_cleanly_at_an_even_level() {
        for p in crate::gtr_library::library().iter().step_by(37) {
            let (db, peak, _) = loudness(p.params.clone(), 2.0);
            assert!(peak.is_finite() && peak < 3.0, "{}: peak {peak}", p.name);
            assert!((db - TARGET_DB).abs() < 4.0, "{} ({}): {db:.1} dB", p.name, p.kind);
        }
    }

    /// Measures every preset and writes the level corrections (run in release).
    #[test]
    #[ignore]
    fn measure_guitar_levels() {
        let lib = crate::gtr_library::library();
        let old = crate::gtr_library::LEVELS;
        let threads = 4;
        let chunk = lib.len().div_ceil(threads);
        let results: Vec<(usize, f32, f32)> = std::thread::scope(|sc| {
            let hs: Vec<_> = lib
                .chunks(chunk)
                .enumerate()
                .map(|(ci, c)| {
                    sc.spawn(move || {
                        c.iter()
                            .enumerate()
                            .map(|(j, p)| {
                                let (db, pk, _) = loudness(p.params.clone(), 2.0);
                                (ci * chunk + j, db, pk)
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            hs.into_iter().flat_map(|h| h.join().unwrap()).collect()
        });
        let mut levels = vec![0.0f32; lib.len()];
        for (i, db, pk) in results {
            let prev = old.get(i).copied().unwrap_or(0.0);
            let corr = (prev + TARGET_DB - db).clamp(-36.0, 36.0);
            levels[i] = (corr * 10.0).round() / 10.0;
            if pk > 2.0 || db < -55.0 || (prev + TARGET_DB - db).abs() > 36.0 {
                println!("CHECK {} ({}): {db:.1} dB peak {pk:.2}", lib[i].name, lib[i].kind);
            }
        }
        let text = format!("[{}]\n", levels.iter().map(|v| format!("{v:.1}")).collect::<Vec<_>>().join(", "));
        std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/src/gtr_levels.in"), text).unwrap();
    }

    /// Writes a demo of a few presets to $DRAGON_DEMO (16-bit WAV).
    #[test]
    #[ignore]
    fn render_demo() {
        let Ok(path) = std::env::var("DRAGON_DEMO") else { return };
        let rate = 48000.0;
        let lib = crate::gtr_library::library();
        let pick = |cat: crate::gtr_library::Category, kind: &str| lib.iter().find(|p| p.category == cat && p.kind == kind).map(|p| p.params.clone()).unwrap();
        use crate::gtr_library::Category as C;
        // (preset, bpm, events: (beat, note, vel, on?)) – C Am F G progressions.
        let prog: [[u8; 3]; 4] = [[48, 52, 55], [45, 48, 52], [41, 45, 48], [43, 47, 50]];
        let mut parts: Vec<(GuitarParams, Vec<(f32, u8, f32, bool)>)> = Vec::new();
        // 1. Fingerstyle arpeggios.
        let mut ev = Vec::new();
        for (bar, ch) in prog.iter().enumerate() {
            let notes = [ch[0] - 12, ch[1], ch[2], ch[0] + 12, ch[2], ch[1], ch[0] + 12, ch[2]];
            for (k, n) in notes.iter().enumerate() {
                let b = bar as f32 * 4.0 + k as f32 * 0.5;
                ev.push((b, *n, 0.7 + 0.1 * (k % 2) as f32, true));
                ev.push((b + 1.4, *n, 0.0, false));
            }
        }
        parts.push((pick(C::Acoustic, "Fingerstyle"), ev));
        // 2. Pattern strumming: hold each chord a bar.
        let mut ev = Vec::new();
        for (bar, ch) in prog.iter().enumerate() {
            for n in ch {
                ev.push((bar as f32 * 4.0, *n, 0.85, true));
                ev.push((bar as f32 * 4.0 + 3.95, *n, 0.0, false));
            }
        }
        parts.push((GuitarParams { pattern: 0, ..pick(C::Rhythm, "Folk 8ths") }, ev.clone()));
        // 3. Nylon bossa.
        parts.push((pick(C::Nylon, "Bossa & Patterns"), ev.clone()));
        // 4. Electric drive power chords, palm-muted eighths then ringing.
        let mut ev = Vec::new();
        for (bar, root) in [40u8, 43, 45, 43].iter().enumerate() {
            for k in 0..8 {
                let b = bar as f32 * 4.0 + k as f32 * 0.5;
                let ks = if k < 6 { 13 } else { 12 };
                ev.push((b - 0.01, ks, 0.8, true));
                ev.push((b, *root, 0.9, true));
                ev.push((b, root + 7, 0.9, true));
                ev.push((b + 0.45, *root, 0.0, false));
                ev.push((b + 0.45, root + 7, 0.0, false));
            }
        }
        let mut drive = pick(C::Drive, "Power Chords");
        drive.mode = PlayMode::Strum;
        parts.push((drive, ev));
        // 5. Finger bass walking line.
        let mut ev = Vec::new();
        let line = [36u8, 40, 43, 45, 33, 36, 40, 43, 29, 33, 36, 38, 31, 35, 38, 41];
        for (k, n) in line.iter().enumerate() {
            ev.push((k as f32, *n, 0.8, true));
            ev.push((k as f32 + 0.9, *n, 0.0, false));
        }
        parts.push((pick(C::Bass, "Finger"), ev));
        let bpm = 100.0;
        let beat = 60.0 / bpm * rate;
        let mut out: Vec<f32> = Vec::new();
        for (p, mut ev) in parts {
            ev.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            let mut g = Guitar::new(rate);
            g.update(&Model::Guitar(Box::new(p.clone())), &SampleCache::default());
            g.set_param("__bpm", bpm);
            let total = (17.5 * beat) as usize;
            let (mut l, mut r) = (vec![0.0f32; total], vec![0.0f32; total]);
            let mut i = 0;
            let mut pos = 0;
            while pos < total {
                while i < ev.len() && (ev[i].0.max(0.0) * beat) as usize <= pos {
                    let (_, n, v, on) = ev[i];
                    if on {
                        g.note_on(NoteOn { note: n, velocity: v, pan: 0.0, detune: 0.0, slide: false });
                    } else {
                        g.note_off(n);
                    }
                    i += 1;
                }
                let end = (pos + 128).min(total);
                g.process(&mut l[pos..end], &mut r[pos..end]);
                pos = end;
            }
            eprintln!("{}: peak {:.2}", p.preset, l.iter().fold(0.0f32, |m, x| m.max(x.abs())));
            for k in 0..total {
                out.push(l[k]);
                out.push(r[k]);
            }
        }
        let mut bytes = Vec::with_capacity(44 + out.len() * 2);
        let data_len = (out.len() * 2) as u32;
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&48000u32.to_le_bytes());
        bytes.extend_from_slice(&(48000u32 * 4).to_le_bytes());
        bytes.extend_from_slice(&4u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for x in out {
            bytes.extend_from_slice(&((x.clamp(-1.0, 1.0) * 32000.0) as i16).to_le_bytes());
        }
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn strings_are_in_tune_and_die_away() {
        for model in [GuitarModel::SteelAcoustic, GuitarModel::Nylon, GuitarModel::Electric, GuitarModel::Bass] {
            let p = GuitarParams { body: 0.0, humanize: 0.0, ..GuitarParams::for_model(model) };
            for note in [40u8, 52, 64, 76] {
                let note = if model == GuitarModel::Bass { note - 12 } else { note };
                let (l, _, _) = render(p.clone(), &[(note, 0.8)], 0.6);
                let f = pitch(&l[9600..9600 + 8192], 48000.0);
                let want = midi_to_hz(note as f32);
                let cents = 1200.0 * (f / want).log2();
                assert!(cents.abs() < 8.0, "{model:?} note {note}: {f:.2} Hz, want {want:.2} ({cents:.1} cents)");
            }
        }
        // A plucked note dies away; the instrument then goes to sleep.
        let (l, _, g) = render(GuitarParams { sustain: 0.5, ..Default::default() }, &[(52, 0.8)], 4.0);
        let early: f32 = l[2400..7200].iter().map(|x| x.abs()).fold(0.0, f32::max);
        let late: f32 = l[l.len() - 4800..].iter().map(|x| x.abs()).fold(0.0, f32::max);
        assert!(early > 0.02 && late < early * 0.01, "early {early} late {late}");
        assert_eq!(g.strs.iter().filter(|s| s.on).count(), 0);
    }

    #[test]
    fn a_new_note_on_the_same_string_cuts_the_old_one() {
        let rate = 48000.0;
        let mut g = Guitar::new(rate);
        g.update(&Model::Guitar(Box::default()), &SampleCache::default());
        let (mut l, mut r) = (vec![0.0; 2048], vec![0.0; 2048]);
        g.note_on(NoteOn { note: 41, velocity: 0.8, pan: 0.0, detune: 0.0, slide: false });
        g.process(&mut l, &mut r);
        g.note_on(NoteOn { note: 43, velocity: 0.8, pan: 0.0, detune: 0.0, slide: false });
        g.process(&mut l, &mut r);
        // Both on the low E string: the first fades, the display shows fret 3.
        assert_eq!(g.display()[0], 3);
        for _ in 0..10 {
            g.process(&mut l, &mut r);
        }
        assert_eq!(g.strs.iter().filter(|s| s.on && s.string == 0).count(), 1);
    }

    #[test]
    fn chords_are_strummed_across_the_strings() {
        let p = GuitarParams { mode: PlayMode::Strum, strum_time: 20.0, strum_dir: StrumDir::Down, humanize: 0.0, ..Default::default() };
        let rate = 48000.0;
        let mut g = Guitar::new(rate);
        g.update(&Model::Guitar(Box::new(p)), &SampleCache::default());
        for n in [48u8, 52, 55, 60, 64] {
            g.note_on(NoteOn { note: n, velocity: 0.8, pan: 0.0, detune: 0.0, slide: false });
        }
        let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
        g.process(&mut l, &mut r);
        // The first string sounds at once, the rest follow 20 ms apart.
        assert_eq!(g.strs.iter().filter(|s| s.on).count(), 1);
        let (mut l, mut r) = (vec![0.0; 4800], vec![0.0; 4800]);
        g.process(&mut l, &mut r);
        assert_eq!(g.strs.iter().filter(|s| s.on).count(), 5);
        let d = g.display();
        assert_eq!(&d[..6], &[-1, 3, 2, 0, 1, 0]);
    }

    #[test]
    fn chord_mode_voices_a_full_chord_from_one_key_and_patterns_keep_time() {
        let p = GuitarParams { mode: PlayMode::Chord, humanize: 0.0, ..Default::default() };
        let (_, _, g) = render(p, &[(52, 0.8)], 0.3);
        // E major from one key: all six strings, open E shape.
        assert_eq!(&g.display()[..6], &[0, 2, 2, 1, 0, 0]);
        // Pattern: "Rock 8ths" at 120 bpm strums every 250 ms while the chord is held.
        let p = GuitarParams { mode: PlayMode::Pattern, pattern: 6, strum_time: 0.0, humanize: 0.0, doubling: 0.0, ..Default::default() };
        let rate = 48000.0;
        let mut g = Guitar::new(rate);
        g.update(&Model::Guitar(Box::new(p)), &SampleCache::default());
        g.set_param("__bpm", 120.0);
        for n in [48u8, 52, 55] {
            g.note_on(NoteOn { note: n, velocity: 0.8, pan: 0.0, detune: 0.0, slide: false });
        }
        let mut onsets: Vec<usize> = Vec::new();
        let (mut l, mut r) = (vec![0.0; 240], vec![0.0; 240]);
        for b in 0..400 {
            let before = g.strs.iter().map(|s| s.age).max().unwrap_or(0);
            g.process(&mut l, &mut r);
            let after = g.strs.iter().map(|s| s.age).max().unwrap_or(0);
            if after != before {
                onsets.push(b);
            }
        }
        // 400 × 240 samples = 2 s: 8 strums at 120 bpm eighths (0, 0.25 s, …).
        assert!((7..=9).contains(&onsets.len()), "{onsets:?}");
        assert!(onsets.windows(2).all(|w| (w[1] - w[0]).abs_diff(50) <= 1), "{onsets:?}");
    }

    #[test]
    fn every_model_plays_clean() {
        for model in GuitarModel::ALL {
            let p = GuitarParams::for_model(model);
            let lo = p.tuning.iter().copied().min().unwrap();
            let (l, r, _) = render(p, &[(lo + 7, 1.0), (lo + 12, 0.7)], 1.5);
            let peak = l.iter().chain(&r).fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(peak.is_finite() && peak > 0.01 && peak < 3.0, "{model:?}: peak {peak}");
        }
    }

    #[test]
    fn key_switches_change_the_articulation() {
        let (l1, _, _) = render(GuitarParams::default(), &[(45, 0.8)], 1.0);
        let (l2, _, _) = render(GuitarParams::default(), &[(Articulation::PalmMute.key(), 0.8), (45, 0.8)], 1.0);
        let tail = |l: &[f32]| l[24000..].iter().map(|x| x * x).sum::<f32>();
        assert!(tail(&l2) < tail(&l1) * 0.01, "palm mute rings much shorter");
    }
}
