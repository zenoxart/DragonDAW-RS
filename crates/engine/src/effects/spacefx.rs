//! Seraph (shimmer reverb: a feedback delay network with a pitch shifter in its loop) and Rime (spectral
//! freeze: holds the spectrum of a moment and resynthesises it as an endless drone).

use super::{Effect, EffectDef, ParamInfo};
use crate::dsp::{db_to_gain, DelayLine, Rng};
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::sync::Arc;

// ----------------------------------------------------------------------------------------------- shimmer

const SHIMMER_PARAMS: &[ParamInfo] = &[
    ParamInfo::lin("shift", "Shift", -12.0, 24.0, 12.0, "st"),
    ParamInfo::lin("shimmer", "Shimmer", 0.0, 100.0, 50.0, "%"),
    ParamInfo::log("decay", "Decay", 0.5, 30.0, 6.0, "s"),
    ParamInfo::lin("size", "Size", 20.0, 100.0, 70.0, "%"),
    ParamInfo::log("damping", "Damping", 1000.0, 16000.0, 6000.0, "Hz"),
    ParamInfo::lin("predelay", "Pre-delay", 0.0, 200.0, 20.0, "ms"),
    ParamInfo::lin("modulation", "Modulation", 0.0, 100.0, 30.0, "%"),
    ParamInfo::lin("mix", "Mix", 0.0, 100.0, 35.0, "%"),
    ParamInfo::lin("output", "Output", -24.0, 12.0, 0.0, "dB"),
];

pub fn shimmer_def() -> EffectDef {
    EffectDef {
        id: "shimmer",
        name: "Seraph (shimmer reverb)",
        category: "Space",
        description: "Shimmer reverb: a dense feedback delay network whose tail is pitch-shifted (an octave up by default) on every round, so it rises into a glowing choir-like halo.",
        params: SHIMMER_PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(Seraph::new(rate)),
    }
}

/// Line lengths (ms at size 100 %): mutually prime-ish so the echoes do not line up.
const LINES_MS: [f32; 4] = [47.3, 61.9, 73.1, 89.7];
const DIFF_MS: [f32; 4] = [4.7, 6.1, 8.9, 11.3];

/// A delay-line pitch shifter: two taps sweep through a short window in opposite halves of a cycle and are
/// crossfaded with `sin²`, so their sum is seamless; the taps' drift `(1 − ρ)` per sample changes the pitch
/// by the ratio `ρ`.
struct Shifter {
    line: DelayLine,
    phase: f32,
    window: f32,
}

impl Shifter {
    fn new(rate: f32) -> Shifter {
        let window = 0.06 * rate;
        Shifter { line: DelayLine::new(window as usize * 2 + 8), phase: 0.0, window }
    }
    fn process(&mut self, x: f32, ratio: f32) -> f32 {
        self.line.write(x);
        self.phase = (self.phase + (1.0 - ratio) / self.window).rem_euclid(1.0);
        let p2 = (self.phase + 0.5).fract();
        let a = self.line.read(1.0 + self.phase * self.window);
        let b = self.line.read(1.0 + p2 * self.window);
        let wa = (std::f32::consts::PI * self.phase).sin();
        let wb = (std::f32::consts::PI * p2).sin();
        a * wa * wa + b * wb * wb
    }
}

struct Allpass {
    line: DelayLine,
    len: f32,
}

impl Allpass {
    fn process(&mut self, x: f32, g: f32) -> f32 {
        let d = self.line.read(self.len);
        let v = x + g * d;
        self.line.write(v);
        d - g * v
    }
}

pub struct Seraph {
    rate: f32,
    shift: f32,
    shimmer: f32,
    decay: f32,
    size: f32,
    damp: f32,
    predelay: f32,
    modulation: f32,
    mix: f32,
    out: f32,
    pre: DelayLine,
    diff: [Allpass; 4],
    lines: [DelayLine; 4],
    lp: [f32; 4],
    shifter: Shifter,
    lfo: f64,
    gains: [f32; 4],
    damp_a: f32,
    level: f32,
    tele: [f32; 2],
}

