//! Tracking and bus processors built from classic circuits:
//!
//! - **Dragon Class-A Strip** – a Class-A transformer microphone preamp (the 1073 school: input
//!   transformer whose core saturates on low frequencies, a single-ended class-A gain stage, output
//!   transformer, stepped inductor-style EQ with high-pass) feeding a tube opto compressor (the CL-1B school:
//!   electro-optical cell with program-dependent attack and two-stage, memory-laden release; fixed or manual
//!   timing; a tube make-up stage). Zero latency, for recording through.
//! - **Dragon Drum Bus** – an all-in-one drum processor: punch (transients), smack (parallel crush), low end,
//!   body, crack and air, drive, a clipper and width, each a single macro.

use super::dynamics;
use super::{Effect, EffectDef, ParamInfo};
use crate::dsp::{db_to_gain, gain_to_db, Biquad, BiquadKind};

// ============================================================================================ class-A strip

pub const HPF_FREQS: &[&str] = &["Off", "50 Hz", "80 Hz", "160 Hz", "300 Hz"];
pub const LOW_FREQS: &[&str] = &["35 Hz", "60 Hz", "110 Hz", "220 Hz"];
pub const MID_FREQS: &[&str] = &["360 Hz", "700 Hz", "1.6 kHz", "3.2 kHz", "4.8 kHz", "7.2 kHz"];
const HPF_HZ: [f32; 5] = [0.0, 50.0, 80.0, 160.0, 300.0];
const LOW_HZ: [f32; 4] = [35.0, 60.0, 110.0, 220.0];
const MID_HZ: [f32; 6] = [360.0, 700.0, 1600.0, 3200.0, 4800.0, 7200.0];

const STRIP_PARAMS: &[ParamInfo] = &[
    ParamInfo::lin("ns", "Noise suppressor", 0.0, 100.0, 0.0, "%"),
    ParamInfo::lin("drive", "Preamp drive", 0.0, 30.0, 6.0, "dB"),
    ParamInfo::toggle("eq_on", "EQ in", true),
    ParamInfo::choice("hpf", "EQ high-pass", HPF_FREQS, 0.0),
    ParamInfo::choice("low_freq", "EQ low freq", LOW_FREQS, 2.0),
    ParamInfo::lin("low_gain", "EQ low gain", -16.0, 16.0, 0.0, "dB"),
    ParamInfo::choice("mid_freq", "EQ mid freq", MID_FREQS, 3.0),
    ParamInfo::lin("mid_gain", "EQ mid gain", -18.0, 18.0, 0.0, "dB"),
    ParamInfo::lin("high_gain", "EQ high gain (12 kHz)", -16.0, 16.0, 0.0, "dB"),
    ParamInfo::toggle("comp_on", "Comp in", true),
    ParamInfo::lin("threshold", "Comp threshold", -40.0, 0.0, -18.0, "dB"),
    ParamInfo::lin("ratio", "Comp ratio", 2.0, 10.0, 4.0, ":1"),
    ParamInfo::choice("comp_mode", "Comp timing", &["Fixed", "Fix / Man", "Manual"], 0.0),
    ParamInfo::log("attack", "Comp attack", 0.5, 300.0, 10.0, "ms"),
    ParamInfo::log("release", "Comp release", 50.0, 10000.0, 400.0, "ms"),
    ParamInfo::lin("comp_gain", "Comp gain", 0.0, 30.0, 4.0, "dB"),
    ParamInfo::choice("order", "Order", &["Pre > EQ > Comp", "Pre > Comp > EQ"], 0.0),
    ParamInfo::lin("output", "Output", -24.0, 12.0, -6.0, "dB"),
];

// ------------------------------------------------------------------------------------- noise suppressor

/// Band edges of the suppressor (Hz): below 250, 250 – 1.2 k, 1.2 k – 5 k, above 5 k.
const NS_EDGES: [f32; 3] = [250.0, 1200.0, 5000.0];

/// A one-fader broadband noise suppressor: four bands, each tracking its own noise floor
/// (the minimum of its level over a sliding 2 s window, rising at most 30 dB/s, kept within -96 … -40 dBFS
/// so that program material louder than that is never taken for noise) and pulled down when its level sits
/// near that floor. The fader sets the margin above the floor (3 … 12 dB), the
/// expansion ratio and the depth (up to 30 dB). The bands are split complementarily (each the remainder of
/// the previous minus a low-pass), so they sum back to the input exactly: at 0 % it is bit-transparent, and
/// it adds no latency (usable while tracking).
#[derive(Clone)]
struct NoiseSuppressor {
    amount: f32,
    lp: [[Biquad; 2]; 3],
    /// Per band: power envelope, noise floor (dB), applied gain (linear).
    env: [f32; 4],
    floor: [f32; 4],
    gain: [f32; 4],
    k_att: f32,
    k_rel: f32,
    rise: f32,
    /// Sliding minimum: per band the minima of the last 8 slots of 250 ms, the current slot's, and the
    /// samples left in it.
    mins: [[f32; 8]; 4],
    slot_min: [f32; 4],
    slot: usize,
    slot_len: usize,
    left: usize,
    g_open: f32,
    g_close: f32,
    /// Deepest band attenuation in the last block (dB, positive).
    red: f32,
}

impl NoiseSuppressor {
    fn new(rate: f32) -> Self {
        let mut lp = [[Biquad::default(); 2]; 3];
        for (k, f) in NS_EDGES.iter().enumerate() {
            for b in &mut lp[k] {
                b.set(BiquadKind::LowPass, f.min(rate * 0.45), std::f32::consts::FRAC_1_SQRT_2, 0.0, rate);
            }
        }
        let k = |ms: f32| (-1.0 / (ms * 0.001 * rate)).exp();
        NoiseSuppressor {
            amount: 0.0,
            lp,
            env: [0.0; 4],
            floor: [-40.0; 4],
            gain: [1.0; 4],
            k_att: k(5.0),
            k_rel: k(25.0),
            rise: 30.0 / rate,
            mins: [[-40.0; 8]; 4],
            slot_min: [0.0; 4],
            slot: 0,
            slot_len: (rate * 0.25) as usize,
            left: (rate * 0.25) as usize,
            g_open: k(2.0),
            g_close: k(40.0),
            red: 0.0,
        }
    }

