//! Mandrake (channel vocoder) and Mercury Wheel (frequency shifter and ring modulator).

use super::{Effect, EffectDef, ParamInfo};
use crate::dsp::{db_to_gain, Biquad, BiquadKind, Rng};

// ----------------------------------------------------------------------------------------------- vocoder

pub const CARRIERS: &[&str] = &["Input + sidechain voice", "Saw", "Pulse", "Noise", "Saw chord"];
pub const BAND_COUNTS: &[&str] = &["8", "12", "16", "24", "32"];
const BANDS_N: [usize; 5] = [8, 12, 16, 24, 32];
const MAX_BANDS: usize = 32;
/// Envelope telemetry for the display (one value per band, padded).
pub const T_ENV: usize = 0;
pub const T_PITCH: usize = MAX_BANDS;

const VOCODER_PARAMS: &[ParamInfo] = &[
    ParamInfo::choice("carrier", "Carrier", CARRIERS, 1.0),
    ParamInfo::choice("bands", "Bands", BAND_COUNTS, 2.0),
    ParamInfo::lin("pitch", "Pitch", -24.0, 24.0, 0.0, "st"),
    ParamInfo::lin("formant", "Formant", -12.0, 12.0, 0.0, "st"),
    ParamInfo::log("low", "Low band", 60.0, 500.0, 100.0, "Hz"),
    ParamInfo::log("high", "High band", 2000.0, 12000.0, 7000.0, "Hz"),
    ParamInfo::log("attack", "Attack", 0.5, 50.0, 4.0, "ms"),
    ParamInfo::log("release", "Release", 5.0, 500.0, 60.0, "ms"),
    ParamInfo::lin("sibilance", "Sibilance", 0.0, 100.0, 40.0, "%"),
    ParamInfo::lin("emphasis", "Emphasis", 0.0, 12.0, 4.0, "dB"),
    ParamInfo::lin("mix", "Mix", 0.0, 100.0, 100.0, "%"),
    ParamInfo::lin("output", "Output", -24.0, 12.0, 0.0, "dB"),
];

pub fn vocoder_def() -> EffectDef {
    EffectDef {
        id: "vocoder",
        name: "Mandrake (vocoder)",
        category: "Vocal",
        description: "Channel vocoder with 8 – 32 bands. The voice shapes a carrier: the input itself (with the voice on the sidechain) or a built-in saw, pulse, noise or chord that follows the voice's pitch.",
        params: VOCODER_PARAMS,
        uses_sidechain: true,
        create: |rate| Box::new(Mandrake::new(rate)),
    }
}

#[derive(Clone, Copy, Default)]
struct Band {
    m: [Biquad; 2],
    c: [[Biquad; 2]; 2],
    env: f32,
}

pub struct Mandrake {
    rate: f32,
    carrier: usize,
    n: usize,
    pitch: f32,
    formant: f32,
    low: f32,
    high: f32,
    att: f32,
    rel: f32,
    sib: f32,
    emph: f32,
    mix: f32,
    out: f32,
    bands: [Band; MAX_BANDS],
    gain_comp: f32,
    sib_hp: [Biquad; 3],
    sib_env: f32,
    tilt: Biquad,
    rng: Rng,
    phases: [f32; 3],
    f0: f32,
    ring: [f32; 2048],
    pos: usize,
    since: usize,
    frame: Vec<f32>,
    scratch: Vec<f32>,
    tele: [f32; MAX_BANDS + 1],
    dirty: bool,
}

impl Mandrake {
    pub fn new(rate: f32) -> Self {
        let mut v = Mandrake {
            rate,
            carrier: 1,
            n: 16,
            pitch: 0.0,
            formant: 0.0,
            low: 100.0,
            high: 7000.0,
            att: 0.0,
            rel: 0.0,
            sib: 0.4,
            emph: 4.0,
            mix: 1.0,
            out: 1.0,
            bands: [Band::default(); MAX_BANDS],
            gain_comp: 1.0,
            sib_hp: [Biquad::default(); 3],
            sib_env: 0.0,
            tilt: Biquad::default(),
            rng: Rng::new(9),
            phases: [0.0; 3],
            f0: 110.0,
            ring: [0.0; 2048],
            pos: 0,
            since: 0,
            frame: vec![0.0; 2048],
            scratch: Vec::with_capacity(2100),
            tele: [0.0; MAX_BANDS + 1],
            dirty: true,
        };
        v.set_param("attack", 4.0);
        v.set_param("release", 60.0);
        v.tune();
        v
    }

