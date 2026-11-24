//! "Clean Voice": the post-production vocal chain of zenoxart/CleanVoice (MIT) ported to Dragon DAW as one
//! stock effect. The stages, their order and the settings baked into them follow that project:
//!
//! ```text
//! in → tube (vari-mu) → comp (leveller) → vari mu → de-esser → multiband 1 → presence (parallel)
//!    → multiband 2 → saturation (2× oversampled) → resonance suppressor → out
//! ```

use super::{Effect, EffectDef, ParamInfo};
use crate::dsp::{Biquad, BiquadKind};

fn to_db(x: f32) -> f32 {
    if x > 1e-7 {
        crate::dsp::gain_to_db(x)
    } else {
        -120.0
    }
}

fn from_db(db: f32) -> f32 {
    if db <= -120.0 {
        0.0
    } else {
        crate::dsp::db_to_gain(db)
    }
}

fn coef(ms: f32, rate: f32) -> f32 {
    if ms <= 0.0 {
        0.0
    } else {
        (-1.0 / (ms * 0.001 * rate)).exp()
    }
}

/// Soft-knee gain computer: gain reduction in dB (≤ 0).
fn soft_knee(over: f32, knee: f32, ratio: f32) -> f32 {
    let slope = 1.0 / ratio - 1.0;
    if 2.0 * over < -knee {
        0.0
    } else if 2.0 * over.abs() <= knee {
        let t = over + knee * 0.5;
        slope * t * t / (2.0 * knee)
    } else {
        slope * over
    }
}

#[derive(Clone, Copy, Default)]
struct Env {
    a: f32,
    r: f32,
    e: f32,
    /// Peaks held through a low-frequency wave's cycle, so the gain does not ride the bass's waveform.
    hold: crate::dsp::PeakHold,
}

impl Env {
    fn set(&mut self, att_ms: f32, rel_ms: f32, rate: f32) {
        self.a = coef(att_ms, rate);
        self.r = coef(rel_ms, rate);
        self.hold.set(30.0, rate);
    }
    #[inline]
    fn next(&mut self, x: f32) -> f32 {
        let x = self.hold.next(x);
        let c = if x > self.e { self.a } else { self.r };
        self.e = x + c * (self.e - x);
        self.e
    }
}

/// Two biquads in series.
#[inline]
fn casc(pair: &mut [Biquad; 2], x: f32) -> f32 {
    let y = pair[0].process(x);
    pair[1].process(y)
}

fn bq(kind: BiquadKind, f: f32, q: f32, g: f32, rate: f32) -> Biquad {
    Biquad::new(kind, f.min(rate * 0.49), q, g, rate)
}

// ---------------------------------------------------------------------------------------------- vari-mu

#[derive(Clone, Copy)]
struct VariMuSettings {
    threshold: f32,
    min_ratio: f32,
    max_ratio: f32,
    ratio_knee: f32,
    knee: f32,
    attack: f32,
    fast_rel: f32,
    slow_rel: f32,
    sc_hp: f32,
    harmonics: f32,
    xf_hz: f32,
    xf_db: f32,
    drive_range: f32,
}

/// Program-dependent valve compressor: ratio grows with drive, dual release, harmonics that follow the
/// gain reduction, transformer roll-off.
struct VariMu {
    s: VariMuSettings,
    drive: f32,
    fast: Env,
    slow: Env,
    gain: f32,
    smooth: f32,
    sc: [Biquad; 2],
    xf: [Biquad; 2],
    dc: [Biquad; 2],
    gr: f32,
}

impl VariMu {
    fn new(s: VariMuSettings, rate: f32) -> Self {
        let mut fast = Env::default();
        fast.set(s.attack, s.fast_rel, rate);
        let mut slow = Env::default();
        slow.set(s.attack * 4.0, s.slow_rel, rate);
        VariMu {
            s,
            drive: 0.0,
            fast,
            slow,
            gain: 1.0,
            smooth: coef(1.0, rate),
            sc: [bq(BiquadKind::HighPass, s.sc_hp, 0.707, 0.0, rate); 2],
            xf: [bq(BiquadKind::HighShelf, s.xf_hz, 0.707, s.xf_db, rate); 2],
            dc: [bq(BiquadKind::HighPass, 12.0, 0.707, 0.0, rate); 2],
            gr: 0.0,
        }
    }

    fn ratio(&self, over: f32) -> f32 {
        let o = over.max(0.0);
        self.s.min_ratio + (self.s.max_ratio - self.s.min_ratio) * o / (o + self.s.ratio_knee)
    }

