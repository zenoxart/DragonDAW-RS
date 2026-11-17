//! Hammerstein identification of an effect: what its nonlinearity and its filtering look like with the
//! current settings.
//!
//! A (generalised) Hammerstein model explains a weakly nonlinear system as parallel branches: the input
//! raised to the power n, then a linear filter G_n. Branch 1 is the effect's ordinary frequency response;
//! branches 2, 3, … are how much 2nd, 3rd, … order distortion it adds, and how that distortion is coloured.
//!
//! The measurement follows Farina / Novak: a synchronised exponential sine sweep is played through a fresh
//! instance of the effect, the output is deconvolved with the sweep, and the higher harmonic impulse
//! responses (which land before the linear one) are cut out. Their spectra are the higher harmonic
//! frequency responses; a triangular linear system (the expansion of sin^m into harmonics) turns those
//! into the Hammerstein kernels G_n.

use crate::effects::Effect;
use rustfft::{num_complex::Complex, FftPlanner};
use std::f32::consts::PI;

/// The highest order measured.
pub const MAX_ORDER: usize = 5;

/// A finished measurement.
#[derive(Debug, Clone)]
pub struct Kernels {
    /// The sweep level the effect was driven with (dBFS peak).
    pub level_db: f32,
    /// Frequencies of the curves (log spaced).
    pub freqs: Vec<f32>,
    /// |G_n| in dB for n = 1 … MAX_ORDER, one value per frequency (smoothed).
    pub kernels: Vec<Vec<f32>>,
    /// Total harmonic distortion (in %) of a sine at each frequency at the sweep level.
    pub thd: Vec<f32>,
    /// The memoryless curve the kernels amount to at 1 kHz: (input, output) for inputs −1 … 1.
    pub curve: Vec<(f32, f32)>,
    /// The effect changes over time (LFOs, pitch tracking): its "harmonics" are partly modulation
    /// sidebands, and the kernels describe it only roughly.
    pub time_varying: bool,
    /// How much the linear response moved between two runs (dB).
    pub wobble_db: f32,
}

impl Kernels {
    /// THD (%) at the frequency closest to `f`.
    pub fn thd_at(&self, f: f32) -> f32 {
        let k = self.freqs.iter().enumerate().min_by(|a, b| (a.1.ln() - f.ln()).abs().total_cmp(&(b.1.ln() - f.ln()).abs())).map(|x| x.0).unwrap_or(0);
        self.thd.get(k).copied().unwrap_or(0.0)
    }
}

fn binom(n: usize, k: usize) -> f32 {
    (0..k).fold(1.0f64, |acc, i| acc * (n - i) as f64 / (i + 1) as f64) as f32
}

/// Complex power of j.
fn jpow(e: i32) -> Complex<f32> {
    match e.rem_euclid(4) {
        0 => Complex::new(1.0, 0.0),
        1 => Complex::new(0.0, 1.0),
        2 => Complex::new(-1.0, 0.0),
        _ => Complex::new(0.0, -1.0),
    }
}

/// The weight of kernel `m` in harmonic `k` for a sine of amplitude `a`, as the deconvolution sees it:
/// 2j · a^(m−1) · (2j)^(−m) · C(m, (m+k)/2) · (−1)^((m−k)/2).
fn weight(m: usize, k: usize, a: f32) -> Complex<f32> {
    if m < k || (m - k) % 2 == 1 {
        return Complex::new(0.0, 0.0);
    }
    let sign = if ((m - k) / 2) % 2 == 1 { -1.0 } else { 1.0 };
    let scale = 2.0 * a.powi(m as i32 - 1) * binom(m, (m + k) / 2) * sign / 2f32.powi(m as i32);
    jpow(1 - m as i32) * scale
}

/// Where the sweep starts: two octaves below the audible range, so every order up to the 3rd is excited
/// down to 20 Hz (and the 5th from 40 Hz), and the band edge rings nowhere near the curves.
const F1: f32 = 5.0;
/// The curves are drawn from here.
pub const SHOW_FROM: f32 = 20.0;
/// The longest the sweep may go on ringing in the effect after it ends (reverbs, delays).
const MAX_TAIL: f32 = 6.0;

