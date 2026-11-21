//! The modelled piano.
//!
//! Strings: every key has one (bass), two or three (treble) strings, each a waveguide loop: a delay
//! line (Lagrange-interpolated, so in tune), a loss filter, and a chain of allpass filters for the
//! string's stiffness – the upper partials run sharp, which is what makes a piano sound like a piano.
//! The strings of a key are detuned a hair (beating, and the slow second decay of a real piano),
//! the bass is tuned flat and the treble sharp (stretch tuning).
//!
//! The hammer: a felt pulse, shorter and brighter the harder the key is struck, fed into the strings
//! at the striking point (an eighth of the string), plus its knock and felt noise. Dampers fall on
//! release unless the sustain pedal holds them; the top keys have none. With the pedal down, a bank
//! of open strings rings in sympathy. Everything then sounds through the soundboard (a set of
//! resonances) and the FX.
//!
//! Electric pianos, toy piano and celesta are struck tines, reeds and bars: a few decaying partials
//! each (modal synthesis), the electrics through a pickup that barks when hit hard, and tremolo.

use super::{midi_to_hz, InstrumentVoice, NoteOn};
use crate::dsp::Rng;
use crate::effects::{self, Effect};
use crate::model::Instrument as Model;
use crate::pno::*;
use crate::samples::SampleCache;

const MAX_VOICES: usize = 48;
const MAX_AP: usize = 8;
const EXC_LEN: usize = 2048;
/// Keys from here up have no dampers (like a real piano).
const NO_DAMPER: u8 = 89;

#[derive(Clone)]
struct PString {
    buf: Vec<f32>,
    w: usize,
    d: f32,
    lp_s: f32,
    lp_y: f32,
    ap_a: f32,
    ap_n: usize,
    ap: [(f32, f32); MAX_AP],
    loss: f32,
    loss_target: f32,
    loss_free: f32,
    loss_damped: f32,
    lp_free: f32,
    lp_damped: f32,
}

impl PString {
    fn new(len: usize) -> Self {
        PString {
            buf: vec![0.0; len],
            w: 0,
            d: 10.0,
            lp_s: 0.2,
            lp_y: 0.0,
            ap_a: 0.0,
            ap_n: 0,
            ap: [(0.0, 0.0); MAX_AP],
            loss: 0.999,
            loss_target: 0.999,
            loss_free: 0.999,
            loss_damped: 0.9,
            lp_free: 0.2,
            lp_damped: 0.5,
        }
    }

    #[inline]
    fn read(&self) -> f32 {
        let len = self.buf.len();
        let di = self.d.floor();
        let f = self.d - di;
        let i = di as usize;
        let at = |k: usize| self.buf[(self.w + len * 2 - k) % len];
        let (ym1, y0, y1, y2) = (at(i.max(2) - 1), at(i), at(i + 1), at(i + 2));
        let c0 = -f * (f - 1.0) * (f - 2.0) / 6.0;
        let c1 = (f + 1.0) * (f - 1.0) * (f - 2.0) / 2.0;
        let c2 = -(f + 1.0) * f * (f - 2.0) / 2.0;
        let c3 = (f + 1.0) * f * (f - 1.0) / 6.0;
        c0 * ym1 + c1 * y0 + c2 * y1 + c3 * y2
    }

    #[inline]
    fn tick(&mut self, input: f32) -> f32 {
        let x = self.read();
        self.lp_y += (1.0 - self.lp_s) * (x - self.lp_y);
        let mut y = self.lp_y;
        for k in 0..self.ap_n {
            // First-order allpass: y = a·x + x1 − a·y1.
            let (x1, y1) = self.ap[k];
            let out = self.ap_a * y + x1 - self.ap_a * y1;
            self.ap[k] = (y, out);
            y = out;
        }
        y *= self.loss;
        let len = self.buf.len();
        self.buf[self.w] = y + input;
        self.w = (self.w + 1) % len;
        y
    }
}

/// Phase delay (samples) of a first-order allpass with coefficient `a` at `w` radians/sample.
fn allpass_delay(a: f32, w: f32) -> f32 {
    1.0 - 2.0 * (a * w.sin()).atan2(1.0 + a * w.cos()) / w
}

/// Phase delay of the one-pole loss filter `y += (1-s)(x-y)`.
fn lowpass_delay(s: f32, w: f32) -> f32 {
    (s * w.sin()).atan2(1.0 - s * w.cos()) / w
}

/// Inharmonicity coefficient B of a piano string (partial k sounds at k·f0·√(1 + B·k²)): low in the
/// wound bass, rising steeply into the short treble strings.
fn inharmonicity(note: f32) -> f32 {
    2.5e-4 * 40f32.powf(((note - 48.0) / 60.0).max(0.0)) * if note < 48.0 { 1.0 - (48.0 - note) / 60.0 } else { 1.0 }
}

/// Designs the stiffness filter: `m` first-order allpasses and the read delay so that the
/// fundamental is in tune and partial `k` lands at its stiff-string frequency. Returns (a, delay).
fn design_dispersion(f0: f32, b: f32, lp_s: f32, rate: f32, m: usize) -> (f32, f32) {
    let k = (5000.0 / f0).floor().clamp(2.0, 16.0);
    let fk = k * f0 * (1.0 + b * k * k).sqrt();
    let (w1, wk) = (std::f32::consts::TAU * f0 / rate, std::f32::consts::TAU * fk / rate);
    if wk >= std::f32::consts::PI * 0.9 || m == 0 {
        return (0.0, rate / f0 - lowpass_delay(lp_s, w1));
    }
    // The loop delay must be one period at w1 and k periods at wk.
    let need = (rate / f0 - k * rate / fk) - (lowpass_delay(lp_s, w1) - lowpass_delay(lp_s, wk));
    let gap = |a: f32| m as f32 * (allpass_delay(a, w1) - allpass_delay(a, wk)) - need;
    let (mut lo, mut hi) = (-0.97f32, 0.0f32);
    if gap(lo) < 0.0 {
        hi = lo;
    } else {
        for _ in 0..40 {
            let mid = 0.5 * (lo + hi);
            if gap(mid) > 0.0 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
    }
    let a = 0.5 * (lo + hi);
    (a, rate / f0 - lowpass_delay(lp_s, w1) - m as f32 * allpass_delay(a, w1))
}

/// Partials of the struck tines, reeds and bars: (ratio, level, ring time factor).
fn modal_partials(m: PianoModel) -> &'static [(f32, f32, f32)] {
    match m {
        // Tine: a strong fundamental, a little second harmonic, and the tine's bell partial.
        PianoModel::Tine => &[(1.0, 1.0, 1.0), (2.0, 0.12, 0.5), (3.0, 0.04, 0.35), (7.1, 0.10, 0.06), (14.2, 0.03, 0.02)],
        // Reed: a reed is close to harmonic with strong odd partials.
        PianoModel::Reed => &[(1.0, 1.0, 1.0), (2.0, 0.35, 0.6), (3.0, 0.3, 0.45), (4.0, 0.08, 0.3), (5.0, 0.12, 0.25), (6.03, 0.04, 0.15)],
        // Toy piano: metal rods, inharmonic.
        PianoModel::Toy => &[(1.0, 1.0, 1.0), (2.76, 0.5, 0.4), (5.4, 0.3, 0.2), (8.93, 0.18, 0.1), (13.3, 0.08, 0.05)],
        // Celesta: bars over tuned resonators.
        _ => &[(1.0, 1.0, 1.0), (2.0, 0.12, 0.6), (3.99, 0.35, 0.3), (9.9, 0.1, 0.08)],
    }
}