    /// Band centres, log-spaced: `f_k = lo·(hi/lo)^{k/(N−1)}`; the Q follows the spacing so neighbouring bands
    /// cross near −3 dB: with ratio `ρ` between centres, `Q = √ρ / (ρ − 1)`.
    fn tune(&mut self) {
        let n = self.n;
        let ratio = (self.high / self.low).powf(1.0 / (n as f32 - 1.0));
        let q = (ratio.sqrt() / (ratio - 1.0)).clamp(1.0, 30.0);
        let shift = 2f32.powf(self.formant / 12.0);
        for k in 0..n {
            let f = self.low * ratio.powi(k as i32);
            let fc = (f * shift).clamp(30.0, self.rate * 0.45);
            let b = &mut self.bands[k];
            for s in b.m.iter_mut() {
                s.set(BiquadKind::BandPass, f.min(self.rate * 0.45), q, 0.0, self.rate);
            }
            for side in b.c.iter_mut() {
                for s in side.iter_mut() {
                    s.set(BiquadKind::BandPass, fc, q, 0.0, self.rate);
                }
            }
        }
        // Two cascaded band-passes have a peak gain of 1 and an effective width of about 0.64× one; summing
        // N bands of a flat carrier gives roughly `N·0.64·(1/Q)`-weighted energy. Normalise so a full-scale
        // modulator and carrier come out near unity.
        self.gain_comp = (q / n as f32).sqrt() * 3.2;
        for s in self.sib_hp.iter_mut() {
            s.set(BiquadKind::HighPass, self.high.min(self.rate * 0.45), 0.707, 0.0, self.rate);
        }
        self.tilt.set(BiquadKind::HighShelf, 2000.0, 0.6, self.emph, self.rate);
        self.dirty = false;
    }

    fn coef(&self, ms: f32) -> f32 {
        (-1.0 / (ms * 0.001 * self.rate)).exp()
    }

    /// PolyBLEP saw (`ph` 0 … 1, increment `dt`).
    fn blep(t: f32, dt: f32) -> f32 {
        if t < dt {
            let x = t / dt;
            2.0 * x - x * x - 1.0
        } else if t > 1.0 - dt {
            let x = (t - 1.0) / dt;
            x * x + 2.0 * x + 1.0
        } else {
            0.0
        }
    }

    fn carrier_sample(&mut self, input: (f32, f32)) -> (f32, f32) {
        let f = (self.f0 * 2f32.powf(self.pitch / 12.0)).clamp(20.0, self.rate * 0.4);
        match self.carrier {
            0 => input,
            3 => (self.rng.bipolar(), self.rng.bipolar()),
            c => {
                // Saw, pulse, or a minor-free power chord (root, fifth, octave) of saws.
                let ratios: &[f32] = if c == 4 { &[1.0, 1.4983, 2.0] } else { &[1.0] };
                let mut s = 0.0;
                for (k, r) in ratios.iter().enumerate() {
                    let dt = f * r / self.rate;
                    let ph = &mut self.phases[k];
                    *ph += dt;
                    if *ph >= 1.0 {
                        *ph -= 1.0;
                    }
                    let saw = 2.0 * *ph - 1.0 - Self::blep(*ph, dt);
                    s += if c == 2 {
                        let p2 = (*ph + 0.5).fract();
                        let saw2 = 2.0 * p2 - 1.0 - Self::blep(p2, dt);
                        0.5 * (saw - saw2)
                    } else {
                        saw
                    };
                }
                let s = s / (ratios.len() as f32).sqrt();
                (s, s)
            }
        }
    }
}