/// Plays the driven sweep (after `pre` samples of silence) through `fx` and returns the output from the
/// sweep's start on, running on until the effect has gone quiet.
fn play(fx: &mut dyn Effect, sweep: &[f32], a: f32, rate: f32, pre: usize) -> Vec<f32> {
    fx.reset();
    let block = 512;
    let (mut lb, mut rb) = (vec![0.0f32; block], vec![0.0f32; block]);
    let mut out = Vec::with_capacity(sweep.len() * 2);
    let latency = fx.latency();
    let max_len = pre + sweep.len() + latency + (MAX_TAIL * rate) as usize;
    let min_len = pre + sweep.len() + latency + (0.3 * rate) as usize;
    let mut pos = 0;
    let mut quiet = 0usize;
    while pos < max_len {
        for k in 0..block {
            let i = pos + k;
            let x = if i >= pre { sweep.get(i - pre).copied().unwrap_or(0.0) * a } else { 0.0 };
            lb[k] = x;
            rb[k] = x;
        }
        fx.process(&mut lb, &mut rb, None);
        let mut peak = 0.0f32;
        for k in 0..block {
            let y = 0.5 * (lb[k] + rb[k]);
            let y = if y.is_finite() { y } else { 0.0 };
            peak = peak.max(y.abs());
            if pos + k >= pre {
                out.push(y);
            }
        }
        pos += block;
        // Done once the tail has stayed below −120 dB for a while.
        quiet = if peak < 1e-6 { quiet + block } else { 0 };
        if pos >= min_len && quiet >= (0.1 * rate) as usize {
            break;
        }
    }
    out
}

/// Harmonic responses of `out` (the effect's answer to `sweep` · `a`) on a common bin grid of `nfft`
/// bins: for each order k, its impulse response cut out around its arrival (L·ln k before the linear
/// one) and lined up there.
struct Harmonics {
    nfft: usize,
    h: Vec<Vec<Complex<f32>>>,
}

fn harmonics(out: &[f32], sweep: &[f32], a: f32, rate: f32, l: f32, f2: f32, latency: usize) -> Harmonics {
    let n = sweep.len();
    let size = (out.len() + n).next_power_of_two() * 2;
    let mut planner = FftPlanner::<f32>::new();
    let fwd = planner.plan_fft_forward(size);
    let inv = planner.plan_fft_inverse(size);
    let mut x: Vec<Complex<f32>> = (0..size).map(|i| Complex::new(sweep.get(i).copied().unwrap_or(0.0) * a, 0.0)).collect();
    let mut y: Vec<Complex<f32>> = (0..size).map(|i| Complex::new(out.get(i).copied().unwrap_or(0.0), 0.0)).collect();
    fwd.process(&mut x);
    fwd.process(&mut y);
    let peak = x.iter().take(size / 2).map(|c| c.norm_sqr()).fold(0.0f32, f32::max);
    let eps = peak * 1e-7;
    // Band limits with smooth (raised-cosine) edges instead of a brick wall, which would ring.
    let edge = |f: f32| -> f32 {
        let lo = ((f - F1) / F1).clamp(0.0, 1.0);
        let hi = ((f2 - f) / (0.05 * f2)).clamp(0.0, 1.0);
        (0.5 - 0.5 * (PI * lo).cos()) * (0.5 - 0.5 * (PI * hi).cos())
    };
    for k in 0..size {
        let f = (k.min(size - k)) as f32 * rate / size as f32;
        let d = x[k].norm_sqr() + eps;
        y[k] = y[k] * x[k].conj() / d * edge(f);
    }
    inv.process(&mut y);
    let h: Vec<f32> = y.iter().map(|c| c.re / size as f32).collect();
    // Window lengths: a harmonic's response may run until the next one (order k − 1) arrives; the linear
    // one runs on through the whole tail.
    let gap = |k: usize| l * ((k as f32) / (k as f32 - 1.0)).ln() * rate;
    // The band edges ring before each arrival too (for ~1 / F1), so a window starts well ahead of it.
    let pre_of = |k: usize| ((gap(k + 1) * 0.35) as usize).max(64);
    let len_of = |k: usize| if k == 1 { out.len().saturating_sub(latency).min(size / 4) } else { (gap(k) * 0.6) as usize };
    let nfft = (1..=MAX_ORDER).map(|k| pre_of(k) + len_of(k)).max().unwrap_or(4096).next_power_of_two().max(8192);
    let fft = planner.plan_fft_forward(nfft);
    let mut hs = Vec::with_capacity(MAX_ORDER);
    for k in 1..=MAX_ORDER {
        let at = latency as f32 - l * (k as f32).ln() * rate;
        let start = at.floor();
        let frac = at - start;
        let (pre, len) = (pre_of(k), len_of(k));
        let fade = (len / 10).max(16);
        let mut buf = vec![Complex::new(0.0, 0.0); nfft];
        for j in 0..pre + len {
            let rel = j as i64 - pre as i64;
            let src = (start as i64 + rel).rem_euclid(size as i64) as usize;
            let win = if j < pre {
                0.5 - 0.5 * (PI * j as f32 / pre as f32).cos()
            } else if j >= pre + len - fade {
                0.5 + 0.5 * (PI * (j - (pre + len - fade)) as f32 / fade as f32).cos()
            } else {
                1.0
            };
            buf[rel.rem_euclid(nfft as i64) as usize] = Complex::new(h[src] * win, 0.0);
        }
        fft.process(&mut buf);
        for (b, c) in buf.iter_mut().enumerate() {
            *c *= Complex::from_polar(1.0, 2.0 * PI * (b as f32 / nfft as f32) * frac);
        }
        buf.truncate(nfft / 2);
        hs.push(buf);
    }
    Harmonics { nfft, h: hs }
}

