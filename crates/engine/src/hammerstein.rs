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

/// Measures `fx` (a fresh instance, already set up) at `rate`, driven with a sweep peaking at `level_db`.
pub fn measure(fx: &mut dyn Effect, rate: f32, level_db: f32) -> Kernels {
    let a = 10f32.powf(level_db / 20.0);
    let f1 = 20.0f32;
    let f2 = (rate * 0.45).min(20_000.0);
    // Synchronised sweep (Novak): L is a multiple of 1/f1 so every harmonic is the sweep shifted in time.
    let target = 2.7f32;
    let l = ((f1 * target / (f2 / f1).ln()).round() / f1).max(1.0 / f1);
    let t = l * (f2 / f1).ln();
    let n = (t * rate) as usize;
    let fade_in = (0.005 * rate) as usize;
    let fade_out = (0.02 * rate) as usize;
    let sweep: Vec<f32> = (0..n)
        .map(|i| {
            let tt = i as f32 / rate;
            let g = if i < fade_in {
                0.5 - 0.5 * (PI * i as f32 / fade_in as f32).cos()
            } else if i + fade_out > n {
                0.5 - 0.5 * (PI * (n - i) as f32 / fade_out as f32).cos()
            } else {
                1.0
            };
            (2.0 * PI as f64 * f1 as f64 * l as f64 * ((tt as f64 / l as f64).exp() - 1.0)).sin() as f32 * g
        })
        .collect();
    let latency = fx.latency();
    let tail = (0.6 * rate) as usize + latency;
    let size = (2 * (n + tail)).next_power_of_two();
    // Play the sweep through the effect (mono in, the mid of its output).
    fx.reset();
    let mut out = vec![0.0f32; size];
    let block = 512;
    let mut lb = vec![0.0f32; block];
    let mut rb = vec![0.0f32; block];
    let mut pos = 0;
    while pos < n + tail {
        let len = block.min(n + tail - pos);
        for k in 0..len {
            let x = sweep.get(pos + k).copied().unwrap_or(0.0) * a;
            lb[k] = x;
            rb[k] = x;
        }
        fx.process(&mut lb[..len], &mut rb[..len], None);
        for k in 0..len {
            let y = 0.5 * (lb[k] + rb[k]);
            out[pos + k] = if y.is_finite() { y } else { 0.0 };
        }
        pos += len;
    }
    // Deconvolve: Y · conj(X) / (|X|² + ε) with X the driven sweep, band limited to the sweep; the linear
    // response comes out as G1 and the weights below carry the powers of the drive `a`.
    let mut planner = FftPlanner::<f32>::new();
    let fwd = planner.plan_fft_forward(size);
    let inv = planner.plan_fft_inverse(size);
    let mut x: Vec<Complex<f32>> = (0..size).map(|i| Complex::new(sweep.get(i).copied().unwrap_or(0.0) * a, 0.0)).collect();
    let mut y: Vec<Complex<f32>> = out.iter().map(|v| Complex::new(*v, 0.0)).collect();
    fwd.process(&mut x);
    fwd.process(&mut y);
    let peak = x.iter().take(size / 2).map(|c| c.norm_sqr()).fold(0.0f32, f32::max);
    let eps = peak * 1e-6;
    for k in 0..size {
        let f = (k.min(size - k)) as f32 * rate / size as f32;
        let band = if f < f1 * 0.9 || f > f2 { 0.0 } else { 1.0 };
        let d = x[k].norm_sqr() + eps;
        y[k] = y[k] * x[k].conj() / d * band;
    }
    inv.process(&mut y);
    let h: Vec<f32> = y.iter().map(|c| c.re / size as f32).collect();
    // Cut out each harmonic's impulse response: harmonic k arrives L·ln(k) before the linear one.
    let w = 4096usize.min(((l * (MAX_ORDER as f32 / (MAX_ORDER - 1) as f32).ln() * rate) as usize).next_power_of_two() / 2).max(256);
    let pre = w / 8;
    let fft_w = planner.plan_fft_forward(w);
    let mut harm: Vec<Vec<Complex<f32>>> = Vec::with_capacity(MAX_ORDER);
    for k in 1..=MAX_ORDER {
        let at = latency as f32 - l * (k as f32).ln() * rate;
        let start = at.floor();
        let frac = at - start;
        let mut buf = vec![Complex::new(0.0, 0.0); w];
        for j in 0..w {
            // Index relative to the arrival, from −pre to w − pre; stored circularly (arrival at 0).
            let rel = j as i64 - pre as i64;
            let src = (start as i64 + rel).rem_euclid(size as i64) as usize;
            let win = if j < pre {
                0.5 - 0.5 * (PI * j as f32 / pre as f32).cos()
            } else {
                let q = (j - pre) as f32 / (w - pre) as f32;
                if q > 0.75 {
                    0.5 + 0.5 * (PI * (q - 0.75) / 0.25).cos()
                } else {
                    1.0
                }
            };
            buf[rel.rem_euclid(w as i64) as usize] = Complex::new(h[src] * win, 0.0);
        }
        fft_w.process(&mut buf);
        // The fractional part of the arrival time.
        for (b, c) in buf.iter_mut().enumerate() {
            let f = b as f32 / w as f32;
            *c *= Complex::from_polar(1.0, 2.0 * PI * f * frac);
        }
        harm.push(buf);
    }
    // Kernels per bin: back-substitution from the highest order.
    let bins = w / 2;
    let mut g = vec![vec![Complex::new(0.0f32, 0.0); bins]; MAX_ORDER];
    for b in 0..bins {
        for k in (1..=MAX_ORDER).rev() {
            let mut acc = harm[k - 1][b];
            for m in k + 1..=MAX_ORDER {
                acc -= weight(m, k, a) * g[m - 1][b];
            }
            g[k - 1][b] = acc / weight(k, k, a);
        }
    }
    // Resample onto a log grid with 1/6-octave smoothing.
    let points = 160;
    let lo = f1.max(rate / w as f32 * 2.0);
    let freqs: Vec<f32> = (0..points).map(|p| lo * (f2 / lo).powf(p as f32 / (points - 1) as f32)).collect();
    let smooth = |spec: &dyn Fn(usize) -> f32, f: f32| -> f32 {
        let (b0, b1) = (f * 2f32.powf(-1.0 / 12.0), f * 2f32.powf(1.0 / 12.0));
        let (i0, i1) = (((b0 / rate * w as f32) as usize).max(1), ((b1 / rate * w as f32).ceil() as usize).min(bins - 1));
        let (mut s, mut c) = (0.0, 0);
        for i in i0..=i1.max(i0) {
            s += spec(i);
            c += 1;
        }
        s / c.max(1) as f32
    };
    let kernels: Vec<Vec<f32>> = (0..MAX_ORDER).map(|k| freqs.iter().map(|f| 10.0 * smooth(&|i| g[k][i].norm_sqr(), *f).max(1e-14).log10()).collect()).collect();
    // THD of a sine at f: the harmonics land at k·f in the harmonic responses.
    let thd: Vec<f32> = freqs
        .iter()
        .map(|f| {
            let fund = smooth(&|i| harm[0][i].norm_sqr(), *f).max(1e-20);
            let mut sum = 0.0;
            for k in 2..=MAX_ORDER {
                let fk = f * k as f32;
                if fk < f2 {
                    sum += smooth(&|i| harm[k - 1][i].norm_sqr(), fk);
                }
            }
            100.0 * (sum / fund).sqrt()
        })
        .collect();
    // The static curve at 1 kHz: the kernels' real parts (signed) as polynomial coefficients.
    let b1k = ((1000.0 / rate * w as f32).round() as usize).min(bins - 1);
    let coef: Vec<f32> = (0..MAX_ORDER).map(|k| g[k][b1k].re).collect();
    let curve = (0..=64)
        .map(|s| {
            let xin = -1.0 + 2.0 * s as f32 / 64.0;
            let v = xin * a;
            let yo: f32 = coef.iter().enumerate().map(|(k, c)| c * v.powi(k as i32 + 1)).sum();
            (xin, yo / a.max(1e-6))
        })
        .collect();
    Kernels { level_db, freqs, kernels, thd, curve }
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
        assert!((at(&k, 1, 1000.0) + 6.02).abs() < 0.5);
        assert!(at(&k, 2, 1000.0) < -60.0 && at(&k, 3, 1000.0) < -60.0);
        assert!(k.thd_at(1000.0) < 0.1);
    }
}
