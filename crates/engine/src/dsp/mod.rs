//! DSP building blocks: biquads, one-pole filters, envelopes, LFOs, delay lines, a noise source and
//! level conversions. Everything is allocation-free after construction.

pub mod loudness;
pub mod spectrum;
pub mod stretch;
pub mod tube;

use crate::model::Envelope;

#[inline]
pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

#[inline]
pub fn gain_to_db(g: f32) -> f32 {
    if g <= 1e-9 {
        -180.0
    } else {
        20.0 * g.log10()
    }
}

/// Equal-power pan law: returns (left gain, right gain) for pan −1 … +1 (0 dB at the centre on both sides
/// for a balanced stereo source, i.e. a balance control).
#[inline]
pub fn balance(pan: f32) -> (f32, f32) {
    let p = pan.clamp(-1.0, 1.0);
    if p <= 0.0 {
        (1.0, 1.0 + p)
    } else {
        (1.0 - p, 1.0)
    }
}

/// Constant-power pan for mono sources.
#[inline]
pub fn pan_law(pan: f32) -> (f32, f32) {
    let a = (pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
    (a.cos() * std::f32::consts::SQRT_2, a.sin() * std::f32::consts::SQRT_2)
}

/// Small, fast xorshift noise source.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.max(1))
    }
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    /// Uniform 0 … 1.
    #[inline]
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
    /// Uniform −1 … 1.
    #[inline]
    pub fn bipolar(&mut self) -> f32 {
        self.next_f32() * 2.0 - 1.0
    }
}

/// One-pole smoother for parameter changes (avoids zipper noise).
#[derive(Debug, Clone, Copy)]
pub struct Smooth {
    pub value: f32,
    coef: f32,
}

