//! The time-bending units: Hex (tempo-synced stutter / glitch), Petrify (tape stop and spin-up) and
//! Sandglass (live granular). All three read a recording of the last seconds of their input.

use super::{Effect, EffectDef, ParamInfo};
use crate::dsp::{db_to_gain, Rng};

/// A circular recording with fractional (linear) reads.
struct Tape {
    buf: Vec<[f32; 2]>,
    w: usize,
}

impl Tape {
    fn new(len: usize) -> Tape {
        Tape { buf: vec![[0.0; 2]; len.max(4)], w: 0 }
    }
    fn len(&self) -> usize {
        self.buf.len()
    }
    fn write(&mut self, l: f32, r: f32) {
        self.buf[self.w] = [l, r];
        self.w = (self.w + 1) % self.buf.len();
    }
    /// Reads at an absolute position (samples, may be fractional, wraps).
    fn at(&self, pos: f64) -> [f32; 2] {
        let n = self.buf.len() as f64;
        let p = pos.rem_euclid(n);
        let i = p as usize;
        let f = (p - i as f64) as f32;
        let a = self.buf[i % self.buf.len()];
        let b = self.buf[(i + 1) % self.buf.len()];
        [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f]
    }
    fn clear(&mut self) {
        self.buf.fill([0.0; 2]);
    }
}

// ----------------------------------------------------------------------------------------------- stutter

pub const STUTTER_MODES: &[&str] = &["Off", "Always", "Random"];
pub const DIVISIONS: &[&str] = &["1/4", "1/8", "1/16", "1/32", "1/64"];
const DIV_BEATS: [f64; 5] = [1.0, 0.5, 0.25, 0.125, 0.0625];
pub const HOLDS: &[&str] = &["2", "4", "8", "16"];
const HOLD_N: [usize; 4] = [2, 4, 8, 16];

const STUTTER_PARAMS: &[ParamInfo] = &[
    ParamInfo::choice("mode", "Mode", STUTTER_MODES, 2.0),
    ParamInfo::choice("division", "Slice", DIVISIONS, 2.0),
    ParamInfo::choice("hold", "Repeats", HOLDS, 1.0),
    ParamInfo::lin("chance", "Chance", 0.0, 100.0, 40.0, "%"),
    ParamInfo::lin("gate", "Gate", 10.0, 100.0, 90.0, "%"),
    ParamInfo::lin("pitch", "Pitch step", -12.0, 12.0, 0.0, "st"),
    ParamInfo::lin("reverse", "Reverse", 0.0, 100.0, 0.0, "%"),
    ParamInfo::lin("mix", "Mix", 0.0, 100.0, 100.0, "%"),
];

pub fn stutter_def() -> EffectDef {
    EffectDef {
        id: "stutter",
        name: "Hex (stutter / glitch)",
        category: "Time",
        description: "Tempo-synced stutter: catches a slice of the beat and repeats it, always or by chance, with gating, a pitch step per repeat and reversed slices.",
        params: STUTTER_PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(Hex::new(rate)),
    }
}

pub struct Hex {
    rate: f32,
    mode: usize,
    div: usize,
    hold: usize,
    chance: f32,
    gate: f32,
    pitch: f32,
    reverse: f32,
    mix: f32,
    bpm: f64,
    beat: f64,
    playing: bool,
    free: f64,
    tape: Tape,
    rng: Rng,
    /// The group being played: index of the group, whether it stutters, where its slice starts on the tape.
    group: i64,
    active: bool,
    start: f64,
    rev: bool,
    last_slice: i64,
    /// Click-free switching between live and repeated sound.
    blend: f32,
    tele: [f32; 3],
}

impl Hex {
    pub fn new(rate: f32) -> Self {
        Hex {
            rate,
            mode: 2,
            div: 2,
            hold: 1,
            chance: 0.4,
            gate: 0.9,
            pitch: 0.0,
            reverse: 0.0,
            mix: 1.0,
            bpm: 120.0,
            beat: 0.0,
            playing: false,
            free: 0.0,
            tape: Tape::new((rate * 8.0) as usize),
            rng: Rng::new(0x4e78),
            group: -1,
            active: false,
            start: 0.0,
            rev: false,
            last_slice: -1,
            blend: 0.0,
            tele: [0.0; 3],
        }
    }
}

