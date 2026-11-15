//! Delay (free or tempo-synced, ping-pong, filtered feedback, modulation), chorus, flanger, phaser and
//! tremolo / auto-pan.

use super::{Effect, EffectDef, ParamInfo};
use crate::dsp::{Biquad, BiquadKind, DelayLine, Lfo, LfoShape, OnePole};

// ------------------------------------------------------------------------------------------------ delay

const SYNC: &[&str] = &["Free", "1/32", "1/16", "1/16 dotted", "1/8 triplet", "1/8", "1/8 dotted", "1/4 triplet", "1/4", "1/4 dotted", "1/2", "1 bar"];
const SYNC_BEATS: [f64; 12] = [0.0, 0.125, 0.25, 0.375, 1.0 / 3.0, 0.5, 0.75, 2.0 / 3.0, 1.0, 1.5, 2.0, 4.0];

const DELAY_PARAMS: &[ParamInfo] = &[
    ParamInfo::choice("sync", "Sync", SYNC, 6.0),
    ParamInfo::log("time", "Time (free)", 1.0, 2000.0, 375.0, "ms"),
    ParamInfo::lin("feedback", "Feedback", 0.0, 0.95, 0.4, ""),
    ParamInfo::choice("mode", "Mode", &["Stereo", "Ping-pong", "Tape"], 1.0),
    ParamInfo::log("lowcut", "Low cut", 20.0, 2000.0, 150.0, "Hz"),
    ParamInfo::log("highcut", "High cut", 1000.0, 20000.0, 8000.0, "Hz"),
    ParamInfo::lin("mod", "Modulation", 0.0, 1.0, 0.1, ""),
    ParamInfo::lin("offset", "R offset", -0.5, 0.5, 0.0, ""),
    ParamInfo::lin("mix", "Mix", 0.0, 1.0, 0.3, ""),
    // Timeless-style additions.
    ParamInfo::lin("cross", "Cross feedback", 0.0, 0.95, 0.0, ""),
    ParamInfo::lin("drive", "Drive (in the loop)", 0.0, 24.0, 0.0, "dB"),
    ParamInfo::lin("reso", "Filter resonance", 0.0, 1.0, 0.0, ""),
    ParamInfo::lin("diffusion", "Diffusion", 0.0, 1.0, 0.0, ""),
    ParamInfo::lin("duck", "Ducking", 0.0, 24.0, 0.0, "dB"),
    ParamInfo::toggle("freeze", "Freeze", false),
    ParamInfo::lin("width", "Width", 0.0, 2.0, 1.0, ""),
    ParamInfo::lin("pitch", "Pitch of the repeats", -12.0, 12.0, 0.0, "st"),
];

/// Two-stage allpass diffuser (smears each repeat, Timeless "diffusion").
#[derive(Clone)]
struct Diffuser {
    lines: [DelayLine; 2],
    len: [f32; 2],
}

impl Diffuser {
    fn new(rate: f32, a_ms: f32, b_ms: f32) -> Self {
        Diffuser { lines: [DelayLine::new((rate * 0.05) as usize), DelayLine::new((rate * 0.05) as usize)], len: [a_ms * 0.001 * rate, b_ms * 0.001 * rate] }
    }
    fn process(&mut self, x: f32, g: f32) -> f32 {
        let mut y = x;
        for k in 0..2 {
            let d = self.lines[k].read(self.len[k]);
            let w = y + d * g;
            self.lines[k].write(w);
            y = d - w * g;
        }
        y
    }
}

pub struct Delay {
    rate: f32,
    bpm: f64,
    sync: usize,
    time_ms: f32,
    feedback: f32,
    mode: usize,
    lc: [OnePole; 2],
    hc: [OnePole; 2],
    lines: [DelayLine; 2],
    lfo: Lfo,
    modulation: f32,
    offset: f32,
    mix: f32,
    smoothed: f32,
    cross: f32,
    drive: f32,
    reso: f32,
    peak: [Biquad; 2],
    highcut: f32,
    diffusion: f32,
    diff: [Diffuser; 2],
    duck: f32,
    env: f32,
    freeze: bool,
    width: f32,
    pitch: f32,
    /// Pitch shift of the repeats: a rotating tap pair inside the loop.
    shift_phase: f32,
}

