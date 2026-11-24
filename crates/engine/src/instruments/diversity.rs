//! How different the presets of a library sound: a few features of a rendered note (brightness at the
//! start and later, how fast it speaks, how long it rings, how wide it is) and the average distance between
//! presets in that space. The libraries' tests keep it above a floor, so presets of one category do not
//! collapse into variations of one sound.

use rustfft::{num_complex::Complex, FftPlanner};

pub const N_FEATURES: usize = 5;

fn centroid(x: &[f32], rate: f32) -> f32 {
    let n = 4096.min(x.len().next_power_of_two() / 2).max(512);
    if x.len() < n {
        return 0.0;
    }
    let mut buf: Vec<Complex<f32>> = (0..n).map(|k| Complex::new(x[k] * (0.5 - 0.5 * (2.0 * std::f32::consts::PI * k as f32 / n as f32).cos()), 0.0)).collect();
    FftPlanner::new().plan_fft_forward(n).process(&mut buf);
    let (mut num, mut den) = (0.0f32, 0.0f32);
    for (k, c) in buf.iter().enumerate().take(n / 2).skip(1) {
        let m = c.norm();
        num += m * k as f32 * rate / n as f32;
        den += m;
    }
    if den <= 0.0 {
        0.0
    } else {
        num / den
    }
}

/// Features of a rendered note: [early brightness (octaves), late brightness, ring time (log2 s), attack
/// (log2 ms), width (side / mid)].
pub fn features(l: &[f32], r: &[f32], rate: f32) -> [f32; N_FEATURES] {
    let mid: Vec<f32> = l.iter().zip(r).map(|(a, b)| 0.5 * (a + b)).collect();
    let side: f32 = l.iter().zip(r).map(|(a, b)| (0.5 * (a - b)).powi(2)).sum();
    let mid_e: f32 = mid.iter().map(|v| v * v).sum::<f32>().max(1e-12);
    // Envelope in 10 ms frames.
    let hop = (rate * 0.01) as usize;
    let env: Vec<f32> = mid.chunks(hop).map(|c| (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt()).collect();
    let (pk_i, pk) = env.iter().enumerate().fold((0, 0.0f32), |b, (i, v)| if *v > b.1 { (i, *v) } else { b });
    let attack_ms = (pk_i as f32 * 10.0).max(1.0);
    let ring = env[pk_i..].iter().position(|v| *v < pk * 0.1).map(|k| k as f32 * 0.01).unwrap_or(((env.len() - pk_i) as f32 * 0.01) * 1.5).max(0.01);
    let at = |t: f32| (pk_i * hop + (t * rate) as usize).min(mid.len().saturating_sub(4096));
    let early = centroid(&mid[at(0.0)..], rate).max(20.0);
    let late = centroid(&mid[at((ring * 0.5).min(1.5))..], rate).max(20.0);
    [early.log2(), late.log2(), ring.log2(), attack_ms.log2() * 0.5, (side / mid_e).sqrt().min(2.0) * 3.0]
}

/// The mean distance between every pair of presets (feature units: octaves, doublings).
pub fn diversity(f: &[[f32; N_FEATURES]]) -> f32 {
    let mut sum = 0.0;
    let mut n = 0;
    for i in 0..f.len() {
        for j in i + 1..f.len() {
            sum += f[i].iter().zip(&f[j]).map(|(a, b)| (a - b).powi(2)).sum::<f32>().sqrt();
            n += 1;
        }
    }
    if n == 0 {
        0.0
    } else {
        sum / n as f32
    }
}

/// The smallest distance from each preset to its nearest neighbour, averaged: low means near-duplicates.
pub fn nearest(f: &[[f32; N_FEATURES]]) -> f32 {
    let mut sum = 0.0;
    for i in 0..f.len() {
        let d = (0..f.len()).filter(|&j| j != i).map(|j| f[i].iter().zip(&f[j]).map(|(a, b)| (a - b).powi(2)).sum::<f32>().sqrt()).fold(f32::MAX, f32::min);
        sum += d;
    }
    sum / f.len().max(1) as f32
}
