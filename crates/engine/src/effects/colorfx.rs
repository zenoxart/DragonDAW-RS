//! Grimoire (lo-fi: tape wow and flutter, age, hiss, vinyl crackle, bit and rate reduction), Wisp (harmonic
//! exciter) and Familiar (auto-filter with LFO and envelope).

use super::{Effect, EffectDef, ParamInfo};
use crate::dsp::{db_to_gain, Biquad, BiquadKind, DelayLine, Rng, Svf};

// ------------------------------------------------------------------------------------------------- lo-fi

const LOFI_PARAMS: &[ParamInfo] = &[
    ParamInfo::lin("wow", "Wow", 0.0, 100.0, 25.0, "%"),
    ParamInfo::lin("flutter", "Flutter", 0.0, 100.0, 15.0, "%"),
    ParamInfo::lin("age", "Age", 0.0, 100.0, 40.0, "%"),
    ParamInfo::lin("drive", "Drive", 0.0, 100.0, 20.0, "%"),
    ParamInfo::lin("hiss", "Hiss", -90.0, -30.0, -66.0, "dB"),
    ParamInfo::lin("crackle", "Crackle", 0.0, 100.0, 20.0, "%"),
    ParamInfo::lin("bits", "Bits", 4.0, 24.0, 24.0, "bit"),
    ParamInfo::log("sample_rate", "Sample rate", 2000.0, 48000.0, 48000.0, "Hz"),
    ParamInfo::lin("mix", "Mix", 0.0, 100.0, 100.0, "%"),
    ParamInfo::lin("output", "Output", -24.0, 12.0, 0.0, "dB"),
];

pub fn lofi_def() -> EffectDef {
    EffectDef {
        id: "lofi",
        name: "Grimoire (lo-fi)",
        category: "Distortion",
        description: "Lo-fi ageing: tape wow and flutter, a narrowing worn-out band, saturation, tape hiss, vinyl crackle, and bit-depth and sample-rate reduction.",
        params: LOFI_PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(Grimoire::new(rate)),
    }
}

/// Centre of the wow / flutter delay (ms): the modulation swings around it.
const LOFI_BASE_MS: f32 = 6.0;

pub struct Grimoire {
    rate: f32,
    wow: f32,
    flutter: f32,
    age: f32,
    drive: f32,
    hiss: f32,
    crackle: f32,
    bits: f32,
    sr: f32,
    mix: f32,
    out: f32,
    delay: [DelayLine; 2],
    wow_ph: f64,
    flut_ph: f64,
    drift: f32,
    hp: [Biquad; 2],
    lp: [Biquad; 2],
    lp2: [Biquad; 2],
    rng: Rng,
    crackle_env: f32,
    hold: [f32; 2],
    hold_t: f32,
    tele: [f32; 2],
}

impl Grimoire {
    pub fn new(rate: f32) -> Self {
        let n = ((LOFI_BASE_MS * 2.5) * 0.001 * rate) as usize + 4;
        let mut g = Grimoire {
            rate,
            wow: 0.25,
            flutter: 0.15,
            age: 0.4,
            drive: 0.2,
            hiss: db_to_gain(-66.0),
            crackle: 0.2,
            bits: 24.0,
            sr: 48000.0,
            mix: 1.0,
            out: 1.0,
            delay: [DelayLine::new(n), DelayLine::new(n)],
            wow_ph: 0.0,
            flut_ph: 0.0,
            drift: 0.0,
            hp: [Biquad::default(); 2],
            lp: [Biquad::default(); 2],
            lp2: [Biquad::default(); 2],
            rng: Rng::new(0x9a1),
            crackle_env: 0.0,
            hold: [0.0; 2],
            hold_t: 1.0,
            tele: [0.0; 2],
        };
        g.tone();
        g
    }

    /// Age narrows the band: high-pass 20 → 220 Hz, low-pass 20 → 3.5 kHz (exponentially).
    fn tone(&mut self) {
        let hp = 20.0 * 11f32.powf(self.age);
        let lp = (20_000.0 * (3500.0f32 / 20_000.0).powf(self.age)).min(self.rate * 0.45);
        for c in 0..2 {
            self.hp[c].set(BiquadKind::HighPass, hp, 0.6, 0.0, self.rate);
            self.lp[c].set(BiquadKind::LowPass, lp, 0.7, 0.0, self.rate);
            self.lp2[c].set(BiquadKind::LowPass, lp * 1.2, 0.6, 0.0, self.rate);
        }
    }
}