/// Measures `fx` (a fresh instance, already set up) at `rate`, driven with a sweep peaking at `level_db`.
pub fn measure(fx: &mut dyn Effect, rate: f32, level_db: f32) -> Kernels {
    let a = 10f32.powf(level_db / 20.0);
    let f2 = (rate * 0.45).min(22_000.0);
    // Synchronised sweep (Novak): L is a multiple of 1/F1 so every harmonic is the sweep shifted in time.
    // Long enough that the harmonics sit far apart (long windows: fine low-frequency resolution).
    let target = 8.0f32;
    let l = ((F1 * target / (f2 / F1).ln()).round() / F1).max(1.0 / F1);
    let t = l * (f2 / F1).ln();
    let n = (t * rate) as usize;
    let fade_in = (0.02 * rate) as usize;
    let fade_out = (0.01 * rate) as usize;
    let sweep: Vec<f32> = (0..n)
        .map(|i| {
            let tt = i as f64 / rate as f64;
            let g = if i < fade_in {
                0.5 - 0.5 * (PI * i as f32 / fade_in as f32).cos()
            } else if i + fade_out > n {
                0.5 - 0.5 * (PI * (n - i) as f32 / fade_out as f32).cos()
            } else {
                1.0
            };
            (2.0 * std::f64::consts::PI * F1 as f64 * l as f64 * ((tt / l as f64).exp() - 1.0)).sin() as f32 * g
        })
        .collect();
    let latency = fx.latency();
    let out = play(fx, &sweep, a, rate, 0);
    let hm = harmonics(&out, &sweep, a, rate, l, f2, latency);
    // Time-varying effects (LFOs, pitch tracking) are not Hammerstein systems: play it again with the
    // effect's clock shifted and compare the linear responses.
    let again = play(fx, &sweep, a, rate, (0.371 * rate) as usize);
    let hm2 = harmonics(&again, &sweep, a, rate, l, f2, latency);
    let (nfft, harm) = (hm.nfft, hm.h);
    let bins = nfft / 2;
    let bin_of = |f: f32| ((f / rate * nfft as f32).round() as usize).min(bins - 1);
    let mut wobble = 0.0f32;
    for f in [100.0f32, 300.0, 1000.0, 3000.0, 8000.0] {
        let (p, q) = (harm[0][bin_of(f)].norm().max(1e-9), hm2.h[0][bin_of(f)].norm().max(1e-9));
        wobble = wobble.max(20.0 * (p / q).log10().abs());
    }
    // Kernels per bin: back-substitution from the highest order.
    let mut g = vec![vec![Complex::new(0.0f32, 0.0); bins]; MAX_ORDER];
    // Only orders the sweep excited at a bin (input f / m inside the sweep) take part there.
    let excited = |m: usize, b: usize| (b as f32 * rate / nfft as f32) / m as f32 >= F1 * 1.3;
    for b in 0..bins {
        for k in (1..=MAX_ORDER).rev() {
            if !excited(k, b) {
                continue;
            }
            let mut acc = harm[k - 1][b];
            for m in k + 1..=MAX_ORDER {
                if excited(m, b) {
                    acc -= weight(m, k, a) * g[m - 1][b];
                }
            }
            g[k - 1][b] = acc / weight(k, k, a);
        }
    }
    // Onto a log grid, 1/12-octave smoothing. An order's value at f comes from input at f / k, so its
    // curve starts where that input is audible (G2 from 40 Hz, G3 from 60 Hz …): below, it would only
    // describe sub-sonic input no music has.
    let points = 200;
    let freqs: Vec<f32> = (0..points).map(|p| SHOW_FROM * (20_000.0f32.min(f2) / SHOW_FROM).powf(p as f32 / (points - 1) as f32)).collect();
    let smooth = |spec: &dyn Fn(usize) -> f32, f: f32| -> f32 {
        let (i0, i1) = (bin_of(f * 2f32.powf(-1.0 / 24.0)).max(1), bin_of(f * 2f32.powf(1.0 / 24.0)));
        let (mut s, mut c) = (0.0, 0);
        for i in i0..=i1.max(i0) {
            s += spec(i);
            c += 1;
        }
        s / c.max(1) as f32
    };
    let kernels: Vec<Vec<f32>> =
        (0..MAX_ORDER).map(|k| freqs.iter().map(|f| if *f / (k + 1) as f32 >= SHOW_FROM * 0.99 { 10.0 * smooth(&|i| g[k][i].norm_sqr(), *f).max(1e-14).log10() } else { f32::NAN }).collect()).collect();
    // THD of a sine at f: the harmonics land at k·f in the harmonic responses.
    let thd: Vec<f32> = freqs
        .iter()
        .map(|f| {
            let fund = smooth(&|i| harm[0][i].norm_sqr(), *f).max(1e-20);
            let mut sum = 0.0;
            for k in 2..=MAX_ORDER {
                let fk = f * k as f32;
                if fk < f2 * 0.95 {
                    sum += smooth(&|i| harm[k - 1][i].norm_sqr(), fk);
                }
            }
            100.0 * (sum / fund).sqrt()
        })
        .collect();
    // The static curve at 1 kHz: the kernels' real parts (signed) as polynomial coefficients.
    let b1k = bin_of(1000.0);
    let coef: Vec<f32> = (0..MAX_ORDER).map(|k| g[k][b1k].re).collect();
    let curve = (0..=64)
        .map(|s| {
            let xin = -1.0 + 2.0 * s as f32 / 64.0;
            let v = xin * a;
            let yo: f32 = coef.iter().enumerate().map(|(k, c)| c * v.powi(k as i32 + 1)).sum();
            (xin, yo / a.max(1e-6))
        })
        .collect();
    Kernels { level_db, freqs, kernels, thd, curve, time_varying: wobble > 0.5, wobble_db: wobble }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// y = x + 0.5x² + 0.25x³, memoryless.
    struct Poly;
    impl Effect for Poly {
        fn set_param(&mut self, _: &str, _: f32) {}
        fn reset(&mut self) {}
        fn process(&mut self, l: &mut [f32], r: &mut [f32], _: Option<(&[f32], &[f32])>) {
            for v in l.iter_mut().chain(r.iter_mut()) {
                let x = *v;
                *v = x + 0.5 * x * x + 0.25 * x * x * x;
            }
        }
    }

    fn at(k: &Kernels, order: usize, f: f32) -> f32 {
        let i = k.freqs.iter().enumerate().min_by(|a, b| (a.1 - f).abs().total_cmp(&(b.1 - f).abs())).unwrap().0;
        k.kernels[order - 1][i]
    }

    #[test]
    fn a_known_polynomial_comes_back_as_its_kernels() {
        let k = measure(&mut Poly, 48_000.0, -6.0);
        for f in [200.0, 1000.0, 5000.0] {
            assert!((at(&k, 1, f) - 0.0).abs() < 0.5, "G1 at {f}: {}", at(&k, 1, f));
            assert!((at(&k, 2, f) - 20.0 * 0.5f32.log10()).abs() < 1.0, "G2 at {f}: {}", at(&k, 2, f));
            assert!((at(&k, 3, f) - 20.0 * 0.25f32.log10()).abs() < 1.0, "G3 at {f}: {}", at(&k, 3, f));
            assert!(at(&k, 4, f) < -40.0 && at(&k, 5, f) < -40.0, "no 4th / 5th order at {f}");
        }
        // Down to the bottom of the shown range: no sag, no phantom harmonics.
        assert!(at(&k, 1, 25.0).abs() < 0.3, "G1 at 25 Hz: {}", at(&k, 1, 25.0));
        assert!((at(&k, 2, 40.0) - 20.0 * 0.5f32.log10()).abs() < 1.0, "G2 at 40 Hz: {}", at(&k, 2, 40.0));
        assert!(!k.time_varying);
        assert!(k.thd_at(1000.0) > 5.0);
    }

    #[test]
    fn a_clean_gain_has_no_distortion() {
        struct Gain;
        impl Effect for Gain {
            fn set_param(&mut self, _: &str, _: f32) {}
            fn reset(&mut self) {}
            fn process(&mut self, l: &mut [f32], r: &mut [f32], _: Option<(&[f32], &[f32])>) {
                for v in l.iter_mut().chain(r.iter_mut()) {
                    *v *= 0.5;
                }
            }
        }
        let k = measure(&mut Gain, 48_000.0, -6.0);
        for f in [25.0, 40.0, 100.0, 1000.0, 10000.0] {
            assert!((at(&k, 1, f) + 6.02).abs() < 0.2, "G1 at {f}: {}", at(&k, 1, f));
            for o in 2..=MAX_ORDER {
                let v = at(&k, o, f);
                assert!(v.is_nan() || v < -70.0, "G{o} at {f}: {v}");
            }
        }
        assert!(k.thd_at(1000.0) < 0.1);
    }
}