    fn reset(&mut self) {
        self.env = [0.0; 4];
        self.floor = [-40.0; 4];
        self.gain = [1.0; 4];
        self.red = 0.0;
        self.mins = [[-40.0; 8]; 4];
        self.slot_min = [0.0; 4];
        self.left = self.slot_len;
        for b in self.lp.iter_mut().flatten() {
            b.reset();
        }
    }

    #[inline]
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        // Split (complementary: the bands sum to the input).
        let mut bl = [0.0f32; 4];
        let mut br = [0.0f32; 4];
        let (mut rl, mut rr) = (l, r);
        for k in 0..3 {
            bl[k] = self.lp[k][0].process(rl);
            br[k] = self.lp[k][1].process(rr);
            rl -= bl[k];
            rr -= br[k];
        }
        bl[3] = rl;
        br[3] = rr;
        if self.amount <= 0.0 {
            return (l, r);
        }
        let a = self.amount;
        let margin = 3.0 + 9.0 * a;
        let ratio = 1.5 + 2.5 * a;
        let depth = 30.0 * a;
        let (mut ol, mut or) = (0.0, 0.0);
        let mut red = self.red;
        // Close a 250 ms slot of the sliding minimum.
        self.left -= 1;
        let close = self.left == 0;
        if close {
            self.left = self.slot_len;
            self.slot = (self.slot + 1) % 8;
        }
        for k in 0..4 {
            let p = bl[k] * bl[k] + br[k] * br[k];
            let c = if p > self.env[k] { self.k_att } else { self.k_rel };
            self.env[k] = p + c * (self.env[k] - p);
            let db = 10.0 * (self.env[k] * 0.5 + 1e-12).log10();
            // Noise floor: the minimum over the last 2 s, rising at most 30 dB/s, within -96 … -40 dBFS.
            self.slot_min[k] = self.slot_min[k].min(db);
            if close {
                self.mins[k][self.slot] = self.slot_min[k];
                self.slot_min[k] = 0.0;
            }
            let target = self.mins[k].iter().fold(self.slot_min[k], |a, &b| a.min(b)).clamp(-96.0, -40.0);
            self.floor[k] = if target < self.floor[k] { target } else { (self.floor[k] + self.rise).min(target) };
            // A running minimum sits a few dB under the noise's mean level: +4 dB bias compensation.
            let under = self.floor[k] + 4.0 + margin - db;
            let att = if under > 0.0 { (under * (ratio - 1.0)).min(depth) } else { 0.0 };
            let want = db_to_gain(-att);
            let c = if want > self.gain[k] { self.g_open } else { self.g_close };
            self.gain[k] = want + c * (self.gain[k] - want);
            red = red.max(-gain_to_db(self.gain[k].max(1e-6)));
            ol += bl[k] * self.gain[k];
            or += br[k] * self.gain[k];
        }
        self.red = red;
        (ol, or)
    }
}

/// One channel of the preamp: input transformer → class-A stage → output transformer.
#[derive(Clone, Copy, Default)]
struct Preamp {
    rate: f32,
    // Input transformer: the low band (core flux) saturates first.
    lf: f32,
    lf_a: f32,
    // Class-A stage coupling.
    dc_x: f32,
    dc_y: f32,
    // Output transformer: slight HF loss and LF resonance.
    hf: f32,
    hf_a: f32,
    bump: Biquad,
}

impl Preamp {
    fn new(rate: f32) -> Self {
        Preamp {
            rate,
            lf_a: (-std::f32::consts::TAU * 150.0 / rate).exp(),
            hf_a: (-std::f32::consts::TAU * 28000.0f32.min(rate * 0.45) / rate).exp(),
            bump: Biquad::new(BiquadKind::Peak, 40.0, 0.8, 0.6, rate),
            ..Default::default()
        }
    }

    #[inline]
    fn process(&mut self, x: f32, drive: f32) -> f32 {
        let x = x * drive;
        // Input transformer: the flux of the low band saturates the core (third harmonic on bass).
        self.lf = x + (self.lf - x) * self.lf_a;
        let low = self.lf;
        let high = x - low;
        let low = (low * 1.6).tanh() / 1.6;
        let x = low + high;
        // Class-A single-ended stage: asymmetric → second harmonic grows smoothly with level.
        let y = x + 0.08 * x * x - 0.01 * x * x * x;
        let y = y / (1.0 + 0.15 * y.abs());
        // Coupling capacitor removes the DC the asymmetry adds.
        let r = 1.0 - std::f32::consts::TAU * 8.0 / self.rate;
        self.dc_y = y - self.dc_x + r * self.dc_y;
        self.dc_x = y;
        // Output transformer: soft limit, a little top-end loss, low resonance.
        let z = (self.dc_y * 0.6).tanh() / 0.6;
        self.hf = z + (self.hf - z) * self.hf_a;
        self.bump.process(self.hf)
    }
}

/// The compressor's timing switch (CL 1B: Fixed, Fix/Man, Manual).
#[derive(Clone, Copy)]
enum Timing {
    Fixed,
    FixMan(f32),
    Manual(f32, f32),
}

/// First-order high-pass (the third pole of the 18 dB/oct filter).
#[derive(Clone, Copy, Default)]
struct Hp1 {
    a: f32,
    x1: f32,
    y1: f32,
}

impl Hp1 {
    #[cfg(test)]
    fn new(f: f32, rate: f32) -> Self {
        Hp1 { a: (-std::f32::consts::TAU * f / rate).exp(), x1: 0.0, y1: 0.0 }
    }
    /// Retunes keeping the state.
    fn set(&mut self, f: f32, rate: f32) {
        self.a = (-std::f32::consts::TAU * f / rate).exp();
    }
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = (1.0 + self.a) * 0.5 * (x - self.x1) + self.a * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }
}

/// Electro-optical gain cell (T4-style): the light follows the control signal with a program-dependent
/// attack; the cell's resistance recovers in two stages – fast at first, then slowly, and more slowly the
/// longer and harder it was lit (memory).
#[derive(Clone, Copy, Default)]
struct OptoCell {
    light: f32,
    fast: f32,
    slow: f32,
    memory: f32,
}