impl Effect for Grimoire {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "wow" => self.wow = v.clamp(0.0, 100.0) / 100.0,
            "flutter" => self.flutter = v.clamp(0.0, 100.0) / 100.0,
            "age" => {
                self.age = v.clamp(0.0, 100.0) / 100.0;
                self.tone();
            }
            "drive" => self.drive = v.clamp(0.0, 100.0) / 100.0,
            "hiss" => self.hiss = if v <= -89.5 { 0.0 } else { db_to_gain(v) },
            "crackle" => self.crackle = v.clamp(0.0, 100.0) / 100.0,
            "bits" => self.bits = v.clamp(4.0, 24.0),
            "sample_rate" => self.sr = v.clamp(2000.0, 48000.0),
            "mix" => self.mix = v.clamp(0.0, 100.0) / 100.0,
            "output" => self.out = db_to_gain(v),
            _ => {}
        }
    }

    fn reset(&mut self) {
        for d in self.delay.iter_mut() {
            d.clear();
        }
        self.tone();
    }

    fn latency(&self) -> usize {
        (LOFI_BASE_MS * 0.001 * self.rate) as usize
    }

    fn telemetry(&self) -> &[f32] {
        &self.tele
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let base = LOFI_BASE_MS * 0.001 * self.rate;
        // Wow 0.6 Hz up to ±2.4 ms, flutter 9 Hz up to ±0.25 ms, plus a slow random drift.
        let wow_amp = self.wow * 2.4e-3 * self.rate;
        let flut_amp = self.flutter * 0.25e-3 * self.rate;
        let drive = 1.0 + 6.0 * self.drive;
        let levels = 2f32.powf(self.bits - 1.0);
        let step = (self.rate / self.sr).max(1.0);
        let mut mod_d = 0.0;
        for i in 0..l.len() {
            self.wow_ph = (self.wow_ph + 0.6 / self.rate as f64).fract();
            self.flut_ph = (self.flut_ph + 9.0 / self.rate as f64).fract();
            self.drift += (self.rng.bipolar() * 0.5 - self.drift) * 2e-5;
            mod_d = wow_amp * ((std::f64::consts::TAU * self.wow_ph).sin() as f32 + self.drift) + flut_amp * (std::f64::consts::TAU * self.flut_ph).sin() as f32;
            let d = (base + mod_d).clamp(1.0, base * 2.4);
            // Crackle: random clicks (density by the knob), each a short decaying burst of noise.
            if self.rng.next_f32() < self.crackle * self.crackle * 40.0 / self.rate {
                self.crackle_env = 0.3 + 0.7 * self.rng.next_f32();
            }
            self.crackle_env *= 0.93;
            let crack = self.crackle_env * self.rng.bipolar() * 0.35;
            // Sample-rate reduction: hold the value for `step` samples.
            self.hold_t += 1.0;
            let take = self.hold_t >= step;
            if take {
                self.hold_t -= step;
            }
            for (c, x) in [&mut l[i], &mut r[i]].into_iter().enumerate() {
                let dry = *x;
                self.delay[c].write(dry);
                let mut v = self.delay[c].read(d);
                v = self.hp[c].process(v);
                v = self.lp2[c].process(self.lp[c].process(v));
                if self.drive > 0.0 {
                    v = (v * drive).tanh() / drive.sqrt();
                }
                if self.bits < 23.5 {
                    v = (v * levels).round() / levels;
                }
                if step > 1.0 {
                    if take {
                        self.hold[c] = v;
                    }
                    v = self.hold[c];
                }
                v += self.hiss * self.rng.bipolar() + crack;
                *x = (dry + (v - dry) * self.mix) * self.out;
            }
        }
        self.tele = [mod_d / self.rate * 1000.0, self.crackle_env];
    }
}

// --------------------------------------------------------------------------------------------- exciter