#[derive(Clone, Copy, Default)]
struct Partial {
    re: f32,
    im: f32,
    c: f32,
    s: f32,
    decay: f32,
    level: f32,
}

#[derive(Clone)]
struct Voice {
    on: bool,
    note: u8,
    key_down: bool,
    damped: bool,
    sostenuto: bool,
    strings: Vec<PString>,
    n_str: usize,
    exc: Vec<f32>,
    exc_pos: usize,
    exc_len: usize,
    partials: [Partial; 6],
    n_partials: usize,
    modal_damp: f32,
    gl: f32,
    gr: f32,
    gain: f32,
    swell: f32,
    swell_step: f32,
    fade: f32,
    fade_mul: f32,
    age: u64,
    quiet: u32,
    /// Bark of the electric pianos' pickups (per note: harder hits bark more).
    bark: f32,
    /// Not silent before this (the strike still travelling round a long bass string).
    wake: u64,
    /// Silent sub-blocks needed before the voice stops: more than one trip round its longest string
    /// (a fresh bass string is a short pulse travelling round a long loop, with silence between).
    quiet_needed: u32,
}

impl Voice {
    fn new(len: usize) -> Self {
        Voice {
            on: false,
            note: 0,
            key_down: false,
            damped: false,
            sostenuto: false,
            strings: (0..3).map(|_| PString::new(len)).collect(),
            n_str: 0,
            exc: vec![0.0; EXC_LEN],
            exc_pos: 0,
            exc_len: 0,
            partials: [Partial::default(); 6],
            n_partials: 0,
            modal_damp: 1.0,
            gl: 0.7,
            gr: 0.7,
            gain: 1.0,
            swell: 1.0,
            swell_step: 0.0,
            fade: 1.0,
            fade_mul: 1.0,
            age: 0,
            quiet: 0,
            bark: 0.0,
            wake: 0,
            quiet_needed: 8,
        }
    }
}

/// A two-pole resonator (constant peak gain) for the soundboard.
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

fn board_modes(m: PianoModel) -> &'static [(f32, f32, f32)] {
    match m {
        PianoModel::Upright | PianoModel::HonkyTonk => &[(110.0, 6.0, 0.8), (240.0, 5.0, 0.7), (520.0, 5.0, 0.5), (1100.0, 4.0, 0.35), (2300.0, 3.0, 0.25)],
        PianoModel::Felt => &[(90.0, 5.0, 0.9), (200.0, 4.0, 0.7), (450.0, 4.0, 0.45), (900.0, 3.0, 0.25)],
        PianoModel::Tine | PianoModel::Reed => &[(120.0, 2.0, 0.3), (800.0, 2.0, 0.2)],
        PianoModel::Toy | PianoModel::Celesta => &[(400.0, 5.0, 0.6), (1300.0, 5.0, 0.5), (3200.0, 4.0, 0.3)],
        _ => &[(70.0, 6.0, 0.7), (140.0, 6.0, 0.8), (290.0, 5.0, 0.6), (600.0, 5.0, 0.45), (1200.0, 4.0, 0.35), (2500.0, 3.0, 0.22), (5000.0, 2.5, 0.12)],
    }
}

/// An open string for sympathetic resonance.
struct Sympathetic {
    buf: Vec<f32>,
    w: usize,
    d: usize,
    lp: f32,
}

pub struct Piano {
    rate: f32,
    p: PianoParams,
    voices: Vec<Voice>,
    rng: Rng,
    clock: u64,
    sustain: bool,
    soft: f32,
    sostenuto: bool,
    bpm: f32,
    // Arpeggiator.
    held: Vec<(u8, f32)>,
    arp_step: usize,
    arp_next: u64,
    arp_off: Vec<(u64, u8)>,
    // Board, resonance, output.
    board: [[Reso; 8]; 2],
    board_n: usize,
    symp: Vec<Sympathetic>,
    hd: [f32; 2],
    hp: [f32; 2],
    trem_phase: f32,
    fx: Vec<(String, Box<dyn Effect>)>,
    scratch: (Vec<f32>, Vec<f32>),
    tail: usize,
    /// Noises not tied to a voice (pedal thumps): remaining samples, level.
    thump: (usize, f32),
}

impl Piano {
    pub fn new(rate: f32) -> Self {
        let len = ((rate / 25.0) as usize + 16).next_power_of_two();
        // Open strings C2 … B3 for the pedal's sympathetic resonance.
        let symp = (36..60u8)
            .map(|n| {
                let d = (rate / midi_to_hz(n as f32)).round() as usize;
                Sympathetic { buf: vec![0.0; d + 2], w: 0, d, lp: 0.0 }
            })
            .collect();
        let mut pn = Piano {
            rate,
            p: PianoParams::default(),
            voices: (0..MAX_VOICES).map(|_| Voice::new(len)).collect(),
            rng: Rng::new(0x51A9_0C0F),
            clock: 0,
            sustain: false,
            soft: 0.0,
            sostenuto: false,
            bpm: 120.0,
            held: Vec::with_capacity(32),
            arp_step: 0,
            arp_next: 0,
            arp_off: Vec::with_capacity(64),
            board: [[Reso::default(); 8]; 2],
            board_n: 0,
            symp,
            hd: [0.0; 2],
            hp: [0.0; 2],
            trem_phase: 0.0,
            fx: Vec::new(),
            scratch: (vec![0.0; 4096], vec![0.0; 4096]),
            tail: 0,
            thump: (0, 0.0),
        };
        pn.set_board();
        pn
    }