impl Delay {
    fn delay_samples(&self) -> f32 {
        let secs = if self.sync == 0 { self.time_ms as f64 / 1000.0 } else { SYNC_BEATS[self.sync] * 60.0 / self.bpm.max(10.0) };
        (secs * self.rate as f64) as f32
    }
}

impl Effect for Delay {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "sync" => self.sync = (v.round() as usize).min(SYNC.len() - 1),
            "time" => self.time_ms = v,
            "feedback" => self.feedback = v.min(0.95),
            "mode" => self.mode = v.round() as usize,
            "lowcut" => self.lc.iter_mut().for_each(|f| f.set(v, self.rate)),
            "highcut" => {
                self.highcut = v;
                self.hc.iter_mut().for_each(|f| f.set(v, self.rate));
                let q = 0.7 + self.reso * 8.0;
                self.peak = [Biquad::new(BiquadKind::Peak, v.min(self.rate * 0.45), q, self.reso * 12.0, self.rate); 2];
            }
            "reso" => {
                self.reso = v;
                let q = 0.7 + v * 8.0;
                self.peak = [Biquad::new(BiquadKind::Peak, self.highcut.min(self.rate * 0.45), q, v * 12.0, self.rate); 2];
            }
            "cross" => self.cross = v.min(0.95),
            "drive" => self.drive = v,
            "diffusion" => self.diffusion = v,
            "duck" => self.duck = v,
            "freeze" => self.freeze = v >= 0.5,
            "width" => self.width = v,
            "pitch" => self.pitch = v,
            "mod" => self.modulation = v,
            "offset" => self.offset = v,
            "mix" => self.mix = v,
            _ => {}
        }
    }
    fn set_tempo(&mut self, bpm: f64) {
        self.bpm = bpm;
    }
    fn reset(&mut self) {
        self.lines.iter_mut().for_each(DelayLine::clear);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let target = self.delay_samples().clamp(1.0, self.lines[0].len() as f32 - 4.0);
        let tape = self.mode == 2;
        let drive = crate::dsp::db_to_gain(self.drive);
        let env_a = (-1.0 / (0.01 * self.rate)).exp();
        let env_r = (-1.0 / (0.25 * self.rate)).exp();
        // Repeats shifted in pitch: two taps sweeping a 50 ms window in opposite phase (crossfaded).
        let ratio = 2f32.powf(self.pitch / 12.0);
        let win = 0.05 * self.rate;
        for i in 0..l.len() {
            // Tape mode glides the delay time (pitch bends on changes), the others jump smoothly.
            self.smoothed += (target - self.smoothed) * if tape { 0.0002 } else { 0.01 };
            let m = self.lfo.next(0.6, self.rate, LfoShape::Sine) * self.modulation * 0.003 * self.rate;
            let dl = self.smoothed + m;
            let dr = (self.smoothed * (1.0 + self.offset) + m).clamp(1.0, self.lines[1].len() as f32 - 4.0);
            let read = |line: &DelayLine, d: f32, phase: f32| -> f32 {
                if (ratio - 1.0).abs() < 1e-3 {
                    return line.read(d);
                }
                let p2 = (phase + 0.5).fract();
                let a = line.read((d + phase * win).min(line.len() as f32 - 4.0));
                let b = line.read((d + p2 * win).min(line.len() as f32 - 4.0));
                let wa = (std::f32::consts::PI * phase).sin();
                let wb = (std::f32::consts::PI * p2).sin();
                a * wa + b * wb
            };
            let yl = read(&self.lines[0], dl, self.shift_phase);
            let yr = read(&self.lines[1], dr, self.shift_phase);
            self.shift_phase = (self.shift_phase + (1.0 - ratio) / win).rem_euclid(1.0);
            // Feedback path: filters (with a resonant peak at the high cut), drive, diffusion.
            let mut fl = self.peak[0].process(self.hc[0].process(self.lc[0].highpass(yl)));
            let mut fr = self.peak[1].process(self.hc[1].process(self.lc[1].highpass(yr)));
            if self.drive > 0.01 {
                fl = (fl * drive).tanh() / drive.sqrt();
                fr = (fr * drive).tanh() / drive.sqrt();
            }
            if self.diffusion > 0.0 {
                let g = self.diffusion * 0.7;
                fl = self.diff[0].process(fl, g);
                fr = self.diff[1].process(fr, g);
            }
            let (fb, cr) = if self.freeze { (1.0, 0.0) } else { (self.feedback, self.cross) };
            let (mut fl2, mut fr2) = (fl * fb + fr * cr, fr * fb + fl * cr);
            if tape {
                fl2 = (fl2 * 1.2).tanh() / 1.2;
                fr2 = (fr2 * 1.2).tanh() / 1.2;
            }
            // Freeze: the loop keeps its content and takes no new input.
            let (il, ir) = if self.freeze { (0.0, 0.0) } else { (l[i], r[i]) };
            if self.freeze {
                self.lines[0].write(yl);
                self.lines[1].write(yr);
            } else if self.mode == 1 {
                // Ping-pong: the left input feeds the left line, feedback crosses sides.
                self.lines[0].write((il + ir) * 0.5 + fr2);
                self.lines[1].write(fl2);
            } else {
                self.lines[0].write(il + fl2);
                self.lines[1].write(ir + fr2);
            }
            // Ducking: the echoes dip while the input plays.
            let x = l[i].abs().max(r[i].abs());
            self.env = x + (self.env - x) * if x > self.env { env_a } else { env_r };
            let duck = if self.duck > 0.0 { crate::dsp::db_to_gain(-self.duck * (crate::dsp::gain_to_db(self.env.max(1e-5)) + 40.0).clamp(0.0, 40.0) / 40.0) } else { 1.0 };
            // Width of the echoes (mid / side).
            let (mid, side) = ((yl + yr) * 0.5, (yl - yr) * 0.5 * self.width);
            let (wl, wr) = ((mid + side) * duck, (mid - side) * duck);
            l[i] = l[i] * (1.0 - self.mix) + wl * self.mix;
            r[i] = r[i] * (1.0 - self.mix) + wr * self.mix;
        }
    }
}