const EXCITER_PARAMS: &[ParamInfo] = &[
    ParamInfo::log("freq", "Frequency", 800.0, 12000.0, 3000.0, "Hz"),
    ParamInfo::lin("drive", "Drive", 0.0, 100.0, 40.0, "%"),
    ParamInfo::lin("character", "Odd / even", -100.0, 100.0, 0.0, "%"),
    ParamInfo::lin("amount", "Amount", 0.0, 100.0, 30.0, "%"),
    ParamInfo::lin("air", "Air", 0.0, 12.0, 0.0, "dB"),
    ParamInfo::lin("sub", "Sub harmonics", 0.0, 100.0, 0.0, "%"),
    ParamInfo::toggle("listen", "Listen", false),
    ParamInfo::lin("output", "Output", -24.0, 12.0, 0.0, "dB"),
];

pub fn exciter_def() -> EffectDef {
    EffectDef {
        id: "exciter",
        name: "Wisp (exciter)",
        category: "Distortion",
        description: "Harmonic exciter: generates new harmonics above a chosen frequency (odd or even), adds air on top and, if asked, sub harmonics below the low end. Listen plays only what it adds.",
        params: EXCITER_PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(Wisp::new(rate)),
    }
}

pub struct Wisp {
    rate: f32,
    freq: f32,
    drive: f32,
    character: f32,
    amount: f32,
    air: f32,
    sub: f32,
    listen: bool,
    out: f32,
    pre: [[Biquad; 2]; 2],
    post: [[Biquad; 2]; 2],
    air_f: [Biquad; 2],
    sub_lp: [Biquad; 2],
    sub_smooth: [Biquad; 2],
    sub_flip: [f32; 2],
    sub_prev: [f32; 2],
    sub_env: [f32; 2],
    level: f32,
    tele: [f32; 1],
}

impl Wisp {
    pub fn new(rate: f32) -> Self {
        let mut w = Wisp {
            rate,
            freq: 3000.0,
            drive: 0.4,
            character: 0.0,
            amount: 0.3,
            air: 0.0,
            sub: 0.0,
            listen: false,
            out: 1.0,
            pre: Default::default(),
            post: Default::default(),
            air_f: Default::default(),
            sub_lp: Default::default(),
            sub_smooth: Default::default(),
            sub_flip: [1.0; 2],
            sub_prev: [0.0; 2],
            sub_env: [0.0; 2],
            level: 0.0,
            tele: [0.0],
        };
        w.tune();
        w
    }

    fn tune(&mut self) {
        let f = self.freq.min(self.rate * 0.4);
        for c in 0..2 {
            for s in self.pre[c].iter_mut() {
                s.set(BiquadKind::HighPass, f, 0.707, 0.0, self.rate);
            }
            // The distortion's own low products are removed by the same split above.
            for s in self.post[c].iter_mut() {
                s.set(BiquadKind::HighPass, f, 0.707, 0.0, self.rate);
            }
            self.air_f[c].set(BiquadKind::HighShelf, 12_000.0f32.min(self.rate * 0.4), 0.7, self.air, self.rate);
            self.sub_lp[c].set(BiquadKind::LowPass, 90.0, 0.707, 0.0, self.rate);
            self.sub_smooth[c].set(BiquadKind::LowPass, 70.0, 0.707, 0.0, self.rate);
        }
    }
}