    fn set_board(&mut self) {
        let modes = board_modes(self.p.model);
        self.board_n = modes.len().min(8);
        for (k, (hz, q, g)) in modes.iter().take(8).enumerate() {
            for ch in 0..2 {
                let spread = if ch == 0 { 0.98 } else { 1.02 };
                self.board[ch][k].set(self.rate, hz * spread, *q, *g);
            }
        }
    }

    fn pedal_down(&self) -> bool {
        self.sustain || self.p.pedal_latch
    }

    /// Frequency of `note`, stretch-tuned.
    fn freq(&self, note: f32) -> f32 {
        let d = note - 69.0;
        let cents = self.p.stretch * 30.0 * (d.abs() / 48.0).powi(2) * d.signum();
        midi_to_hz(note + cents / 100.0)
    }

    /// Strikes `note` (a new voice, or the ringing string again).
    fn strike(&mut self, note: u8, vel: f32) {
        let (lo, hi) = self.p.model.range();
        let note = note.clamp(lo, hi);
        let vel = (self.p.velocity_low + vel.clamp(0.0, 1.0) * (self.p.velocity_high - self.p.velocity_low)).clamp(0.01, 1.0);
        // The same key restrikes its own strings, as on a real piano.
        let idx = match self.voices.iter().position(|v| v.on && v.note == note) {
            Some(i) => i,
            None => match self.voices.iter().position(|v| !v.on) {
                Some(i) => i,
                None => {
                    let mut best = 0;
                    for (i, v) in self.voices.iter().enumerate() {
                        let score = |v: &Voice| (v.key_down as u8, v.age);
                        if score(v) < score(&self.voices[best]) {
                            best = i;
                        }
                    }
                    best
                }
            },
        };
        let restrike = self.voices[idx].on && self.voices[idx].note == note;
        let p = &self.p;
        let pos = (note as f32 - 21.0) / 87.0;
        let f0 = self.freq(note as f32);
        let soft = self.soft;
        let tone = (p.tone - 0.3 * soft).clamp(0.0, 1.0);
        let level = ((1.0 - p.dynamics) + p.dynamics * vel.powf(1.6)) * (1.0 - 0.2 * soft);
        let pan = ((note as f32 - 64.0) / 44.0).clamp(-1.0, 1.0) * p.width * 0.8;
        let (gl, gr) = ((0.5 * (1.0 - pan)).sqrt(), (0.5 * (1.0 + pan)).sqrt());
        let rate = self.rate;
        let model = p.model;
        let r1 = self.rng.bipolar();
        let clock = self.clock;
        if !model.strings() {
            // Struck tines, reeds and bars.
            let parts = modal_partials(model);
            let ring = (p.sustain * 3.5 * (261.0 / f0).powf(0.6)).clamp(0.15, 12.0);
            let bright = tone * 0.6 + vel * 0.6;
            let v = &mut self.voices[idx];
            v.n_partials = parts.len().min(6);
            for (k, (ratio, lvl, rt)) in parts.iter().take(6).enumerate() {
                let f = f0 * ratio;
                if f > rate * 0.45 {
                    v.partials[k].level = 0.0;
                    continue;
                }
                let w = std::f32::consts::TAU * f / rate;
                let t = (ring * rt).max(0.01);
                let upper = if k == 0 { 1.0 } else { bright.powf(1.2) * 1.6 };
                v.partials[k] = Partial { re: lvl * upper, im: 0.0, c: w.cos(), s: w.sin(), decay: (-6.9 / (t * rate)).exp(), level: 1.0 };
            }
            v.bark = p.drive * (0.3 + vel * 1.4);
            v.modal_damp = 1.0;
            v.n_str = 0;
            v.exc_len = 0;
        } else {
            // Strings: one in the low bass, two in the tenor, three above.
            let n_str = if note < 33 {
                1
            } else if note < 48 {
                2
            } else {
                3
            };
            let detune_spread = (1.0 - p.tightness).powf(1.5) * 14.0 + 0.4;
            let ring = (p.sustain * 18.0 * (130.0 / f0).powf(0.55)).clamp(0.25, 45.0);
            let damp_t = (p.release * (1.0 + (1.0 - pos) * 1.5)).clamp(0.02, 3.0);
            let bright = (0.15 + tone * 0.6 + vel * 0.3).clamp(0.0, 1.0);
            let lp_free = (0.02 + (1.0 - bright) * 0.35 + pos * 0.1).clamp(0.0, 0.8);
            let lp_damped = (lp_free + 0.4).min(0.92);
            let b_coef = inharmonicity(note as f32) * (0.15 + 1.7 * p.stiffness);
            let ap_n = if note < 45 {
                8
            } else if note < 64 {
                6
            } else if note < 84 {
                4
            } else {
                2
            };
            // The felt hammer: a pulse a few milliseconds long, shorter and harder when struck hard.
            let width_ms = (0.6 + 3.8 * (1.0 - pos)) * (1.35 - 0.75 * vel) * (1.3 - 0.6 * tone);
            let pw = ((width_ms / 1000.0 * rate) as usize).clamp(4, 600);
            let noise_len = ((0.004 + 0.01 * (1.0 - pos)) * rate) as usize;
            let felt = p.hammer * 0.25 * (0.4 + vel);
            let v = &mut self.voices[idx];
            v.n_str = n_str;
            v.n_partials = 0;
            for s in 0..n_str {
                // Each string a few cents apart, the first ringing longest (two-stage decay).
                let cents = match s {
                    0 => 0.0,
                    1 => detune_spread * (0.6 + 0.2 * r1),
                    _ => -detune_spread * (0.5 - 0.2 * r1),
                };
                let fs = f0 * 2f32.powf(cents / 1200.0);
                let w = std::f32::consts::TAU * fs / rate;
                let period = rate / fs;
                // Fewer allpasses where the period is too short to hold their delay.
                let mut m = ap_n;
                let (mut ap_a, mut d) = design_dispersion(fs, b_coef, lp_free, rate, m);
                while d < 3.0 && m > 0 {
                    m /= 2;
                    (ap_a, d) = design_dispersion(fs, b_coef, lp_free, rate, m);
                }
                let st = &mut v.strings[s];
                st.ap_n = m;
                st.ap_a = ap_a;
                st.lp_free = lp_free;
                st.lp_damped = lp_damped;
                if !restrike {
                    st.lp_s = lp_free;
                    st.lp_y = 0.0;
                    st.ap = [(0.0, 0.0); MAX_AP];
                    st.buf.iter_mut().for_each(|x| *x = 0.0);
                }
                let _ = period;
                st.d = d.clamp(2.5, (st.buf.len() - 4) as f32);
                let h = (1.0 - lp_free) / (1.0 - 2.0 * lp_free * w.cos() + lp_free * lp_free).sqrt();
                let t = ring * if s == 0 { 1.0 } else { 0.55 };
                st.loss_free = (10f32.powf(-3.0 / (fs * t)) / h).min(0.99995);
                st.loss_damped = 10f32.powf(-3.0 / (fs * damp_t));
                st.loss = st.loss_free;
                st.loss_target = st.loss_free;
            }
            // The excitation: hammer pulse + felt noise, combed at the striking point (1/8 of the string).
            let period = rate / f0;
            let strike = ((period * (0.12 + 0.02 * r1)).round() as usize).max(1);
            let len = (pw + noise_len + strike + 2).min(EXC_LEN);
            let mut lpn = 0.0f32;
            let noise_a = 0.1 + 0.5 * tone;
            for k in 0..len {
                let mut e = 0.0;
                if k < pw {
                    let t = k as f32 / pw as f32;
                    // A slightly skewed half-sine: the felt compresses faster than it lets go.
                    e = (std::f32::consts::PI * t.powf(0.8)).sin().powf(1.5);
                }
                if k < noise_len {
                    let n = self.rng.bipolar();
                    lpn += noise_a * (n - lpn);
                    e += lpn * felt * (1.0 - k as f32 / noise_len as f32);
                }
                v.exc[k] = e;
            }
            for k in (0..len).rev() {
                let back = if k >= strike { v.exc[k - strike] } else { 0.0 };
                v.exc[k] -= back;
            }
            // Pulse energy rises with its length: keep the level per key even.
            let norm = 1.0 / (pw as f32).sqrt() * 6.0;
            for x in v.exc[..len].iter_mut() {
                *x *= norm * level / n_str as f32;
            }
            v.exc_len = len;
            v.exc_pos = 0;
        }
        let swell = self.p.swell;
        let v = &mut self.voices[idx];
        v.on = true;
        v.note = note;
        v.key_down = true;
        v.damped = false;
        v.sostenuto = false;
        v.gl = gl;
        v.gr = gr;
        v.gain = level;
        v.fade = 1.0;
        v.fade_mul = 1.0;
        v.age = clock;
        v.quiet = 0;
        let longest = v.strings[..v.n_str].iter().map(|s| s.d as u64).max().unwrap_or(0);
        v.wake = clock + v.exc_len as u64 + longest + 64;
        v.quiet_needed = (longest as u32 / 32 + 3).max(8);
        if swell > 0.001 {
            v.swell = if restrike { v.swell } else { 0.0 };
            v.swell_step = 1.0 / (swell * rate);
        } else {
            v.swell = 1.0;
            v.swell_step = 0.0;
        }
        for s in 0..v.n_str {
            v.strings[s].loss_target = v.strings[s].loss_free;
            v.strings[s].lp_s = v.strings[s].lp_free;
        }
    }