impl OptoCell {
    /// `drive_db` = dB over threshold × (1 − 1/ratio): the reduction asked for. Returns the reduction applied.
    #[inline]
    fn next(&mut self, want: f32, rate: f32, timing: Timing) -> f32 {
        let dt = 1.0 / rate;
        // CL 1B manual: Fixed = 1 ms attack / 50 ms release (the opto cell's own slow recovery and memory
        // still follow); Fix/Man = the fixed attack, fast 50 ms release for short peaks and the manual release
        // for long ones; Manual = attack 0.5–300 ms, release 0.05–10 s.
        let (att, rel_fast, rel_slow) = match timing {
            Timing::Manual(a, r) => (a * 0.001, r * 0.001, r * 0.004),
            Timing::FixMan(r) => (0.001, 0.05, r * 0.001),
            Timing::Fixed => (0.001, 0.05, 0.5 + 2.5 * self.memory),
        };
        let a_c = (-dt / att.max(1e-4)).exp();
        if want > self.light {
            self.light = want + (self.light - want) * a_c;
        } else {
            self.light = want;
        }
        // Two-stage release towards the light level.
        let tgt = self.light;
        let f_c = (-dt / rel_fast).exp();
        let s_c = (-dt / rel_slow).exp();
        self.fast = if tgt > self.fast { tgt } else { tgt + (self.fast - tgt) * f_c };
        self.slow = if tgt > self.slow { tgt } else { tgt + (self.slow - tgt) * s_c };
        // Memory builds while compressing, fades over seconds.
        let m_t = (want / 12.0).min(1.0);
        self.memory += (m_t - self.memory) * if m_t > self.memory { dt / 2.0 } else { dt / 6.0 };
        0.6 * self.fast + 0.4 * self.slow
    }
}

pub struct ClassAStrip {
    rate: f32,
    drive: f32,
    eq_on: bool,
    hpf: [Biquad; 2],
    hpf1: [Hp1; 2],
    hpf_on: bool,
    low: [Biquad; 2],
    mid: [Biquad; 2],
    high: [Biquad; 2],
    eq_p: (usize, usize, f32, usize, f32, f32),
    comp_on: bool,
    threshold: f32,
    ratio: f32,
    mode: usize,
    attack: f32,
    release: f32,
    comp_gain: f32,
    comp_first: bool,
    /// Keeps the detected level steady through a low-frequency wave's cycle.
    steady: crate::dsp::PeakHold,
    output: f32,
    pre: [Preamp; 2],
    cell: OptoCell,
    tube: [crate::dsp::tube::TubeStage; 2],
    gr: f32,
    ns: NoiseSuppressor,
    tele: [f32; 5],
}

impl ClassAStrip {
    pub fn new(rate: f32) -> Self {
        let mut tube = crate::dsp::tube::TubeStage::new(crate::dsp::tube::TUBES[1], rate);
        tube.bias = -0.2;
        let mut s = ClassAStrip {
            rate,
            drive: db_to_gain(6.0),
            eq_on: true,
            hpf: [Biquad::default(); 2],
            hpf1: [Hp1::default(); 2],
            hpf_on: false,
            low: [Biquad::default(); 2],
            mid: [Biquad::default(); 2],
            high: [Biquad::default(); 2],
            eq_p: (0, 2, 0.0, 3, 0.0, 0.0),
            comp_on: true,
            threshold: -18.0,
            ratio: 4.0,
            mode: 0,
            attack: 10.0,
            release: 400.0,
            comp_gain: db_to_gain(4.0),
            comp_first: false,
            steady: crate::dsp::PeakHold::new(30.0, rate),
            output: db_to_gain(-6.0),
            pre: [Preamp::new(rate); 2],
            cell: OptoCell::default(),
            tube: [tube; 2],
            gr: 0.0,
            ns: NoiseSuppressor::new(rate),
            tele: [0.0; 5],
        };
        s.update_eq();
        s
    }

    fn update_eq(&mut self) {
        let (h, lf, lg, mf, mg, hg) = self.eq_p;
        let r = self.rate;
        self.hpf_on = h > 0;
        if h > 0 {
            // 18 dB/oct (third-order Butterworth) like the 1073's: a 2nd-order section (Q 1) and a 1st-order one.
            for ch in 0..2 {
                self.hpf[ch].set(BiquadKind::HighPass, HPF_HZ[h], 1.0, 0.0, r);
                self.hpf1[ch].set(HPF_HZ[h], r);
            }
        }
        // Inductor-style shelves have a broad bump before the shelf; a low Q gets close.
        for ch in 0..2 {
            self.low[ch].set(BiquadKind::LowShelf, LOW_HZ[lf.min(3)], 0.55, lg, r);
            // The mid band's Q tightens slightly with gain (as the inductor bands do).
            let q = 1.0 + mg.abs() / 18.0 * 0.6;
            self.mid[ch].set(BiquadKind::Peak, MID_HZ[mf.min(5)], q, mg, r);
            self.high[ch].set(BiquadKind::HighShelf, 12000.0f32.min(r * 0.45), 0.6, hg, r);
        }
    }

    #[inline]
    fn eq(&mut self, ch: usize, x: f32) -> f32 {
        if !self.eq_on {
            return x;
        }
        let mut y = x;
        if self.hpf_on {
            y = self.hpf[ch].process(y);
            y = self.hpf1[ch].process(y);
        }
        y = self.low[ch].process(y);
        y = self.mid[ch].process(y);
        self.high[ch].process(y)
    }
}