impl Seraph {
    pub fn new(rate: f32) -> Self {
        let ms = |m: f32| (m * 0.001 * rate) as usize + 16;
        let mut s = Seraph {
            rate,
            shift: 12.0,
            shimmer: 0.5,
            decay: 6.0,
            size: 0.7,
            damp: 6000.0,
            predelay: 20.0,
            modulation: 0.3,
            mix: 0.35,
            out: 1.0,
            pre: DelayLine::new(ms(210.0)),
            diff: DIFF_MS.map(|m| Allpass { line: DelayLine::new(ms(m)), len: m * 0.001 * rate }),
            lines: LINES_MS.map(|m| DelayLine::new(ms(m * 1.02))),
            lp: [0.0; 4],
            shifter: Shifter::new(rate),
            lfo: 0.0,
            gains: [0.0; 4],
            damp_a: 0.0,
            level: 0.0,
            tele: [0.0; 2],
        };
        s.update();
        s
    }

    /// Each line's loop gain gives −60 dB after the decay time: `g_i = 10^{−3 L_i / (T60 f_s)}`.
    fn update(&mut self) {
        for (g, m) in self.gains.iter_mut().zip(LINES_MS) {
            let len = m * self.size * 0.001 * self.rate;
            *g = 10f32.powf(-3.0 * len / (self.decay * self.rate));
        }
        self.damp_a = (-std::f32::consts::TAU * self.damp.min(self.rate * 0.45) / self.rate).exp();
    }
}

impl Effect for Seraph {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "shift" => self.shift = v.clamp(-12.0, 24.0),
            "shimmer" => self.shimmer = v.clamp(0.0, 100.0) / 100.0,
            "decay" => self.decay = v.clamp(0.5, 30.0),
            "size" => self.size = v.clamp(20.0, 100.0) / 100.0,
            "damping" => self.damp = v.clamp(1000.0, 16000.0),
            "predelay" => self.predelay = v.clamp(0.0, 200.0),
            "modulation" => self.modulation = v.clamp(0.0, 100.0) / 100.0,
            "mix" => self.mix = v.clamp(0.0, 100.0) / 100.0,
            "output" => self.out = db_to_gain(v),
            _ => return,
        }
        self.update();
    }

    fn reset(&mut self) {
        self.pre.clear();
        for a in self.diff.iter_mut() {
            a.line.clear();
        }
        for l in self.lines.iter_mut() {
            l.clear();
        }
        self.lp = [0.0; 4];
        self.shifter.line.clear();
    }

    fn telemetry(&self) -> &[f32] {
        &self.tele
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let ratio = 2f32.powf(self.shift / 12.0);
        let pre = self.predelay * 0.001 * self.rate;
        let lens: [f32; 4] = LINES_MS.map(|m| m * self.size * 0.001 * self.rate);
        let depth = self.modulation * 0.0012 * self.rate;
        let mut peak = 0.0f32;
        for i in 0..l.len() {
            let (xl, xr) = (l[i], r[i]);
            self.pre.write(0.5 * (xl + xr));
            let mut x = if pre >= 1.0 { self.pre.read(pre) } else { 0.5 * (xl + xr) };
            for a in self.diff.iter_mut() {
                x = a.process(x, 0.6);
            }
            self.lfo = (self.lfo + 0.31 / self.rate as f64).fract();
            let m = (std::f64::consts::TAU * self.lfo).sin() as f32;
            // Read the four lines (two modulated in opposite directions against metallic ringing).
            let mut o = [0.0f32; 4];
            for k in 0..4 {
                let mo = match k {
                    0 => m,
                    2 => -m,
                    _ => 0.0,
                } * depth;
                o[k] = self.lines[k].read((lens[k] + mo).max(2.0));
            }
            // The shimmer: the tail's sum, pitch-shifted, fed back in.
            let sum = 0.5 * (o[0] + o[1] + o[2] + o[3]);
            let shifted = self.shifter.process(sum, ratio);
            // Householder mix: y_i = o_i − (2/N)·Σo.
            let s = 0.5 * (o[0] + o[1] + o[2] + o[3]);
            for k in 0..4 {
                let fb = (o[k] - s) * (1.0 - self.shimmer) + shifted * self.shimmer * 0.5;
                // Damping: a one-pole low-pass in each loop.
                self.lp[k] = fb + (self.lp[k] - fb) * self.damp_a;
                // A gentle ceiling keeps the loop bounded whatever the settings.
                let v = (self.lp[k] * self.gains[k]).clamp(-4.0, 4.0);
                let v = v / (1.0 + 0.05 * v.abs());
                self.lines[k].write(x * 0.5 + v);
            }
            let wl = o[0] + o[2];
            let wr = o[1] + o[3];
            l[i] = (xl + (wl * 0.7 - xl) * self.mix) * self.out;
            r[i] = (xr + (wr * 0.7 - xr) * self.mix) * self.out;
            peak = peak.max(wl.abs()).max(wr.abs());
        }
        self.level = self.level * 0.9 + peak * 0.1;
        self.tele = [self.level, ratio];
    }
}