    fn makeup(&self, threshold: f32) -> f32 {
        let over = -14.0 - threshold;
        -soft_knee(over, self.s.knee, self.ratio(over))
    }

    #[inline]
    fn valve(x: f32, colour: f32) -> f32 {
        if colour <= 0.0 {
            return x;
        }
        let d = x * (1.0 + colour * 2.0);
        let shaped = (d + colour * 0.25 * d * d).tanh();
        x + colour * (shaped / (1.0 + colour * 2.0) - x)
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let threshold = self.s.threshold - self.drive * self.s.drive_range;
        let in_gain = from_db(self.drive * 4.0);
        let makeup = from_db(self.makeup(threshold));
        let mut peak = 0.0f32;
        for i in 0..l.len() {
            let (a, b) = (l[i] * in_gain, r[i] * in_gain);
            let rect = self.sc[0].process(a).abs().max(self.sc[1].process(b).abs());
            let det = self.fast.next(rect).max(self.slow.next(rect));
            let over = to_db(det) - threshold;
            let target = from_db(soft_knee(over, self.s.knee, self.ratio(over)));
            self.gain = target + self.smooth * (self.gain - target);
            peak = peak.max(-to_db(self.gain));
            let colour = self.s.harmonics * (0.35 + 0.65 * (1.0 - self.gain));
            l[i] = self.dc[0].process(self.xf[0].process(Self::valve(a * self.gain * makeup, colour)));
            r[i] = self.dc[1].process(self.xf[1].process(Self::valve(b * self.gain * makeup, colour)));
        }
        self.gr = peak;
    }
}

// ---------------------------------------------------------------------------------------------- leveller

/// Fast high-ratio leveller: one control moves the threshold down and the make-up up, plus a safety catch.
struct Leveller {
    amount: f32,
    env: Env,
    lim: Env,
    gain: f32,
    lim_gain: f32,
    smooth: f32,
    gr: f32,
}

impl Leveller {
    fn new(rate: f32) -> Self {
        let mut env = Env::default();
        env.set(1.0, 130.0, rate);
        let mut lim = Env::default();
        lim.set(0.05, 40.0, rate);
        Leveller { amount: 0.35, env, lim, gain: 1.0, lim_gain: 1.0, smooth: coef(0.8, rate), gr: 0.0 }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let thr = -self.amount * 24.0;
        let makeup = from_db(self.amount * 20.0);
        let ceiling = from_db(-0.3);
        let mut peak = 0.0f32;
        for i in 0..l.len() {
            let det = self.env.next(l[i].abs().max(r[i].abs()));
            let target = from_db(soft_knee(to_db(det) - thr, 6.0, 6.0));
            self.gain = target + self.smooth * (self.gain - target);
            peak = peak.max(-to_db(self.gain));
            let (a, b) = (l[i] * self.gain * makeup, r[i] * self.gain * makeup);
            let le = self.lim.next(a.abs().max(b.abs()));
            let lt = if le > ceiling { ceiling / le } else { 1.0 };
            self.lim_gain = lt + 0.5 * (self.lim_gain - lt);
            l[i] = a * self.lim_gain;
            r[i] = b * self.lim_gain;
        }
        self.gr = peak;
    }
}

// ---------------------------------------------------------------------------------------------- de-esser

/// De-esser: sibilance sidechain (2 kHz high-pass ×2 plus a 6 kHz tilt) on the mid signal, a threshold that
/// can ride the voice's level ("single vocal"), wide-band or split-band reduction, 5 ms look-ahead.
struct DeEsser {
    threshold: f32,
    range: f32,
    single: bool,
    split: bool,
    la: [Vec<f32>; 2],
    w: usize,
    hp1: Biquad,
    hp2: Biquad,
    tilt: Biquad,
    lo: [[Biquad; 2]; 2],
    hi: [[Biquad; 2]; 2],
    fast: Env,
    prog: Env,
    gain: f32,
    smooth: f32,
    gr: f32,
}

impl DeEsser {
    fn new(rate: f32) -> Self {
        let n = (0.005 * rate).round() as usize;
        let mut fast = Env::default();
        fast.set(0.1, 45.0, rate);
        let mut prog = Env::default();
        prog.set(120.0, 600.0, rate);
        DeEsser {
            threshold: -24.0,
            range: 6.0,
            single: true,
            split: false,
            la: [vec![0.0; n.max(1)], vec![0.0; n.max(1)]],
            w: 0,
            hp1: bq(BiquadKind::HighPass, 2000.0, 0.707, 0.0, rate),
            hp2: bq(BiquadKind::HighPass, 2000.0, 0.707, 0.0, rate),
            tilt: bq(BiquadKind::HighShelf, 6000.0, 0.6, 6.0, rate),
            lo: [[bq(BiquadKind::LowPass, 2000.0, std::f32::consts::FRAC_1_SQRT_2, 0.0, rate); 2]; 2],
            hi: [[bq(BiquadKind::HighPass, 2000.0, std::f32::consts::FRAC_1_SQRT_2, 0.0, rate); 2]; 2],
            fast,
            prog,
            gain: 1.0,
            smooth: coef(0.6, rate),
            gr: 0.0,
        }
    }