impl Effect for ClassAStrip {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "ns" => self.ns.amount = (v / 100.0).clamp(0.0, 1.0),
            "drive" => self.drive = db_to_gain(v),
            "eq_on" => self.eq_on = v > 0.5,
            "hpf" => self.eq_p.0 = (v.round() as usize).min(4),
            "low_freq" => self.eq_p.1 = (v.round() as usize).min(3),
            "low_gain" => self.eq_p.2 = v,
            "mid_freq" => self.eq_p.3 = (v.round() as usize).min(5),
            "mid_gain" => self.eq_p.4 = v,
            "high_gain" => self.eq_p.5 = v,
            "comp_on" => self.comp_on = v > 0.5,
            "threshold" => self.threshold = v,
            "ratio" => self.ratio = v.max(1.0),
            "comp_mode" => self.mode = v.round() as usize,
            "attack" => self.attack = v,
            "release" => self.release = v,
            "comp_gain" => self.comp_gain = db_to_gain(v),
            "order" => self.comp_first = v >= 0.5,
            "output" => self.output = db_to_gain(v),
            _ => return,
        }
        if matches!(id, "hpf" | "low_freq" | "low_gain" | "mid_freq" | "mid_gain" | "high_gain") {
            self.update_eq();
        }
    }
    fn reset(&mut self) {
        self.cell = OptoCell::default();
        self.tube.iter_mut().for_each(|t| t.reset());
        self.ns.reset();
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let manual = match self.mode {
            0 => Timing::Fixed,
            1 => Timing::FixMan(self.release),
            _ => Timing::Manual(self.attack, self.release),
        };
        let mut peak_gr = 0.0f32;
        let (mut pin, mut pout) = (0.0f32, 0.0f32);
        self.ns.red = 0.0;
        for i in 0..l.len() {
            pin = pin.max(l[i].abs().max(r[i].abs()));
            // Noise suppression first: the preamp's gain would otherwise lift the noise with the signal.
            let (x, y) = self.ns.process(l[i], r[i]);
            let mut a = self.pre[0].process(x, self.drive);
            let mut b = self.pre[1].process(y, self.drive);
            if !self.comp_first {
                a = self.eq(0, a);
                b = self.eq(1, b);
            }
            if self.comp_on {
                // Feed-forward detector into the opto cell, soft knee.
                let level = gain_to_db(self.steady.next(a.abs().max(b.abs())).max(1e-6));
                let over = level - self.threshold;
                let knee = 8.0;
                let over = if over < -knee / 2.0 {
                    0.0
                } else if over < knee / 2.0 {
                    (over + knee / 2.0).powi(2) / (2.0 * knee)
                } else {
                    over
                };
                let want = over * (1.0 - 1.0 / self.ratio);
                let red = self.cell.next(want, self.rate, manual);
                peak_gr = peak_gr.max(red);
                let g = db_to_gain(-red) * self.comp_gain;
                a = self.tube[0].process(a * g * 0.5) * 2.0;
                b = self.tube[1].process(b * g * 0.5) * 2.0;
            }
            if self.comp_first {
                a = self.eq(0, a);
                b = self.eq(1, b);
            }
            // The line amp's rails (~ +24 dBFS): inaudible at normal levels, keeps extreme settings bounded.
            l[i] = 16.0 * (a * self.output / 16.0).tanh();
            r[i] = 16.0 * (b * self.output / 16.0).tanh();
            pout = pout.max(l[i].abs().max(r[i].abs()));
        }
        self.gr = peak_gr;
        self.tele = [pin, peak_gr, pout, gain_to_db(self.drive), self.ns.red];
    }
    fn gain_reduction(&self) -> f32 {
        self.gr
    }
    fn telemetry(&self) -> &[f32] {
        &self.tele
    }
}

pub fn class_a_strip_def() -> EffectDef {
    EffectDef {
        id: "class_a_strip",
        name: "Golem Strip (class-A channel strip)",
        category: "Channel strip",
        description: "Recording channel: a one-fader noise suppressor, Class-A transformer mic preamp with stepped inductor-style EQ into a tube opto compressor (program-dependent opto attack, two-stage release with memory, fixed or manual timing). Zero latency – track through it.",
        params: STRIP_PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(ClassAStrip::new(rate)),
    }
}

// ============================================================================================ drum bus

/// Drum Bus, laid out as modules: Input with target meter, Drive
/// (three colours, high-pass after it), Harmonics (low-end focus at 50 or 80 Hz), Compression (two modes,
/// mix), Lift, a five-band Harmonic EQ in 1 dB steps, four Parallels (Sub, Low, Loud, Grit) and Output.
pub const DRUM_COLOURS: &[&str] = &["A (warm)", "B (tape)", "C (crunch)"];
/// The Harmonic EQ's fixed bands (Hz). Our own choice of frequencies.
pub const HEQ_FREQS: [f32; 5] = [60.0, 180.0, 900.0, 3500.0, 10000.0];

const DRUM_PARAMS: &[ParamInfo] = &[
    ParamInfo::lin("input", "Input", -24.0, 24.0, 0.0, "dB"),
    ParamInfo::lin("drive", "Drive", 0.0, 100.0, 20.0, "%"),
    ParamInfo::choice("colour", "Drive colour", DRUM_COLOURS, 0.0),
    ParamInfo::log("drive_hp", "Drive high-pass", 10.0, 120.0, 20.0, "Hz"),
    ParamInfo::lin("harmonics", "Harmonics", 0.0, 100.0, 20.0, "%"),
    ParamInfo::choice("focus", "Harmonics focus", &["50 Hz", "80 Hz"], 0.0),
    ParamInfo::choice("comp_mode", "Compression mode", &["A (glue)", "B (punch)"], 0.0),
    ParamInfo::lin("comp_mix", "Compression", 0.0, 100.0, 30.0, "%"),
    ParamInfo::lin("lift", "Lift", 0.0, 100.0, 15.0, "%"),
    ParamInfo::lin("heq1", "Harmonic EQ 60 Hz", 0.0, 10.0, 0.0, "dB"),
    ParamInfo::lin("heq2", "Harmonic EQ 180 Hz", 0.0, 10.0, 0.0, "dB"),
    ParamInfo::lin("heq3", "Harmonic EQ 900 Hz", 0.0, 10.0, 0.0, "dB"),
    ParamInfo::lin("heq4", "Harmonic EQ 3.5 kHz", 0.0, 10.0, 0.0, "dB"),
    ParamInfo::lin("heq5", "Harmonic EQ 10 kHz", 0.0, 10.0, 0.0, "dB"),
    ParamInfo::lin("sub", "Parallel: Sub", 0.0, 100.0, 0.0, "%"),
    ParamInfo::lin("low", "Parallel: Low", 0.0, 100.0, 0.0, "%"),
    ParamInfo::lin("loud", "Parallel: Loud", 0.0, 100.0, 0.0, "%"),
    ParamInfo::lin("grit", "Parallel: Grit", 0.0, 100.0, 0.0, "%"),
    ParamInfo::lin("output", "Output", -24.0, 12.0, 0.0, "dB"),
];