// --------------------------------------------------------------------------------------- spectral freeze

const N: usize = 2048;
const HOP: usize = N / 4;
const HB: usize = N / 2 + 1;
/// Spectrum bands sent to the display.
pub const FREEZE_BANDS: usize = 64;

const FREEZE_PARAMS: &[ParamInfo] = &[
    ParamInfo::toggle("freeze", "Freeze", false),
    ParamInfo::log("fade", "Fade", 10.0, 4000.0, 300.0, "ms"),
    ParamInfo::lin("blur", "Blur", 0.0, 100.0, 20.0, "%"),
    ParamInfo::lin("shift", "Shift", -24.0, 24.0, 0.0, "st"),
    ParamInfo::lin("tilt", "Tilt", -6.0, 6.0, 0.0, "dB/oct"),
    ParamInfo::lin("drift", "Drift", 0.0, 100.0, 30.0, "%"),
    ParamInfo::lin("mix", "Mix", 0.0, 100.0, 100.0, "%"),
    ParamInfo::lin("output", "Output", -24.0, 12.0, 0.0, "dB"),
];

pub fn freeze_def() -> EffectDef {
    EffectDef {
        id: "freeze",
        name: "Rime (spectral freeze)",
        category: "Space",
        description: "Spectral freeze: Freeze holds the spectrum of the moment and plays it on as an endless drone – blurred, shifted, tilted and drifting – fading in and out over the fade time.",
        params: FREEZE_PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(Rime::new(rate)),
    }
}

pub struct Rime {
    rate: f32,
    on: bool,
    fade_ms: f32,
    blur: f32,
    shift: f32,
    tilt: f32,
    drift: f32,
    mix: f32,
    out: f32,
    fwd: Arc<dyn Fft<f32>>,
    inv: Arc<dyn Fft<f32>>,
    scratch: Vec<Complex<f32>>,
    window: Vec<f32>,
    /// Input history (both sides), the newest `N` samples.
    inp: [Vec<f32>; 2],
    fill: usize,
    /// Overlap-add output accumulators (2N, read from the front).
    ola: [Vec<f32>; 2],
    buf: Vec<Complex<f32>>,
    /// Smoothed live magnitudes and the held ones, per side.
    live: [Vec<f32>; 2],
    held: [Vec<f32>; 2],
    shaped: Vec<f32>,
    phase: [Vec<f32>; 2],
    /// Fade 0 … 1 of the frozen sound.
    amount: f32,
    was_on: bool,
    rng: Rng,
    dry: [DelayLine; 2],
    tele: [f32; FREEZE_BANDS + 1],
}

