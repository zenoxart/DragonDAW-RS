//! **Ouroboros** – an auto-panner: the sound moves across the
//! stereo field following a shape, locked to the song's bars (Sync), at a free rate (Free), or one step per
//! hit (Trigger: every transient over the threshold sends it to its next position, gliding there –
//! signature). Width, centre and phase place the motion; Smooth rounds its corners; the pan law keeps a
//! stereo source's image (Balance) or pans its mono sum with constant power; Binaural adds the interaural
//! delay and head shadow of the far ear for headphones.

use super::{Effect, EffectDef, ParamInfo};
use crate::dsp::db_to_gain;
use std::f32::consts::{FRAC_PI_4, PI, SQRT_2, TAU};

pub const ORBIT_MODES: &[&str] = &["Sync", "Free", "Trigger"];
pub const ORBIT_SYNC: &[&str] = &["1/16", "1/8 triplet", "1/8", "1/8 dotted", "1/4 triplet", "1/4", "1/4 dotted", "1/2", "1 bar", "2 bars", "4 bars"];
const ORBIT_BEATS: [f64; 11] = [0.25, 1.0 / 3.0, 0.5, 0.75, 2.0 / 3.0, 1.0, 1.5, 2.0, 4.0, 8.0, 16.0];
pub const ORBIT_SHAPES: &[&str] = &["Sine", "Triangle", "Square", "Saw up", "Saw down", "Random steps", "Smooth random"];
pub const ORBIT_LAWS: &[&str] = &["Equal power (mono)", "Balance (stereo)"];

const PARAMS: &[ParamInfo] = &[
    ParamInfo::choice("mode", "Mode", ORBIT_MODES, 0.0),
    ParamInfo::choice("sync", "Sync rate", ORBIT_SYNC, 7.0),
    ParamInfo::log("rate", "Free rate", 0.05, 20.0, 1.0, "Hz"),
    ParamInfo::choice("shape", "Shape", ORBIT_SHAPES, 0.0),
    ParamInfo::lin("width", "Width", 0.0, 100.0, 70.0, "%"),
    ParamInfo::lin("center", "Centre", -100.0, 100.0, 0.0, "%"),
    ParamInfo::lin("phase", "Phase", 0.0, 360.0, 0.0, "°"),
    ParamInfo::lin("smooth", "Smooth", 0.0, 100.0, 10.0, "%"),
    ParamInfo::lin("threshold", "Trigger threshold", -60.0, 0.0, -24.0, "dB"),
    ParamInfo::log("glide", "Trigger glide", 5.0, 2000.0, 120.0, "ms"),
    ParamInfo::choice("law", "Pan law", ORBIT_LAWS, 0.0),
    ParamInfo::toggle("binaural", "Binaural", false),
];

/// Interaural delay of a source at the side (s).
const ITD: f32 = 0.00065;
const RING: usize = 512;

/// A random value in -1 … 1 for cycle `n` (the same cycle always gives the same value).
fn hash(n: i64) -> f32 {
    let mut z = (n as u64).wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u64 << 23) as f32 - 1.0
}