    /// The dampers fall on `v` (unless it is above the damped range).
    fn damp(v: &mut Voice, rate: f32, release: f32) {
        if v.note >= NO_DAMPER {
            return;
        }
        v.damped = true;
        for s in 0..v.n_str {
            v.strings[s].loss_target = v.strings[s].loss_damped;
            v.strings[s].lp_s = v.strings[s].lp_damped;
        }
        v.modal_damp = (-6.9 / (release.max(0.02) * rate)).exp();
    }

    fn release_key(&mut self, note: u8) {
        let pedal = self.pedal_down();
        let rate = self.rate;
        let release = self.p.release;
        let mut noise = 0.0;
        for v in self.voices.iter_mut().filter(|v| v.on && v.note == note && v.key_down) {
            v.key_down = false;
            if !pedal && !v.sostenuto {
                Self::damp(v, rate, release);
                noise = v.gain;
            }
        }
        if noise > 0.0 && self.p.release_noise > 0.0 && self.p.model.strings() {
            // The damper felt settling on the strings.
            self.thump = ((0.03 * rate) as usize, self.thump.1.max(noise * self.p.release_noise * 0.04));
        }
    }

    fn set_sustain(&mut self, down: bool) {
        if down == self.sustain {
            return;
        }
        self.sustain = down;
        self.thump = ((0.05 * self.rate) as usize, self.p.pedal_noise * 0.05);
        if !self.pedal_down() {
            let rate = self.rate;
            let release = self.p.release;
            for v in self.voices.iter_mut().filter(|v| v.on && !v.key_down && !v.damped && !v.sostenuto) {
                Self::damp(v, rate, release);
            }
        }
    }