impl Rime {
    pub fn new(rate: f32) -> Self {
        let mut planner = FftPlanner::new();
        let fwd = planner.plan_fft_forward(N);
        let inv = planner.plan_fft_inverse(N);
        let scratch = vec![Complex::default(); fwd.get_inplace_scratch_len().max(inv.get_inplace_scratch_len())];
        // Periodic Hann for analysis and synthesis; at 75 % overlap Σw² = 1.5.
        let window = (0..N).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / N as f32).cos()).collect();
        Rime {
            rate,
            on: false,
            fade_ms: 300.0,
            blur: 0.2,
            shift: 0.0,
            tilt: 0.0,
            drift: 0.3,
            mix: 1.0,
            out: 1.0,
            fwd,
            inv,
            scratch,
            window,
            inp: [vec![0.0; N], vec![0.0; N]],
            fill: 0,
            ola: [vec![0.0; 2 * N], vec![0.0; 2 * N]],
            buf: vec![Complex::default(); N],
            live: [vec![0.0; HB], vec![0.0; HB]],
            held: [vec![0.0; HB], vec![0.0; HB]],
            shaped: vec![0.0; HB],
            phase: [vec![0.0; HB], vec![0.0; HB]],
            amount: 0.0,
            was_on: false,
            rng: Rng::new(0x2147),
            dry: [DelayLine::new(N + 2), DelayLine::new(N + 2)],
            tele: [-90.0; FREEZE_BANDS + 1],
        }
    }

    fn frame(&mut self) {
        let hop_s = HOP as f32 / self.rate;
        let fade_step = if self.fade_ms > 0.0 { hop_s * 1000.0 / self.fade_ms } else { 1.0 };
        if self.on {
            self.amount = (self.amount + fade_step).min(1.0);
        } else {
            self.amount = (self.amount - fade_step).max(0.0);
        }
        for c in 0..2 {
            // Analysis: the live spectrum, averaged over the last frames (≈ 50 ms), so the held moment is not
            // one frame's accident.
            for i in 0..N {
                self.buf[i] = Complex::new(self.inp[c][i] * self.window[i], 0.0);
            }
            self.fwd.process_with_scratch(&mut self.buf, &mut self.scratch);
            for k in 0..HB {
                let m = self.buf[k].norm();
                self.live[c][k] = self.live[c][k] * 0.6 + m * 0.4;
            }
            if self.on && !self.was_on {
                self.held[c].copy_from_slice(&self.live[c]);
            }
            if self.amount <= 0.0 {
                continue;
            }
            // Shape the held spectrum: shift (bins resampled), tilt, blur across neighbouring bins.
            let ratio = 2f32.powf(self.shift / 12.0);
            for k in 0..HB {
                let src = k as f32 / ratio;
                let i = src as usize;
                let f = src - i as f32;
                let a = self.held[c].get(i).copied().unwrap_or(0.0);
                let b = self.held[c].get(i + 1).copied().unwrap_or(0.0);
                let hz = (k.max(1) as f32) * self.rate / N as f32;
                let t = db_to_gain(self.tilt * (hz / 1000.0).log2());
                self.shaped[k] = (a + (b - a) * f) * t;
            }
            if self.blur > 0.0 {
                let a = 0.95 * self.blur;
                let mut acc = 0.0;
                for k in 0..HB {
                    acc = self.shaped[k] + (acc - self.shaped[k]) * a;
                    self.shaped[k] = acc;
                }
                for k in (0..HB).rev() {
                    acc = self.shaped[k] + (acc - self.shaped[k]) * a;
                    self.shaped[k] = acc;
                }
            }
            // Resynthesis: each bin advances by its expected phase per hop, jittered by the drift; amplitudes
            // wobble a little with it too.
            for k in 0..HB {
                let adv = std::f32::consts::TAU * k as f32 * HOP as f32 / N as f32;
                let ph = &mut self.phase[c][k];
                *ph = (*ph + adv + self.drift * std::f32::consts::PI * self.rng.bipolar()).rem_euclid(std::f32::consts::TAU);
                let mag = self.shaped[k] * (1.0 + 0.3 * self.drift * self.rng.bipolar());
                self.buf[k] = Complex::from_polar(mag, *ph);
            }
            for k in HB..N {
                self.buf[k] = self.buf[N - k].conj();
            }
            self.inv.process_with_scratch(&mut self.buf, &mut self.scratch);
            // Window gain: analysis Hann (sum 0.5·N) + synthesis Hann, 75 % overlap Σw² = 1.5.
            let norm = 1.0 / (N as f32 * 1.5) * 2.0;
            for i in 0..N {
                self.ola[c][i] += self.buf[i].re * self.window[i] * norm * self.amount;
            }
        }
        self.was_on = self.on;
        // Display: the held (or live) spectrum in log bands.
        let src = if self.amount > 0.0 { &self.shaped } else { &self.live[0] };
        for b in 0..FREEZE_BANDS {
            let f = 30.0 * (16000.0f32 / 30.0).powf(b as f32 / (FREEZE_BANDS - 1) as f32);
            let k = ((f * N as f32 / self.rate) as usize).min(HB - 1);
            self.tele[b] = (20.0 * (src[k] * 4.0 / N as f32).max(1e-6).log10()).max(-90.0);
        }
        self.tele[FREEZE_BANDS] = self.amount;
    }
}