pub fn delay_def() -> EffectDef {
    EffectDef {
        id: "delay",
        name: "Delay",
        category: "Delay",
        description:
            "Delay after Timeless: tempo-synced or free, stereo / ping-pong / tape, cross feedback, resonant filters, drive and diffusion in the loop, pitch-shifted repeats, ducking, freeze, width and modulation.",
        params: DELAY_PARAMS,
        uses_sidechain: false,
        create: |rate| {
            Box::new(Delay {
                rate,
                bpm: 120.0,
                sync: 6,
                time_ms: 375.0,
                feedback: 0.4,
                mode: 1,
                lc: [OnePole::new(150.0, rate); 2],
                hc: [OnePole::new(8000.0, rate); 2],
                lines: [DelayLine::new((rate * 4.2) as usize), DelayLine::new((rate * 4.2) as usize)],
                lfo: Lfo::default(),
                modulation: 0.1,
                offset: 0.0,
                mix: 0.3,
                smoothed: rate * 0.3,
                cross: 0.0,
                drive: 0.0,
                reso: 0.0,
                peak: [Biquad::new(BiquadKind::Peak, 8000.0, 0.7, 0.0, rate); 2],
                highcut: 8000.0,
                diffusion: 0.0,
                diff: [Diffuser::new(rate, 7.3, 11.1), Diffuser::new(rate, 8.9, 13.7)],
                duck: 0.0,
                env: 0.0,
                freeze: false,
                width: 1.0,
                pitch: 0.0,
                shift_phase: 0.0,
            })
        },
    }
}

// ------------------------------------------------------------------------------------------------ chorus / flanger

struct ModDelay {
    rate: f32,
    lines: [DelayLine; 2],
    lfo: [Lfo; 2],
    speed: f32,
    depth: f32,
    base_ms: f32,
    feedback: f32,
    mix: f32,
    fb: [f32; 2],
}