impl Effect for Hex {
    fn set_param(&mut self, id: &str, v: f32) {
        let i = v.round().max(0.0) as usize;
        match id {
            "mode" => self.mode = i.min(2),
            "division" => self.div = i.min(4),
            "hold" => self.hold = i.min(3),
            "chance" => self.chance = v.clamp(0.0, 100.0) / 100.0,
            "gate" => self.gate = v.clamp(10.0, 100.0) / 100.0,
            "pitch" => self.pitch = v,
            "reverse" => self.reverse = v.clamp(0.0, 100.0) / 100.0,
            "mix" => self.mix = v.clamp(0.0, 100.0) / 100.0,
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.tape.clear();
        self.group = -1;
        self.active = false;
        self.blend = 0.0;
    }

    fn set_tempo(&mut self, bpm: f64) {
        self.bpm = bpm.max(1.0);
    }

    fn set_position(&mut self, beat: f64, playing: bool) {
        self.beat = beat;
        self.playing = playing;
    }

    fn telemetry(&self) -> &[f32] {
        &self.tele
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let slice_beats = DIV_BEATS[self.div];
        let hold = HOLD_N[self.hold];
        let spb = self.rate as f64 * 60.0 / self.bpm; // samples per beat
        let slice_len = slice_beats * spb;
        let bps = 1.0 / spb;
        let start_beat = if self.playing { self.beat } else { self.free };
        let fade = (0.002 * self.rate) as f64;
        let mut pos_in = 0.0f64;
        for i in 0..l.len() {
            let (xl, xr) = (l[i], r[i]);
            self.tape.write(xl, xr);
            let b = start_beat + i as f64 * bps;
            let slice = (b / slice_beats).floor() as i64;
            let group = slice.div_euclid(hold as i64);
            if group != self.group {
                self.group = group;
                self.active = match self.mode {
                    0 => false,
                    1 => true,
                    _ => self.rng.next_f32() < self.chance,
                };
                // The group's first slice is what gets repeated: it starts now on the tape.
                self.start = (self.tape.w as f64 - 1.0).rem_euclid(self.tape.len() as f64);
            }
            if slice != self.last_slice {
                self.last_slice = slice;
                self.rev = self.rng.next_f32() < self.reverse;
            }
            let k = slice.rem_euclid(hold as i64) as usize; // repeat number in the group
            let in_slice = (b / slice_beats - (b / slice_beats).floor()) * slice_len;
            pos_in = in_slice / slice_len;
            let repeating = self.active && k > 0;
            let target = if repeating { 1.0 } else { 0.0 };
            self.blend += (target - self.blend) * 0.02;
            let mut y = [xl, xr];
            if self.blend > 1e-4 {
                // Repeat k plays the slice at 2^(k·step/12) speed (shorter and higher, or longer and lower).
                let speed = 2f64.powf((k as f64 * self.pitch as f64) / 12.0);
                let t = in_slice * speed;
                let off = if self.rev { slice_len - 1.0 - t } else { t };
                let rep = if t < slice_len { self.tape.at(self.start + off.max(0.0)) } else { [0.0, 0.0] };
                // Gate with 2 ms edges.
                let open = slice_len * self.gate as f64;
                let g = if in_slice < fade {
                    in_slice / fade
                } else if in_slice > open {
                    0.0
                } else if in_slice > open - fade {
                    (open - in_slice) / fade
                } else {
                    1.0
                } as f32;
                y = [xl + (rep[0] * g - xl) * self.blend, xr + (rep[1] * g - xr) * self.blend];
            }
            l[i] = xl + (y[0] - xl) * self.mix;
            r[i] = xr + (y[1] - xr) * self.mix;
        }
        if !self.playing {
            self.free += l.len() as f64 * bps;
        }
        self.tele = [if self.active { 1.0 } else { 0.0 }, pos_in as f32, self.blend];
    }
}

// ----------------------------------------------------------------------------------------------- tape stop

pub const TAPE_MODES: &[&str] = &["Manual", "Every bar", "Every 2 bars", "Every 4 bars"];

const TAPE_PARAMS: &[ParamInfo] = &[
    ParamInfo::toggle("stop", "Stop", false),
    ParamInfo::choice("mode", "Trigger", TAPE_MODES, 0.0),
    ParamInfo::log("stop_time", "Stop time", 50.0, 4000.0, 600.0, "ms"),
    ParamInfo::log("start_time", "Start time", 10.0, 2000.0, 200.0, "ms"),
    ParamInfo::lin("curve", "Curve", -100.0, 100.0, 30.0, "%"),
    ParamInfo::lin("darken", "Darken", 0.0, 100.0, 50.0, "%"),
    ParamInfo::lin("mix", "Mix", 0.0, 100.0, 100.0, "%"),
];

pub fn tape_stop_def() -> EffectDef {
    EffectDef {
        id: "tape_stop",
        name: "Petrify (tape stop)",
        category: "Time",
        description: "Tape stop and spin-up: the sound slows to a halt (pitch falling with the speed) and starts again. Triggered by its switch (automate it) or at the end of every bar, 2 or 4 bars.",
        params: TAPE_PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(Petrify::new(rate)),
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum TapeState {
    Running,
    Stopping,
    Stopped,
    Starting,
    /// Crossfading from the late tape back to the live input.
    Rejoin,
}

pub struct Petrify {
    rate: f32,
    stop: bool,
    mode: usize,
    stop_ms: f32,
    start_ms: f32,
    curve: f32,
    darken: f32,
    mix: f32,
    bpm: f64,
    beat: f64,
    playing: bool,
    tape: Tape,
    state: TapeState,
    /// Read position on the tape and the time since the state began (samples).
    read: f64,
    t: f64,
    speed: f32,
    lp: [f32; 2],
    tele: [f32; 2],
}

impl Petrify {
    pub fn new(rate: f32) -> Self {
        Petrify {
            rate,
            stop: false,
            mode: 0,
            stop_ms: 600.0,
            start_ms: 200.0,
            curve: 0.3,
            darken: 0.5,
            mix: 1.0,
            bpm: 120.0,
            beat: 0.0,
            playing: false,
            tape: Tape::new((rate * 10.0) as usize),
            state: TapeState::Running,
            read: 0.0,
            t: 0.0,
            speed: 1.0,
            lp: [0.0; 2],
            tele: [1.0, 0.0],
        }
    }

    /// The speed during a ramp at progress `u ∈ [0, 1]` (1 → 0 when stopping), bent by the curve:
    /// `s = (1 − u)^{2^{2c}}` – a positive curve brakes early (a heavy reel), a negative one late.
    fn ramp(&self, u: f32) -> f32 {
        (1.0 - u.clamp(0.0, 1.0)).powf(2f32.powf(2.0 * self.curve))
    }

    fn wants_stop(&self, b: f64) -> bool {
        if self.stop {
            return true;
        }
        if self.mode == 0 || !self.playing {
            return false;
        }
        // Automatic: the stop ends exactly on the bar line (4 beats per bar).
        let bars = [1.0, 1.0, 2.0, 4.0][self.mode] * 4.0;
        let stop_beats = self.stop_ms as f64 * 0.001 * self.bpm / 60.0;
        let into = b.rem_euclid(bars);
        into >= bars - stop_beats
    }
}

impl Effect for Petrify {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "stop" => self.stop = v >= 0.5,
            "mode" => self.mode = (v.round().max(0.0) as usize).min(3),
            "stop_time" => self.stop_ms = v.clamp(50.0, 4000.0),
            "start_time" => self.start_ms = v.clamp(10.0, 2000.0),
            "curve" => self.curve = v.clamp(-100.0, 100.0) / 100.0,
            "darken" => self.darken = v.clamp(0.0, 100.0) / 100.0,
            "mix" => self.mix = v.clamp(0.0, 100.0) / 100.0,
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.tape.clear();
        self.state = TapeState::Running;
        self.speed = 1.0;
    }

    fn set_tempo(&mut self, bpm: f64) {
        self.bpm = bpm.max(1.0);
    }

    fn set_position(&mut self, beat: f64, playing: bool) {
        self.beat = beat;
        self.playing = playing;
    }

    fn telemetry(&self) -> &[f32] {
        &self.tele
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let bps = self.bpm / 60.0 / self.rate as f64;
        let stop_n = (self.stop_ms * 0.001 * self.rate) as f64;
        let start_n = (self.start_ms * 0.001 * self.rate) as f64;
        let rejoin_n = (0.012 * self.rate) as f64;
        for i in 0..l.len() {
            let (xl, xr) = (l[i], r[i]);
            self.tape.write(xl, xr);
            let want = self.wants_stop(self.beat + i as f64 * bps);
            let live_pos = self.tape.w as f64 - 1.0;
            // State changes.
            match self.state {
                TapeState::Running | TapeState::Rejoin if want => {
                    self.state = TapeState::Stopping;
                    self.read = live_pos;
                    self.t = 0.0;
                }
                TapeState::Stopping | TapeState::Stopped if !want => {
                    self.state = TapeState::Starting;
                    self.t = 0.0;
                }
                TapeState::Starting if want => {
                    self.state = TapeState::Stopping;
                    self.t = stop_n * (1.0 - self.speed as f64);
                }
                _ => {}
            }
            let y = match self.state {
                TapeState::Running => [xl, xr],
                TapeState::Stopping => {
                    self.speed = self.ramp((self.t / stop_n) as f32);
                    self.t += 1.0;
                    if self.t >= stop_n {
                        self.state = TapeState::Stopped;
                    }
                    self.read += self.speed as f64;
                    self.tape.at(self.read)
                }
                TapeState::Stopped => {
                    self.speed = 0.0;
                    [0.0, 0.0]
                }
                TapeState::Starting => {
                    self.speed = 1.0 - self.ramp((self.t / start_n.max(1.0)) as f32);
                    self.t += 1.0;
                    self.read += self.speed as f64;
                    if self.t >= start_n {
                        self.state = TapeState::Rejoin;
                        self.t = 0.0;
                    }
                    self.tape.at(self.read)
                }
                TapeState::Rejoin => {
                    // At full speed again, but late: fade over to the live input.
                    self.speed = 1.0;
                    self.read += 1.0;
                    let late = self.tape.at(self.read);
                    let u = (self.t / rejoin_n).min(1.0) as f32;
                    self.t += 1.0;
                    if self.t >= rejoin_n {
                        self.state = TapeState::Running;
                    }
                    [late[0] + (xl - late[0]) * u, late[1] + (xr - late[1]) * u]
                }
            };
            // The slower the tape, the duller the sound (a one-pole low-pass that closes with the speed).
            let fc = 400.0 + 19_000.0 * self.speed.powf(1.0 + 2.0 * self.darken);
            let a = (-std::f32::consts::TAU * fc.min(self.rate * 0.45) / self.rate).exp();
            let mut out = y;
            if self.state != TapeState::Running && self.darken > 0.0 {
                for c in 0..2 {
                    self.lp[c] = out[c] + (self.lp[c] - out[c]) * a;
                    out[c] = self.lp[c];
                }
            } else {
                self.lp = out;
            }
            l[i] = xl + (out[0] - xl) * self.mix;
            r[i] = xr + (out[1] - xr) * self.mix;
        }
        self.tele = [self.speed, if self.state == TapeState::Running { 0.0 } else { 1.0 }];
    }
}

// ----------------------------------------------------------------------------------------------- granular

const MAX_GRAINS: usize = 64;

const GRANULAR_PARAMS: &[ParamInfo] = &[
    ParamInfo::log("size", "Grain size", 10.0, 500.0, 80.0, "ms"),
    ParamInfo::log("density", "Density", 1.0, 100.0, 20.0, "/s"),
    ParamInfo::lin("position", "Delay", 0.0, 2000.0, 150.0, "ms"),
    ParamInfo::lin("spray", "Spray", 0.0, 1000.0, 80.0, "ms"),
    ParamInfo::lin("pitch", "Pitch", -24.0, 24.0, 0.0, "st"),
    ParamInfo::lin("pitch_rand", "Pitch random", 0.0, 12.0, 0.0, "st"),
    ParamInfo::lin("reverse", "Reverse", 0.0, 100.0, 0.0, "%"),
    ParamInfo::lin("spread", "Spread", 0.0, 100.0, 60.0, "%"),
    ParamInfo::lin("feedback", "Feedback", 0.0, 90.0, 0.0, "%"),
    ParamInfo::toggle("freeze", "Freeze", false),
    ParamInfo::lin("mix", "Mix", 0.0, 100.0, 50.0, "%"),
    ParamInfo::lin("output", "Output", -24.0, 12.0, 0.0, "dB"),
];

pub fn granular_def() -> EffectDef {
    EffectDef {
        id: "granular",
        name: "Sandglass (granular)",
        category: "Time",
        description: "Live granular processor: scatters short grains of the last seconds of the input – size, density, delay and spray, pitch and its randomness, reversed grains, stereo spread, feedback and freeze.",
        params: GRANULAR_PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(Sandglass::new(rate)),
    }
}

#[derive(Clone, Copy, Default)]
struct Grain {
    on: bool,
    pos: f64,
    speed: f64,
    len: f64,
    age: f64,
    gl: f32,
    gr: f32,
}

pub struct Sandglass {
    rate: f32,
    size: f32,
    density: f32,
    position: f32,
    spray: f32,
    pitch: f32,
    pitch_rand: f32,
    reverse: f32,
    spread: f32,
    feedback: f32,
    freeze: bool,
    mix: f32,
    out: f32,
    tape: Tape,
    grains: [Grain; MAX_GRAINS],
    until_next: f64,
    rng: Rng,
    fb: [f32; 2],
    tele: [f32; MAX_GRAINS],
}

impl Sandglass {
    pub fn new(rate: f32) -> Self {
        Sandglass {
            rate,
            size: 80.0,
            density: 20.0,
            position: 150.0,
            spray: 80.0,
            pitch: 0.0,
            pitch_rand: 0.0,
            reverse: 0.0,
            spread: 0.6,
            feedback: 0.0,
            freeze: false,
            mix: 0.5,
            out: 1.0,
            tape: Tape::new((rate * 4.0) as usize),
            grains: [Grain::default(); MAX_GRAINS],
            until_next: 0.0,
            rng: Rng::new(0x5a2d),
            fb: [0.0; 2],
            tele: [0.0; MAX_GRAINS],
        }
    }

    fn spawn(&mut self) {
        let Some(slot) = self.grains.iter().position(|g| !g.on) else { return };
        let len = (self.size * 0.001 * self.rate) as f64;
        let st = self.pitch + self.pitch_rand * self.rng.bipolar();
        let mut speed = 2f64.powf(st as f64 / 12.0);
        if self.rng.next_f32() < self.reverse {
            speed = -speed;
        }
        // Start far enough behind the write head that the grain never overtakes it.
        let behind = (self.position + self.spray * self.rng.next_f32()) as f64 * 0.001 * self.rate as f64;
        let need = if speed > 0.0 { len * (speed - 1.0).max(0.0) } else { 0.0 } + 2.0;
        let behind = behind.max(need).min(self.tape.len() as f64 - len * speed.abs() - 4.0);
        let start = self.tape.w as f64 - behind - if speed < 0.0 { -len * speed } else { 0.0 };
        let pan = self.spread * self.rng.bipolar();
        let a = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
        self.grains[slot] = Grain { on: true, pos: start, speed, len, age: 0.0, gl: a.cos() * std::f32::consts::SQRT_2, gr: a.sin() * std::f32::consts::SQRT_2 };
    }
}

impl Effect for Sandglass {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "size" => self.size = v.clamp(10.0, 500.0),
            "density" => self.density = v.clamp(1.0, 100.0),
            "position" => self.position = v.clamp(0.0, 2000.0),
            "spray" => self.spray = v.clamp(0.0, 1000.0),
            "pitch" => self.pitch = v,
            "pitch_rand" => self.pitch_rand = v.clamp(0.0, 12.0),
            "reverse" => self.reverse = v.clamp(0.0, 100.0) / 100.0,
            "spread" => self.spread = v.clamp(0.0, 100.0) / 100.0,
            "feedback" => self.feedback = v.clamp(0.0, 90.0) / 100.0,
            "freeze" => self.freeze = v >= 0.5,
            "mix" => self.mix = v.clamp(0.0, 100.0) / 100.0,
            "output" => self.out = db_to_gain(v),
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.tape.clear();
        self.grains = [Grain::default(); MAX_GRAINS];
        self.fb = [0.0; 2];
    }

    fn telemetry(&self) -> &[f32] {
        &self.tele
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        // Overlapping grains add up: normalise by the expected overlap (density × size), at least 1.
        let overlap = (self.density * self.size * 0.001).max(1.0);
        let norm = 1.0 / overlap.sqrt();
        let period = self.rate as f64 / self.density as f64;
        for i in 0..l.len() {
            let (xl, xr) = (l[i], r[i]);
            if !self.freeze {
                // The feedback is soft-limited so stacked grains can never run away.
                self.tape.write(xl + (self.fb[0] * self.feedback).tanh(), xr + (self.fb[1] * self.feedback).tanh());
            }
            self.until_next -= 1.0;
            if self.until_next <= 0.0 {
                // Slightly irregular spacing (±25 %) avoids a buzz at the grain rate.
                self.until_next += period * (0.75 + 0.5 * self.rng.next_f32() as f64);
                self.spawn();
            }
            let (mut wl, mut wr) = (0.0f32, 0.0f32);
            for g in self.grains.iter_mut().filter(|g| g.on) {
                let u = g.age / g.len;
                // Hann window.
                let w = 0.5 - 0.5 * (std::f64::consts::TAU * u).cos();
                let s = self.tape.at(g.pos);
                wl += s[0] * w as f32 * g.gl;
                wr += s[1] * w as f32 * g.gr;
                g.pos += g.speed;
                g.age += 1.0;
                if g.age >= g.len {
                    g.on = false;
                }
            }
            wl *= norm;
            wr *= norm;
            self.fb = [wl, wr];
            l[i] = (xl + (wl - xl) * self.mix) * self.out;
            r[i] = (xr + (wr - xr) * self.mix) * self.out;
        }
        let w = self.tape.w as f64;
        let n = self.tape.len() as f64;
        for (t, g) in self.tele.iter_mut().zip(&self.grains) {
            // Each grain: how far behind the write head it reads, in seconds (negative = idle).
            *t = if g.on { ((w - g.pos).rem_euclid(n) / self.rate as f64) as f32 } else { -1.0 };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tape_stop_slows_to_silence_and_comes_back() {
        let rate = 48000.0;
        let mut p = Petrify::new(rate);
        p.set_param("stop_time", 200.0);
        p.set_param("start_time", 50.0);
        let n = 48000;
        let tone: Vec<f32> = (0..n).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        let mut l = tone.clone();
        let mut r = tone.clone();
        p.set_param("stop", 1.0);
        p.process(&mut l[..19200], &mut r[..19200], None);
        assert!(l[15000..19200].iter().all(|x| x.abs() < 1e-4), "silent once stopped");
        p.set_param("stop", 0.0);
        p.process(&mut l[19200..], &mut r[19200..], None);
        assert_eq!(p.state, TapeState::Running);
        assert!((l[n - 1] - tone[n - 1]).abs() < 1e-5, "back to the live input");
    }

    #[test]
    fn stutter_repeats_the_first_slice() {
        let rate = 48000.0;
        let mut h = Hex::new(rate);
        h.set_param("mode", 1.0);
        h.set_param("gate", 100.0);
        h.set_tempo(120.0);
        h.set_position(0.0, true);
        // 1/16 at 120 BPM = 6000 samples; a ramp makes slices distinguishable.
        let n = 24000;
        let ramp: Vec<f32> = (0..n).map(|i| i as f32 / n as f32).collect();
        let mut l = ramp.clone();
        let mut r = ramp.clone();
        h.process(&mut l, &mut r, None);
        // The second slice (repeat 1) replays the first: sample 6000 + 3000 ≈ ramp[3000].
        assert!((l[9000] - ramp[3000]).abs() < 0.02, "{} vs {}", l[9000], ramp[3000]);
    }

    #[test]
    fn granular_stays_bounded() {
        let rate = 48000.0;
        let mut g = Sandglass::new(rate);
        g.set_param("density", 100.0);
        g.set_param("size", 500.0);
        g.set_param("feedback", 90.0);
        g.set_param("mix", 100.0);
        let n = 96000;
        let mut l: Vec<f32> = (0..n).map(|i| (i as f32 * 0.03).sin() * 0.8).collect();
        let mut r = l.clone();
        for (a, b) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
            g.process(a, b, None);
        }
        let peak = l.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(peak.is_finite() && peak < 8.0, "{peak}");
    }
}