    fn latency(&self) -> usize {
        self.la[0].len()
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = self.la[0].len();
        let mut peak = 0.0f32;
        for i in 0..l.len() {
            let mid = 0.5 * (l[i] + r[i]);
            let s = self.tilt.process(self.hp2.process(self.hp1.process(mid)));
            let sib = to_db(self.fast.next(s.abs()));
            let prog = to_db(self.prog.next(mid.abs()));
            let adapt = if self.single { (0.7 * (prog + 20.0)).clamp(-12.0, 12.0) } else { 0.0 };
            let target_db = soft_knee(sib - (self.threshold + adapt), 4.0, 8.0).max(-self.range);
            let target = if self.range <= 0.0 { 1.0 } else { from_db(target_db) };
            self.gain = target + self.smooth * (self.gain - target);
            peak = peak.max(-to_db(self.gain));
            // Look-ahead delay.
            let (dl, dr) = (self.la[0][self.w], self.la[1][self.w]);
            self.la[0][self.w] = l[i];
            self.la[1][self.w] = r[i];
            self.w = (self.w + 1) % n;
            for (ch, (x, out)) in [(dl, &mut l[i]), (dr, &mut r[i])].into_iter().enumerate() {
                *out = if self.split {
                    let lo = casc(&mut self.lo[ch], x);
                    let hi = casc(&mut self.hi[ch], x);
                    lo + hi * self.gain
                } else {
                    x * self.gain
                };
            }
        }
        self.gr = peak;
    }
}

// ---------------------------------------------------------------------------------------------- multiband

/// Four-band compressor (100 Hz / 1 kHz / 10 kHz Linkwitz-Riley tree with all-pass alignment), per-band
/// threshold / range / attack / release, "electro" knee and ARC-style program-dependent release. Only the
/// low-mid and high-mid ranges are exposed, as in the original.
struct Multiband {
    lp: [[[Biquad; 2]; 2]; 3],
    hp: [[[Biquad; 2]; 2]; 3],
    ap: [[Biquad; 2]; 3],
    ap_extra: [Biquad; 2],
    bands: [(f32, f32, f32, f32); 4],
    fast: [Env; 4],
    slow: [Env; 4],
    ripple: [f32; 4],
    ripple_c: [f32; 4],
    gain: [f32; 4],
    smooth: f32,
    rate: f32,
    pub gr: [f32; 4],
}

impl Multiband {
    fn new(rate: f32) -> Self {
        let xo = [100.0f32, 1000.0, 10000.0];
        let q = std::f32::consts::FRAC_1_SQRT_2;
        let mut m = Multiband {
            lp: [[[Biquad::default(); 2]; 2]; 3],
            hp: [[[Biquad::default(); 2]; 2]; 3],
            ap: [[Biquad::default(); 2]; 3],
            ap_extra: [bq(BiquadKind::AllPass, xo[2], q, 0.0, rate); 2],
            bands: [(-27.0, 0.0, 34.99, 40.27), (-19.2, -2.0, 35.0, 15.0), (-30.1, -2.0, 40.0, 10.0), (-35.3, 0.0, 2.0, 5.0)],
            fast: [Env::default(); 4],
            slow: [Env::default(); 4],
            ripple: [0.0; 4],
            ripple_c: [coef(40.0, rate), coef(16.0, rate), coef(2.0, rate), coef(0.3, rate)],
            gain: [1.0; 4],
            smooth: coef(1.5, rate),
            rate,
            gr: [0.0; 4],
        };
        for s in 0..3 {
            for ch in 0..2 {
                for st in 0..2 {
                    m.lp[s][ch][st] = bq(BiquadKind::LowPass, xo[s], q, 0.0, rate);
                    m.hp[s][ch][st] = bq(BiquadKind::HighPass, xo[s], q, 0.0, rate);
                }
                m.ap[s][ch] = bq(BiquadKind::AllPass, xo[s], q, 0.0, rate);
            }
        }
        m.update_times();
        m
    }