/// The shape's value (-1 … 1) at `phase` (0 … 1) of cycle `n`.
pub fn shape_value(shape: usize, phase: f32, n: i64) -> f32 {
    let ph = phase.rem_euclid(1.0);
    match shape {
        0 => (TAU * ph).sin(),
        1 => 1.0 - 4.0 * (((ph + 0.25) % 1.0) - 0.5).abs(),
        2 => {
            if ph < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        3 => 2.0 * ph - 1.0,
        4 => 1.0 - 2.0 * ph,
        5 => hash(n),
        _ => {
            let (a, b) = (hash(n), hash(n + 1));
            a + (b - a) * (0.5 - 0.5 * (PI * ph).cos())
        }
    }
}

pub struct Orbit {
    rate: f32,
    bpm: f64,
    mode: usize,
    sync: usize,
    hz: f32,
    shape: usize,
    width: f32,
    center: f32,
    phase_off: f32,
    smooth: f32,
    threshold: f32,
    glide: f32,
    law: usize,
    binaural: bool,
    /// Song position at the next block and whether it plays.
    beat: f64,
    playing: bool,
    /// Free-running cycles (Free mode, and Sync while stopped).
    cycles: f64,
    pos: f32,
    /// Trigger mode: envelope, armed, samples since the last step, steps taken, target.
    env: f32,
    armed: bool,
    since: usize,
    steps: i64,
    target: f32,
    flash: f32,
    ring: [[f32; RING]; 2],
    w: usize,
    shade: [f32; 2],
    tele: [f32; 3],
}

impl Orbit {
    pub fn new(rate: f32) -> Self {
        Orbit {
            rate,
            bpm: 120.0,
            mode: 0,
            sync: 7,
            hz: 1.0,
            shape: 0,
            width: 0.7,
            center: 0.0,
            phase_off: 0.0,
            smooth: 0.1,
            threshold: -24.0,
            glide: 120.0,
            law: 0,
            binaural: false,
            beat: 0.0,
            playing: false,
            cycles: 0.0,
            pos: 0.0,
            env: 0.0,
            armed: true,
            since: usize::MAX / 2,
            steps: 0,
            target: 0.0,
            flash: 0.0,
            ring: [[0.0; RING]; 2],
            w: 0,
            shade: [0.0; 2],
            tele: [0.0; 3],
        }
    }

    fn cycle_beats(&self) -> f64 {
        ORBIT_BEATS[self.sync.min(ORBIT_BEATS.len() - 1)]
    }

    /// Seconds per cycle in the current mode.
    fn period(&self) -> f32 {
        match self.mode {
            1 => 1.0 / self.hz.max(0.01),
            _ => (self.cycle_beats() * 60.0 / self.bpm.max(1.0)) as f32,
        }
    }

    /// Where the shape puts the sound for a cycle position.
    fn place(&self, v: f32) -> f32 {
        (self.center + self.width * v).clamp(-1.0, 1.0)
    }

    /// Delayed (fractional) read from channel `ch`'s ring, `d` samples back.
    #[inline]
    fn tap(&self, ch: usize, d: f32) -> f32 {
        let d = d.clamp(0.0, (RING - 2) as f32);
        let i = d.floor();
        let fr = d - i;
        let a = self.ring[ch][(self.w + RING - i as usize) % RING];
        let b = self.ring[ch][(self.w + RING - i as usize - 1) % RING];
        a + (b - a) * fr
    }
}

impl Effect for Orbit {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "mode" => self.mode = (v.round() as usize).min(2),
            "sync" => self.sync = (v.round() as usize).min(ORBIT_SYNC.len() - 1),
            "rate" => self.hz = v,
            "shape" => self.shape = (v.round() as usize).min(ORBIT_SHAPES.len() - 1),
            "width" => self.width = v / 100.0,
            "center" => self.center = v / 100.0,
            "phase" => self.phase_off = v / 360.0,
            "smooth" => self.smooth = v / 100.0,
            "threshold" => self.threshold = v,
            "glide" => self.glide = v,
            "law" => self.law = (v.round() as usize).min(1),
            "binaural" => self.binaural = v >= 0.5,
            _ => {}
        }
    }
    fn set_tempo(&mut self, bpm: f64) {
        self.bpm = bpm;
    }
    fn set_position(&mut self, beat: f64, playing: bool) {
        self.beat = beat;
        self.playing = playing;
    }
    fn reset(&mut self) {
        self.cycles = 0.0;
        self.pos = self.center;
        self.env = 0.0;
        self.armed = true;
        self.steps = 0;
        self.target = self.center;
        self.ring = [[0.0; RING]; 2];
        self.shade = [0.0; 2];
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let n = l.len();
        let rate = self.rate;
        let sync = self.mode == 0 && self.playing && self.beat >= 0.0;
        let beats_per_sample = self.bpm / 60.0 / rate as f64;
        let cyc_per_sample = match self.mode {
            1 => self.hz as f64 / rate as f64,
            _ => beats_per_sample / self.cycle_beats(),
        };
        // Smoothing: at least 2 ms (no clicks on squares), up to a quarter cycle; Trigger glides.
        let tau = if self.mode == 2 { self.glide * 0.001 } else { 0.002 + self.smooth * 0.25 * self.period() };
        let k = 1.0 - (-1.0 / (tau.max(0.0005) * rate)).exp();
        let thr = db_to_gain(self.threshold);
        let rearm = db_to_gain(self.threshold - 3.0);
        let (att, rel) = ((-1.0 / (0.0005 * rate)).exp(), (-1.0 / (0.04 * rate)).exp());
        let min_gap = (0.03 * rate) as usize;
        let mut ph = 0.0;
        let mut shade_k = 1.0;
        for i in 0..n {
            // ---- where the sound should be
            let want = if self.mode == 2 {
                let x = l[i].abs().max(r[i].abs());
                let c = if x > self.env { att } else { rel };
                self.env = x + c * (self.env - x);
                self.since = self.since.saturating_add(1);
                if self.armed && self.env > thr && self.since >= min_gap {
                    self.armed = false;
                    self.since = 0;
                    self.steps += 1;
                    self.flash = 1.0;
                    // Square ping-pongs between the sides; the others walk their shape in eighths.
                    let step = if self.shape == 2 { 0.5 } else { 0.125 };
                    let p = self.phase_off + self.steps as f32 * step;
                    self.target = self.place(shape_value(self.shape, p, self.steps));
                } else if self.env < rearm {
                    self.armed = true;
                }
                self.target
            } else {
                let c = if sync { (self.beat + i as f64 * beats_per_sample) / self.cycle_beats() } else { self.cycles + i as f64 * cyc_per_sample };
                let c = c + self.phase_off as f64;
                let cycle = c.floor();
                ph = (c - cycle) as f32;
                self.place(shape_value(self.shape, ph, cycle as i64))
            };
            self.pos += (want - self.pos) * k;
            let p = self.pos;
            // ---- pan (binaural: both ears hear the mono source; the far one quieter, later and darker)
            let (mut a, mut b) = if self.binaural {
                let m = 0.5 * (l[i] + r[i]);
                let far = m * (1.0 - 0.6 * p.abs());
                if p > 0.0 {
                    (far, m)
                } else {
                    (m, far)
                }
            } else if self.law == 0 {
                let m = 0.5 * (l[i] + r[i]);
                let th = (p + 1.0) * FRAC_PI_4;
                (m * th.cos() * SQRT_2, m * th.sin() * SQRT_2)
            } else {
                (l[i] * (1.0 - p).min(1.0), r[i] * (1.0 + p).min(1.0))
            };
            if self.binaural {
                // Interaural delay and head shadow on the far ear.
                self.ring[0][self.w] = a;
                self.ring[1][self.w] = b;
                let d = ITD * p.abs() * rate;
                if i % 8 == 0 {
                    let fc = 20000.0 * 0.12f32.powf(p.abs());
                    shade_k = 1.0 - (-TAU * fc.min(rate * 0.45) / rate).exp();
                }
                let far = if p > 0.0 { 0 } else { 1 };
                let x = self.tap(far, d);
                self.shade[far] += (x - self.shade[far]) * shade_k;
                let near = 1 - far;
                self.shade[near] = if near == 0 { a } else { b };
                if far == 0 {
                    a = self.shade[0];
                } else {
                    b = self.shade[1];
                }
                self.w = (self.w + 1) % RING;
            }
            l[i] = a;
            r[i] = b;
        }
        if !sync {
            self.cycles = (self.cycles + n as f64 * cyc_per_sample) % 1.0e6;
        }
        self.flash *= (-(n as f32) / (0.15 * rate)).exp();
        self.tele = [self.pos, self.flash, ph];
    }
    fn telemetry(&self) -> &[f32] {
        &self.tele
    }
}

