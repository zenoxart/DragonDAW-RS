//! Tracking and bus processors built from classic circuits:
//!
//! - **Dragon Class-A Strip** – a British class-A transformer microphone preamp (the 1073 school: input
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
    ParamInfo::lin("ratio", "Comp ratio", 1.0, 20.0, 4.0, ":1"),
    ParamInfo::choice("comp_mode", "Comp timing", &["Fixed (opto)", "Fixed / manual", "Manual"], 0.0),
    ParamInfo::log("attack", "Comp attack", 0.2, 80.0, 10.0, "ms"),
    ParamInfo::log("release", "Comp release", 40.0, 5000.0, 400.0, "ms"),
    ParamInfo::lin("comp_gain", "Comp gain", 0.0, 24.0, 4.0, "dB"),
    ParamInfo::choice("order", "Order", &["Pre → EQ → Comp", "Pre → Comp → EQ"], 0.0),
    ParamInfo::lin("output", "Output", -24.0, 12.0, -6.0, "dB"),
];

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
    fn next(&mut self, want: f32, rate: f32, manual: Option<(f32, f32)>) -> f32 {
        let dt = 1.0 / rate;
        let (att, rel_fast, rel_slow) = match manual {
            Some((a, r)) => (a * 0.001, r * 0.001, r * 0.004),
            // Program-dependent: harder hits light the cell faster.
            None => (0.010 / (1.0 + want * 0.15), 0.06, 0.5 + 2.5 * self.memory),
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
    hpf: [[Biquad; 2]; 2],
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
    output: f32,
    pre: [Preamp; 2],
    cell: OptoCell,
    tube: [crate::dsp::tube::TubeStage; 2],
    gr: f32,
    tele: [f32; 4],
}

impl ClassAStrip {
    pub fn new(rate: f32) -> Self {
        let mut tube = crate::dsp::tube::TubeStage::new(crate::dsp::tube::TUBES[1], rate);
        tube.bias = -0.2;
        let mut s = ClassAStrip {
            rate,
            drive: db_to_gain(6.0),
            eq_on: true,
            hpf: [[Biquad::default(); 2]; 2],
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
            output: db_to_gain(-6.0),
            pre: [Preamp::new(rate); 2],
            cell: OptoCell::default(),
            tube: [tube; 2],
            gr: 0.0,
            tele: [0.0; 4],
        };
        s.update_eq();
        s
    }

    fn update_eq(&mut self) {
        let (h, lf, lg, mf, mg, hg) = self.eq_p;
        let r = self.rate;
        self.hpf_on = h > 0;
        if h > 0 {
            // 18 dB/oct high-pass like the inductor filter: a 2nd-order plus a gentle 1st-order-ish section.
            for ch in 0..2 {
                self.hpf[ch] = [Biquad::new(BiquadKind::HighPass, HPF_HZ[h], 0.71, 0.0, r), Biquad::new(BiquadKind::HighPass, HPF_HZ[h] * 0.8, 0.5, 0.0, r)];
            }
        }
        // Inductor-style shelves have a broad bump before the shelf; a low Q gets close.
        for ch in 0..2 {
            self.low[ch] = Biquad::new(BiquadKind::LowShelf, LOW_HZ[lf.min(3)], 0.55, lg, r);
            // The mid band's Q tightens slightly with gain (as the inductor bands do).
            let q = 1.0 + mg.abs() / 18.0 * 0.6;
            self.mid[ch] = Biquad::new(BiquadKind::Peak, MID_HZ[mf.min(5)], q, mg, r);
            self.high[ch] = Biquad::new(BiquadKind::HighShelf, 12000.0f32.min(r * 0.45), 0.6, hg, r);
        }
    }

    #[inline]
    fn eq(&mut self, ch: usize, x: f32) -> f32 {
        if !self.eq_on {
            return x;
        }
        let mut y = x;
        if self.hpf_on {
            y = self.hpf[ch][0].process(y);
            y = self.hpf[ch][1].process(y);
        }
        y = self.low[ch].process(y);
        y = self.mid[ch].process(y);
        self.high[ch].process(y)
    }
}

impl Effect for ClassAStrip {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
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
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let manual = match self.mode {
            0 => None,
            // Fixed / manual: fixed opto attack, manual release.
            1 => Some((10.0, self.release)),
            _ => Some((self.attack, self.release)),
        };
        let mut peak_gr = 0.0f32;
        let (mut pin, mut pout) = (0.0f32, 0.0f32);
        for i in 0..l.len() {
            pin = pin.max(l[i].abs().max(r[i].abs()));
            let mut a = self.pre[0].process(l[i], self.drive);
            let mut b = self.pre[1].process(r[i], self.drive);
            if !self.comp_first {
                a = self.eq(0, a);
                b = self.eq(1, b);
            }
            if self.comp_on {
                // Feed-forward detector into the opto cell, soft knee.
                let level = gain_to_db(a.abs().max(b.abs()).max(1e-6));
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
        self.tele = [pin, peak_gr, pout, gain_to_db(self.drive)];
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
        name: "Class-A Strip",
        category: "Channel strip",
        description: "Recording channel: British class-A transformer mic preamp with stepped inductor-style EQ into a tube opto compressor (program-dependent opto attack, two-stage release with memory, fixed or manual timing). Zero latency – track through it.",
        params: STRIP_PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(ClassAStrip::new(rate)),
    }
}

// ============================================================================================ drum bus

const DRUM_PARAMS: &[ParamInfo] = &[
    ParamInfo::lin("punch", "Punch", 0.0, 100.0, 40.0, "%"),
    ParamInfo::lin("smack", "Smack (parallel crush)", 0.0, 100.0, 30.0, "%"),
    ParamInfo::lin("low", "Low end", -6.0, 12.0, 3.0, "dB"),
    ParamInfo::lin("body", "Body", -6.0, 6.0, 0.0, "dB"),
    ParamInfo::lin("crack", "Crack", -6.0, 9.0, 2.0, "dB"),
    ParamInfo::lin("air", "Air", -6.0, 9.0, 1.5, "dB"),
    ParamInfo::lin("drive", "Drive", 0.0, 100.0, 25.0, "%"),
    ParamInfo::lin("clip", "Clipper", 0.0, 100.0, 30.0, "%"),
    ParamInfo::lin("width", "Width", 0.0, 100.0, 50.0, "%"),
    ParamInfo::lin("mix", "Mix", 0.0, 100.0, 100.0, "%"),
    ParamInfo::lin("output", "Output", -18.0, 12.0, 0.0, "dB"),
];

pub struct DrumBus {
    rate: f32,
    p: [f32; 11],
    punch: Box<dyn Effect>,
    crush: Box<dyn Effect>,
    sat: Box<dyn Effect>,
    eq: [[Biquad; 4]; 2],
    tight: [Biquad; 2],
    side_hp: Biquad,
    dry: Vec<f32>,
    dry_r: Vec<f32>,
    par_l: Vec<f32>,
    par_r: Vec<f32>,
    tele: [f32; 4],
}

impl DrumBus {
    pub fn new(rate: f32) -> Self {
        let mut d = DrumBus {
            rate,
            p: [40.0, 30.0, 3.0, 0.0, 2.0, 1.5, 25.0, 30.0, 50.0, 100.0, 0.0],
            punch: (dynamics::transient_def().create)(rate),
            crush: (dynamics::compressor_def().create)(rate),
            sat: (super::color::saturator_def().create)(rate),
            eq: [[Biquad::default(); 4]; 2],
            tight: [Biquad::new(BiquadKind::HighPass, 25.0, 0.7, 0.0, rate); 2],
            side_hp: Biquad::new(BiquadKind::HighPass, 180.0, 0.7, 0.0, rate),
            dry: Vec::new(),
            dry_r: Vec::new(),
            par_l: Vec::new(),
            par_r: Vec::new(),
            tele: [0.0; 4],
        };
        d.update();
        d
    }

    fn update(&mut self) {
        let [punch, _smack, low, body, crack, air, drive, ..] = self.p;
        let r = self.rate;
        for ch in 0..2 {
            self.eq[ch] = [
                Biquad::new(BiquadKind::LowShelf, 70.0, 0.7, low, r),
                Biquad::new(BiquadKind::Peak, 280.0, 0.9, body, r),
                Biquad::new(BiquadKind::Peak, 3500.0, 0.8, crack, r),
                Biquad::new(BiquadKind::HighShelf, 11000.0f32.min(r * 0.45), 0.6, air, r),
            ];
        }
        self.punch.set_param("attack", punch);
        self.punch.set_param("att_dur", 22.0);
        self.punch.set_param("att_sens", 70.0);
        self.punch.set_param("sustain", -punch * 0.25);
        self.punch.set_param("guard", 0.0);
        // Smack: a hard, fast "crush" compressor blended in parallel (New York style).
        self.crush.set_param("threshold", -30.0);
        self.crush.set_param("ratio", 10.0);
        self.crush.set_param("attack", 1.0);
        self.crush.set_param("release", 60.0);
        self.crush.set_param("knee", 2.0);
        self.crush.set_param("makeup", 18.0);
        self.sat.set_param("mode", 0.0);
        self.sat.set_param("tube", 5.0);
        self.sat.set_param("drive", drive / 100.0 * 18.0);
        self.sat.set_param("output", -drive / 100.0 * 9.0);
        self.sat.set_param("mix", 0.4 + drive / 100.0 * 0.5);
    }
}

impl Effect for DrumBus {
    fn set_param(&mut self, id: &str, v: f32) {
        if let Some(k) = DRUM_PARAMS.iter().position(|p| p.id == id) {
            self.p[k] = v;
            self.update();
        }
    }
    fn reset(&mut self) {
        self.punch.reset();
        self.crush.reset();
        self.sat.reset();
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let n = l.len();
        let [_, smack, _, _, _, _, drive, clip, width, mix, out] = self.p;
        self.dry.clear();
        self.dry.extend_from_slice(l);
        self.dry_r.clear();
        self.dry_r.extend_from_slice(r);
        let pin = l.iter().chain(r.iter()).fold(0.0f32, |m, v| m.max(v.abs()));
        // Tone (with a sub tightening high-pass) → punch.
        for i in 0..n {
            for (ch, x) in [&mut l[i], &mut r[i]].into_iter().enumerate() {
                let mut y = self.tight[ch].process(*x);
                for b in &mut self.eq[ch] {
                    y = b.process(y);
                }
                *x = y;
            }
        }
        self.punch.process(l, r, None);
        // Smack: crushed copy mixed in.
        if smack > 0.0 {
            self.par_l.clear();
            self.par_l.extend_from_slice(l);
            self.par_r.clear();
            self.par_r.extend_from_slice(r);
            self.crush.process(&mut self.par_l, &mut self.par_r, None);
            let m = smack / 100.0 * 0.6;
            for i in 0..n {
                l[i] += self.par_l[i] * m;
                r[i] += self.par_r[i] * m;
            }
        }
        if drive > 0.0 {
            self.sat.process(l, r, None);
        }
        // Width on the side above 180 Hz (kick and bass stay centred).
        let w = width / 50.0;
        for i in 0..n {
            let m = (l[i] + r[i]) * 0.5;
            let s = self.side_hp.process((l[i] - r[i]) * 0.5) * w;
            l[i] = m + s;
            r[i] = m - s;
        }
        // Clipper: soft knee towards −0.3 dBFS.
        let ceil = db_to_gain(-0.3);
        let knee = ceil * (1.0 - clip / 100.0 * 0.5);
        let og = db_to_gain(out);
        let m = mix / 100.0;
        let mut pout = 0.0f32;
        for i in 0..n {
            for (x, d) in [(&mut l[i], self.dry[i]), (&mut r[i], self.dry_r[i])] {
                let mut y = *x * og;
                if clip > 0.0 && y.abs() > knee {
                    let over = y.abs() - knee;
                    y = y.signum() * (knee + (ceil - knee) * (over / (ceil - knee + 1e-6)).tanh());
                }
                *x = d + (y - d) * m;
                pout = pout.max(x.abs());
            }
        }
        self.tele = [pin, pout, self.crush.gain_reduction(), self.punch.telemetry().get(1).copied().unwrap_or(0.0)];
    }
    fn gain_reduction(&self) -> f32 {
        self.crush.gain_reduction() * self.p[1] / 100.0
    }
    fn telemetry(&self) -> &[f32] {
        &self.tele
    }
}

pub fn drum_bus_def() -> EffectDef {
    EffectDef {
        id: "drum_bus",
        name: "Drum Bus",
        category: "Drums",
        description: "All-in-one drum processor: punch, smack (parallel crush), low end, body, crack, air, tube drive, clipper and width – each a single macro.",
        params: DRUM_PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(DrumBus::new(rate)),
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
                c.next(12.0, rate, None);
            }
            let mut t = 0;
            while c.next(0.0, rate, None) > 3.0 && t < 10 * 48000 {
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

    #[test]
    fn drum_bus_punches_and_clips() {
        let rate = 48000.0;
        let x: Vec<f32> = (0..96000)
            .map(|i| {
                let t = (i % 24000) as f32 / rate;
                (t * 70.0 * std::f32::consts::TAU).sin() * 0.8 * (-t / 0.12).exp()
            })
            .collect();
        let mut d = DrumBus::new(rate);
        d.set_param("output", 6.0);
        let (mut l, mut r) = (x.clone(), x.clone());
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            d.process(a, b, None);
        }
        assert!(l.iter().all(|v| v.abs() <= 1.0), "clipper holds the ceiling");
        let rms = |v: &[f32]| (v[24000..].iter().map(|s| s * s).sum::<f32>() / 72000.0).sqrt();
        assert!(rms(&l) > rms(&x), "louder, denser");
    }
}