    fn update_times(&mut self) {
        for b in 0..4 {
            let (_, _, a, r) = self.bands[b];
            self.fast[b].set(a, r, self.rate);
            self.slow[b].set(a * 2.0, r * 4.0, self.rate);
        }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let mut peak = [0.0f32; 4];
        for i in 0..l.len() {
            let mut bands = [[0.0f32; 2]; 4];
            for (ch, x) in [l[i], r[i]].into_iter().enumerate() {
                let low0 = casc(&mut self.lp[0][ch], x);
                let high0 = casc(&mut self.hp[0][ch], x);
                let low1 = casc(&mut self.lp[1][ch], high0);
                let high1 = casc(&mut self.hp[1][ch], high0);
                let low2 = casc(&mut self.lp[2][ch], high1);
                let high2 = casc(&mut self.hp[2][ch], high1);
                let low0 = self.ap[1][ch].process(low0);
                let low0 = self.ap[2][ch].process(low0);
                let low1 = self.ap_extra[ch].process(low1);
                bands[0][ch] = low0;
                bands[1][ch] = low1;
                bands[2][ch] = low2;
                bands[3][ch] = high2;
            }
            let (mut ol, mut or) = (0.0, 0.0);
            for b in 0..4 {
                let (thr, range, _, _) = self.bands[b];
                if range < 0.0 {
                    let rect = bands[b][0].abs().max(bands[b][1].abs());
                    self.ripple[b] = rect + self.ripple_c[b] * (self.ripple[b] - rect);
                    let f = self.fast[b].next(self.ripple[b]);
                    let s = self.slow[b].next(self.ripple[b]);
                    let crest = ((f - s) / f.max(1e-6)).clamp(0.0, 1.0);
                    let det = f.max(s * (1.0 - 0.5 * crest));
                    let red = soft_knee(to_db(det) - thr, 6.0, 3.0).max(range);
                    let target = from_db(red);
                    self.gain[b] = target + self.smooth * (self.gain[b] - target);
                    peak[b] = peak[b].max(-to_db(self.gain[b]));
                } else {
                    self.gain[b] = 1.0;
                }
                ol += bands[b][0] * self.gain[b];
                or += bands[b][1] * self.gain[b];
            }
            l[i] = ol;
            r[i] = or;
        }
        self.gr = peak;
    }
}

// ---------------------------------------------------------------------------------------------- presence

/// Air / presence / sub shelves blended in parallel against the dry signal.
struct Presence {
    mix: f32,
    air: [Biquad; 2],
    pres: [Biquad; 2],
    sub: [Biquad; 2],
}