impl Effect for Wisp {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "freq" => self.freq = v.clamp(800.0, 12000.0),
            "drive" => self.drive = v.clamp(0.0, 100.0) / 100.0,
            "character" => self.character = v.clamp(-100.0, 100.0) / 100.0,
            "amount" => self.amount = v.clamp(0.0, 100.0) / 100.0,
            "air" => self.air = v.clamp(0.0, 12.0),
            "sub" => self.sub = v.clamp(0.0, 100.0) / 100.0,
            "listen" => self.listen = v >= 0.5,
            "output" => self.out = db_to_gain(v),
            _ => return,
        }
        if matches!(id, "freq" | "air") {
            self.tune();
        }
    }

    fn reset(&mut self) {
        self.pre = Default::default();
        self.post = Default::default();
        self.air_f = Default::default();
        self.sub_lp = Default::default();
        self.sub_smooth = Default::default();
        self.tune();
    }

    fn telemetry(&self) -> &[f32] {
        &self.tele
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let g = 1.0 + 15.0 * self.drive;
        // Odd harmonics from tanh, even ones from the squared term; blended by the character.
        let odd = (1.0 - self.character) * 0.5;
        let even = (1.0 + self.character) * 0.5;
        let mut peak = 0.0f32;
        for (c, buf) in [&mut *l, &mut *r].into_iter().enumerate() {
            for x in buf.iter_mut() {
                let dry = *x;
                let h = {
                    let t = self.pre[c][0].process(dry);
                    self.pre[c][1].process(t)
                };
                let u = h * g;
                let shaped = odd * u.tanh() + even * 0.5 * (u * u).tanh();
                // Only what is new: the distorted band minus its fundamental share, high-passed again.
                let added = {
                    let t = self.post[c][0].process(shaped / g.sqrt() - h * 0.5);
                    self.post[c][1].process(t)
                };
                // Sub harmonics: a square at half the low band's frequency (flip-flop on its zero crossings),
                // following the low band's level.
                let lo = self.sub_lp[c].process(dry);
                if self.sub_prev[c] <= 0.0 && lo > 0.0 {
                    self.sub_flip[c] = -self.sub_flip[c];
                }
                self.sub_prev[c] = lo;
                self.sub_env[c] += (lo.abs() - self.sub_env[c]) * 0.002;
                // The square's edges are rounded off by a second low-pass (a sub, not a buzz).
                let sub = self.sub_smooth[c].process(self.sub_flip[c] * self.sub_env[c] * 1.4 * self.sub);
                let extra = added * self.amount * 2.0 + sub;
                let y = if self.listen { extra } else { self.air_f[c].process(dry + extra) };
                *x = y * self.out;
                peak = peak.max(extra.abs());
            }
        }
        self.level = self.level * 0.9 + peak * 0.1;
        self.tele = [self.level];
    }
}

// ------------------------------------------------------------------------------------------- auto-filter

pub const FILTER_TYPES: &[&str] = &["Low-pass", "Band-pass", "High-pass", "Notch"];
pub const SYNCS: &[&str] = &["Free", "1/16", "1/8", "1/4", "1/2", "1 bar", "2 bars", "4 bars"];
const SYNC_BEATS: [f64; 8] = [0.0, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0];
pub const LFO_SHAPES: &[&str] = &["Sine", "Triangle", "Saw up", "Saw down", "Square", "Random"];

const AUTOFILTER_PARAMS: &[ParamInfo] = &[
    ParamInfo::choice("type", "Type", FILTER_TYPES, 0.0),
    ParamInfo::log("cutoff", "Cutoff", 30.0, 18000.0, 800.0, "Hz"),
    ParamInfo::lin("resonance", "Resonance", 0.0, 95.0, 30.0, "%"),
    ParamInfo::lin("drive", "Drive", 0.0, 100.0, 0.0, "%"),
    ParamInfo::choice("sync", "LFO sync", SYNCS, 3.0),
    ParamInfo::log("lfo_rate", "LFO rate", 0.02, 20.0, 1.0, "Hz"),
    ParamInfo::choice("lfo_shape", "LFO shape", LFO_SHAPES, 0.0),
    ParamInfo::lin("lfo_depth", "LFO depth", 0.0, 5.0, 2.0, "oct"),
    ParamInfo::lin("stereo", "Stereo phase", 0.0, 180.0, 0.0, "°"),
    ParamInfo::lin("env_amount", "Envelope", -5.0, 5.0, 0.0, "oct"),
    ParamInfo::log("env_attack", "Env attack", 0.5, 200.0, 5.0, "ms"),
    ParamInfo::log("env_release", "Env release", 10.0, 2000.0, 200.0, "ms"),
    ParamInfo::lin("mix", "Mix", 0.0, 100.0, 100.0, "%"),
    ParamInfo::lin("output", "Output", -24.0, 12.0, 0.0, "dB"),
];

pub fn autofilter_def() -> EffectDef {
    EffectDef {
        id: "autofilter",
        name: "Familiar (auto-filter)",
        category: "Filter",
        description:
            "Auto-filter: a resonant low-, band-, high-pass or notch whose cutoff an LFO (free or synced, six shapes, stereo phase) and an envelope follower (positive or negative, from the input or the sidechain) move.",
        params: AUTOFILTER_PARAMS,
        uses_sidechain: true,
        create: |rate| Box::new(Familiar::new(rate)),
    }
}

