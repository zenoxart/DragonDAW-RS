//! Delay (free or tempo-synced, ping-pong, filtered feedback, modulation), chorus, flanger, phaser and
//! tremolo / auto-pan.

use super::{Effect, EffectDef, ParamInfo};
use crate::dsp::{Biquad, BiquadKind, DelayLine, Lfo, LfoShape, OnePole};

// ------------------------------------------------------------------------------------------------ delay

const SYNC: &[&str] = &["Free", "1/32", "1/16", "1/16 dotted", "1/8 triplet", "1/8", "1/8 dotted", "1/4 triplet", "1/4", "1/4 dotted", "1/2", "1 bar"];
/// Beats per delay sync value (index 0 is free time).
pub const DELAY_SYNC_BEATS: [f64; 12] = SYNC_BEATS;
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
    // Extra features.
    ParamInfo::lin("cross", "Cross feedback", 0.0, 0.95, 0.0, ""),
    ParamInfo::lin("drive", "Drive (in the loop)", 0.0, 24.0, 0.0, "dB"),
    ParamInfo::lin("reso", "Filter resonance", 0.0, 1.0, 0.0, ""),
    ParamInfo::lin("diffusion", "Diffusion", 0.0, 1.0, 0.0, ""),
    ParamInfo::lin("duck", "Ducking", 0.0, 24.0, 0.0, "dB"),
    ParamInfo::toggle("freeze", "Freeze", false),
    ParamInfo::lin("width", "Width", 0.0, 2.0, 1.0, ""),
    ParamInfo::lin("pitch", "Pitch of the repeats", -12.0, 12.0, 0.0, "st"),
    // Modulation matrix: two LFOs and an envelope follower, four routing slots.
    ParamInfo::choice("lfo1_sync", "LFO 1 sync", SYNC, 0.0),
    ParamInfo::log("lfo1_rate", "LFO 1 rate", 0.02, 20.0, 0.5, "Hz"),
    ParamInfo::choice("lfo1_shape", "LFO 1 shape", DMOD_SHAPES, 0.0),
    ParamInfo::choice("lfo2_sync", "LFO 2 sync", SYNC, 0.0),
    ParamInfo::log("lfo2_rate", "LFO 2 rate", 0.02, 20.0, 2.0, "Hz"),
    ParamInfo::choice("lfo2_shape", "LFO 2 shape", DMOD_SHAPES, 1.0),
    ParamInfo::choice("m1_src", "Slot 1 source", DMOD_SOURCES, 0.0),
    ParamInfo::choice("m1_dst", "Slot 1 target", DMOD_TARGETS, 0.0),
    ParamInfo::lin("m1_amt", "Slot 1 amount", -100.0, 100.0, 0.0, "%"),
    ParamInfo::choice("m2_src", "Slot 2 source", DMOD_SOURCES, 0.0),
    ParamInfo::choice("m2_dst", "Slot 2 target", DMOD_TARGETS, 3.0),
    ParamInfo::lin("m2_amt", "Slot 2 amount", -100.0, 100.0, 0.0, "%"),
    ParamInfo::choice("m3_src", "Slot 3 source", DMOD_SOURCES, 0.0),
    ParamInfo::choice("m3_dst", "Slot 3 target", DMOD_TARGETS, 1.0),
    ParamInfo::lin("m3_amt", "Slot 3 amount", -100.0, 100.0, 0.0, "%"),
    ParamInfo::choice("m4_src", "Slot 4 source", DMOD_SOURCES, 0.0),
    ParamInfo::choice("m4_dst", "Slot 4 target", DMOD_TARGETS, 7.0),
    ParamInfo::lin("m4_amt", "Slot 4 amount", -100.0, 100.0, 0.0, "%"),
];