    /// The arpeggiator: the next step when it is due.
    fn arp_tick(&mut self) {
        // Notes whose gate ran out.
        let mut i = 0;
        while i < self.arp_off.len() {
            if self.arp_off[i].0 <= self.clock {
                let (_, n) = self.arp_off.swap_remove(i);
                self.release_key(n);
            } else {
                i += 1;
            }
        }
        if !self.p.arp.on || self.held.is_empty() {
            return;
        }
        while self.arp_next <= self.clock {
            let a = self.p.arp.clone();
            let mut notes: Vec<(u8, f32)> = Vec::with_capacity(self.held.len() * 4);
            for o in 0..a.octaves.clamp(1, 4) {
                for &(n, v) in &self.held {
                    notes.push((n.saturating_add(12 * o), v));
                }
            }
            if a.mode != ArpMode::AsPlayed {
                notes.sort_by_key(|x| x.0);
            }
            let len = notes.len().max(1);
            let step = self.arp_step;
            let pick: Vec<(u8, f32)> = match a.mode {
                ArpMode::Up | ArpMode::AsPlayed => vec![notes[step % len]],
                ArpMode::Down => vec![notes[len - 1 - step % len]],
                ArpMode::UpDown => {
                    let cycle = (2 * len).saturating_sub(2).max(1);
                    let k = step % cycle;
                    vec![notes[if k < len { k } else { cycle - k }]]
                }
                ArpMode::Random => vec![notes[(self.rng.next_u64() % len as u64) as usize]],
                ArpMode::Chord => self.held.clone(),
            };
            let beat_samples = 60.0 / self.bpm.max(20.0) * self.rate;
            let step_len = (a.rate * beat_samples) as u64;
            let accent = if (step as f32 * a.rate).fract() < 1e-3 { 1.0 + a.accent * 0.4 } else { 1.0 - a.accent * 0.15 };
            for (n, v) in pick {
                self.strike(n, (v * accent).min(1.0));
                let off = self.clock + ((a.gate.clamp(0.05, 1.0) * step_len as f32) as u64).max(1);
                if self.arp_off.len() < 64 {
                    self.arp_off.push((off, n));
                }
            }
            self.arp_step += 1;
            // Swing: every second step comes late, the next one early.
            let swing = (a.swing.clamp(0.0, 0.5) * step_len as f32) as i64;
            let delta = step_len as i64 + if self.arp_step % 2 == 1 { swing } else { -swing };
            self.arp_next = (self.arp_next as i64 + delta.max(1)) as u64;
        }
    }
}

impl InstrumentVoice for Piano {
    fn note_on(&mut self, n: NoteOn) {
        if self.p.arp.on {
            if self.held.is_empty() {
                self.arp_step = 0;
                self.arp_next = self.clock;
            }
            if !self.held.iter().any(|h| h.0 == n.note) && self.held.len() < 32 {
                self.held.push((n.note, n.velocity));
            }
            return;
        }
        self.strike(n.note, n.velocity);
    }

    fn note_off(&mut self, note: u8) {
        if self.p.arp.on {
            self.held.retain(|h| h.0 != note);
            return;
        }
        self.release_key(note);
    }

    fn choke(&mut self, seconds: f32) {
        let mul = (-6.9 / (seconds.max(0.002) * self.rate)).exp();
        for v in self.voices.iter_mut().filter(|v| v.on) {
            v.fade_mul = v.fade_mul.min(mul);
            v.key_down = false;
        }
        self.held.clear();
        self.arp_off.clear();
        self.sustain = false;
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len().min(r.len());
        if self.scratch.0.len() < n {
            self.scratch = (vec![0.0; n], vec![0.0; n]);
        }
        let (mut sl, mut sr) = std::mem::take(&mut self.scratch);
        sl[..n].iter_mut().for_each(|x| *x = 0.0);
        sr[..n].iter_mut().for_each(|x| *x = 0.0);
        let start = self.clock;
        let mut k0 = 0;
        while k0 < n {
            let k1 = (k0 + 32).min(n);
            self.clock = start + k0 as u64;
            self.arp_tick();
            for v in self.voices.iter_mut().filter(|v| v.on) {
                let mut peak = 0.0f32;
                // Dampers close in smoothly (no click).
                for s in 0..v.n_str {
                    let st = &mut v.strings[s];
                    st.loss += (st.loss_target - st.loss) * 0.3;
                }
                for k in k0..k1 {
                    let mut out = 0.0;
                    if v.n_str > 0 {
                        let input = if v.exc_pos < v.exc_len {
                            let e = v.exc[v.exc_pos];
                            v.exc_pos += 1;
                            e
                        } else {
                            0.0
                        };
                        for s in 0..v.n_str {
                            out += v.strings[s].tick(input);
                        }
                        out *= 0.5;
                    } else {
                        let damp = if v.damped { v.modal_damp } else { 1.0 };
                        for pt in v.partials[..v.n_partials].iter_mut() {
                            let re = pt.re * pt.c - pt.im * pt.s;
                            let im = pt.re * pt.s + pt.im * pt.c;
                            pt.re = re * pt.decay * damp;
                            pt.im = im * pt.decay * damp;
                            out += pt.im * pt.level;
                        }
                        out *= v.gain * 0.35;
                        if v.bark > 0.0 {
                            // The pickup: asymmetric, so harder hits bark.
                            let x = out * (1.0 + v.bark * 3.0);
                            out = (x + 0.3 * v.bark * x * x).tanh() / (1.0 + v.bark);
                        }
                    }
                    if v.swell < 1.0 {
                        v.swell = (v.swell + v.swell_step).min(1.0);
                        out *= v.swell * v.swell;
                    }
                    out *= v.fade;
                    v.fade *= v.fade_mul;
                    sl[k] += out * v.gl;
                    sr[k] += out * v.gr;
                    peak = peak.max(out.abs());
                }
                let now = start + k1 as u64;
                if now > v.wake && v.swell >= 1.0 && (peak < 2e-5 || v.fade < 1e-4) {
                    v.quiet += 1;
                    if v.quiet > v.quiet_needed || v.fade < 1e-4 {
                        v.on = false;
                    }
                } else {
                    v.quiet = 0;
                }
            }
            k0 = k1;
        }
        self.clock = start + n as u64;
        // Noises not tied to a note: pedal and damper thumps.
        if self.thump.0 > 0 {
            let len = self.thump.0.min(n);
            let total = (0.05 * self.rate).max(1.0);
            for k in 0..len {
                let left = (self.thump.0 - k) as f32 / total;
                let x = self.rng.bipolar() * self.thump.1 * left * left;
                sl[k] += x;
                sr[k] += x;
            }
            self.thump.0 -= len;
        }
        // Sympathetic resonance: open strings fed by what is playing, louder with the pedal down.
        let p = &self.p;
        let res = p.resonance * if self.sustain || p.pedal_latch { 1.0 } else { 0.25 };
        if res > 0.001 && p.model.strings() {
            let feed = 0.02 * res;
            let g = 0.9995;
            for k in 0..n {
                let x = (sl[k] + sr[k]) * feed;
                let mut acc = 0.0;
                for s in self.symp.iter_mut() {
                    let len = s.buf.len();
                    let y = s.buf[(s.w + len - s.d) % len];
                    s.lp += 0.6 * (y - s.lp);
                    s.buf[s.w] = s.lp * g + x;
                    s.w = (s.w + 1) % len;
                    acc += y;
                }
                let a = acc * res * 0.12;
                sl[k] += a;
                sr[k] += a;
            }
        }
        // Soundboard, high damp, a low cut; electric pianos: stereo tremolo.
        let body = p.body.clamp(0.0, 1.0);
        let hd_a = 1.0 - (-std::f32::consts::TAU * (18000.0 * (1.0 - p.high_damp * 0.85).powi(2) + 600.0) / self.rate).exp();
        let hp_a = 1.0 - (-std::f32::consts::TAU * 30.0 / self.rate).exp();
        let bn = self.board_n;
        for (ch, buf) in [&mut sl, &mut sr].into_iter().enumerate() {
            for x in buf[..n].iter_mut() {
                let mut y = *x;
                if body > 0.0 {
                    let mut b = 0.0;
                    for m in self.board[ch][..bn].iter_mut() {
                        b += m.run(y);
                    }
                    y = y * (1.0 - 0.35 * body) + b * body * 1.2;
                }
                self.hd[ch] += hd_a * (y - self.hd[ch]);
                y = self.hd[ch];
                self.hp[ch] += hp_a * (y - self.hp[ch]);
                *x = y - self.hp[ch];
            }
        }
        if p.tremolo_depth > 0.001 && !p.model.strings() {
            let step = std::f32::consts::TAU * p.tremolo_rate / self.rate;
            for k in 0..n {
                self.trem_phase = (self.trem_phase + step) % std::f32::consts::TAU;
                let m = self.trem_phase.sin() * p.tremolo_depth;
                sl[k] *= 1.0 - m.max(0.0);
                sr[k] *= 1.0 + m.min(0.0);
            }
        }
        let busy = self.voices.iter().any(|v| v.on);
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
        let Model::Piano(p) = model else { return };
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
        let board_changed = p.model != self.p.model;
        let latch_off = self.p.pedal_latch && !p.pedal_latch;
        if self.p.arp.on && !p.arp.on {
            self.held.clear();
        }
        self.p = (**p).clone();
        if board_changed {
            self.set_board();
        }
        if latch_off {
            let s = self.sustain;
            self.sustain = true;
            self.set_sustain(s);
        }
    }