#[cfg(test)]
mod survey {
    use super::*;

    /// `cargo test --release -p dragon-engine hammerstein::survey -- --ignored --nocapture`: every stock
    /// effect's kernels at its default settings.
    #[test]
    #[ignore]
    fn print_every_effect() {
        let fs = [25.0f32, 40.0, 60.0, 100.0, 200.0, 1000.0];
        let at = |k: &Kernels, o: usize, f: f32| {
            let i = k.freqs.iter().enumerate().min_by(|a, b| (a.1 - f).abs().total_cmp(&(b.1 - f).abs())).unwrap().0;
            k.kernels[o][i]
        };
        let only = std::env::var("ONLY").ok();
        for def in crate::effects::registry() {
            if only.as_deref().is_some_and(|o| o != def.id) {
                continue;
            }
            let Some(mut fx) = crate::effects::create(def.id, 48_000.0, &def.defaults()) else { continue };
            fx.set_tempo(120.0);
            let k = measure(fx.as_mut(), 48_000.0, -6.0);
            let tv = if k.time_varying { format!(" time-varying ({:.1} dB)", k.wobble_db) } else { String::new() };
            let row: Vec<String> = (0..MAX_ORDER).map(|o| fs.iter().map(|f| format!("{:6.1}", at(&k, o, *f))).collect::<Vec<_>>().join(" ")).collect();
            println!("{:16} THD1k {:7.3}%  G1 [{}]  G2 [{}]  G3 [{}]{tv}", def.id, k.thd_at(1000.0), row[0], row[1], row[2]);
        }
    }
}