impl ModDelay {
    fn new(rate: f32, base_ms: f32) -> Self {
        ModDelay {
            rate,
            lines: [DelayLine::new((rate * 0.06) as usize), DelayLine::new((rate * 0.06) as usize)],
            lfo: [Lfo::with_phase(0.0), Lfo::with_phase(0.25)],
            speed: 0.8,
            depth: 0.5,
            base_ms,
            feedback: 0.0,
            mix: 0.5,
            fb: [0.0; 2],
        }
    }
    fn run(&mut self, l: &mut [f32], r: &mut [f32], depth_ms: f32) {
        for i in 0..l.len() {
            for (ch, x) in [&mut l[i], &mut r[i]].into_iter().enumerate() {
                let m = (self.lfo[ch].next(self.speed, self.rate, LfoShape::Sine) * 0.5 + 0.5) * self.depth * depth_ms;
                let d = (self.base_ms + m) * 0.001 * self.rate;
                let y = self.lines[ch].read(d);
                self.lines[ch].write(*x + y * self.feedback);
                self.fb[ch] = y;
                *x = *x * (1.0 - self.mix * 0.5) + y * self.mix;
            }
        }
    }
    fn set(&mut self, id: &str, v: f32) {
        match id {
            "rate" => self.speed = v,
            "depth" => self.depth = v,
            "feedback" => self.feedback = v,
            "mix" => self.mix = v,
            "delay" => self.base_ms = v,
            _ => {}
        }
    }
}

pub struct Chorus(ModDelay);
pub struct Flanger(ModDelay);

const CHORUS_PARAMS: &[ParamInfo] =
    &[ParamInfo::log("rate", "Rate", 0.05, 8.0, 0.8, "Hz"), ParamInfo::lin("depth", "Depth", 0.0, 1.0, 0.5, ""), ParamInfo::log("delay", "Delay", 5.0, 30.0, 12.0, "ms"), ParamInfo::lin("mix", "Mix", 0.0, 1.0, 0.5, "")];
const FLANGER_PARAMS: &[ParamInfo] = &[
    ParamInfo::log("rate", "Rate", 0.02, 5.0, 0.25, "Hz"),
    ParamInfo::lin("depth", "Depth", 0.0, 1.0, 0.7, ""),
    ParamInfo::lin("feedback", "Feedback", -0.9, 0.9, 0.6, ""),
    ParamInfo::log("delay", "Delay", 0.1, 10.0, 1.0, "ms"),
    ParamInfo::lin("mix", "Mix", 0.0, 1.0, 0.5, ""),
];

impl Effect for Chorus {
    fn set_param(&mut self, id: &str, v: f32) {
        self.0.set(id, v);
    }
    fn reset(&mut self) {
        self.0.lines.iter_mut().for_each(DelayLine::clear);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        self.0.run(l, r, 8.0);
    }
}

impl Effect for Flanger {
    fn set_param(&mut self, id: &str, v: f32) {
        self.0.set(id, v);
    }
    fn reset(&mut self) {
        self.0.lines.iter_mut().for_each(DelayLine::clear);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        self.0.run(l, r, 5.0);
    }
}

pub fn chorus_def() -> EffectDef {
    EffectDef {
        id: "chorus",
        name: "Chorus",
        category: "Modulation",
        description: "Stereo chorus with quadrature LFOs.",
        params: CHORUS_PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(Chorus(ModDelay::new(rate, 12.0))),
    }
}

pub fn flanger_def() -> EffectDef {
    EffectDef {
        id: "flanger",
        name: "Flanger",
        category: "Modulation",
        description: "Through-zero style flanger with positive or negative feedback.",
        params: FLANGER_PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(Flanger(ModDelay::new(rate, 1.0))),
    }
}

// ------------------------------------------------------------------------------------------------ phaser

const PHASER_PARAMS: &[ParamInfo] = &[
    ParamInfo::log("rate", "Rate", 0.02, 8.0, 0.4, "Hz"),
    ParamInfo::lin("depth", "Depth", 0.0, 1.0, 0.8, ""),
    ParamInfo::choice("stages", "Stages", &["2", "4", "6", "8", "12"], 2.0),
    ParamInfo::lin("feedback", "Feedback", 0.0, 0.9, 0.5, ""),
    ParamInfo::log("center", "Center", 100.0, 5000.0, 900.0, "Hz"),
    ParamInfo::lin("mix", "Mix", 0.0, 1.0, 0.5, ""),
];

pub struct Phaser {
    rate: f32,
    ap: [[Biquad; 12]; 2],
    lfo: [Lfo; 2],
    speed: f32,
    depth: f32,
    stages: usize,
    feedback: f32,
    center: f32,
    mix: f32,
    fb: [f32; 2],
    counter: usize,
}