pub struct Familiar {
    rate: f32,
    kind: usize,
    cutoff: f32,
    reso: f32,
    drive: f32,
    sync: usize,
    lfo_rate: f32,
    shape: usize,
    depth: f32,
    stereo: f32,
    env_amt: f32,
    env_a: f32,
    env_r: f32,
    mix: f32,
    out: f32,
    bpm: f64,
    beat: f64,
    playing: bool,
    phase: f64,
    env: f32,
    svf: [Svf; 2],
    rng: Rng,
    sh: [f32; 2],
    last_cycle: [i64; 2],
    lfo_s: [f32; 2],
    lfo_c: f32,
    tele: [f32; 3],
}

impl Familiar {
    pub fn new(rate: f32) -> Self {
        let mut f = Familiar {
            rate,
            kind: 0,
            cutoff: 800.0,
            reso: 0.3,
            drive: 0.0,
            sync: 3,
            lfo_rate: 1.0,
            shape: 0,
            depth: 2.0,
            stereo: 0.0,
            env_amt: 0.0,
            env_a: 0.0,
            env_r: 0.0,
            mix: 1.0,
            out: 1.0,
            bpm: 120.0,
            beat: 0.0,
            playing: false,
            phase: 0.0,
            env: 0.0,
            svf: [Svf::default(); 2],
            rng: Rng::new(0xfa),
            sh: [0.0; 2],
            last_cycle: [-1; 2],
            lfo_s: [0.0; 2],
            lfo_c: (-1.0 / (0.002 * rate)).exp(),
            tele: [0.0; 3],
        };
        f.set_param("env_attack", 5.0);
        f.set_param("env_release", 200.0);
        f
    }

    /// The LFO value in −1 … 1 at phase `p`.
    fn lfo(&mut self, p: f64, side: usize) -> f32 {
        let p = p.rem_euclid(1.0) as f32;
        match self.shape {
            0 => (std::f32::consts::TAU * p).sin(),
            1 => 1.0 - 4.0 * (p - 0.5).abs(),
            2 => 2.0 * p - 1.0,
            3 => 1.0 - 2.0 * p,
            4 => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            _ => self.sh[side],
        }
    }
}