const NP: usize = 19;

#[derive(Clone, Copy, Default)]
struct Pair([Biquad; 2]);

impl Pair {
    fn new(kind: BiquadKind, f: f32, q: f32, g: f32, r: f32) -> Self {
        Pair([Biquad::new(kind, f, q, g, r); 2])
    }
    /// Retunes in place, keeping the filters' state (a new pair would click while a knob turns).
    fn set(&mut self, kind: BiquadKind, f: f32, q: f32, g: f32, r: f32) {
        for b in &mut self.0 {
            b.set(kind, f, q, g, r);
        }
    }
    #[inline]
    fn run(&mut self, ch: usize, x: f32) -> f32 {
        self.0[ch].process(x)
    }
}

pub struct DrumBus {
    rate: f32,
    p: [f32; NP],
    sat: Box<dyn Effect>,
    drive_hp: Pair,
    harm_bp: Pair,
    harm_lp: Pair,
    comp: Box<dyn Effect>,
    lift_hp: Pair,
    heq: [Pair; 5],
    /// The harmonic EQ amounts actually applied (gliding towards the knobs).
    heq_amt: [f32; 5],
    sub_lp: Pair,
    low_lp: Pair,
    low_comp: Box<dyn Effect>,
    loud_comp: Box<dyn Effect>,
    grit_hp: Pair,
    grit_ts: Box<dyn Effect>,
    buf: [Vec<f32>; 2],
    par: [Vec<f32>; 2],
    acc: [Vec<f32>; 2],
    rms_in: f32,
    rms_out: f32,
    /// Telemetry: input peak, output peak, compression GR (dB), input RMS, output RMS, last hit.
    tele: [f32; 6],
}

impl DrumBus {
    pub fn new(rate: f32) -> Self {
        let mut d = DrumBus {
            rate,
            p: std::array::from_fn(|k| DRUM_PARAMS[k].default),
            sat: (super::color::saturator_def().create)(rate),
            drive_hp: Pair::default(),
            harm_bp: Pair::default(),
            harm_lp: Pair::default(),
            comp: (dynamics::compressor_def().create)(rate),
            lift_hp: Pair::new(BiquadKind::HighPass, 2500.0, 0.7, 0.0, rate),
            heq: [Pair::default(); 5],
            heq_amt: [0.0; 5],
            sub_lp: Pair::new(BiquadKind::LowPass, 70.0, 0.7, 0.0, rate),
            low_lp: Pair::new(BiquadKind::LowPass, 160.0, 0.7, 0.0, rate),
            low_comp: (dynamics::compressor_def().create)(rate),
            loud_comp: (dynamics::compressor_def().create)(rate),
            grit_hp: Pair::new(BiquadKind::HighPass, 1200.0, 0.7, 0.0, rate),
            grit_ts: (dynamics::transient_def().create)(rate),
            buf: [Vec::new(), Vec::new()],
            par: [Vec::new(), Vec::new()],
            acc: [Vec::new(), Vec::new()],
            rms_in: 0.0,
            rms_out: 0.0,
            tele: [0.0; 6],
        };
        for (c, set) in [
            (&mut d.low_comp, [("threshold", -30.0), ("ratio", 8.0), ("attack", 15.0), ("release", 120.0), ("makeup", 14.0)]),
            (&mut d.loud_comp, [("threshold", -36.0), ("ratio", 20.0), ("attack", 0.5), ("release", 60.0), ("makeup", 22.0)]),
        ] {
            for (k, v) in set {
                c.set_param(k, v);
            }
        }
        d.grit_ts.set_param("attack", 80.0);
        d.grit_ts.set_param("sustain", -40.0);
        d.grit_ts.set_param("att_dur", 12.0);
        d.update();
        d
    }

    fn v(&self, id: &str) -> f32 {
        DRUM_PARAMS.iter().position(|p| p.id == id).map(|k| self.p[k]).unwrap_or(0.0)
    }

    fn update(&mut self) {
        let r = self.rate;
        let drive = self.v("drive") / 100.0;
        // Drive colours: a = warm triode, b = tape, c = hard pentode crunch.
        let (mode, tube) = match self.v("colour").round() as i32 {
            1 => (1.0, 0.0),
            2 => (0.0, 5.0),
            _ => (0.0, 0.0),
        };
        self.sat.set_param("mode", mode);
        self.sat.set_param("tube", tube);
        self.sat.set_param("drive", drive * 24.0);
        self.sat.set_param("output", -drive * 10.0);
        self.sat.set_param("mix", 1.0);
        self.drive_hp.set(BiquadKind::HighPass, self.v("drive_hp"), 0.7, 0.0, r);
        let focus = if self.v("focus") >= 0.5 { 80.0 } else { 50.0 };
        self.harm_bp.set(BiquadKind::BandPass, focus, 1.2, 0.0, r);
        self.harm_lp.set(BiquadKind::LowPass, focus * 4.0, 0.7, 0.0, r);
        // Compression modes: A glue (bus style), B punch (slower attack lets the hit through).
        let b = self.v("comp_mode") >= 0.5;
        for (k, v) in
            [("style", if b { 6.0 } else { 5.0 }), ("threshold", -22.0), ("ratio", if b { 6.0 } else { 4.0 }), ("attack", if b { 25.0 } else { 10.0 }), ("release", if b { 80.0 } else { 150.0 }), ("auto_makeup", 1.0)]
        {
            self.comp.set_param(k, v);
        }
        for (k, f) in HEQ_FREQS.iter().enumerate() {
            self.heq[k].set(BiquadKind::BandPass, f.min(r * 0.45), 0.9, 0.0, r);
        }
    }
}