impl Effect for Phaser {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "rate" => self.speed = v,
            "depth" => self.depth = v,
            "stages" => self.stages = [2, 4, 6, 8, 12][(v.round() as usize).min(4)],
            "feedback" => self.feedback = v,
            "center" => self.center = v,
            "mix" => self.mix = v,
            _ => {}
        }
    }
    fn reset(&mut self) {
        self.ap.iter_mut().flatten().for_each(Biquad::reset);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        for i in 0..l.len() {
            for (ch, x) in [&mut l[i], &mut r[i]].into_iter().enumerate() {
                let m = self.lfo[ch].next(self.speed, self.rate, LfoShape::Triangle);
                if self.counter.is_multiple_of(16) {
                    let f = self.center * 2f32.powf(m * self.depth * 2.0);
                    for s in 0..self.stages {
                        self.ap[ch][s].set(BiquadKind::AllPass, f * (1.0 + s as f32 * 0.15), 0.6, 0.0, self.rate);
                    }
                }
                let mut y = *x + self.fb[ch] * self.feedback;
                for s in 0..self.stages {
                    y = self.ap[ch][s].process(y);
                }
                self.fb[ch] = y;
                *x = *x * (1.0 - self.mix * 0.5) + y * self.mix * 0.5;
            }
            self.counter = self.counter.wrapping_add(1);
        }
    }
}

pub fn phaser_def() -> EffectDef {
    EffectDef {
        id: "phaser",
        name: "Phaser",
        category: "Modulation",
        description: "2–12 stage all-pass phaser with feedback.",
        params: PHASER_PARAMS,
        uses_sidechain: false,
        create: |rate| {
            Box::new(Phaser {
                rate,
                ap: [[Biquad::default(); 12]; 2],
                lfo: [Lfo::with_phase(0.0), Lfo::with_phase(0.25)],
                speed: 0.4,
                depth: 0.8,
                stages: 6,
                feedback: 0.5,
                center: 900.0,
                mix: 0.5,
                fb: [0.0; 2],
                counter: 0,
            })
        },
    }
}

// ------------------------------------------------------------------------------------------------ tremolo / auto-pan

const TREM_PARAMS: &[ParamInfo] = &[
    ParamInfo::choice("sync", "Sync", SYNC, 5.0),
    ParamInfo::log("rate", "Rate (free)", 0.1, 20.0, 4.0, "Hz"),
    ParamInfo::lin("depth", "Depth", 0.0, 1.0, 0.6, ""),
    ParamInfo::choice("shape", "Shape", &["Sine", "Triangle", "Square"], 0.0),
    ParamInfo::toggle("pan", "Auto-pan", false),
];

pub struct Tremolo {
    rate: f32,
    bpm: f64,
    sync: usize,
    speed: f32,
    depth: f32,
    shape: LfoShape,
    pan: bool,
    lfo: Lfo,
}

impl Effect for Tremolo {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "sync" => self.sync = (v.round() as usize).min(SYNC.len() - 1),
            "rate" => self.speed = v,
            "depth" => self.depth = v,
            "shape" => self.shape = [LfoShape::Sine, LfoShape::Triangle, LfoShape::Square][(v.round() as usize).min(2)],
            "pan" => self.pan = v >= 0.5,
            _ => {}
        }
    }
    fn set_tempo(&mut self, bpm: f64) {
        self.bpm = bpm;
    }
    fn reset(&mut self) {}
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let hz = if self.sync == 0 { self.speed } else { (self.bpm / 60.0 / SYNC_BEATS[self.sync]) as f32 };
        for i in 0..l.len() {
            let m = self.lfo.next(hz, self.rate, self.shape);
            if self.pan {
                let p = m * self.depth;
                l[i] *= (1.0 - p).min(1.0);
                r[i] *= (1.0 + p).min(1.0);
            } else {
                let g = 1.0 - self.depth * (0.5 - 0.5 * m);
                l[i] *= g;
                r[i] *= g;
            }
        }
    }
}