impl Effect for Mandrake {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "carrier" => self.carrier = (v.round().max(0.0) as usize).min(CARRIERS.len() - 1),
            "bands" => {
                self.n = BANDS_N[(v.round().max(0.0) as usize).min(4)];
                self.dirty = true;
            }
            "pitch" => self.pitch = v,
            "formant" => {
                self.formant = v;
                self.dirty = true;
            }
            "low" => {
                self.low = v.clamp(60.0, 500.0);
                self.dirty = true;
            }
            "high" => {
                self.high = v.clamp(2000.0, 12000.0);
                self.dirty = true;
            }
            "attack" => self.att = self.coef(v.clamp(0.5, 50.0)),
            "release" => self.rel = self.coef(v.clamp(5.0, 500.0)),
            "sibilance" => self.sib = v.clamp(0.0, 100.0) / 100.0,
            "emphasis" => {
                self.emph = v;
                self.dirty = true;
            }
            "mix" => self.mix = v.clamp(0.0, 100.0) / 100.0,
            "output" => self.out = db_to_gain(v),
            _ => {}
        }
        if self.dirty {
            self.tune();
        }
    }

    fn reset(&mut self) {
        for b in self.bands.iter_mut() {
            b.env = 0.0;
        }
        self.ring = [0.0; 2048];
        self.tune();
    }

    fn telemetry(&self) -> &[f32] {
        &self.tele
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], sc: Option<(&[f32], &[f32])>) {
        let n = self.n;
        for i in 0..l.len() {
            let (xl, xr) = (l[i], r[i]);
            // The modulator (the voice): the sidechain when the input is the carrier, else the input.
            let m = match (self.carrier, sc) {
                (0, Some((a, b))) => 0.5 * (a[i] + b[i]),
                _ => 0.5 * (xl + xr),
            };
            let m = self.tilt.process(m);
            // Pitch of the voice for the built-in carriers (YIN every 512 samples on the last 2048).
            self.ring[self.pos] = m;
            self.pos = (self.pos + 1) % 2048;
            self.since += 1;
            if self.since >= 512 && self.carrier != 0 {
                self.since = 0;
                for k in 0..2048 {
                    self.frame[k] = self.ring[(self.pos + k) % 2048];
                }
                if let Some((f, c)) = super::pitch::detect_pitch_with(&self.frame, self.rate, 60.0, 800.0, &mut self.scratch) {
                    if c > 0.5 {
                        self.f0 = f;
                    }
                }
            }
            let (cl, cr) = self.carrier_sample((xl, xr));
            let (mut yl, mut yr) = (0.0f32, 0.0f32);
            for b in self.bands[..n].iter_mut() {
                let v = {
                    let t = b.m[0].process(m);
                    b.m[1].process(t)
                }
                .abs();
                b.env = if v > b.env { v + (b.env - v) * self.att } else { v + (b.env - v) * self.rel };
                yl += {
                    let t = b.c[0][0].process(cl);
                    b.c[0][1].process(t)
                } * b.env;
                yr += {
                    let t = b.c[1][0].process(cr);
                    b.c[1][1].process(t)
                } * b.env;
            }
            // Sibilance: the voice's own top (above the highest band) passes on noise / itself, so "s" and "t"
            // stay intelligible – vocoded carriers have little energy there.
            let s = {
                let t = self.sib_hp[0].process(m);
                self.sib_hp[1].process(t)
            };
            let s_env = s.abs();
            self.sib_env = if s_env > self.sib_env { s_env } else { s_env + (self.sib_env - s_env) * self.rel };
            let hiss = self.sib_hp[2].process(self.rng.bipolar()) * self.sib_env * 4.0 + s;
            let g = self.gain_comp * 4.0;
            let (wl, wr) = (yl * g + hiss * self.sib, yr * g + hiss * self.sib);
            l[i] = (xl + (wl - xl) * self.mix) * self.out;
            r[i] = (xr + (wr - xr) * self.mix) * self.out;
        }
        for k in 0..MAX_BANDS {
            self.tele[T_ENV + k] = if k < n { self.bands[k].env } else { 0.0 };
        }
        self.tele[T_PITCH] = if self.carrier == 0 { 0.0 } else { self.f0 * 2f32.powf(self.pitch / 12.0) };
    }
}

// ----------------------------------------------------------------------------------------- frequency shifter

pub const SHIFT_MODES: &[&str] = &["Shift up", "Shift down", "Both (L up, R down)", "Ring mod"];

