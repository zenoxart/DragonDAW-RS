//! Tidecaller: a volume shaper and ducker. A gain curve repeats in time with the song (pumping, trance gates,
//! swells), and/or a sidechain input ducks the signal when it hits (the kick pushing the bass down).
//!
//! The applied gain is `G = 1 − d·(1 − g)` for the curve value `g ∈ [0, 1]` and depth `d`, multiplied by the
//! ducking gain, then smoothed with a one-pole of the *Smooth* time so square shapes do not click.

use super::{Effect, EffectDef, ParamInfo};
use crate::dsp::db_to_gain;

pub const SHAPES: &[&str] = &["Pump", "Duck", "Gate", "Saw up", "Saw down", "Sine", "Triangle", "Swell"];
pub const RATES: &[&str] = &["1/32", "1/16", "1/8", "1/4", "1/2", "1 bar", "2 bars", "4 bars"];
/// Length of each rate in beats.
pub const RATE_BEATS: [f64; 8] = [0.125, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0];
pub const MODES: &[&str] = &["Shape", "Sidechain duck", "Both"];

const PARAMS: &[ParamInfo] = &[
    ParamInfo::choice("mode", "Mode", MODES, 0.0),
    ParamInfo::choice("shape", "Shape", SHAPES, 0.0),
    ParamInfo::choice("rate", "Rate", RATES, 3.0),
    ParamInfo::lin("depth", "Depth", 0.0, 100.0, 80.0, "%"),
    ParamInfo::lin("tension", "Tension", -100.0, 100.0, 0.0, "%"),
    ParamInfo::lin("offset", "Phase", 0.0, 100.0, 0.0, "%"),
    ParamInfo::lin("smooth", "Smooth", 0.1, 20.0, 2.0, "ms"),
    ParamInfo::lin("threshold", "Threshold", -60.0, 0.0, -24.0, "dB"),
    ParamInfo::log("attack", "Attack", 0.1, 50.0, 2.0, "ms"),
    ParamInfo::log("hold", "Hold", 1.0, 500.0, 30.0, "ms"),
    ParamInfo::log("release", "Release", 10.0, 2000.0, 200.0, "ms"),
    ParamInfo::lin("output", "Output", -24.0, 12.0, 0.0, "dB"),
];

pub fn volume_shaper_def() -> EffectDef {
    EffectDef {
        id: "volume_shaper",
        name: "Tidecaller (volume shaper)",
        category: "Modulation",
        description: "Volume shaper and ducker. A tempo-synced gain curve (pump, duck, gate, saws, sine, swell) and / or ducking from a sidechain input.",
        params: PARAMS,
        uses_sidechain: true,
        create: |rate| Box::new(VolumeShaper::new(rate)),
    }
}

/// The curve value `g(φ) ∈ [0, 1]` of shape `s` at phase `φ ∈ [0, 1)`, bent by the tension `t ∈ [−1, 1]`
/// (`g ← g^{2^{2t}}`: positive tension stays low longer, negative recovers sooner).
pub fn shape(s: usize, phase: f32, tension: f32) -> f32 {
    let p = phase.rem_euclid(1.0);
    let g = match s {
        // Sidechain-style pump: an exponential recovery from silence.
        0 => (1.0 - (-6.0 * p).exp()) / (1.0 - (-6.0f32).exp()),
        // A short duck: back up within the first quarter (smoothstep).
        1 => {
            let x = (p / 0.25).min(1.0);
            x * x * (3.0 - 2.0 * x)
        }
        2 => {
            if p < 0.5 {
                1.0
            } else {
                0.0
            }
        }
        3 => p,
        4 => 1.0 - p,
        5 => 0.5 - 0.5 * (std::f32::consts::TAU * p).cos(),
        6 => 1.0 - (2.0 * p - 1.0).abs(),
        _ => p * p,
    };
    let k = 2f32.powf(2.0 * tension.clamp(-1.0, 1.0));
    g.clamp(0.0, 1.0).powf(k)
}

pub struct VolumeShaper {
    rate: f32,
    mode: usize,
    shape: usize,
    rate_idx: usize,
    depth: f32,
    tension: f32,
    offset: f32,
    smooth_c: f32,
    threshold: f32,
    attack: f32,
    hold: f32,
    release: f32,
    out: f32,
    bpm: f64,
    beat: f64,
    playing: bool,
    /// Free-running phase while the transport stops (beats).
    free: f64,
    gain: f32,
    /// Ducking: detector envelope, duck gain, samples left in hold.
    env: f32,
    duck: f32,
    hold_left: usize,
    tele: [f32; 4],
}

impl VolumeShaper {
    pub fn new(rate: f32) -> Self {
        let mut v = VolumeShaper {
            rate,
            mode: 0,
            shape: 0,
            rate_idx: 3,
            depth: 0.8,
            tension: 0.0,
            offset: 0.0,
            smooth_c: 0.0,
            threshold: -24.0,
            attack: 2.0,
            hold: 30.0,
            release: 200.0,
            out: 1.0,
            bpm: 120.0,
            beat: 0.0,
            playing: false,
            free: 0.0,
            gain: 1.0,
            env: 0.0,
            duck: 1.0,
            hold_left: 0,
            tele: [0.0; 4],
        };
        v.set_param("smooth", 2.0);
        v
    }