pub const DMOD_SHAPES: &[&str] = &["Sine", "Triangle", "Square", "Random"];
pub const DMOD_SOURCES: &[&str] = &["Off", "LFO 1", "LFO 2", "Envelope"];
/// What a slot modulates; at 100 %: time ±25 %, feedback ±0.5, filters ±3 octaves, drive +24 dB,
/// pitch ±12 st, width ±1, mix ±0.5.
pub const DMOD_TARGETS: &[&str] = &["Time", "Feedback", "Low cut", "High cut", "Drive", "Pitch", "Width", "Mix"];
const DMOD_N: usize = 4;

/// Soft limit for feedback loops: unity below ~1, approaching ±4 above.
#[inline]
fn loop_limit(x: f32) -> f32 {
    let a = x.abs();
    if a <= 1.0 {
        x
    } else {
        x.signum() * (1.0 + 3.0 * ((a - 1.0) / 3.0).tanh())
    }
}

/// Two-stage allpass diffuser (smears each repeat, "diffusion").
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
    /// The echo time in effect (ms, synced or free), and the input and output peaks of the last block.
    tele: [f32; 3],
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
    lowcut: f32,
    /// Modulation matrix: LFO (sync, Hz, shape), slots (source, target, amount), the LFOs and the
    /// input envelope.
    mlfo: [(usize, f32, usize); 2],
    slots: [(usize, usize, f32); DMOD_N],
    mlfos: [Lfo; 2],
    menv: f32,
    /// Filter frequencies the last modulated retune used (to skip unchanged ones).
    mod_freqs: (f32, f32),
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
            "lowcut" => {
                self.lowcut = v;
                self.lc.iter_mut().for_each(|f| f.set(v, self.rate));
            }
            "highcut" => {
                self.highcut = v;
                self.hc.iter_mut().for_each(|f| f.set(v, self.rate));
                let q = 0.7 + self.reso * 8.0;
                for b in &mut self.peak {
                    b.set(BiquadKind::Peak, v.min(self.rate * 0.45), q, self.reso * 12.0, self.rate);
                }
            }
            "reso" => {
                self.reso = v;
                let q = 0.7 + v * 8.0;
                for b in &mut self.peak {
                    b.set(BiquadKind::Peak, self.highcut.min(self.rate * 0.45), q, v * 12.0, self.rate);
                }
            }
            "cross" => self.cross = v.min(0.95),
            "drive" => self.drive = v,
            "diffusion" => self.diffusion = v,
            "duck" => self.duck = v,
            "freeze" => self.freeze = v >= 0.5,
            "width" => self.width = v,
            "pitch" => self.pitch = v,
            "mod" => self.modulation = v,
            "lfo1_sync" => self.mlfo[0].0 = (v.round() as usize).min(SYNC.len() - 1),
            "lfo1_rate" => self.mlfo[0].1 = v,
            "lfo1_shape" => self.mlfo[0].2 = (v.round() as usize).min(DMOD_SHAPES.len() - 1),
            "lfo2_sync" => self.mlfo[1].0 = (v.round() as usize).min(SYNC.len() - 1),
            "lfo2_rate" => self.mlfo[1].1 = v,
            "lfo2_shape" => self.mlfo[1].2 = (v.round() as usize).min(DMOD_SHAPES.len() - 1),
            id if id.starts_with('m') && id.len() > 4 && id.as_bytes()[2] == b'_' => {
                let k = (id.as_bytes()[1].wrapping_sub(b'1')) as usize;
                if k < DMOD_N {
                    match &id[3..] {
                        "src" => self.slots[k].0 = (v.round() as usize).min(DMOD_SOURCES.len() - 1),
                        "dst" => self.slots[k].1 = (v.round() as usize).min(DMOD_TARGETS.len() - 1),
                        "amt" => self.slots[k].2 = v / 100.0,
                        _ => {}
                    }
                }
            }
            "offset" => self.offset = v,
            "mix" => self.mix = v,
            _ => {}
        }
    }
    fn telemetry(&self) -> &[f32] {
        &self.tele
    }
    fn set_tempo(&mut self, bpm: f64) {
        self.bpm = bpm;
    }
    fn reset(&mut self) {
        self.lines.iter_mut().for_each(DelayLine::clear);
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let peak = |l: &[f32], r: &[f32]| l.iter().chain(r.iter()).fold(0.0f32, |m, x| m.max(x.abs()));
        let input = peak(l, r);
        let target = self.delay_samples().clamp(1.0, self.lines[0].len() as f32 - 4.0);
        self.tele[0] = target / self.rate * 1000.0;
        self.tele[1] = input;
        let tape = self.mode == 2;
        let drive = crate::dsp::db_to_gain(self.drive);
        let env_a = (-1.0 / (0.01 * self.rate)).exp();
        let env_r = (-1.0 / (0.25 * self.rate)).exp();
        // Repeats shifted in pitch: two taps sweeping a 50 ms window in opposite phase (crossfaded).
        let mut ratio = 2f32.powf(self.pitch / 12.0);
        let win = 0.05 * self.rate;
        let active = self.slots.iter().any(|s| s.0 > 0 && s.2 != 0.0);
        let lfo_hz = |(sync, hz, _): (usize, f32, usize)| if sync == 0 { hz } else { (self.bpm / 60.0 / SYNC_BEATS[sync]) as f32 };
        let hz = [lfo_hz(self.mlfo[0]), lfo_hz(self.mlfo[1])];
        let shape = |k: usize| [LfoShape::Sine, LfoShape::Triangle, LfoShape::Square, LfoShape::SampleHold][self.mlfo[k].2.min(3)];
        let shapes = [shape(0), shape(1)];
        for i in 0..l.len() {
            // ---- modulation matrix: the sum of the slots per target
            let mut md = [0.0f32; 8];
            if active {
                let x = l[i].abs().max(r[i].abs());
                self.menv = x + (self.menv - x) * if x > self.menv { env_a } else { env_r };
                let src = [0.0, self.mlfos[0].next(hz[0], self.rate, shapes[0]), self.mlfos[1].next(hz[1], self.rate, shapes[1]), ((crate::dsp::gain_to_db(self.menv.max(1e-6)) + 48.0) / 48.0).clamp(0.0, 1.0)];
                for &(s, t, a) in &self.slots {
                    if s > 0 {
                        md[t] += src[s] * a;
                    }
                }
                // Pitch and the filters retune every 32 samples (keeping their state).
                if i % 32 == 0 {
                    ratio = 2f32.powf((self.pitch + md[5] * 12.0).clamp(-24.0, 24.0) / 12.0);
                    let lc = (self.lowcut * 2f32.powf(md[2] * 3.0)).clamp(20.0, 2000.0);
                    let hc = (self.highcut * 2f32.powf(md[3] * 3.0)).clamp(500.0, 20000.0).min(self.rate * 0.45);
                    if (lc - self.mod_freqs.0).abs() > 0.5 {
                        self.lc.iter_mut().for_each(|f| f.set(lc, self.rate));
                        self.mod_freqs.0 = lc;
                    }
                    if (hc - self.mod_freqs.1).abs() > 1.0 {
                        self.hc.iter_mut().for_each(|f| f.set(hc, self.rate));
                        self.mod_freqs.1 = hc;
                    }
                }
            }
            // Tape mode glides the delay time (pitch bends on changes), the others jump smoothly.
            self.smoothed += (target - self.smoothed) * if tape { 0.0002 } else { 0.01 };
            let m = self.lfo.next(0.6, self.rate, LfoShape::Sine) * self.modulation * 0.003 * self.rate;
            let tm = 1.0 + (md[0] * 0.25).clamp(-0.9, 0.9);
            let dl = (self.smoothed * tm + m).clamp(1.0, self.lines[0].len() as f32 - 4.0);
            let dr = (self.smoothed * (1.0 + self.offset) * tm + m).clamp(1.0, self.lines[1].len() as f32 - 4.0);
            let read = |line: &DelayLine, d: f32, phase: f32| -> f32 {
                if (ratio - 1.0).abs() < 1e-3 {
                    return line.read(d);
                }
                let p2 = (phase + 0.5).fract();
                let a = line.read((d + phase * win).min(line.len() as f32 - 4.0));
                let b = line.read((d + p2 * win).min(line.len() as f32 - 4.0));
                // sin² windows: the two weights always sum to 1 (sin windows reach 1.41, which a frozen
                // loop would compound every pass).
                let wa = (std::f32::consts::PI * phase).sin().powi(2);
                let wb = (std::f32::consts::PI * p2).sin().powi(2);
                a * wa + b * wb
            };
            let yl = read(&self.lines[0], dl, self.shift_phase);
            let yr = read(&self.lines[1], dr, self.shift_phase);
            self.shift_phase = (self.shift_phase + (1.0 - ratio) / win).rem_euclid(1.0);
            // Feedback path: filters (with a resonant peak at the high cut), drive, diffusion.
            let mut fl = self.peak[0].process(self.hc[0].process(self.lc[0].highpass(yl)));
            let mut fr = self.peak[1].process(self.hc[1].process(self.lc[1].highpass(yr)));
            let drive = if md[4] != 0.0 { crate::dsp::db_to_gain((self.drive + md[4] * 24.0).clamp(0.0, 36.0)) } else { drive };
            if drive > 1.0012 {
                fl = (fl * drive).tanh() / drive.sqrt();
                fr = (fr * drive).tanh() / drive.sqrt();
            }
            if self.diffusion > 0.0 {
                let g = self.diffusion * 0.7;
                fl = self.diff[0].process(fl, g);
                fr = self.diff[1].process(fr, g);
            }
            let (fb, cr) = if self.freeze { (1.0, 0.0) } else { ((self.feedback + md[1] * 0.5).clamp(0.0, 0.98), self.cross) };
            let (mut fl2, mut fr2) = (fl * fb + fr * cr, fr * fb + fl * cr);
            if tape {
                fl2 = (fl2 * 1.2).tanh() / 1.2;
                fr2 = (fr2 * 1.2).tanh() / 1.2;
            }
            // The resonant peak (up to +12 dB), cross feedback and modulated feedback can make the loop gain
            // exceed 1: a soft limiter in the loop (transparent below full scale, bounded at ±4) keeps the
            // repeats from running away.
            fl2 = loop_limit(fl2);
            fr2 = loop_limit(fr2);
            // Freeze: the loop keeps its content and takes no new input.
            let (il, ir) = if self.freeze { (0.0, 0.0) } else { (l[i], r[i]) };
            if self.freeze {
                self.lines[0].write(loop_limit(yl));
                self.lines[1].write(loop_limit(yr));
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
            let (mid, side) = ((yl + yr) * 0.5, (yl - yr) * 0.5 * (self.width + md[6]).clamp(0.0, 2.0));
            let (wl, wr) = ((mid + side) * duck, (mid - side) * duck);
            let mix = (self.mix + md[7] * 0.5).clamp(0.0, 1.0);
            l[i] = l[i] * (1.0 - mix) + wl * mix;
            r[i] = r[i] * (1.0 - mix) + wr * mix;
        }
        self.tele[2] = peak(l, r);
    }
}