pub fn orbit_def() -> EffectDef {
    EffectDef {
        id: "orbit",
        name: "Ouroboros (auto-panner)",
        category: "Modulation",
        description: "Auto-panner: moves the sound across the stereo field locked to the bars, at a free rate, or one step per hit (Trigger, with glide). Seven shapes incl. random, width / centre / phase, smoothing, constant-power or balance pan law, binaural mode for headphones.",
        params: PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(Orbit::new(rate)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(n: usize) -> Vec<f32> {
        (0..n).map(|i| (i as f32 * 0.05).sin() * 0.5).collect()
    }

    /// Where the energy sits: -1 (left) … +1 (right).
    fn balance(l: &[f32], r: &[f32]) -> f32 {
        let (el, er): (f32, f32) = (l.iter().map(|x| x * x).sum(), r.iter().map(|x| x * x).sum());
        (er - el) / (er + el).max(1e-12)
    }

    /// Sync mode follows the song position: a 1-bar sine is centred on beats 0 and 2, hard right on
    /// beat 1 and hard left on beat 3, wherever playback starts.
    #[test]
    fn sync_locks_to_the_bar() {
        let rate = 48000.0;
        let mut o = Orbit::new(rate);
        o.set_param("sync", 8.0); // 1 bar
        o.set_param("width", 100.0);
        o.set_param("smooth", 0.0);
        o.set_tempo(120.0);
        let at = |o: &mut Orbit, beat: f64| {
            o.reset();
            // Settle the 2 ms smoothing on a short block, then measure 10 ms.
            o.set_position(beat - 0.02, true);
            let s = tone(960);
            let (mut l, mut r) = (s.clone(), s.clone());
            o.process(&mut l, &mut r, None);
            o.set_position(beat, true);
            let (mut l, mut r) = (tone(480), tone(480));
            o.process(&mut l, &mut r, None);
            balance(&l, &r)
        };
        assert!(at(&mut o, 1.0) > 0.95, "beat 1: right");
        assert!(at(&mut o, 3.0) < -0.95, "beat 3: left");
        assert!(at(&mut o, 8.0).abs() < 0.1, "beat 8 (two bars later): centre");
        assert!(at(&mut o, 13.0) > 0.95, "beat 13: right again");
    }

    /// Trigger mode moves once per hit: square ping-pongs right, left, right.
    #[test]
    fn trigger_steps_once_per_hit() {
        let rate = 48000.0;
        let mut o = Orbit::new(rate);
        o.set_param("mode", 2.0);
        o.set_param("shape", 2.0);
        o.set_param("width", 100.0);
        o.set_param("glide", 5.0);
        o.set_param("threshold", -20.0);
        o.reset();
        let mut sides = Vec::new();
        for _hit in 0..3 {
            // A 60 ms burst, then 200 ms of quiet.
            let mut l: Vec<f32> = (0..2880).map(|i| (i as f32 * 0.07).sin() * 0.5).chain(std::iter::repeat_n(0.0, 9600)).collect();
            let mut r = l.clone();
            o.process(&mut l, &mut r, None);
            sides.push(balance(&l[1200..2880], &r[1200..2880]));
        }
        assert!(sides[0] < -0.9 && sides[1] > 0.9 && sides[2] < -0.9, "{sides:?}");
        assert_eq!(o.steps, 3);
    }

    /// Centre with zero width is transparent for a mono source (constant power: unity at the centre).
    #[test]
    fn centre_is_unity() {
        let mut o = Orbit::new(48000.0);
        o.set_param("width", 0.0);
        o.reset();
        let s = tone(4800);
        let (mut l, mut r) = (s.clone(), s.clone());
        o.process(&mut l, &mut r, None);
        let d = s.iter().zip(&l).chain(s.iter().zip(&r)).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(d < 1e-5, "{d}");
    }

    /// Binaural: hard right delays the left ear by ~0.65 ms, darkens it and keeps it audible (-8 dB).
    #[test]
    fn binaural_delays_the_far_ear() {
        let rate = 48000.0;
        let mut o = Orbit::new(rate);
        o.set_param("width", 0.0);
        o.set_param("center", 100.0);
        o.set_param("binaural", 1.0);
        o.reset();
        o.pos = 1.0;
        let mut l = vec![0.0f32; 400];
        l[10] = 1.0;
        let mut r = l.clone();
        o.process(&mut l, &mut r, None);
        let peak = |v: &[f32]| v.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).map(|x| x.0).unwrap();
        let lag = peak(&l) as i64 - peak(&r) as i64;
        assert!((25..=40).contains(&lag), "left ear lags by {lag} samples");
        let (el, er): (f32, f32) = (l.iter().map(|x| x * x).sum(), r.iter().map(|x| x * x).sum());
        assert!(el > er * 0.02 && el < er * 0.4, "far ear quieter but there: {el} vs {er}");
    }
}