impl Effect for DrumBus {
    fn set_param(&mut self, id: &str, v: f32) {
        if let Some(k) = DRUM_PARAMS.iter().position(|p| p.id == id) {
            // The Harmonic EQ moves in 1 dB steps.
            self.p[k] = if id.starts_with("heq") { v.round() } else { v };
            self.update();
        }
    }
    fn reset(&mut self) {
        for e in [&mut self.sat, &mut self.comp, &mut self.low_comp, &mut self.loud_comp, &mut self.grit_ts] {
            e.reset();
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let n = l.len();
        let ig = db_to_gain(self.v("input"));
        let mut pin = 0.0f32;
        let mut sq_in = 0.0f32;
        for i in 0..n {
            l[i] *= ig;
            r[i] *= ig;
            pin = pin.max(l[i].abs()).max(r[i].abs());
            sq_in += 0.5 * (l[i] * l[i] + r[i] * r[i]);
        }
        // Drive, then its high-pass.
        if self.v("drive") > 0.0 {
            self.sat.process(l, r, None);
        }
        for i in 0..n {
            l[i] = self.drive_hp.run(0, l[i]);
            r[i] = self.drive_hp.run(1, r[i]);
        }
        // Harmonics: the kick's fundamental region, rectified and saturated into upper harmonics.
        let harm = self.v("harmonics") / 100.0;
        if harm > 0.0 {
            for i in 0..n {
                for (ch, x) in [&mut l[i], &mut r[i]].into_iter().enumerate() {
                    let b = self.harm_bp.run(ch, *x);
                    let h = self.harm_lp.run(ch, (b * 6.0).tanh() * 0.5 + (b.abs() * 3.0).tanh() * 0.3);
                    *x += h * harm * 1.5;
                }
            }
        }
        // Compression, blended in parallel by the mix.
        let cmix = self.v("comp_mix") / 100.0;
        if cmix > 0.0 {
            for ch in 0..2 {
                self.buf[ch].clear();
                self.buf[ch].extend_from_slice(if ch == 0 { &*l } else { &*r });
            }
            let [bl, br] = &mut self.buf;
            self.comp.process(bl, br, None);
            for i in 0..n {
                l[i] += (self.buf[0][i] - l[i]) * cmix;
                r[i] += (self.buf[1][i] - r[i]) * cmix;
            }
        }
        // Lift: excitement across the spectrum, mostly the top (presence and transient detail).
        let lift = self.v("lift") / 100.0;
        if lift > 0.0 {
            for i in 0..n {
                for (ch, x) in [&mut l[i], &mut r[i]].into_iter().enumerate() {
                    let h = self.lift_hp.run(ch, *x);
                    *x += (h * 3.0).tanh() * 0.35 * lift + ((*x * 1.5).tanh() / 1.5 - *x) * 0.3 * lift;
                }
            }
        }
        // Harmonic EQ: each band adds saturated content of its region (gain in dB of that region).
        // The knobs move in 1 dB steps; the applied amount glides between steps (~5 ms) so a step is
        // heard as a change, not a click.
        let heq_target: [f32; 5] = std::array::from_fn(|k| db_to_gain(self.p[9 + k]) - 1.0);
        if heq_target.iter().chain(self.heq_amt.iter()).any(|g| *g > 1e-6) {
            let glide = 1.0 - (-1.0 / (0.005 * self.rate)).exp();
            for i in 0..n {
                for k in 0..5 {
                    self.heq_amt[k] += (heq_target[k] - self.heq_amt[k]) * glide;
                }
                for (ch, x) in [&mut l[i], &mut r[i]].into_iter().enumerate() {
                    let src = *x;
                    let mut add = 0.0;
                    for k in 0..5 {
                        if self.heq_amt[k] > 1e-6 {
                            let b = self.heq[k].run(ch, src);
                            add += ((b * 2.0).tanh() * 0.5 + b * 0.5) * self.heq_amt[k];
                        }
                    }
                    *x = src + add;
                }
            }
        }
        // Parallels: Sub (saturated sub), Low (squashed lows), Loud (crushed full band), Grit (transients).
        let (sub, low, loud, grit) = (self.v("sub") / 100.0, self.v("low") / 100.0, self.v("loud") / 100.0, self.v("grit") / 100.0);
        for ch in 0..2 {
            self.acc[ch].clear();
            self.acc[ch].resize(n, 0.0);
        }
        if sub > 0.0 {
            for i in 0..n {
                for ch in 0..2 {
                    let x = if ch == 0 { l[i] } else { r[i] };
                    let s = self.sub_lp.run(ch, x);
                    self.acc[ch][i] += (s * 4.0).tanh() * 0.5 * sub;
                }
            }
        }
        let par_with = |sel: f32, pre: &mut dyn FnMut(usize, f32) -> f32, comp: &mut Box<dyn Effect>, me: &mut [Vec<f32>; 2], acc: &mut [Vec<f32>; 2]| {
            if sel <= 0.0 {
                return;
            }
            for (ch, buf) in me.iter_mut().enumerate() {
                buf.clear();
                for i in 0..n {
                    buf.push(pre(ch, if ch == 0 { l[i] } else { r[i] }));
                }
            }
            let [a, b] = me;
            comp.process(a, b, None);
            for ch in 0..2 {
                for i in 0..n {
                    acc[ch][i] += me[ch][i] * sel * 0.6;
                }
            }
        };
        let low_lp = &mut self.low_lp;
        par_with(low, &mut |ch, x| low_lp.run(ch, x), &mut self.low_comp, &mut self.par, &mut self.acc);
        par_with(loud, &mut |_, x| x, &mut self.loud_comp, &mut self.par, &mut self.acc);
        let grit_hp = &mut self.grit_hp;
        par_with(grit, &mut |ch, x| (grit_hp.run(ch, x) * 3.0).tanh() * 0.6, &mut self.grit_ts, &mut self.par, &mut self.acc);
        let og = db_to_gain(self.v("output"));
        let mut pout = 0.0f32;
        let mut sq_out = 0.0f32;
        for i in 0..n {
            // Output stage rails (~ +24 dBFS) keep extreme settings bounded; inaudible at normal levels.
            l[i] = 16.0 * ((l[i] + self.acc[0][i]) * og / 16.0).tanh();
            r[i] = 16.0 * ((r[i] + self.acc[1][i]) * og / 16.0).tanh();
            pout = pout.max(l[i].abs()).max(r[i].abs());
            sq_out += 0.5 * (l[i] * l[i] + r[i] * r[i]);
        }
        // RMS over ~300 ms for the target meters.
        let a = (-(n as f32) / (0.3 * self.rate)).exp();
        self.rms_in = sq_in / n.max(1) as f32 + (self.rms_in - sq_in / n.max(1) as f32) * a;
        self.rms_out = sq_out / n.max(1) as f32 + (self.rms_out - sq_out / n.max(1) as f32) * a;
        self.tele = [pin, pout, self.comp.gain_reduction() * cmix, self.rms_in.sqrt(), self.rms_out.sqrt(), if pin > self.rms_in.sqrt() * 3.0 { 1.0 } else { 0.0 }];
    }
    fn gain_reduction(&self) -> f32 {
        self.comp.gain_reduction() * self.v("comp_mix") / 100.0
    }
    fn telemetry(&self) -> &[f32] {
        &self.tele
    }
}

pub fn drum_bus_def() -> EffectDef {
    EffectDef {
        id: "drum_bus",
        name: "Dragonforge (drum bus)",
        category: "Drums",
        description: "Drum bus processor: input with target meter, drive (three colours, high-pass), harmonics (50 / 80 Hz focus), compression (glue / punch, mix), lift, five-band harmonic EQ in 1 dB steps, parallel Sub / Low / Loud / Grit, output.",
        params: DRUM_PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(DrumBus::new(rate)),
    }
}

#[cfg(test)]
mod ns_tests {
    use super::*;