impl Presence {
    fn new(rate: f32) -> Self {
        Presence { mix: 0.5, air: [bq(BiquadKind::HighShelf, 10000.0, 0.40, 12.0, rate); 2], pres: [bq(BiquadKind::Peak, 2500.0, 0.80, 1.5, rate); 2], sub: [bq(BiquadKind::LowShelf, 40.0, 0.70, 1.5, rate); 2] }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        for i in 0..l.len() {
            for (ch, x) in [&mut l[i], &mut r[i]].into_iter().enumerate() {
                let dry = *x;
                let s = self.air[ch].process(self.pres[ch].process(self.sub[ch].process(dry)));
                *x = dry + self.mix * (s - dry);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------- saturation

/// Pre-emphasis → asymmetric curve → de-emphasis, at twice the sample rate, blended against the dry signal.
struct Saturation {
    mix: f32,
    pre: [Biquad; 2],
    de: [Biquad; 2],
    tone: [Biquad; 2],
    firm: [Biquad; 2],
    dc: [Biquad; 2],
    up: [[Biquad; 2]; 2],
    down: [[Biquad; 2]; 2],
}

impl Saturation {
    fn new(rate: f32) -> Self {
        let os = rate * 2.0;
        // Fourth-order Butterworth anti-imaging / anti-aliasing filters at 0.45 × the base rate.
        let aa = |q: f32| bq(BiquadKind::LowPass, rate * 0.45, q, 0.0, os);
        Saturation {
            mix: 0.5,
            pre: [bq(BiquadKind::HighShelf, 3000.0, 0.70, 4.0, os); 2],
            de: [bq(BiquadKind::HighShelf, 3000.0, 0.70, -4.0, os); 2],
            tone: [bq(BiquadKind::HighShelf, 9000.0, 0.70, -1.5, os); 2],
            firm: [bq(BiquadKind::LowShelf, 80.0, 0.70, -1.0, os); 2],
            dc: [bq(BiquadKind::HighPass, 15.0, 0.707, 0.0, os); 2],
            up: [[aa(0.5412), aa(1.3066)]; 2],
            down: [[aa(0.5412), aa(1.3066)]; 2],
        }
    }

    #[inline]
    fn curve(x: f32) -> f32 {
        let b = x + 0.12 * x * x;
        (b * 1.6).tanh() * 0.72
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        if self.mix <= 0.0 {
            return;
        }
        for i in 0..l.len() {
            for (ch, x) in [&mut l[i], &mut r[i]].into_iter().enumerate() {
                let dry = *x;
                let mut out = 0.0;
                // Zero-stuffed 2× upsampling (×2 gain), shaping, filtering and decimation.
                for k in 0..2 {
                    let s = if k == 0 { dry * 2.0 } else { 0.0 };
                    let s = casc(&mut self.up[ch], s);
                    let y = self.pre[ch].process(s * 1.7);
                    let y = Self::curve(y);
                    let y = self.firm[ch].process(self.tone[ch].process(self.de[ch].process(y)));
                    let y = self.dc[ch].process(y) * 1.25;
                    let y = casc(&mut self.down[ch], y);
                    if k == 0 {
                        out = y;
                    }
                }
                *x = dry + self.mix * (out - dry);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------- resonance suppressor

const FFT: usize = 1024;
const HOP: usize = FFT / 4;
const BINS: usize = FFT / 2 + 1;

/// Dynamic resonance suppressor: STFT, every bin compared to a smoothed neighbourhood, whatever sticks out is
/// pulled down (depth 3.0, sharpness 4.6, selectivity 3.6, fast release, the original sensitivity curve).
struct Suppressor {
    mix: f32,
    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,
    ifft: std::sync::Arc<dyn rustfft::Fft<f32>>,
    window: Vec<f32>,
    norm: f32,
    sens: Vec<f32>,
    smooth_c: Vec<f32>,
    att: f32,
    rel: f32,
    state: [SupState; 2],
    dry: [Vec<f32>; 2],
    dry_w: usize,
    buf: Vec<rustfft::num_complex::Complex<f32>>,
    /// FFT scratch (rustfft's `process` would allocate one on every call – in the audio thread).
    fft_scratch: Vec<rustfft::num_complex::Complex<f32>>,
    mag: Vec<f32>,
    base: Vec<f32>,
    gr: f32,
}

struct SupState {
    input: Vec<f32>,
    output: Vec<f32>,
    gain: Vec<f32>,
    pos: usize,
    hop: usize,
}

impl Suppressor {
    fn new(rate: f32) -> Self {
        let mut planner = rustfft::FftPlanner::new();
        let (fft, ifft) = (planner.plan_fft_forward(FFT), planner.plan_fft_inverse(FFT));
        let scratch_len = fft.get_inplace_scratch_len().max(ifft.get_inplace_scratch_len());
        let window: Vec<f32> = (0..FFT).map(|n| 0.5 - 0.5 * (std::f32::consts::TAU * n as f32 / FFT as f32).cos()).collect();
        let sum: f32 = (0..FFT).step_by(HOP).map(|n| window[n] * window[n]).sum();
        let nodes_f = [200.0f32, 450.0, 1000.0, 2500.0, 5000.0, 8000.0, 16000.0, 22000.0];
        let nodes_v = [-30.0f32, 0.0, 0.0, 0.0, 6.0, 0.0, 0.0, -12.0];
        let sens = (0..BINS)
            .map(|k| {
                let f = (k as f32 * rate / FFT as f32).max(1.0);
                let x = f.ln();
                if x <= nodes_f[0].ln() {
                    return nodes_v[0];
                }
                for i in 1..nodes_f.len() {
                    let x1 = nodes_f[i].ln();
                    if x <= x1 {
                        let x0 = nodes_f[i - 1].ln();
                        let t = ((x - x0) / (x1 - x0).max(1e-6)).clamp(0.0, 1.0);
                        let t = t * t * (3.0 - 2.0 * t);
                        return nodes_v[i - 1] + t * (nodes_v[i] - nodes_v[i - 1]);
                    }
                }
                nodes_v[nodes_v.len() - 1]
            })
            .collect();
        let octaves = 1.6 + (0.25 - 1.6) * (3.6 / 10.0);
        let smooth_c = (0..BINS).map(|k| (-1.0 / (k as f32 * (2f32.powf(octaves) - 1.0) * 0.5).max(1.0)).exp()).collect();
        let st = || SupState { input: vec![0.0; FFT], output: vec![0.0; FFT], gain: vec![1.0; BINS], pos: 0, hop: 0 };
        Suppressor {
            mix: 0.5,
            fft_scratch: vec![Default::default(); scratch_len],
            fft,
            ifft,
            window,
            norm: 1.0 / sum.max(1e-6),
            sens,
            smooth_c,
            att: (-1.0 / (5.0 * 0.001 * rate / HOP as f32).max(1.0)).exp(),
            rel: (-1.0 / (20.0 * 0.001 * rate / HOP as f32).max(1.0)).exp(),
            state: [st(), st()],
            dry: [vec![0.0; FFT], vec![0.0; FFT]],
            dry_w: 0,
            buf: vec![Default::default(); FFT],
            mag: vec![0.0; BINS],
            base: vec![0.0; BINS],
            gr: 0.0,
        }
    }

    fn frame(&mut self, ch: usize) -> f32 {
        let s = &mut self.state[ch];
        for n in 0..FFT {
            let idx = (s.pos + n) % FFT;
            self.buf[n] = rustfft::num_complex::Complex::new(s.input[idx] * self.window[n], 0.0);
        }
        self.fft.process_with_scratch(&mut self.buf, &mut self.fft_scratch);
        for k in 0..BINS {
            self.mag[k] = to_db(self.buf[k].norm() / FFT as f32 * 2.0);
        }
        // Zero-phase smoothing of the magnitude (forward then backward).
        let mut run = self.mag[0];
        for k in 0..BINS {
            run = self.mag[k] + self.smooth_c[k] * (run - self.mag[k]);
            self.base[k] = run;
        }
        run = self.base[BINS - 1];
        for k in (0..BINS).rev() {
            run = self.base[k] + self.smooth_c[k] * (run - self.base[k]);
            self.base[k] = run;
        }
        let (offset, slope, max_red) = (9.0 - 0.8 * 4.6, 0.30 * 3.0, 15.0);
        let mut worst = 0.0f32;
        for k in 0..BINS {
            let excess = self.mag[k] - self.base[k] - offset + self.sens[k];
            let target = if excess > 0.0 { from_db(-(excess * slope).min(max_red)) } else { 1.0 };
            let g = &mut s.gain[k];
            let c = if target < *g { self.att } else { self.rel };
            *g = target + c * (*g - target);
            worst = worst.max(-to_db(*g));
            self.buf[k] *= *g;
            if k > 0 && k < FFT / 2 {
                self.buf[FFT - k] *= *g;
            }
        }
        self.ifft.process_with_scratch(&mut self.buf, &mut self.fft_scratch);
        for n in 0..FFT {
            let idx = (s.pos + n) % FFT;
            s.output[idx] += self.buf[n].re / FFT as f32 * self.window[n] * self.norm;
        }
        worst
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let mut worst = 0.0f32;
        for i in 0..l.len() {
            let w = self.dry_w;
            for ch in 0..2 {
                let x = if ch == 0 { l[i] } else { r[i] };
                let wet = {
                    let s = &mut self.state[ch];
                    let wet = s.output[s.pos];
                    s.output[s.pos] = 0.0;
                    s.input[s.pos] = x;
                    s.pos = (s.pos + 1) % FFT;
                    s.hop += 1;
                    wet
                };
                if self.state[ch].hop >= HOP {
                    self.state[ch].hop = 0;
                    worst = worst.max(self.frame(ch));
                }
                // Latency-matched dry signal.
                let dry = self.dry[ch][w];
                self.dry[ch][w] = x;
                let y = dry + self.mix * (wet - dry);
                if ch == 0 {
                    l[i] = y;
                } else {
                    r[i] = y;
                }
            }
            self.dry_w = (w + 1) % FFT;
        }
        self.gr = worst;
    }
}

// ---------------------------------------------------------------------------------------------- the effect

const CV_PARAMS: &[ParamInfo] = &[
    ParamInfo::toggle("tube", "Tube", true),
    ParamInfo::lin("comp", "Comp", 0.0, 100.0, 35.0, "%"),
    ParamInfo::lin("varimu", "Vari Mu", 0.0, 100.0, 25.0, "%"),
    ParamInfo::lin("ds_thresh", "Deesser threshold", -60.0, 0.0, -24.0, "dB"),
    ParamInfo::lin("ds_range", "Deesser range", 0.0, 24.0, 6.0, "dB"),
    ParamInfo::choice("ds_detect", "Deesser detection", &["Single vocal", "Allround"], 0.0),
    ParamInfo::choice("ds_band", "Deesser band", &["Wide band", "Split band"], 0.0),
    ParamInfo::lin("mb1_low", "Multiband 1 lowmids", -12.0, 0.0, -2.0, "dB"),
    ParamInfo::lin("mb1_high", "Multiband 1 highmids", -12.0, 0.0, -2.0, "dB"),
    ParamInfo::lin("presence", "Presence", 0.0, 100.0, 50.0, "%"),
    ParamInfo::lin("mb2_low", "Multiband 2 lowmids", -12.0, 0.0, -2.0, "dB"),
    ParamInfo::lin("mb2_high", "Multiband 2 highmids", -12.0, 0.0, -2.0, "dB"),
    ParamInfo::lin("saturation", "Saturation", 0.0, 100.0, 50.0, "%"),
    ParamInfo::lin("soothe", "Resonance tamer mix", 0.0, 100.0, 50.0, "%"),
];

pub struct CleanVoice {
    tube_on: bool,
    tube: VariMu,
    comp: Leveller,
    varimu: VariMu,
    deess: DeEsser,
    mb1: Multiband,
    presence: Presence,
    mb2: Multiband,
    sat: Saturation,
    soothe: Suppressor,
    /// [comp GR, vari-mu GR, de-esser GR, mb1 low, mb1 high, mb2 low, mb2 high, soothe GR, tube on]
    /// Gain reductions (comp, vari-mu, de-ess, MB1 low/high, MB2 low/high, soothe, dB), the valve on/off,
    /// and the input and output peaks of the block (dBFS).
    tele: [f32; 11],
}

impl CleanVoice {
    pub fn new(rate: f32) -> Self {
        let tube = VariMuSettings {
            threshold: -20.0,
            min_ratio: 1.6,
            max_ratio: 4.0,
            ratio_knee: 12.0,
            knee: 16.0,
            attack: 0.2,
            fast_rel: 300.0,
            slow_rel: 800.0,
            sc_hp: 60.0,
            harmonics: 0.30,
            xf_hz: 12000.0,
            xf_db: -1.0,
            drive_range: 0.0,
        };
        let vm = VariMuSettings {
            threshold: -14.0,
            min_ratio: 1.4,
            max_ratio: 3.0,
            ratio_knee: 12.0,
            knee: 18.0,
            attack: 8.0,
            fast_rel: 250.0,
            slow_rel: 1200.0,
            sc_hp: 90.0,
            harmonics: 0.22,
            xf_hz: 14000.0,
            xf_db: -0.7,
            drive_range: 14.0,
        };
        let mut varimu = VariMu::new(vm, rate);
        varimu.drive = 0.25;
        CleanVoice {
            tube_on: true,
            tube: VariMu::new(tube, rate),
            comp: Leveller::new(rate),
            varimu,
            deess: DeEsser::new(rate),
            mb1: Multiband::new(rate),
            presence: Presence::new(rate),
            mb2: Multiband::new(rate),
            sat: Saturation::new(rate),
            soothe: Suppressor::new(rate),
            tele: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -120.0, -120.0],
        }
    }
}

impl Effect for CleanVoice {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "tube" => self.tube_on = v > 0.5,
            "comp" => self.comp.amount = (v / 100.0).clamp(0.0, 1.0),
            "varimu" => self.varimu.drive = (v / 100.0).clamp(0.0, 1.0),
            "ds_thresh" => self.deess.threshold = v,
            "ds_range" => self.deess.range = v.max(0.0),
            "ds_detect" => self.deess.single = v < 0.5,
            "ds_band" => self.deess.split = v >= 0.5,
            "mb1_low" => self.mb1.bands[1].1 = v.min(0.0),
            "mb1_high" => self.mb1.bands[2].1 = v.min(0.0),
            "presence" => self.presence.mix = (v / 100.0).clamp(0.0, 1.0),
            "mb2_low" => self.mb2.bands[1].1 = v.min(0.0),
            "mb2_high" => self.mb2.bands[2].1 = v.min(0.0),
            "saturation" => self.sat.mix = (v / 100.0).clamp(0.0, 1.0),
            "soothe" => self.soothe.mix = (v / 100.0).clamp(0.0, 1.0),
            _ => {}
        }
    }
    fn reset(&mut self) {
        for s in &mut self.soothe.state {
            s.input.fill(0.0);
            s.output.fill(0.0);
            s.gain.fill(1.0);
        }
        self.soothe.dry.iter_mut().for_each(|d| d.fill(0.0));
        self.deess.la.iter_mut().for_each(|d| d.fill(0.0));
    }
    fn latency(&self) -> usize {
        self.deess.latency() + FFT
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let peak = |l: &[f32], r: &[f32]| 20.0 * l.iter().chain(r.iter()).fold(0.0f32, |m, x| m.max(x.abs())).max(1e-6).log10();
        let input = peak(l, r);
        if self.tube_on {
            self.tube.process(l, r);
        }
        self.comp.process(l, r);
        self.varimu.process(l, r);
        self.deess.process(l, r);
        self.mb1.process(l, r);
        self.presence.process(l, r);
        self.mb2.process(l, r);
        self.sat.process(l, r);
        self.soothe.process(l, r);
        self.tele = [self.comp.gr, self.varimu.gr, self.deess.gr, self.mb1.gr[1], self.mb1.gr[2], self.mb2.gr[1], self.mb2.gr[2], self.soothe.gr, if self.tube_on { 1.0 } else { 0.0 }, input, peak(l, r)];
    }
    fn gain_reduction(&self) -> f32 {
        self.comp.gr + self.varimu.gr
    }
    fn telemetry(&self) -> &[f32] {
        &self.tele
    }
}

pub fn clean_voice_def() -> EffectDef {
    EffectDef {
        id: "clean_voice",
        name: "Purify (voice cleanup)",
        category: "Vocal",
        description: "Post-production vocal chain in one plugin (after zenoxart/CleanVoice): valve vari-mu, leveller, vari-mu, de-esser, two 4-band multibands, parallel presence, oversampled saturation and a resonance suppressor.",
        params: CV_PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(CleanVoice::new(rate)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(n: usize) -> Vec<f32> {
        // A buzzy "vowel" with level swings and an "s" burst every second.
        let mut rng = crate::dsp::Rng::new(9);
        (0..n)
            .map(|i| {
                let t = i as f32 / 48000.0;
                let env = 0.25 + 0.2 * (t * 2.0).sin().abs();
                let vowel = (0..12).map(|h| ((h + 1) as f32 * 160.0 * std::f32::consts::TAU * t).sin() / (h + 1) as f32).sum::<f32>() * 0.25 * env;
                let s = if (t % 1.0) > 0.8 { (rng.next_f32() * 2.0 - 1.0) * 0.3 } else { 0.0 };
                vowel + s
            })
            .collect()
    }

    #[test]
    fn chain_is_stable_levels_and_reports_latency() {
        let mut e = CleanVoice::new(48000.0);
        let x: Vec<f32> = voice(96000).iter().map(|v| v * 2.5).collect();
        let (mut l, mut r) = (x.clone(), x.clone());
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            e.process(a, b, None);
        }
        assert!(l.iter().all(|v| v.is_finite() && v.abs() < 1.5));
        // The leveller and vari-mu are working on a vocal at this level.
        assert!(e.tele[0] > 0.5 || e.tele[1] > 0.5, "leveller / vari-mu GR {:?}", e.tele);
        // The display's input and output readings are the real block peaks.
        let peak = |v: &[f32]| 20.0 * v.iter().fold(0.0f32, |m, x| m.max(x.abs())).log10();
        let n = l.len();
        assert!((e.tele[10] - peak(&l[n - 256..]).max(peak(&r[n - 256..]))).abs() < 0.01, "output peak {:?}", e.tele);
        assert!((e.tele[9] - peak(&x[n - 256..])).abs() < 0.01, "input peak {:?}", e.tele);
        assert_eq!(e.latency(), 240 + 1024);
        let rms = |v: &[f32]| (v.iter().map(|s| s * s).sum::<f32>() / v.len() as f32).sqrt();
        let ratio = rms(&l[48000..]) / rms(&x[48000..]);
        assert!((0.3..4.0).contains(&ratio), "output level sane: {ratio}");
    }

    #[test]
    fn de_esser_ducks_sibilance() {
        let mut d = DeEsser::new(48000.0);
        d.threshold = -40.0;
        d.range = 12.0;
        d.single = false;
        let mut rng = crate::dsp::Rng::new(4);
        let mut l: Vec<f32> = (0..48000).map(|_| (rng.next_f32() * 2.0 - 1.0) * 0.4).collect();
        let mut r = l.clone();
        d.process(&mut l, &mut r);
        assert!(d.gr > 3.0, "{}", d.gr);
    }

    #[test]
    fn multiband_sums_flat_when_idle() {
        let mut m = Multiband::new(48000.0);
        m.bands = [(0.0, 0.0, 10.0, 10.0); 4];
        let x: Vec<f32> = (0..48000).map(|i| (i as f32 * 0.07).sin() * 0.5).collect();
        let (mut l, mut r) = (x.clone(), x.clone());
        m.process(&mut l, &mut r);
        let rms = |v: &[f32]| (v[24000..].iter().map(|s| s * s).sum::<f32>() / 24000.0).sqrt();
        assert!((rms(&l) / rms(&x) - 1.0).abs() < 0.05, "{}", rms(&l) / rms(&x));
    }
}