    fn coef(&self, ms: f32) -> f32 {
        (-1.0 / (ms.max(0.01) * 0.001 * self.rate)).exp()
    }
}

impl Effect for VolumeShaper {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "mode" => self.mode = (v.round().max(0.0) as usize).min(MODES.len() - 1),
            "shape" => self.shape = (v.round().max(0.0) as usize).min(SHAPES.len() - 1),
            "rate" => self.rate_idx = (v.round().max(0.0) as usize).min(RATES.len() - 1),
            "depth" => self.depth = v.clamp(0.0, 100.0) / 100.0,
            "tension" => self.tension = v.clamp(-100.0, 100.0) / 100.0,
            "offset" => self.offset = v.clamp(0.0, 100.0) / 100.0,
            "smooth" => self.smooth_c = self.coef(v.clamp(0.1, 20.0)),
            "threshold" => self.threshold = v,
            "attack" => self.attack = v.clamp(0.1, 50.0),
            "hold" => self.hold = v.clamp(1.0, 500.0),
            "release" => self.release = v.clamp(10.0, 2000.0),
            "output" => self.out = db_to_gain(v),
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.gain = 1.0;
        self.env = 0.0;
        self.duck = 1.0;
        self.hold_left = 0;
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
        let period = RATE_BEATS[self.rate_idx];
        let beats_per_sample = self.bpm / 60.0 / self.rate as f64;
        let start = if self.playing { self.beat } else { self.free };
        let use_shape = self.mode != 1;
        let use_duck = self.mode != 0;
        let thr = db_to_gain(self.threshold);
        let (det_a, det_r) = (self.coef(0.5), self.coef(30.0));
        let att_c = self.coef(self.attack);
        let rel_c = self.coef(self.release);
        let hold_n = (self.hold * 0.001 * self.rate) as usize;
        let floor = 1.0 - self.depth;
        let mut phase = 0.0f32;
        for i in 0..l.len() {
            let b = start + i as f64 * beats_per_sample;
            phase = ((b / period + self.offset as f64).rem_euclid(1.0)) as f32;
            let mut target = 1.0f32;
            if use_shape {
                let g = shape(self.shape, phase, self.tension);
                target *= 1.0 - self.depth * (1.0 - g);
            }
            if use_duck {
                // Detector: the sidechain (or the input itself without one), peak envelope.
                let x = match sc {
                    Some((a, c)) => a[i].abs().max(c[i].abs()),
                    None => l[i].abs().max(r[i].abs()),
                };
                self.env = if x > self.env { x + (self.env - x) * det_a } else { x + (self.env - x) * det_r };
                if self.env > thr {
                    self.hold_left = hold_n;
                }
                let goal = if self.hold_left > 0 {
                    self.hold_left -= 1;
                    floor
                } else {
                    1.0
                };
                let c = if goal < self.duck { att_c } else { rel_c };
                self.duck = goal + (self.duck - goal) * c;
                target *= self.duck;
            }
            self.gain = target + (self.gain - target) * self.smooth_c;
            let g = self.gain * self.out;
            l[i] *= g;
            r[i] *= g;
        }
        if !self.playing {
            self.free = (self.free + l.len() as f64 * beats_per_sample) % 64.0;
        }
        self.tele = [phase, self.gain, self.duck, if self.playing { 1.0 } else { 0.0 }];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_stay_in_range() {
        for s in 0..SHAPES.len() {
            for k in 0..100 {
                let g = shape(s, k as f32 / 100.0, 0.3);
                assert!((0.0..=1.0).contains(&g), "{} at {k}", SHAPES[s]);
            }
        }
    }

    /// A pump at depth 100 % is silent on the beat and back near full level before the next one.
    #[test]
    fn pump_follows_the_beat() {
        let mut v = VolumeShaper::new(48000.0);
        v.set_param("depth", 100.0);
        v.set_param("smooth", 0.1);
        v.set_tempo(120.0);
        v.set_position(0.0, true);
        let n = 24000; // one beat at 120 BPM
        let mut l = vec![1.0f32; n];
        let mut r = vec![1.0f32; n];
        v.process(&mut l, &mut r, None);
        assert!(l[60] < 0.05, "{}", l[60]);
        assert!(l[n - 10] > 0.95, "{}", l[n - 10]);
    }

    /// A sidechain hit ducks the signal by the depth.
    #[test]
    fn sidechain_ducks() {
        let mut v = VolumeShaper::new(48000.0);
        v.set_param("mode", 1.0);
        v.set_param("depth", 50.0);
        let n = 4800;
        let sc: Vec<f32> = (0..n).map(|i| if i < 100 { 1.0 } else { 0.0 }).collect();
        let mut l = vec![1.0f32; n];
        let mut r = vec![1.0f32; n];
        v.process(&mut l, &mut r, Some((&sc, &sc)));
        let min = l.iter().cloned().fold(1.0f32, f32::min);
        assert!((min - 0.5).abs() < 0.05, "{min}");
    }
}