impl Effect for Rime {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "freeze" => self.on = v >= 0.5,
            "fade" => self.fade_ms = v.clamp(10.0, 4000.0),
            "blur" => self.blur = v.clamp(0.0, 100.0) / 100.0,
            "shift" => self.shift = v.clamp(-24.0, 24.0),
            "tilt" => self.tilt = v.clamp(-6.0, 6.0),
            "drift" => self.drift = v.clamp(0.0, 100.0) / 100.0,
            "mix" => self.mix = v.clamp(0.0, 100.0) / 100.0,
            "output" => self.out = db_to_gain(v),
            _ => {}
        }
    }

    fn reset(&mut self) {
        for c in 0..2 {
            self.inp[c].fill(0.0);
            self.ola[c].fill(0.0);
            self.live[c].fill(0.0);
            self.dry[c].clear();
        }
        self.fill = 0;
        self.amount = 0.0;
    }

    fn latency(&self) -> usize {
        N
    }

    fn telemetry(&self) -> &[f32] {
        &self.tele
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        for i in 0..l.len() {
            let x = [l[i], r[i]];
            for c in 0..2 {
                self.inp[c][N - HOP + self.fill] = x[c];
                self.dry[c].write(x[c]);
            }
            // The frozen sound, read from the front of the overlap-add buffer (one frame late).
            let w = [self.ola[0][self.fill], self.ola[1][self.fill]];
            self.fill += 1;
            if self.fill == HOP {
                self.fill = 0;
                for c in 0..2 {
                    self.ola[c].copy_within(HOP.., 0);
                    let n = self.ola[c].len();
                    self.ola[c][n - HOP..].fill(0.0);
                }
                self.frame();
                for c in 0..2 {
                    self.inp[c].copy_within(HOP.., 0);
                }
            }
            for (c, s) in [&mut l[i], &mut r[i]].into_iter().enumerate() {
                let d = self.dry[c].read(N as f32);
                // While frozen the drone replaces the dry sound by the mix; otherwise the dry passes.
                let a = self.amount * self.mix;
                *s = (d * (1.0 - a) + w[c] * self.mix) * self.out;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shimmer_stays_bounded_and_rings() {
        let rate = 48000.0;
        let mut s = Seraph::new(rate);
        s.set_param("decay", 30.0);
        s.set_param("shimmer", 100.0);
        s.set_param("mix", 100.0);
        let n = 48000 * 4;
        let mut l = vec![0.0f32; n];
        let mut r = vec![0.0f32; n];
        for v in l.iter_mut().take(2400) {
            *v = 0.5;
        }
        for (a, b) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
            s.process(a, b, None);
        }
        let peak = l.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        let tail: f32 = l[n - 4800..].iter().map(|x| x * x).sum();
        assert!(peak.is_finite() && peak < 6.0, "{peak}");
        assert!(tail > 1e-4, "the tail still rings after 4 s");
    }

    /// A held sine keeps sounding after the input stops.
    #[test]
    fn freeze_holds_a_tone() {
        let rate = 48000.0;
        let mut f = Rime::new(rate);
        f.set_param("fade", 10.0);
        f.set_param("drift", 0.0);
        f.set_param("blur", 0.0);
        let n = 48000;
        let mut l: Vec<f32> = (0..n).map(|i| if i < n / 2 { (i as f32 * 0.1).sin() * 0.5 } else { 0.0 }).collect();
        let mut r = l.clone();
        f.process(&mut l[..20000], &mut r[..20000], None);
        f.set_param("freeze", 1.0);
        f.process(&mut l[20000..], &mut r[20000..], None);
        let late: f32 = l[n - 6000..].iter().map(|x| x * x).sum::<f32>() / 6000.0;
        assert!(late > 0.01, "rms² {late}");
        assert!(l.iter().all(|x| x.is_finite() && x.abs() < 4.0));
    }
}