impl Smooth {
    pub fn new(value: f32, ms: f32, rate: f32) -> Self {
        Smooth { value, coef: (-1.0 / (ms.max(0.01) * 0.001 * rate)).exp() }
    }
    #[inline]
    pub fn next(&mut self, target: f32) -> f32 {
        self.value = target + (self.value - target) * self.coef;
        self.value
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BiquadKind {
    #[default]
    Peak,
    LowShelf,
    HighShelf,
    LowPass,
    HighPass,
    BandPass,
    Notch,
    AllPass,
}

impl BiquadKind {
    pub const ALL: [BiquadKind; 7] = [BiquadKind::Peak, BiquadKind::LowShelf, BiquadKind::HighShelf, BiquadKind::LowPass, BiquadKind::HighPass, BiquadKind::BandPass, BiquadKind::Notch];
    pub fn from_index(i: f32) -> Self {
        Self::ALL.get(i.round().max(0.0) as usize).copied().unwrap_or_default()
    }
    pub fn name(self) -> &'static str {
        match self {
            BiquadKind::Peak => "Bell",
            BiquadKind::LowShelf => "Low shelf",
            BiquadKind::HighShelf => "High shelf",
            BiquadKind::LowPass => "Low pass",
            BiquadKind::HighPass => "High pass",
            BiquadKind::BandPass => "Band pass",
            BiquadKind::Notch => "Notch",
            BiquadKind::AllPass => "All pass",
        }
    }
}

/// RBJ cookbook biquad (transposed direct form II), one channel.
#[derive(Debug, Clone, Copy, Default)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    pub fn new(kind: BiquadKind, freq: f32, q: f32, gain_db: f32, rate: f32) -> Self {
        let mut b = Biquad::default();
        b.set(kind, freq, q, gain_db, rate);
        b
    }

    /// From normalized coefficients (a0 = 1).
    pub fn from_coefficients(b0: f32, b1: f32, b2: f32, a1: f32, a2: f32) -> Self {
        Biquad { b0, b1, b2, a1, a2, z1: 0.0, z2: 0.0 }
    }

    pub fn set(&mut self, kind: BiquadKind, freq: f32, q: f32, gain_db: f32, rate: f32) {
        let f = freq.clamp(10.0, rate * 0.49);
        let w0 = std::f32::consts::TAU * f / rate;
        let (sin, cos) = w0.sin_cos();
        let q = q.max(0.025);
        let alpha = sin / (2.0 * q);
        let a = 10f32.powf(gain_db / 40.0);
        let (b0, b1, b2, a0, a1, a2) = match kind {
            BiquadKind::Peak => (1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a),
            BiquadKind::LowShelf => {
                let s = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cos + s),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                    a * ((a + 1.0) - (a - 1.0) * cos - s),
                    (a + 1.0) + (a - 1.0) * cos + s,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                    (a + 1.0) + (a - 1.0) * cos - s,
                )
            }
            BiquadKind::HighShelf => {
                let s = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cos + s),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                    a * ((a + 1.0) + (a - 1.0) * cos - s),
                    (a + 1.0) - (a - 1.0) * cos + s,
                    2.0 * ((a - 1.0) - (a + 1.0) * cos),
                    (a + 1.0) - (a - 1.0) * cos - s,
                )
            }
            BiquadKind::LowPass => ((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            BiquadKind::HighPass => ((1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            BiquadKind::BandPass => (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            BiquadKind::Notch => (1.0, -2.0 * cos, 1.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            BiquadKind::AllPass => (1.0 - alpha, -2.0 * cos, 1.0 + alpha, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
        };
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        if !y.is_finite() {
            self.reset();
            return 0.0;
        }
        y
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    /// Magnitude response in dB at `freq` (for drawing EQ curves).
    pub fn response_db(&self, freq: f32, rate: f32) -> f32 {
        let w = std::f64::consts::TAU * freq as f64 / rate as f64;
        let (c1, s1) = (w.cos(), w.sin());
        let (c2, s2) = ((2.0 * w).cos(), (2.0 * w).sin());
        let (b0, b1, b2, a1, a2) = (self.b0 as f64, self.b1 as f64, self.b2 as f64, self.a1 as f64, self.a2 as f64);
        let nr = b0 + b1 * c1 + b2 * c2;
        let ni = -(b1 * s1 + b2 * s2);
        let dr = 1.0 + a1 * c1 + a2 * c2;
        let di = -(a1 * s1 + a2 * s2);
        let mag = ((nr * nr + ni * ni) / (dr * dr + di * di)).sqrt();
        (20.0 * mag.max(1e-12).log10()) as f32
    }
}

/// One-pole low-pass (`process`) and high-pass (`highpass`).
#[derive(Debug, Clone, Copy, Default)]
pub struct OnePole {
    a: f32,
    z: f32,
}

impl OnePole {
    pub fn new(freq: f32, rate: f32) -> Self {
        let mut o = OnePole::default();
        o.set(freq, rate);
        o
    }
    pub fn set(&mut self, freq: f32, rate: f32) {
        self.a = (-std::f32::consts::TAU * freq.clamp(1.0, rate * 0.49) / rate).exp();
    }
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        self.z = x * (1.0 - self.a) + self.z * self.a;
        self.z
    }
    #[inline]
    pub fn highpass(&mut self, x: f32) -> f32 {
        x - self.process(x)
    }
    pub fn reset(&mut self) {
        self.z = 0.0;
    }
}

/// State-variable filter (Chamberlin/TPT, Zavalishin) with resonance – used by the synth.
#[derive(Debug, Clone, Copy, Default)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
}

impl Svf {
    /// Returns (low, band, high).
    #[inline]
    pub fn process(&mut self, x: f32, cutoff: f32, resonance: f32, rate: f32) -> (f32, f32, f32) {
        let g = (std::f32::consts::PI * cutoff.clamp(20.0, rate * 0.45) / rate).tan();
        let k = 2.0 - 1.95 * resonance.clamp(0.0, 1.0);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let v3 = x - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        if !self.ic1.is_finite() || !self.ic2.is_finite() {
            *self = Svf::default();
        }
        (v2, v1, x - k * v1 - v2)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Idle,
    Delay,
    Attack,
    Hold,
    Decay,
    Sustain,
    Release,
}

/// DAHDSR envelope generator with curve tension.
#[derive(Debug, Clone, Copy)]
pub struct EnvGen {
    p: Envelope,
    rate: f32,
    stage: Stage,
    t: f32,
    level: f32,
    release_from: f32,
}

impl EnvGen {
    pub fn new(p: Envelope, rate: f32) -> Self {
        EnvGen { p, rate, stage: Stage::Idle, t: 0.0, level: 0.0, release_from: 0.0 }
    }

    pub fn set(&mut self, p: Envelope) {
        self.p = p;
    }

    pub fn trigger(&mut self) {
        self.stage = if self.p.delay > 0.0 { Stage::Delay } else { Stage::Attack };
        self.t = 0.0;
    }

    pub fn release(&mut self) {
        if self.stage != Stage::Idle && self.stage != Stage::Release {
            self.release_from = self.level;
            self.stage = Stage::Release;
            self.t = 0.0;
        }
    }

    /// Fast release used for chokes (`seconds` overrides the release time).
    pub fn choke(&mut self, seconds: f32) {
        self.release_from = self.level;
        self.p.release = seconds;
        self.stage = Stage::Release;
        self.t = 0.0;
    }

    pub fn is_idle(&self) -> bool {
        self.stage == Stage::Idle
    }

    pub fn level(&self) -> f32 {
        self.level
    }

    fn bend(x: f32, tension: f32) -> f32 {
        if tension.abs() < 1e-3 {
            x
        } else {
            let k = tension * 5.0;
            ((k * x).exp() - 1.0) / (k.exp() - 1.0)
        }
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        let dt = 1.0 / self.rate;
        let p = self.p;
        match self.stage {
            Stage::Idle => self.level = 0.0,
            Stage::Delay => {
                self.t += dt;
                if self.t >= p.delay {
                    self.stage = Stage::Attack;
                    self.t = 0.0;
                }
            }
            Stage::Attack => {
                self.t += dt;
                let x = if p.attack <= 0.0 { 1.0 } else { (self.t / p.attack).min(1.0) };
                self.level = Self::bend(x, -p.attack_tension);
                if x >= 1.0 {
                    self.stage = Stage::Hold;
                    self.t = 0.0;
                }
            }
            Stage::Hold => {
                self.level = 1.0;
                self.t += dt;
                if self.t >= p.hold {
                    self.stage = Stage::Decay;
                    self.t = 0.0;
                }
            }
            Stage::Decay => {
                self.t += dt;
                let x = if p.decay <= 0.0 { 1.0 } else { (self.t / p.decay).min(1.0) };
                self.level = 1.0 + (p.sustain - 1.0) * Self::bend(x, p.release_tension);
                if x >= 1.0 {
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => self.level = p.sustain,
            Stage::Release => {
                self.t += dt;
                let x = if p.release <= 0.0 { 1.0 } else { (self.t / p.release).min(1.0) };
                self.level = self.release_from * (1.0 - Self::bend(x, p.release_tension));
                if x >= 1.0 {
                    self.stage = Stage::Idle;
                    self.level = 0.0;
                }
            }
        }
        self.level
    }
}

/// Fractional stereo delay line with linear interpolation.
#[derive(Debug, Clone)]
pub struct DelayLine {
    buf: Vec<f32>,
    pos: usize,
}

impl DelayLine {
    pub fn new(max_samples: usize) -> Self {
        DelayLine { buf: vec![0.0; max_samples.max(2) + 2], pos: 0 }
    }
    pub fn len(&self) -> usize {
        self.buf.len() - 2
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    #[inline]
    pub fn write(&mut self, x: f32) {
        self.pos = (self.pos + 1) % self.buf.len();
        self.buf[self.pos] = x;
    }
    /// Reads `delay` samples back (fractional).
    #[inline]
    pub fn read(&self, delay: f32) -> f32 {
        let n = self.buf.len();
        let d = delay.clamp(0.0, (n - 2) as f32);
        let i = d as usize;
        let f = d - i as f32;
        let a = self.buf[(self.pos + n - i) % n];
        let b = self.buf[(self.pos + n - i - 1) % n];
        a + (b - a) * f
    }
    pub fn clear(&mut self) {
        self.buf.iter_mut().for_each(|x| *x = 0.0);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LfoShape {
    #[default]
    Sine,
    Triangle,
    Square,
    SampleHold,
}

#[derive(Debug, Clone)]
pub struct Lfo {
    phase: f32,
    held: f32,
    rng: Rng,
}

impl Default for Lfo {
    fn default() -> Self {
        Lfo { phase: 0.0, held: 0.0, rng: Rng::new(99) }
    }
}

impl Lfo {
    pub fn with_phase(phase: f32) -> Self {
        Lfo { phase, ..Default::default() }
    }
    /// Advances by one sample; returns −1 … 1.
    #[inline]
    pub fn next(&mut self, freq: f32, rate: f32, shape: LfoShape) -> f32 {
        self.phase += freq / rate;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
            self.held = self.rng.bipolar();
        }
        match shape {
            LfoShape::Sine => (self.phase * std::f32::consts::TAU).sin(),
            LfoShape::Triangle => 4.0 * (self.phase - 0.5).abs() - 1.0,
            LfoShape::Square => {
                if self.phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            LfoShape::SampleHold => self.held,
        }
    }
}

/// Envelope follower (peak, attack/release in ms).
#[derive(Debug, Clone, Copy)]
pub struct Follower {
    env: f32,
    att: f32,
    rel: f32,
}

impl Follower {
    pub fn new(attack_ms: f32, release_ms: f32, rate: f32) -> Self {
        let mut f = Follower { env: 0.0, att: 0.0, rel: 0.0 };
        f.set(attack_ms, release_ms, rate);
        f
    }
    pub fn set(&mut self, attack_ms: f32, release_ms: f32, rate: f32) {
        self.att = (-1.0 / (attack_ms.max(0.01) * 0.001 * rate)).exp();
        self.rel = (-1.0 / (release_ms.max(0.1) * 0.001 * rate)).exp();
    }
    #[inline]
    pub fn next(&mut self, x: f32) -> f32 {
        let x = x.abs();
        let c = if x > self.env { self.att } else { self.rel };
        self.env = x + (self.env - x) * c;
        self.env
    }
    pub fn value(&self) -> f32 {
        self.env
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn biquad_lowpass_attenuates_highs() {
        let rate = 48000.0;
        let b = Biquad::new(BiquadKind::LowPass, 1000.0, 0.707, 0.0, rate);
        assert!(b.response_db(100.0, rate).abs() < 0.5);
        assert!(b.response_db(10000.0, rate) < -30.0);
        let p = Biquad::new(BiquadKind::Peak, 1000.0, 1.0, 6.0, rate);
        assert!((p.response_db(1000.0, rate) - 6.0).abs() < 0.1);
    }

    #[test]
    fn envelope_runs_through_stages() {
        let env = Envelope { enabled: true, delay: 0.0, attack: 0.01, hold: 0.0, decay: 0.01, sustain: 0.5, release: 0.01, attack_tension: 0.0, release_tension: 0.0 };
        let mut e = EnvGen::new(env, 1000.0);
        e.trigger();
        let mut peak = 0.0f32;
        for _ in 0..20 {
            peak = peak.max(e.next());
        }
        assert!((peak - 1.0).abs() < 1e-3);
        for _ in 0..30 {
            e.next();
        }
        assert!((e.level() - 0.5).abs() < 1e-3);
        e.release();
        for _ in 0..20 {
            e.next();
        }
        assert!(e.is_idle());
    }

    #[test]
    fn delay_line_reads_back() {
        let mut d = DelayLine::new(100);
        for i in 0..10 {
            d.write(i as f32);
        }
        assert_eq!(d.read(0.0), 9.0);
        assert_eq!(d.read(3.0), 6.0);
        assert!((d.read(1.5) - 7.5).abs() < 1e-6);
    }
}