    fn active_voices(&self) -> usize {
        let v = self.voices.iter().filter(|v| v.on).count();
        let arp = usize::from(self.p.arp.on && !self.held.is_empty()) + self.arp_off.len().min(1);
        let tail = usize::from(v == 0 && self.tail > 0 && (!self.fx.is_empty() || self.p.resonance > 0.0));
        v + arp + tail + usize::from(self.thump.0 > 0)
    }

    fn set_param(&mut self, id: &str, value: f32) {
        match id {
            "__bpm" => self.bpm = value,
            "__sustain" => self.set_sustain(value >= 0.5),
            "__soft" => self.soft = value.clamp(0.0, 1.0),
            "__sostenuto" => {
                let down = value >= 0.5;
                if down && !self.sostenuto {
                    for v in self.voices.iter_mut().filter(|v| v.on && v.key_down) {
                        v.sostenuto = true;
                    }
                } else if !down && self.sostenuto {
                    let (rate, release, pedal) = (self.rate, self.p.release, self.pedal_down());
                    for v in self.voices.iter_mut().filter(|v| v.on && v.sostenuto) {
                        v.sostenuto = false;
                        if !v.key_down && !pedal {
                            Self::damp(v, rate, release);
                        }
                    }
                }
                self.sostenuto = down;
            }
            "tone" => self.p.tone = value.clamp(0.0, 1.0),
            "body" => self.p.body = value.clamp(0.0, 1.0),
            "high_damp" => self.p.high_damp = value.clamp(0.0, 1.0),
            "resonance" => self.p.resonance = value.clamp(0.0, 1.0),
            "master" => self.p.master = value.clamp(0.0, 1.5),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn render(p: PianoParams, notes: &[(u8, f32)], hold: f32, secs: f32) -> (Vec<f32>, Vec<f32>) {
        let rate = 48000.0;
        let mut pn = Piano::new(rate);
        pn.update(&Model::Piano(Box::new(p)), &SampleCache::default());
        pn.set_param("__bpm", 120.0);
        for &(n, v) in notes {
            pn.note_on(NoteOn { note: n, velocity: v, pan: 0.0, detune: 0.0, slide: false });
        }
        let len = (secs * rate) as usize;
        let off = (hold * rate) as usize;
        let (mut l, mut r) = (vec![0.0; len], vec![0.0; len]);
        let mut pos = 0;
        while pos < len {
            if pos <= off && off < pos + 256 {
                for &(n, _) in notes {
                    pn.note_off(n);
                }
            }
            let end = (pos + 256).min(len);
            pn.process(&mut l[pos..end], &mut r[pos..end]);
            pos = end;
        }
        (l, r)
    }

    fn pitch(x: &[f32], rate: f32, guess: f32) -> f32 {
        // Autocorrelation around the expected period.
        let (lo, hi) = ((rate / guess / 1.06) as usize, (rate / guess * 1.06) as usize + 1);
        let c = |lag: usize| -> f32 { x.iter().zip(&x[lag..]).map(|(a, b)| a * b).sum() };
        let mut best = (lo, f32::MIN);
        for lag in lo..=hi {
            let v = c(lag);
            if v > best.1 {
                best = (lag, v);
            }
        }
        let (a, b, cc) = (c(best.0 - 1), best.1, c(best.0 + 1));
        rate / (best.0 as f32 + 0.5 * (a - cc) / (a - 2.0 * b + cc))
    }

    #[test]
    fn keys_are_in_tune_with_stretch_tuning() {
        let p = PianoParams { stretch: 0.0, body: 0.0, tightness: 1.0, ..Default::default() };
        for note in [33u8, 45, 57, 69, 81] {
            let (l, _) = render(p.clone(), &[(note, 0.7)], 2.0, 1.0);
            let want = midi_to_hz(note as f32);
            let f = pitch(&l[12000..12000 + 16384], 48000.0, want);
            let cents = 1200.0 * (f / want).log2();
            assert!(cents.abs() < 6.0, "note {note}: {f:.2} Hz vs {want:.2} ({cents:.1} ct)");
        }
        // With stretch, the top is sharp and the bottom flat.
        let pn = Piano { p: PianoParams { stretch: 1.0, ..Default::default() }, ..Piano::new(48000.0) };
        assert!(pn.freq(105.0) > midi_to_hz(105.0) * 1.005);
        assert!(pn.freq(33.0) < midi_to_hz(33.0) * 0.995);
    }

    #[test]
    fn upper_partials_run_sharp() {
        // Stiff strings: the 8th partial sits above 8 × f0.
        let p = PianoParams { stiffness: 1.0, stretch: 0.0, body: 0.0, tightness: 1.0, hammer: 0.0, resonance: 0.0, ..Default::default() };
        let (l, _) = render(p, &[(36, 0.9)], 3.0, 1.5);
        let x = &l[24000..24000 + 32768];
        let rate = 48000.0;
        let f0 = midi_to_hz(36.0);
        // Find the 13th partial's peak by DFT (the 8th sits in the hammer's notch).
        let mag = |f: f32| -> f32 {
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for (i, v) in x.iter().enumerate() {
                let ph = std::f32::consts::TAU * f * i as f32 / rate;
                re += v * ph.cos();
                im += v * ph.sin();
            }
            re * re + im * im
        };
        let mut best = (0.0, 0.0);
        let mut f = 13.0 * f0 * 0.99;
        while f < 13.0 * f0 * 1.06 {
            let m = mag(f);
            if m > best.1 {
                best = (f, m);
            }
            f += 0.2;
        }
        assert!(best.0 > 13.0 * f0 * 1.003, "13th partial at {:.2} Hz, harmonic would be {:.2}", best.0, 13.0 * f0);
    }

    #[test]
    fn dampers_stop_notes_unless_the_pedal_holds_them() {
        let p = PianoParams { resonance: 0.0, ..Default::default() };
        let energy = |l: &[f32]| l.iter().map(|x| x * x).sum::<f32>();
        let (dry, _) = render(p.clone(), &[(48, 0.8)], 0.3, 1.5);
        let late_dry = energy(&dry[48000..]);
        // Same, with the pedal down.
        let rate = 48000.0;
        let mut pn = Piano::new(rate);
        pn.update(&Model::Piano(Box::new(p)), &SampleCache::default());
        pn.set_param("__sustain", 1.0);
        pn.note_on(NoteOn { note: 48, velocity: 0.8, pan: 0.0, detune: 0.0, slide: false });
        let (mut l, mut r) = (vec![0.0; 72000], vec![0.0; 72000]);
        pn.process(&mut l[..14400], &mut r[..14400]);
        pn.note_off(48);
        pn.process(&mut l[14400..], &mut r[14400..]);
        let late_ped = energy(&l[48000..]);
        assert!(late_ped > late_dry * 100.0, "pedal {late_ped} vs damped {late_dry}");
        // Pedal up: now it stops.
        pn.set_param("__sustain", 0.0);
        let (mut l2, mut r2) = (vec![0.0; 48000], vec![0.0; 48000]);
        pn.process(&mut l2, &mut r2);
        assert!(energy(&l2[24000..]) < late_ped * 0.001);
    }

    #[test]
    fn harder_is_louder_and_brighter() {
        let p = PianoParams::default();
        let (soft, _) = render(p.clone(), &[(60, 0.2)], 1.0, 0.5);
        let (hard, _) = render(p, &[(60, 1.0)], 1.0, 0.5);
        let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
        // Brightness: the share of the signal's first difference.
        let bright = |x: &[f32]| rms(&x.windows(2).map(|w| w[1] - w[0]).collect::<Vec<_>>()) / rms(x);
        assert!(rms(&hard[..12000]) > rms(&soft[..12000]) * 2.0);
        assert!(bright(&hard[..12000]) > bright(&soft[..12000]) * 1.2);
    }

    #[test]
    fn every_model_plays_clean() {
        for model in PianoModel::ALL {
            let p = PianoParams::for_model(model);
            let (lo, hi) = model.range();
            for note in [lo + 2, (lo + hi) / 2, hi - 2] {
                let (l, r) = render(p.clone(), &[(note, 1.0), (note + 4, 0.6)], 0.5, 1.0);
                let peak = l.iter().chain(&r).fold(0.0f32, |m, v| m.max(v.abs()));
                assert!(peak.is_finite() && peak > 0.005 && peak < 3.0, "{model:?} note {note}: {peak}");
            }
        }
    }

    #[test]
    fn the_arpeggiator_plays_in_time() {
        let p = PianoParams { arp: Arp { on: true, rate: 0.25, octaves: 1, ..Arp::default() }, ..Default::default() };
        let rate = 48000.0;
        let mut pn = Piano::new(rate);
        pn.update(&Model::Piano(Box::new(p)), &SampleCache::default());
        pn.set_param("__bpm", 120.0);
        for n in [60u8, 64, 67] {
            pn.note_on(NoteOn { note: n, velocity: 0.8, pan: 0.0, detune: 0.0, slide: false });
        }
        // 1/16 at 120 bpm = 125 ms: 8 notes in a second, C E G C E G …
        let mut notes = Vec::new();
        let (mut l, mut r) = (vec![0.0; 240], vec![0.0; 240]);
        for _ in 0..200 {
            let block = pn.clock;
            pn.process(&mut l, &mut r);
            // Keys struck during this block.
            let mut struck: Vec<(u64, u8)> = pn.voices.iter().filter(|v| v.on && v.age >= block).map(|v| (v.age, v.note)).collect();
            struck.sort();
            notes.extend(struck.into_iter().map(|s| s.1));
        }
        assert!((7..=9).contains(&notes.len()), "{notes:?}");
        assert_eq!(&notes[..4], &[60, 64, 67, 60]);
    }
}
#[cfg(test)]
mod levels {
    use super::*;

    /// The loudest 50 ms (dBFS RMS) and the peak of a preset's audition chord.
    fn loudness(p: PianoParams) -> (f32, f32) {
        let notes = crate::pno_library::audition(&p);
        let rate = 48000.0;
        let mut pn = Piano::new(rate);
        pn.update(&Model::Piano(Box::new(p)), &SampleCache::default());
        pn.set_param("__bpm", 120.0);
        for n in notes {
            pn.note_on(NoteOn { note: n, velocity: 0.75, pan: 0.0, detune: 0.0, slide: false });
        }
        let len = (2.5 * rate) as usize;
        let (mut l, mut r) = (vec![0.0; len], vec![0.0; len]);
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            pn.process(a, b);
        }
        let win = 2400;
        let mut best = 0.0f32;
        for k in (0..len.saturating_sub(win)).step_by(win / 2) {
            let e: f32 = l[k..k + win].iter().chain(&r[k..k + win]).map(|v| v * v).sum::<f32>() / (2 * win) as f32;
            best = best.max(e);
        }
        let peak = l.iter().chain(&r).fold(0.0f32, |m, v| m.max(v.abs()));
        (10.0 * best.max(1e-12).log10(), peak)
    }

    const TARGET_DB: f32 = -17.0;

    #[test]
    fn library_presets_play_cleanly_at_an_even_level() {
        for p in crate::pno_library::library().iter().step_by(41) {
            let (db, peak) = loudness(p.params.clone());
            assert!(peak.is_finite() && peak < 3.0, "{}: peak {peak}", p.name);
            assert!((db - TARGET_DB).abs() < 4.0, "{} ({}): {db:.1} dB", p.name, p.kind);
        }
    }

    #[test]
    #[ignore]
    fn render_piano_demo() {
        let Ok(path) = std::env::var("DRAGON_DEMO") else { return };
        let rate = 48000.0;
        let lib = crate::pno_library::library();
        use crate::pno_library::Category as C;
        let pick = |c: C, kind: &str| lib.iter().find(|p| p.category == c && p.kind == kind).map(|p| p.params.clone()).unwrap();
        // A ballad: left-hand chords with the pedal, right-hand melody (beat, note, vel, len).
        let chords: [[u8; 4]; 4] = [[36, 55, 60, 64], [33, 52, 57, 60], [29, 53, 57, 60], [31, 50, 55, 59]];
        let melody: [(f32, u8, f32); 12] =
            [(0.0, 76, 0.7), (1.0, 74, 0.55), (2.0, 72, 0.6), (3.0, 71, 0.5), (4.0, 72, 0.7), (5.5, 76, 0.6), (8.0, 77, 0.75), (9.0, 76, 0.55), (10.0, 74, 0.6), (12.0, 74, 0.65), (13.0, 71, 0.5), (14.0, 67, 0.55)];
        let mut ballad: Vec<(f32, u8, f32, f32)> = Vec::new();
        for (b, ch) in chords.iter().enumerate() {
            for (k, n) in ch.iter().enumerate() {
                ballad.push((b as f32 * 4.0 + k as f32 * 0.04, *n, 0.5 + 0.1 * (k == 0) as u8 as f32, 3.8));
            }
        }
        for (b, n, v) in melody {
            ballad.push((b, n, v, 0.95));
        }
        let parts: Vec<(PianoParams, Vec<(f32, u8, f32, f32)>, bool)> = vec![
            (pick(C::Concert, "Hall"), ballad.clone(), true),
            (pick(C::Felt, "Felt"), ballad.clone(), true),
            (pick(C::Tine, "Tine EP"), ballad.clone(), false),
            (pick(C::Arp, "Arpeggio"), chords.iter().enumerate().flat_map(|(b, ch)| ch[1..].iter().map(move |n| (b as f32 * 4.0, *n, 0.7, 3.95))).collect(), false),
        ];
        let bpm = 76.0;
        let beat = 60.0 / bpm * rate;
        let mut out: Vec<f32> = Vec::new();
        for (p, ev, pedal) in parts {
            let mut pn = Piano::new(rate);
            pn.update(&Model::Piano(Box::new(p.clone())), &SampleCache::default());
            pn.set_param("__bpm", bpm);
            let total = (17.0 * beat) as usize;
            let (mut l, mut r) = (vec![0.0f32; total], vec![0.0f32; total]);
            let mut evs: Vec<(usize, u8, f32, bool)> = Vec::new();
            for (b, n, v, len) in ev {
                evs.push(((b * beat) as usize, n, v, true));
                evs.push((((b + len) * beat) as usize, n, 0.0, false));
            }
            evs.sort_by_key(|e| (e.0, e.3));
            let (mut i, mut pos) = (0, 0);
            while pos < total {
                if pedal {
                    // Pedal down on each bar, up just before the next.
                    let bar = (pos as f32 / beat / 4.0).fract();
                    pn.set_param("__sustain", if bar > 0.02 && bar < 0.97 { 1.0 } else { 0.0 });
                }
                while i < evs.len() && evs[i].0 <= pos {
                    let (_, n, v, on) = evs[i];
                    if on {
                        pn.note_on(NoteOn { note: n, velocity: v, pan: 0.0, detune: 0.0, slide: false });
                    } else {
                        pn.note_off(n);
                    }
                    i += 1;
                }
                let end = (pos + 128).min(total);
                pn.process(&mut l[pos..end], &mut r[pos..end]);
                pos = end;
            }
            eprintln!("{}: peak {:.2}", p.preset, l.iter().fold(0.0f32, |m, x| m.max(x.abs())));
            for k in 0..total {
                out.push(l[k]);
                out.push(r[k]);
            }
        }
        let data_len = (out.len() * 2) as u32;
        let mut bytes = Vec::with_capacity(44 + out.len() * 2);
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
    #[ignore]
    fn measure_piano_levels() {
        let lib = crate::pno_library::library();
        let old = crate::pno_library::LEVELS;
        let chunk = lib.len().div_ceil(4);
        let results: Vec<(usize, f32, f32)> = std::thread::scope(|sc| {
            let hs: Vec<_> = lib
                .chunks(chunk)
                .enumerate()
                .map(|(ci, c)| {
                    sc.spawn(move || {
                        c.iter()
                            .enumerate()
                            .map(|(j, p)| {
                                let (db, pk) = loudness(p.params.clone());
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
            levels[i] = ((prev + TARGET_DB - db).clamp(-36.0, 36.0) * 10.0).round() / 10.0;
            if pk > 2.0 || db < -55.0 {
                println!("CHECK {} ({}): {db:.1} dB peak {pk:.2}", lib[i].name, lib[i].kind);
            }
        }
        let text = format!("[{}]\n", levels.iter().map(|v| format!("{v:.1}")).collect::<Vec<_>>().join(", "));
        std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/src/pno_levels.in"), text).unwrap();
    }
}