    /// A tone with hiss between its notes: at 0 % untouched; at 70 % the hiss gaps drop by well over
    /// 10 dB while the tone keeps its level.
    #[test]
    fn noise_suppressor_cleans_gaps_and_keeps_the_signal() {
        let rate = 48000.0;
        let mut seed = 1u32;
        let mut noise = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5
        };
        // 4 s: alternating 0.5 s of tone + hiss and 0.5 s of hiss alone.
        let n = (rate * 4.0) as usize;
        let sig: Vec<f32> = (0..n)
            .map(|i| {
                let on = (i / (rate as usize / 2)).is_multiple_of(2);
                let tone = if on { (i as f32 * 2.0 * std::f32::consts::PI * 440.0 / rate).sin() * 0.3 } else { 0.0 };
                tone + noise() * 0.006
            })
            .collect();
        let run = |amount: f32| {
            let mut ns = NoiseSuppressor::new(rate);
            ns.amount = amount;
            sig.iter().map(|&x| ns.process(x, x).0).collect::<Vec<f32>>()
        };
        let rms = |v: &[f32]| (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt();
        let half = rate as usize / 2;
        // Last gap (3.5 … 4 s, skipping its first 100 ms) and last tone (3 … 3.5 s).
        let gap = |v: &[f32]| rms(&v[7 * half + 4800..8 * half]);
        let tone = |v: &[f32]| rms(&v[6 * half + 4800..7 * half]);
        let off = run(0.0);
        assert_eq!(off, sig, "0 % is bit-transparent");
        let on = run(0.7);
        let gap_drop = 20.0 * (gap(&sig) / gap(&on)).log10();
        let tone_change = 20.0 * (tone(&on) / tone(&sig)).log10();
        assert!(gap_drop > 10.0, "hiss gaps down by {gap_drop:.1} dB");
        assert!(tone_change.abs() < 1.0, "tone level changed by {tone_change:.2} dB");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| (i as f32 * hz * std::f32::consts::TAU / 48000.0).sin() * amp).collect()
    }

    fn harmonic(y: &[f32], hz: f32) -> f32 {
        let w = std::f32::consts::TAU * hz / 48000.0;
        let (mut re, mut im) = (0.0f32, 0.0f32);
        for (i, v) in y.iter().enumerate() {
            re += v * (w * i as f32).cos();
            im += v * (w * i as f32).sin();
        }
        (re * re + im * im).sqrt() / y.len() as f32
    }

    /// The preamp colours more the harder it is driven, and bass saturates the input transformer more than
    /// treble does (third harmonic).
    #[test]
    fn preamp_drive_and_transformer() {
        let mut p = Preamp::new(48000.0);
        let lo: Vec<f32> = tone(60.0, 0.25, 48000).iter().map(|x| p.process(*x, 2.0)).collect();
        let mut p = Preamp::new(48000.0);
        let hi: Vec<f32> = tone(2000.0, 0.25, 48000).iter().map(|x| p.process(*x, 2.0)).collect();
        let thd = |y: &[f32], f: f32| harmonic(&y[24000..], f * 3.0) / harmonic(&y[24000..], f);
        assert!(thd(&lo, 60.0) > thd(&hi, 2000.0) * 1.5, "bass saturates the core: {} vs {}", thd(&lo, 60.0), thd(&hi, 2000.0));
        let mut p = Preamp::new(48000.0);
        let gentle: Vec<f32> = tone(1000.0, 0.1, 48000).iter().map(|x| p.process(*x, 1.0)).collect();
        let h2 = |y: &[f32]| harmonic(&y[24000..], 2000.0) / harmonic(&y[24000..], 1000.0);
        let mut p = Preamp::new(48000.0);
        let hot: Vec<f32> = tone(1000.0, 0.1, 48000).iter().map(|x| p.process(*x, 8.0)).collect();
        assert!(h2(&hot) > h2(&gentle) * 2.0, "drive adds second harmonic");
    }

    /// The opto cell releases in two stages and remembers: after long, heavy compression it recovers more
    /// slowly than after a short burst.
    #[test]
    fn opto_release_has_memory() {
        let rate = 48000.0;
        let recover = |hold_s: f32| {
            let mut c = OptoCell::default();
            for _ in 0..(hold_s * rate) as usize {
                c.next(12.0, rate, Timing::Fixed);
            }
            let mut t = 0;
            while c.next(0.0, rate, Timing::Fixed) > 3.0 && t < 10 * 48000 {
                t += 1;
            }
            t as f32 / rate
        };
        let short = recover(0.1);
        let long = recover(4.0);
        assert!(long > short * 1.3, "memory: {short} s after a burst, {long} s after long compression");
    }