const SHIFT_PARAMS: &[ParamInfo] = &[
    ParamInfo::choice("mode", "Mode", SHIFT_MODES, 0.0),
    ParamInfo::lin("coarse", "Shift", 0.0, 2000.0, 100.0, "Hz"),
    ParamInfo::lin("fine", "Fine", -50.0, 50.0, 0.0, "Hz"),
    ParamInfo::log("lfo_rate", "LFO rate", 0.02, 20.0, 0.5, "Hz"),
    ParamInfo::lin("lfo_depth", "LFO depth", 0.0, 500.0, 0.0, "Hz"),
    ParamInfo::lin("feedback", "Feedback", 0.0, 90.0, 0.0, "%"),
    ParamInfo::lin("delay", "Delay", 1.0, 500.0, 50.0, "ms"),
    ParamInfo::lin("mix", "Mix", 0.0, 100.0, 50.0, "%"),
    ParamInfo::lin("output", "Output", -24.0, 12.0, 0.0, "dB"),
];

pub fn freqshift_def() -> EffectDef {
    EffectDef {
        id: "freqshift",
        name: "Mercury Wheel (frequency shifter)",
        category: "Modulation",
        description: "Frequency shifter (every partial moves by the same number of Hz, up, down or both ways) and ring modulator, with an LFO on the shift and a feedback delay for spiralling barber-pole sweeps.",
        params: SHIFT_PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(MercuryWheel::new(rate)),
    }
}

/// One path of the 90° phase-difference network: four second-order all-pass sections
/// `y[n] = a²(x[n] + y[n−2]) − x[n−2]`.
#[derive(Clone, Copy, Default)]
struct AllpassChain {
    a: [f32; 4],
    x1: [f32; 4],
    x2: [f32; 4],
    y1: [f32; 4],
    y2: [f32; 4],
}

impl AllpassChain {
    fn new(c: [f32; 4]) -> Self {
        AllpassChain { a: c.map(|v| v * v), ..Default::default() }
    }
    fn process(&mut self, mut x: f32) -> f32 {
        for k in 0..4 {
            let y = self.a[k] * (x + self.y2[k]) - self.x2[k];
            self.x2[k] = self.x1[k];
            self.x1[k] = x;
            self.y2[k] = self.y1[k];
            self.y1[k] = y;
            x = y;
        }
        x
    }
}

/// A Hilbert transformer: two all-pass paths whose outputs differ by 90° from ~20 Hz to ~0.45·fs
/// (coefficients by O. Niemitalo), the first delayed by one sample.
#[derive(Clone, Copy, Default)]
struct Hilbert {
    p1: AllpassChain,
    p2: AllpassChain,
    d: f32,
}

impl Hilbert {
    fn new() -> Self {
        Hilbert { p1: AllpassChain::new([0.692_387_8, 0.936_065_43, 0.988_229_5, 0.998_748_84]), p2: AllpassChain::new([0.402_192_1, 0.856_171_1, 0.972_290_95, 0.995_288_5]), d: 0.0 }
    }
    /// (in-phase, quadrature).
    fn process(&mut self, x: f32) -> (f32, f32) {
        let re = self.d;
        self.d = self.p1.process(x);
        (re, self.p2.process(x))
    }
}

pub struct MercuryWheel {
    rate: f32,
    mode: usize,
    shift: f32,
    fine: f32,
    lfo_rate: f32,
    lfo_depth: f32,
    feedback: f32,
    delay: f32,
    mix: f32,
    out: f32,
    hil: [Hilbert; 2],
    osc: f64,
    lfo: f64,
    fb: [crate::dsp::DelayLine; 2],
    tele: [f32; 2],
}

impl MercuryWheel {
    pub fn new(rate: f32) -> Self {
        let max = (0.51 * rate) as usize;
        MercuryWheel {
            rate,
            mode: 0,
            shift: 100.0,
            fine: 0.0,
            lfo_rate: 0.5,
            lfo_depth: 0.0,
            feedback: 0.0,
            delay: 50.0,
            mix: 0.5,
            out: 1.0,
            hil: [Hilbert::new(), Hilbert::new()],
            osc: 0.0,
            lfo: 0.0,
            fb: [crate::dsp::DelayLine::new(max), crate::dsp::DelayLine::new(max)],
            tele: [0.0; 2],
        }
    }
}