pub fn tremolo_def() -> EffectDef {
    EffectDef {
        id: "tremolo",
        name: "Tremolo / Auto-pan",
        category: "Modulation",
        description: "Tempo-synced volume or pan modulation.",
        params: TREM_PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(Tremolo { rate, bpm: 120.0, sync: 5, speed: 4.0, depth: 0.6, shape: LfoShape::Sine, pan: false, lfo: Lfo::default() }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn synced_delay_echoes_on_the_beat() {
        let mut p = std::collections::BTreeMap::new();
        p.insert("sync".into(), 8.0); // 1/4
        p.insert("mix".into(), 1.0);
        p.insert("mode".into(), 0.0);
        p.insert("mod".into(), 0.0);
        p.insert("lowcut".into(), 20.0);
        p.insert("highcut".into(), 20000.0);
        let mut e = crate::effects::create("delay", 48000.0, &p).unwrap();
        e.set_tempo(120.0); // quarter = 0.5 s = 24000 samples
        let mut l = vec![0.0f32; 30000];
        let mut r = l.clone();
        // Let the time smoother settle first.
        let mut warm_l = vec![0.0f32; 48000 * 2];
        let mut warm_r = warm_l.clone();
        e.process(&mut warm_l, &mut warm_r, None);
        l[0] = 1.0;
        e.process(&mut l, &mut r, None);
        let (idx, _) = l.iter().enumerate().skip(100).fold((0, 0.0f32), |a, (i, &x)| if x.abs() > a.1 { (i, x.abs()) } else { a });
        assert!((idx as i64 - 24000).abs() < 50, "{idx}");
    }

    fn delay_with(set: &[(&str, f32)]) -> Box<dyn Effect> {
        let mut p = delay_def().defaults();
        p.insert("sync".into(), 0.0);
        p.insert("time".into(), 100.0);
        p.insert("mix".into(), 1.0);
        p.insert("mode".into(), 0.0);
        p.insert("lowcut".into(), 20.0);
        p.insert("highcut".into(), 20000.0);
        p.insert("mod".into(), 0.0);
        for (k, v) in set {
            p.insert((*k).into(), *v);
        }
        crate::effects::create("delay", 48000.0, &p).unwrap()
    }

    /// Timeless-style features: freeze holds the loop with no decay; ducking lowers echoes under the input;
    /// pitch shifts the repeats.
    #[test]
    fn delay_freeze_duck_and_pitch() {
        let rms = |v: &[f32]| (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt();
        // A burst, then silence: with freeze on after the burst, the echo level stays.
        let mut e = delay_with(&[("feedback", 0.5)]);
        let mut l: Vec<f32> = (0..48000).map(|i| if i < 2400 { (i as f32 * 0.1).sin() * 0.5 } else { 0.0 }).collect();
        let mut r = l.clone();
        e.process(&mut l[..9600], &mut r[..9600], None);
        e.set_param("freeze", 1.0);
        e.process(&mut l[9600..], &mut r[9600..], None);
        let (a, b) = (rms(&l[9600..19200]), rms(&l[38400..48000]));
        assert!(b > a * 0.8, "frozen loop keeps its level: {a} {b}");
        // Ducking: continuous input – the echoes are lower with ducking on.
        let tone: Vec<f32> = (0..48000).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        let run = |e: &mut Box<dyn Effect>| {
            let (mut l, mut r) = (tone.clone(), tone.clone());
            e.process(&mut l, &mut r, None);
            rms(&l[24000..])
        };
        let plain = run(&mut delay_with(&[("feedback", 0.3)]));
        let ducked = run(&mut delay_with(&[("feedback", 0.3), ("duck", 18.0)]));
        assert!(ducked < plain * 0.5, "{plain} {ducked}");
        // Pitch: a 1 kHz tone repeated an octave up shows up near 2 kHz.
        let x: Vec<f32> = (0..48000).map(|i| (std::f32::consts::TAU * 1000.0 * i as f32 / 48000.0).sin() * 0.5).collect();
        let mut e = delay_with(&[("feedback", 0.0), ("pitch", 12.0)]);
        let (mut l, mut r) = (x.clone(), x.clone());
        e.process(&mut l, &mut r, None);
        let bin = |y: &[f32], f: f32| {
            let w = std::f32::consts::TAU * f / 48000.0;
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for (i, v) in y.iter().enumerate() {
                re += v * (w * i as f32).cos();
                im += v * (w * i as f32).sin();
            }
            (re * re + im * im).sqrt()
        };
        assert!(bin(&l[24000..], 2000.0) > bin(&l[24000..], 1000.0), "repeats an octave up");
    }
}