    #[test]
    fn strip_compresses_and_stays_bounded() {
        let mut s = ClassAStrip::new(48000.0);
        s.set_param("threshold", -24.0);
        s.set_param("ratio", 8.0);
        let x = tone(220.0, 0.8, 48000);
        let (mut l, mut r) = (x.clone(), x);
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            s.process(a, b, None);
        }
        assert!(s.gain_reduction() > 6.0, "{}", s.gain_reduction());
        assert!(l.iter().all(|v| v.is_finite() && v.abs() < 2.0));
    }

    fn kick_loop(rate: f32) -> Vec<f32> {
        (0..96000)
            .map(|i| {
                let t = (i % 24000) as f32 / rate;
                (t * 55.0 * std::f32::consts::TAU).sin() * 0.5 * (-t / 0.15).exp() + if t < 0.003 { (i as f32 * 1.7).sin() * 0.3 } else { 0.0 }
            })
            .collect()
    }

    fn run(set: &[(&str, f32)]) -> Vec<f32> {
        let mut d = DrumBus::new(48000.0);
        for (k, v) in DRUM_PARAMS.iter().map(|p| (p.id, 0.0f32)) {
            if !matches!(k, "colour" | "focus" | "comp_mode" | "drive_hp") {
                d.set_param(k, v);
            }
        }
        for (k, v) in set {
            d.set_param(k, *v);
        }
        let x = kick_loop(48000.0);
        let (mut l, mut r) = (x.clone(), x);
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            d.process(a, b, None);
        }
        l
    }

    /// Every module does something on a kick loop, and with everything at zero the bus is (nearly)
    /// transparent apart from the drive high-pass.
    #[test]
    fn drum_bus_modules_after_orion() {
        let x = kick_loop(48000.0);
        let flat = run(&[]);
        let diff = |a: &[f32], b: &[f32]| (a[24000..].iter().zip(&b[24000..]).map(|(p, q)| (p - q).powi(2)).sum::<f32>() / a[24000..].iter().map(|p| p * p).sum::<f32>()).sqrt();
        let rms = |v: &[f32]| (v[24000..].iter().map(|p| p * p).sum::<f32>() / 72000.0).sqrt();
        assert!((gain_to_db(rms(&flat) / rms(&x))).abs() < 1.0, "neutral level: {} vs {}", rms(&flat), rms(&x));
        for (name, set) in [
            ("drive", vec![("drive", 80.0)]),
            ("harmonics", vec![("harmonics", 80.0)]),
            ("compression", vec![("comp_mix", 100.0), ("input", 12.0)]),
            ("lift", vec![("lift", 100.0)]),
            ("harmonic EQ", vec![("heq1", 6.0)]),
            ("sub", vec![("sub", 100.0)]),
            ("low", vec![("low", 100.0)]),
            ("loud", vec![("loud", 100.0)]),
            ("grit", vec![("grit", 100.0)]),
        ] {
            let y = run(&set);
            let mut base = run(&set.iter().filter(|(k, _)| *k == "input").copied().collect::<Vec<_>>());
            if base.is_empty() {
                base = flat.clone();
            }
            assert!(diff(&y, &base) > 0.05, "{name} changes the sound ({})", diff(&y, &base));
            assert!(y.iter().all(|v| v.is_finite()));
        }
        // Harmonics: the 80 Hz focus puts more energy around 160–320 Hz than the 50 Hz one does on a kick.
        let band = |y: &[f32], f: f32| harmonic(&y[24000..], f);
        let h50 = run(&[("harmonics", 100.0), ("focus", 0.0)]);
        let h80 = run(&[("harmonics", 100.0), ("focus", 1.0)]);
        assert!(band(&h50, 110.0) > band(&flat, 110.0), "harmonics add the kick's upper harmonics");
        assert!(h50 != h80);
        // The harmonic EQ moves in whole dB.
        let mut d = DrumBus::new(48000.0);
        d.set_param("heq3", 2.4);
        assert_eq!(d.v("heq3"), 2.0);
    }

    /// Published 1073 / CL 1B values: EQ points, ±16 / ±18 dB ranges, an 18 dB/oct high-pass, ratio 2–10,
    /// attack 0.5–300 ms, release 0.05–10 s, gain up to 30 dB.
    #[test]
    fn class_a_strip_matches_the_published_specs() {
        assert_eq!(HPF_HZ[1..], [50.0, 80.0, 160.0, 300.0]);
        assert_eq!(LOW_HZ, [35.0, 60.0, 110.0, 220.0]);
        assert_eq!(MID_HZ, [360.0, 700.0, 1600.0, 3200.0, 4800.0, 7200.0]);
        let p = |id: &str| *STRIP_PARAMS.iter().find(|p| p.id == id).unwrap();
        assert_eq!((p("low_gain").min, p("low_gain").max), (-16.0, 16.0));
        assert_eq!((p("mid_gain").min, p("mid_gain").max), (-18.0, 18.0));
        assert_eq!((p("high_gain").min, p("high_gain").max), (-16.0, 16.0));
        assert_eq!((p("ratio").min, p("ratio").max), (2.0, 10.0));
        assert_eq!((p("attack").min, p("attack").max), (0.5, 300.0));
        assert_eq!((p("release").min, p("release").max), (50.0, 10000.0));
        assert_eq!(p("comp_gain").max, 30.0);
        // The high-pass falls 18 dB per octave well below its corner (measured on the filter itself).
        let mut s = ClassAStrip::new(48000.0);
        s.set_param("hpf", 3.0); // 160 Hz
        s.update_eq();
        let level = |s: &mut ClassAStrip, f: f32| {
            s.hpf = [Biquad::new(BiquadKind::HighPass, 160.0, 1.0, 0.0, 48000.0); 2];
            s.hpf1 = [Hp1::new(160.0, 48000.0); 2];
            let x = tone(f, 0.5, 96000);
            let y: Vec<f32> = x.iter().map(|v| s.hpf1[0].process(s.hpf[0].process(*v))).collect();
            gain_to_db(y[48000..].iter().fold(0.0f32, |m, v| m.max(v.abs())) / 0.5)
        };
        let slope = level(&mut s, 40.0) - level(&mut s, 20.0);
        assert!((slope - 18.0).abs() < 1.5, "{slope} dB/oct");
        // Fixed timing releases fast after a short peak (50 ms).
        let mut c = OptoCell::default();
        for _ in 0..480 {
            c.next(10.0, 48000.0, Timing::Fixed);
        }
        let mut t = 0;
        while c.next(0.0, 48000.0, Timing::Fixed) > 3.0 && t < 48000 {
            t += 1;
        }
        assert!(t < 48000 / 2, "fixed release after a 10 ms peak: {} ms", t / 48);
    }
}