impl Effect for Familiar {
    fn set_param(&mut self, id: &str, v: f32) {
        let coef = |ms: f32, rate: f32| (-1.0 / (ms * 0.001 * rate)).exp();
        match id {
            "type" => self.kind = (v.round().max(0.0) as usize).min(3),
            "cutoff" => self.cutoff = v.clamp(30.0, 18000.0),
            "resonance" => self.reso = v.clamp(0.0, 95.0) / 100.0,
            "drive" => self.drive = v.clamp(0.0, 100.0) / 100.0,
            "sync" => self.sync = (v.round().max(0.0) as usize).min(SYNCS.len() - 1),
            "lfo_rate" => self.lfo_rate = v.clamp(0.02, 20.0),
            "lfo_shape" => self.shape = (v.round().max(0.0) as usize).min(LFO_SHAPES.len() - 1),
            "lfo_depth" => self.depth = v.clamp(0.0, 5.0),
            "stereo" => self.stereo = v.clamp(0.0, 180.0),
            "env_amount" => self.env_amt = v.clamp(-5.0, 5.0),
            "env_attack" => self.env_a = coef(v.clamp(0.5, 200.0), self.rate),
            "env_release" => self.env_r = coef(v.clamp(10.0, 2000.0), self.rate),
            "mix" => self.mix = v.clamp(0.0, 100.0) / 100.0,
            "output" => self.out = db_to_gain(v),
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.svf = [Svf::default(); 2];
        self.env = 0.0;
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

    fn process(&mut self, l: &mut [f32], r: &mut [f32], sc: Option<(&[f32], &[f32])>) {
        let bps = self.bpm / 60.0 / self.rate as f64;
        let synced = self.sync > 0 && self.playing;
        let drive = 1.0 + 8.0 * self.drive;
        let off = self.stereo as f64 / 360.0;
        let mut fc_shown = self.cutoff;
        for i in 0..l.len() {
            let ph = if synced {
                (self.beat + i as f64 * bps) / SYNC_BEATS[self.sync]
            } else {
                let rate = if self.sync > 0 { self.bpm / 60.0 / SYNC_BEATS[self.sync] } else { self.lfo_rate as f64 };
                self.phase = (self.phase + rate / self.rate as f64).rem_euclid(1.0);
                self.phase
            };
            let det = match sc {
                Some((a, b)) => a[i].abs().max(b[i].abs()),
                None => l[i].abs().max(r[i].abs()),
            };
            self.env = if det > self.env { det + (self.env - det) * self.env_a } else { det + (self.env - det) * self.env_r };
            let env_oct = self.env_amt * (self.env * 4.0).min(1.0);
            for (c, x) in [&mut l[i], &mut r[i]].into_iter().enumerate() {
                let p = ph + if c == 1 { off } else { 0.0 };
                // Sample-and-hold: a new random value per cycle.
                let cyc = p.floor() as i64;
                if cyc != self.last_cycle[c] {
                    self.last_cycle[c] = cyc;
                    self.sh[c] = self.rng.bipolar();
                }
                // The LFO's jumps (saw edges, square, random steps) are rounded over ~2 ms, so the cutoff
                // never steps (a stepping resonant filter clicks).
                let raw = self.lfo(p, c);
                self.lfo_s[c] = raw + (self.lfo_s[c] - raw) * self.lfo_c;
                let oct = self.lfo_s[c] * self.depth * 0.5 + env_oct;
                let fc = (self.cutoff * 2f32.powf(oct)).clamp(20.0, self.rate * 0.45);
                if c == 0 {
                    fc_shown = fc;
                }
                let dry = *x;
                let input = if self.drive > 0.0 { (dry * drive).tanh() / drive.sqrt() } else { dry };
                let (lp, bp, hp) = self.svf[c].process(input, fc, self.reso, self.rate);
                let y = match self.kind {
                    0 => lp,
                    1 => bp,
                    2 => hp,
                    _ => lp + hp,
                };
                *x = (dry + (y - dry) * self.mix) * self.out;
            }
        }
        self.tele = [fc_shown, self.env, ph_frac(self.phase)];
    }
}

fn ph_frac(p: f64) -> f32 {
    p.rem_euclid(1.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lofi_age_darkens() {
        let rate = 48000.0;
        let energy_hi = |age: f32| {
            let mut g = Grimoire::new(rate);
            for (k, v) in [("wow", 0.0), ("flutter", 0.0), ("hiss", -90.0), ("crackle", 0.0), ("drive", 0.0), ("age", age)] {
                g.set_param(k, v);
            }
            let n = 24000;
            let mut l: Vec<f32> = (0..n).map(|i| (std::f32::consts::TAU * 8000.0 * i as f32 / rate).sin() * 0.5).collect();
            let mut r = l.clone();
            g.process(&mut l, &mut r, None);
            l[4000..].iter().map(|x| x * x).sum::<f32>()
        };
        assert!(energy_hi(90.0) < energy_hi(0.0) * 0.05);
    }

    #[test]
    fn exciter_adds_highs_only_above_its_frequency() {
        let rate = 48000.0;
        let mut w = Wisp::new(rate);
        w.set_param("listen", 1.0);
        w.set_param("amount", 100.0);
        let n = 24000;
        // A 200 Hz tone is far below the 3 kHz split: nothing is added.
        let mut l: Vec<f32> = (0..n).map(|i| (std::f32::consts::TAU * 200.0 * i as f32 / rate).sin() * 0.5).collect();
        let mut r = l.clone();
        w.process(&mut l, &mut r, None);
        let e: f32 = l[4000..].iter().map(|x| x * x).sum();
        assert!(e < 1.0, "{e}");
    }

    #[test]
    fn autofilter_sweeps() {
        let rate = 48000.0;
        let mut f = Familiar::new(rate);
        f.set_param("sync", 0.0);
        f.set_param("lfo_rate", 2.0);
        f.set_param("lfo_depth", 4.0);
        let mut seen = (f32::MAX, 0.0f32);
        let mut l = vec![0.1f32; 128];
        let mut r = vec![0.1f32; 128];
        for _ in 0..400 {
            f.process(&mut l, &mut r, None);
            seen = (seen.0.min(f.tele[0]), seen.1.max(f.tele[0]));
        }
        assert!(seen.1 / seen.0 > 3.0, "{seen:?}");
    }
}