pub fn delay_def() -> EffectDef {
    EffectDef {
        id: "delay",
        name: "Echo Cavern (delay)",
        category: "Delay",
        description:
            "Delay: tempo-synced or free, stereo / ping-pong / tape, cross feedback, resonant filters, drive and diffusion in the loop, pitch-shifted repeats, ducking, freeze, width, and a modulation matrix (two LFOs and an envelope follower routed through four slots to time, feedback, filters, drive, pitch, width or mix).",
        params: DELAY_PARAMS,
        uses_sidechain: false,
        create: |rate| {
            Box::new(Delay {
                rate,
                tele: [375.0, 0.0, 0.0],
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
                lowcut: 150.0,
                mlfo: [(0, 0.5, 0), (0, 2.0, 1)],
                slots: [(0, 0, 0.0), (0, 3, 0.0), (0, 1, 0.0), (0, 7, 0.0)],
                mlfos: [Lfo::default(), Lfo::default()],
                menv: 0.0,
                mod_freqs: (150.0, 8000.0),
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
        name: "Coven (chorus)",
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
        name: "Quicksilver (flanger)",
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
        name: "Moonphase (phaser)",
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
    /// The modulation, smoothed over ~1.5 ms: a square wave's edges would otherwise click.
    sm: f32,
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
        let k = 1.0 - (-1.0 / (0.0015 * self.rate)).exp();
        for i in 0..l.len() {
            let raw = self.lfo.next(hz, self.rate, self.shape);
            self.sm += (raw - self.sm) * k;
            let m = self.sm;
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
        name: "Wingbeat (tremolo / auto-pan)",
        category: "Modulation",
        description: "Tempo-synced volume or pan modulation.",
        params: TREM_PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(Tremolo { rate, bpm: 120.0, sync: 5, speed: 4.0, depth: 0.6, shape: LfoShape::Sine, pan: false, lfo: Lfo::default(), sm: 0.0 }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Freeze with pitch-shifted repeats: the frozen loop keeps its level instead of growing every pass
    /// (it ran away to infinity before).
    #[test]
    fn frozen_pitched_delay_stays_bounded() {
        let mut p = std::collections::BTreeMap::new();
        for (k, v) in [("sync", 0.0), ("time", 40.0), ("feedback", 0.9), ("pitch", 7.0), ("mix", 1.0), ("reso", 1.0)] {
            p.insert(k.to_string(), v);
        }
        let mut e = crate::effects::create("delay", 48000.0, &p).unwrap();
        let (mut l, mut r) = (vec![0.0f32; 1024], vec![0.0f32; 1024]);
        let mut peak = 0.0f32;
        for b in 0..500 {
            if b == 20 {
                e.set_param("freeze", 1.0);
            }
            for k in 0..1024 {
                let x = if b < 20 { ((k * 7919 + b * 13) % 1000) as f32 / 1000.0 - 0.5 } else { 0.0 };
                l[k] = x;
                r[k] = x;
            }
            e.process(&mut l, &mut r, None);
            peak = l.iter().chain(r.iter()).fold(peak, |m, x| m.max(x.abs()));
            assert!(peak.is_finite());
        }
        assert!(peak < 8.0, "frozen loop grew to {peak}");
    }

    /// The delay's modulation matrix: a square LFO routed to the feedback changes how long the echoes of a
    /// burst ring; a slot without a source does nothing.
    #[test]
    fn delay_matrix_moves_its_target() {
        let rate = 48000.0;
        let run = |slot: Option<(f32, f32, f32)>| {
            let mut p = std::collections::BTreeMap::new();
            p.insert("sync".into(), 0.0);
            p.insert("time".into(), 50.0);
            p.insert("feedback".into(), 0.6);
            p.insert("mix".into(), 1.0);
            p.insert("mod".into(), 0.0);
            p.insert("lfo1_rate".into(), 0.02);
            p.insert("lfo1_shape".into(), 2.0);
            if let Some((src, dst, amt)) = slot {
                p.insert("m1_src".into(), src);
                p.insert("m1_dst".into(), dst);
                p.insert("m1_amt".into(), amt);
            }
            let mut e = crate::effects::create("delay", rate, &p).unwrap();
            let mut rng = crate::dsp::Rng::new(3);
            let n = (rate * 2.0) as usize;
            let mut l: Vec<f32> = (0..n).map(|i| if i < 4800 { rng.bipolar() * 0.3 } else { 0.0 }).collect();
            let mut r = l.clone();
            for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
                e.process(a, b, None);
            }
            l[24000..].iter().map(|x| x * x).sum::<f32>()
        };
        let off = run(None);
        assert_eq!(off, run(Some((0.0, 1.0, 100.0))), "a slot without a source does nothing");
        let moved = run(Some((1.0, 1.0, 100.0)));
        let ratio = (moved / off.max(1e-12)).max(off / moved.max(1e-12));
        assert!(ratio > 4.0, "LFO on feedback: tail energy {moved} vs {off}");
    }
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

    /// Extra features: freeze holds the loop with no decay; ducking lowers echoes under the input;
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