impl Effect for MercuryWheel {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "mode" => self.mode = (v.round().max(0.0) as usize).min(SHIFT_MODES.len() - 1),
            "coarse" => self.shift = v.clamp(0.0, 2000.0),
            "fine" => self.fine = v.clamp(-50.0, 50.0),
            "lfo_rate" => self.lfo_rate = v,
            "lfo_depth" => self.lfo_depth = v,
            "feedback" => self.feedback = v.clamp(0.0, 90.0) / 100.0,
            "delay" => self.delay = v.clamp(1.0, 500.0),
            "mix" => self.mix = v.clamp(0.0, 100.0) / 100.0,
            "output" => self.out = db_to_gain(v),
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.hil = [Hilbert::new(), Hilbert::new()];
        for d in self.fb.iter_mut() {
            d.clear();
        }
    }

    fn telemetry(&self) -> &[f32] {
        &self.tele
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let d = self.delay * 0.001 * self.rate;
        let mut f = 0.0f32;
        for i in 0..l.len() {
            self.lfo = (self.lfo + self.lfo_rate as f64 / self.rate as f64).fract();
            f = self.shift + self.fine + self.lfo_depth * (std::f64::consts::TAU * self.lfo).sin() as f32;
            self.osc = (self.osc + f as f64 / self.rate as f64).rem_euclid(1.0);
            let (s, c) = (std::f64::consts::TAU * self.osc).sin_cos();
            let (s, c) = (s as f32, c as f32);
            for (k, x) in [&mut l[i], &mut r[i]].into_iter().enumerate() {
                let dry = *x;
                let input = dry + self.fb[k].read(d) * self.feedback;
                let (re, im) = self.hil[k].process(input);
                // Single-sideband: the network's quadrature output lags by 90°, so re·cos ωt + im·sin ωt keeps the upper
                // sideband and re·cos ωt − im·sin ωt the lower one.
                let up = re * c + im * s;
                let down = re * c - im * s;
                let wet = match self.mode {
                    0 => up,
                    1 => down,
                    2 => {
                        if k == 0 {
                            up
                        } else {
                            down
                        }
                    }
                    _ => input * s,
                };
                self.fb[k].write(wet);
                *x = (dry + (wet - dry) * self.mix) * self.out;
            }
        }
        self.tele = [f, self.osc as f32];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1 kHz sine shifted up by 300 Hz comes out at 1.3 kHz (the 700 Hz image suppressed).
    #[test]
    fn shifts_a_sine() {
        let rate = 48000.0;
        let mut m = MercuryWheel::new(rate);
        m.set_param("coarse", 300.0);
        m.set_param("mix", 100.0);
        let n = 48000;
        let mut l: Vec<f32> = (0..n).map(|i| (TAU * 1000.0 * i as f32 / rate).sin() * 0.5).collect();
        let mut r = l.clone();
        m.process(&mut l, &mut r, None);
        let mag = |f: f32| {
            let (mut s, mut c) = (0.0f32, 0.0f32);
            for (i, v) in l[8000..].iter().enumerate() {
                let ph = TAU * f * (i + 8000) as f32 / rate;
                s += v * ph.sin();
                c += v * ph.cos();
            }
            (s * s + c * c).sqrt() / (n - 8000) as f32
        };
        let (want, image) = (mag(1300.0), mag(700.0));
        assert!(want > 0.2 && image < want * 0.05, "1300 Hz {want}, 700 Hz {image}");
    }

    #[test]
    fn vocoder_follows_the_voice() {
        let rate = 48000.0;
        let mut v = Mandrake::new(rate);
        let n = 24000;
        // A voice-like buzz: the vocoder's output follows its level.
        let mut l: Vec<f32> = (0..n).map(|i| if i < n / 2 { ((TAU * 150.0 * i as f32 / rate).sin() * 0.5).signum() * 0.3 } else { 0.0 }).collect();
        let mut r = l.clone();
        v.process(&mut l, &mut r, None);
        let loud = l[6000..12000].iter().map(|x| x * x).sum::<f32>();
        let quiet = l[20000..].iter().map(|x| x * x).sum::<f32>();
        assert!(loud > 1.0 && quiet < loud * 1e-2, "{loud} {quiet}");
        assert!(l.iter().all(|x| x.is_finite() && x.abs() < 4.0));
    }

    use std::f32::consts::TAU;
}
