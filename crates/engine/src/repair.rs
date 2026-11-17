//! Audio repair, offline, on a whole clip (or a part of it): the iZotope RX family – spectral de-noise
//! with a learned noise profile, de-hum, de-click, de-clip, distortion repair (peaks flattened by soft
//! clipping), de-reverb, spectral attenuation of a time × frequency selection, gain – and Melodyne-style
//! note editing: notes ("blobs") detected with their pitch curves, shifted per note and their drift /
//! vibrato flattened.
//!
//! Every function takes audio and returns new audio of the same length and rate, so a repair can be
//! rendered to a file and undone by pointing the clip back at the original.

use crate::dsp::{Biquad, BiquadKind};
use crate::effects::pitch::{detect_pitch, hz_to_midi, PitchShifter};
use crate::samples::AudioData;
use rustfft::{num_complex::Complex, FftPlanner};
use std::f32::consts::PI;

// ------------------------------------------------------------------------------------------------ STFT

const N: usize = 2048;
const HOP: usize = 512;

fn hann(n: usize) -> Vec<f32> {
    (0..n).map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / n as f32).cos()).collect()
}

/// Short-time spectra of `x` (frames of `N`, hop `HOP`, Hann window), `N / 2 + 1` bins each.
fn stft(x: &[f32]) -> Vec<Vec<Complex<f32>>> {
    let w = hann(N);
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(N);
    let frames = x.len().div_ceil(HOP) + 1;
    let mut out = Vec::with_capacity(frames);
    let mut buf = vec![Complex::new(0.0, 0.0); N];
    for f in 0..frames {
        let start = f as isize * HOP as isize - (N / 2) as isize;
        for (i, b) in buf.iter_mut().enumerate() {
            let k = start + i as isize;
            let v = if k >= 0 && (k as usize) < x.len() { x[k as usize] } else { 0.0 };
            *b = Complex::new(v * w[i], 0.0);
        }
        fft.process(&mut buf);
        out.push(buf[..N / 2 + 1].to_vec());
    }
    out
}

/// Back to `len` samples (weighted overlap-add, Hann synthesis window, normalised).
fn istft(spec: &[Vec<Complex<f32>>], len: usize) -> Vec<f32> {
    let w = hann(N);
    let mut planner = FftPlanner::<f32>::new();
    let ifft = planner.plan_fft_inverse(N);
    let mut out = vec![0.0f32; len + N];
    let mut norm = vec![0.0f32; len + N];
    let mut buf = vec![Complex::new(0.0, 0.0); N];
    for (f, frame) in spec.iter().enumerate() {
        for k in 0..=N / 2 {
            buf[k] = frame[k];
            if k > 0 && k < N / 2 {
                buf[N - k] = frame[k].conj();
            }
        }
        ifft.process(&mut buf);
        let start = f as isize * HOP as isize - (N / 2) as isize;
        for i in 0..N {
            let k = start + i as isize;
            if k >= 0 && (k as usize) < len {
                out[k as usize] += buf[i].re / N as f32 * w[i];
                norm[k as usize] += w[i] * w[i];
            }
        }
    }
    out.truncate(len);
    for (o, n) in out.iter_mut().zip(norm) {
        if n > 1e-6 {
            *o /= n;
        }
    }
    out
}

fn bin_hz(k: usize, rate: f32) -> f32 {
    k as f32 * rate / N as f32
}

/// Applies `f` to each channel.
fn per_channel(data: &AudioData, mut f: impl FnMut(&[f32]) -> Vec<f32>) -> AudioData {
    let l = f(&data.l);
    let r = f(&data.r);
    AudioData { rate: data.rate, l, r, source_channels: data.source_channels, source_rate: data.source_rate }
}

fn mono(data: &AudioData) -> Vec<f32> {
    data.l.iter().zip(&data.r).map(|(a, b)| 0.5 * (a + b)).collect()
}

fn secs_to_frames(data: &AudioData, t: f64) -> usize {
    ((t.max(0.0) * data.rate as f64) as usize).min(data.frames())
}

// ------------------------------------------------------------------------------------------------ de-noise

/// The noise's average magnitude per bin.
#[derive(Debug, Clone, PartialEq)]
pub struct NoiseProfile(pub Vec<f32>);

/// Learns the noise from `range` (seconds; a stretch of noise alone), or, without one, from the
/// quietest tenth of the clip.
pub fn learn_noise(data: &AudioData, range: Option<(f64, f64)>) -> NoiseProfile {
    let x = mono(data);
    let spec = stft(&x);
    let frames: Vec<&Vec<Complex<f32>>> = match range {
        Some((a, b)) => {
            let (fa, fb) = ((secs_to_frames(data, a) / HOP).min(spec.len()), (secs_to_frames(data, b) / HOP + 1).min(spec.len()));
            spec[fa..fb.max(fa + 1).min(spec.len())].iter().collect()
        }
        None => {
            let mut e: Vec<(usize, f32)> = spec.iter().enumerate().map(|(i, f)| (i, f.iter().map(|c| c.norm_sqr()).sum())).collect();
            e.sort_by(|a, b| a.1.total_cmp(&b.1));
            let n = (e.len() / 10).max(1);
            e[..n].iter().map(|(i, _)| &spec[*i]).collect()
        }
    };
    let mut prof = vec![0.0f32; N / 2 + 1];
    for f in &frames {
        for (p, c) in prof.iter_mut().zip(f.iter()) {
            *p += c.norm();
        }
    }
    let n = frames.len().max(1) as f32;
    prof.iter_mut().for_each(|p| *p /= n);
    NoiseProfile(prof)
}

/// Spectral de-noise: each bin is turned down by how much of it the noise explains (a Wiener-style
/// gain, smoothed over time and frequency against "musical noise"), by at most `reduction_db`.
/// `sensitivity` > 1 removes more (treats the noise as louder than learned).
pub fn denoise(data: &AudioData, profile: &NoiseProfile, reduction_db: f32, sensitivity: f32) -> AudioData {
    let floor = 10f32.powf(-reduction_db.abs() / 20.0);
    per_channel(data, |x| {
        let mut spec = stft(x);
        let bins = N / 2 + 1;
        // Decision-directed Wiener filter (Ephraim–Malah): the a-priori SNR leans on the previous frame's
        // clean estimate, which keeps the gains steady (no "musical noise").
        // Noise power per bin: a Rayleigh magnitude's mean m has power m² · 4 / π.
        let noise: Vec<f32> = profile.0.iter().map(|m| (m * sensitivity).powi(2) * 4.0 / PI + 1e-20).collect();
        let mut prev_clean = vec![0.0f32; bins];
        let alpha = 0.97f32;
        for frame in spec.iter_mut() {
            let mut g: Vec<f32> = (0..bins)
                .map(|k| {
                    let post = frame[k].norm_sqr() / noise[k];
                    let prior = alpha * prev_clean[k] / noise[k] + (1.0 - alpha) * (post - 1.0).max(0.0);
                    (prior / (1.0 + prior)).max(floor)
                })
                .collect();
            // A little smoothing across neighbouring bins.
            let g2: Vec<f32> = (0..bins).map(|k| (g[k.saturating_sub(1)] + 2.0 * g[k] + g[(k + 1).min(bins - 1)]) / 4.0).collect();
            for k in 0..bins {
                g[k] = g2[k].max(floor);
                frame[k] *= g[k];
                prev_clean[k] = frame[k].norm_sqr();
            }
        }
        istft(&spec, x.len())
    })
}

// ------------------------------------------------------------------------------------------------ de-hum

/// The mains frequency in `data`: 50 or 60 Hz, whichever (with its harmonics) is stronger.
pub fn detect_hum(data: &AudioData) -> f32 {
    let x = mono(data);
    let rate = data.rate as f32;
    let energy = |f: f32| -> f32 {
        let w = 2.0 * PI * f / rate;
        let c = 2.0 * w.cos();
        let (mut s1, mut s2) = (0.0f32, 0.0f32);
        for v in x.iter().take((rate * 10.0) as usize) {
            let s0 = v + c * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        s1 * s1 + s2 * s2 - c * s1 * s2
    };
    let e50: f32 = (1..=3).map(|k| energy(50.0 * k as f32)).sum();
    let e60: f32 = (1..=3).map(|k| energy(60.0 * k as f32)).sum();
    if e60 > e50 {
        60.0
    } else {
        50.0
    }
}

/// Removes mains hum: narrow notches at `base` (0 = detect 50 / 60 Hz) and its harmonics, run forwards
/// and backwards so nothing is phase-shifted.
pub fn dehum(data: &AudioData, base: f32, harmonics: usize, q: f32) -> AudioData {
    let base = if base > 0.0 { base } else { detect_hum(data) };
    let rate = data.rate as f32;
    let freqs: Vec<f32> = (1..=harmonics.max(1)).map(|k| base * k as f32).filter(|f| *f < rate * 0.45).collect();
    per_channel(data, |x| {
        let mut y = x.to_vec();
        for f in &freqs {
            for pass in 0..2 {
                let mut b = Biquad::default();
                b.set(BiquadKind::Notch, *f, q, 0.0, rate);
                if pass == 1 {
                    y.reverse();
                }
                for v in y.iter_mut() {
                    *v = b.process(*v);
                }
                if pass == 1 {
                    y.reverse();
                }
            }
        }
        y
    })
}

// ------------------------------------------------------------------------------------------------ de-click

/// Fills `y[a..b]` with a cubic that meets the samples around it in value and slope.
fn bridge(y: &mut [f32], a: usize, b: usize) {
    if a < 2 || b + 2 >= y.len() || b <= a {
        return;
    }
    let (p0, p1) = (y[a - 1], y[b]);
    let (m0, m1) = (y[a - 1] - y[a - 2], y[b + 1] - y[b]);
    let n = (b - a + 1) as f32;
    for i in a..b {
        let t = (i - a + 1) as f32 / n;
        let (t2, t3) = (t * t, t * t * t);
        y[i] = (2.0 * t3 - 3.0 * t2 + 1.0) * p0 + (t3 - 2.0 * t2 + t) * m0 * n + (-2.0 * t3 + 3.0 * t2) * p1 + (t3 - t2) * m1 * n;
    }
}

/// Linear-prediction coefficients (order `p`) of `x` (autocorrelation method, Levinson–Durbin).
fn lpc(x: &[f32], p: usize) -> Vec<f64> {
    let n = x.len();
    let w: Vec<f64> = (0..n).map(|i| x[i] as f64 * (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n.max(2) as f64).cos())).collect();
    let r: Vec<f64> = (0..=p).map(|k| (k..n).map(|i| w[i] * w[i - k]).sum::<f64>()).collect();
    let mut a = vec![0.0f64; p + 1];
    a[0] = 1.0;
    let mut e = r[0] * (1.0 + 1e-9) + 1e-12;
    for i in 1..=p {
        let acc: f64 = (1..i).map(|j| a[j] * r[i - j]).sum::<f64>() + r[i];
        let k = -acc / e;
        let prev = a.clone();
        for j in 1..i {
            a[j] = prev[j] + k * prev[i - j];
        }
        a[i] = k;
        e *= 1.0 - k * k;
        if e <= 1e-15 {
            break;
        }
    }
    a
}

/// Fills `y[a..b]` by linear prediction: forwards from the audio before the gap, backwards from the audio
/// after it, crossfaded – tonal content carries on through the gap (a cubic cannot follow it).
fn ar_fill(y: &mut [f32], a: usize, b: usize) {
    let p = 32usize;
    let ctx = 1024usize;
    if b <= a || a < p + 8 || b + p + 8 >= y.len() {
        bridge(y, a, b);
        return;
    }
    let left = &y[a.saturating_sub(ctx)..a];
    let right = &y[b..(b + ctx).min(y.len())];
    let mut both = left.to_vec();
    both.extend_from_slice(right);
    let c = lpc(&both, p);
    let len = b - a;
    // Forwards.
    let mut f: Vec<f64> = y[a - p..a].iter().map(|v| *v as f64).collect();
    for _ in 0..len {
        let k = f.len();
        let v: f64 = -(1..=p).map(|j| c[j] * f[k - j]).sum::<f64>();
        f.push(v);
    }
    // Backwards (the same predictor on the reversed signal).
    let mut g: Vec<f64> = y[b..b + p].iter().rev().map(|v| *v as f64).collect();
    for _ in 0..len {
        let k = g.len();
        let v: f64 = -(1..=p).map(|j| c[j] * g[k - j]).sum::<f64>();
        g.push(v);
    }
    for i in 0..len {
        let w = (i as f64 + 1.0) / (len as f64 + 1.0);
        let fw = f[p + i];
        let bw = g[p + (len - 1 - i)];
        y[a + i] = ((1.0 - w) * fw + w * bw) as f32;
    }
}

/// Finds clicks (samples whose second difference stands far out of the signal's usual range) and fills
/// them in from their surroundings. `sensitivity` 1 … 10 (higher catches fainter clicks). Returns the
/// repaired audio and how many clicks were fixed.
pub fn declick(data: &AudioData, sensitivity: f32) -> (AudioData, usize) {
    let mut count = 0;
    let rate = data.rate as f32;
    let max_len = (rate * 0.003) as usize;
    let pad = 3usize;
    let out = per_channel(data, |x| {
        let mut y = x.to_vec();
        if x.len() < 16 {
            return y;
        }
        let d2: Vec<f32> = (0..x.len()).map(|i| if i == 0 || i + 1 >= x.len() { 0.0 } else { (x[i + 1] - 2.0 * x[i] + x[i - 1]).abs() }).collect();
        // A local scale: the median of |d2| over 20 ms around (blocks).
        let block = (rate * 0.02) as usize;
        let mut scale = vec![0.0f32; x.len()];
        for (bi, chunk) in d2.chunks(block.max(1)).enumerate() {
            let mut v: Vec<f32> = chunk.to_vec();
            v.sort_by(f32::total_cmp);
            let m = v[v.len() / 2].max(1e-5);
            for s in scale.iter_mut().skip(bi * block).take(chunk.len()) {
                *s = m;
            }
        }
        let k = 60.0 / sensitivity.clamp(0.5, 20.0);
        let mut i = 2;
        while i + 2 < x.len() {
            if d2[i] > k * scale[i] {
                let a = i.saturating_sub(pad).max(2);
                let mut b = i;
                while b + 1 < x.len() && b - i < max_len && d2[b + 1] > k * scale[b + 1] * 0.3 {
                    b += 1;
                }
                let b = (b + pad).min(x.len() - 3);
                if b - a <= max_len + 2 * pad {
                    ar_fill(&mut y, a, b);
                    count += 1;
                }
                i = b + 1;
            } else {
                i += 1;
            }
        }
        y
    });
    (out, count)
}

// ------------------------------------------------------------------------------------------------ de-clip

/// Rebuilds clipped peaks: runs of samples stuck at the clip level (within `threshold` × peak) are
/// replaced by a cubic through the waveform's slope on either side; the result is turned down if the
/// restored peaks would clip again. Returns the audio and the number of repaired runs.
pub fn declip(data: &AudioData, threshold: f32) -> (AudioData, usize) {
    let peak = data.l.iter().chain(&data.r).fold(0.0f32, |m, v| m.max(v.abs())).max(1e-9);
    let level = peak * threshold.clamp(0.5, 1.0);
    let mut count = 0;
    let mut out = per_channel(data, |x| {
        let mut y = x.to_vec();
        let mut i = 2;
        while i + 3 < x.len() {
            if x[i].abs() >= level && x[i - 1].abs() >= level && x[i].signum() == x[i - 1].signum() {
                let a = i - 1;
                let mut b = i;
                while b + 3 < x.len() && x[b + 1].abs() >= level && x[b + 1].signum() == x[i].signum() {
                    b += 1;
                }
                // A cubic from the slope entering the run to the slope leaving it.
                bridge(&mut y, a, b + 1);
                count += 1;
                i = b + 2;
            } else {
                i += 1;
            }
        }
        y
    });
    let restored = out.l.iter().chain(&out.r).fold(0.0f32, |m, v| m.max(v.abs()));
    if restored > 0.99 {
        let g = 0.99 / restored;
        out.l.iter_mut().chain(out.r.iter_mut()).for_each(|v| *v *= g);
    }
    (out, count)
}

// ------------------------------------------------------------------------------------------------ distortion

/// The level above which the waveform looks squashed: soft clipping piles the peaks up just under the
/// ceiling, so the knee sits where the peaks stop spreading out.
fn soft_knee(x: &[f32]) -> f32 {
    let mut peaks: Vec<f32> = (1..x.len().saturating_sub(1)).filter(|&i| x[i].abs() >= x[i - 1].abs() && x[i].abs() > x[i + 1].abs()).map(|i| x[i].abs()).collect();
    if peaks.is_empty() {
        return 1.0;
    }
    peaks.sort_by(f32::total_cmp);
    let top = peaks[peaks.len() * 98 / 100];
    top * 0.72
}

/// Distortion repair for soft clipping and overdriven peaks: above the knee (detected, or `knee` × peak
/// when > 0), each squashed peak is rebuilt from the slopes leading in and out of it, blended in by
/// `amount` (0 … 1); the result is turned down if needed. Returns the audio and the peaks repaired.
pub fn dedistort(data: &AudioData, amount: f32, knee: f32) -> (AudioData, usize) {
    let amount = amount.clamp(0.0, 1.0);
    let peak = data.l.iter().chain(&data.r).fold(0.0f32, |m, v| m.max(v.abs())).max(1e-9);
    let mut count = 0;
    let mut out = per_channel(data, |x| {
        let k = if knee > 0.0 { knee * peak } else { soft_knee(x) };
        let mut y = x.to_vec();
        let mut i = 2;
        while i + 3 < x.len() {
            if x[i].abs() > k && x[i - 1].abs() <= k {
                let a = i;
                let s = x[i].signum();
                let mut b = i;
                while b + 3 < x.len() && x[b + 1].abs() > k && x[b + 1].signum() == s {
                    b += 1;
                }
                if b > a + 1 {
                    let mut r = x.to_vec();
                    bridge(&mut r, a, b + 1);
                    for j in a..=b {
                        // Only ever louder than the squashed original (the peak is restored, not reshaped).
                        let restored = if r[j].abs() > x[j].abs() && r[j].signum() == s { r[j] } else { x[j] };
                        y[j] = x[j] + (restored - x[j]) * amount;
                    }
                    count += 1;
                }
                i = b + 1;
            } else {
                i += 1;
            }
        }
        y
    });
    let restored = out.l.iter().chain(&out.r).fold(0.0f32, |m, v| m.max(v.abs()));
    if restored > 0.99 {
        let g = 0.99 / restored;
        out.l.iter_mut().chain(out.r.iter_mut()).for_each(|v| *v *= g);
    }
    (out, count / 2)
}

// ------------------------------------------------------------------------------------------------ de-reverb

/// De-reverb (Lebart): the late reverb at each moment is estimated from the spectrum a little earlier,
/// decayed by the room's reverb time `t60` (seconds), and subtracted, by `amount` (0 … 1).
pub fn dereverb(data: &AudioData, amount: f32, t60: f32) -> AudioData {
    let rate = data.rate as f32;
    let delay_frames = ((0.05 * rate) / HOP as f32).round().max(1.0) as usize;
    let td = delay_frames as f32 * HOP as f32 / rate;
    let decay = (-2.0 * (1000f32).ln() * td / t60.max(0.1)).exp();
    let floor = 0.15f32;
    per_channel(data, |x| {
        let mut spec = stft(x);
        // Power, smoothed over three frames and five bins (the reverb's average, not one random frame).
        let raw: Vec<Vec<f32>> = spec.iter().map(|f| f.iter().map(|c| c.norm_sqr()).collect()).collect();
        let bins_n = N / 2 + 1;
        let mags: Vec<Vec<f32>> = (0..raw.len())
            .map(|t| {
                (0..bins_n)
                    .map(|k| {
                        let mut s = 0.0;
                        let mut c = 0.0;
                        for tt in t.saturating_sub(1)..(t + 2).min(raw.len()) {
                            for kk in k.saturating_sub(2)..(k + 3).min(bins_n) {
                                s += raw[tt][kk];
                                c += 1.0;
                            }
                        }
                        s / c
                    })
                    .collect()
            })
            .collect();
        let bins = N / 2 + 1;
        let mut prev = vec![1.0f32; bins];
        for t in 0..spec.len() {
            for k in 0..bins {
                let late = if t >= delay_frames { decay * mags[t - delay_frames][k] } else { 0.0 };
                let s = raw[t][k].max(1e-20);
                // Over-subtract a little: the estimate is an average, single frames swing around it.
                let g = (1.0 - 2.0 * amount * late / s).max(floor * floor).sqrt();
                let g = g + (prev[k] - g) * 0.4;
                prev[k] = g;
                spec[t][k] *= g;
            }
        }
        istft(&spec, x.len())
    })
}

// ------------------------------------------------------------------------------------------------ selection

/// Turns a time × frequency selection up or down by `gain_db` (spectral repair / attenuate), with soft
/// edges so nothing clicks or rings.
pub fn attenuate(data: &AudioData, t0: f64, t1: f64, f0: f32, f1: f32, gain_db: f32) -> AudioData {
    let rate = data.rate as f32;
    let g = 10f32.powf(gain_db / 20.0);
    let (fa, fb) = (secs_to_frames(data, t0.min(t1)) as f32 / HOP as f32, secs_to_frames(data, t0.max(t1)) as f32 / HOP as f32);
    let (lo, hi) = (f0.min(f1).max(1.0), f0.max(f1).max(2.0));
    per_channel(data, |x| {
        let mut spec = stft(x);
        for (t, frame) in spec.iter_mut().enumerate() {
            let tt = t as f32;
            // Soft in time: two frames each side.
            let wt = ((tt - fa + 2.0) / 2.0).clamp(0.0, 1.0) * ((fb - tt + 2.0) / 2.0).clamp(0.0, 1.0);
            if wt <= 0.0 {
                continue;
            }
            for (k, c) in frame.iter_mut().enumerate() {
                let f = bin_hz(k, rate).max(1.0);
                // Soft in frequency: a sixth of an octave each side.
                let o = 1.0 / 6.0;
                let wf = (((f / lo).log2() + o) / o).clamp(0.0, 1.0) * (((hi / f).log2() + o) / o).clamp(0.0, 1.0);
                let w = wt * wf;
                if w > 0.0 {
                    *c *= 1.0 + (g - 1.0) * w;
                }
            }
        }
        istft(&spec, x.len())
    })
}

/// Gain on a time range (seconds) with 5 ms ramps.
pub fn gain(data: &AudioData, t0: f64, t1: f64, gain_db: f32) -> AudioData {
    let g = 10f32.powf(gain_db / 20.0);
    let (a, b) = (secs_to_frames(data, t0.min(t1)), secs_to_frames(data, t0.max(t1)));
    let ramp = (data.rate as f32 * 0.005) as usize;
    per_channel(data, |x| {
        let mut y = x.to_vec();
        for (i, v) in y.iter_mut().enumerate() {
            let w = if i + ramp < a || i > b + ramp {
                0.0
            } else if i < a {
                1.0 - (a - i) as f32 / ramp.max(1) as f32
            } else if i > b {
                1.0 - (i - b) as f32 / ramp.max(1) as f32
            } else {
                1.0
            };
            *v *= 1.0 + (g - 1.0) * w;
        }
        y
    })
}

// ------------------------------------------------------------------------------------------------ notes

/// A detected note (Melodyne's "blob"): where it is, its centre pitch and how the pitch moves inside it.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteBlob {
    pub start: f64,
    pub end: f64,
    /// Centre pitch (MIDI, fractional).
    pub midi: f32,
    /// (seconds, MIDI) through the note.
    pub curve: Vec<(f64, f32)>,
    /// Peak level (linear) – the blob's height.
    pub level: f32,
}

/// What to do with a note: shift it and flatten its drift / vibrato.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PitchEdit {
    /// Index into the detected notes.
    pub note: usize,
    /// Semitones (fractional allowed).
    pub shift: f32,
    /// 0 = keep the pitch movement, 1 = flatten it onto the note's centre.
    pub drift: f32,
}

const PITCH_WIN: usize = 2048;
const PITCH_HOP: usize = 256;

/// The pitch track: (seconds, MIDI) for voiced frames.
fn pitch_track(data: &AudioData) -> Vec<(f64, Option<f32>, f32)> {
    let x = mono(data);
    let rate = data.rate as f32;
    let mut out = Vec::new();
    let mut i = 0;
    while i + PITCH_WIN <= x.len() {
        let w = &x[i..i + PITCH_WIN];
        let lvl = w.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let p = if lvl > 0.01 { detect_pitch(w, rate, 55.0, 1200.0).filter(|(_, c)| *c > 0.85).map(|(hz, _)| hz_to_midi(hz)) } else { None };
        out.push(((i + PITCH_WIN / 2) as f64 / rate as f64, p, lvl));
        i += PITCH_HOP;
    }
    out
}

/// Finds the notes of a monophonic recording (voice, bass, a solo instrument).
pub fn detect_notes(data: &AudioData) -> Vec<NoteBlob> {
    let track = pitch_track(data);
    let hop = PITCH_HOP as f64 / data.rate as f64;
    let mut notes = Vec::new();
    let mut cur: Vec<(f64, f32, f32)> = Vec::new();
    let flush = |cur: &mut Vec<(f64, f32, f32)>, notes: &mut Vec<NoteBlob>| {
        if cur.len() as f64 * hop >= 0.08 {
            let mut v: Vec<f32> = cur.iter().map(|c| c.1).collect();
            v.sort_by(f32::total_cmp);
            let midi = v[v.len() / 2];
            notes.push(NoteBlob { start: cur[0].0 - hop / 2.0, end: cur[cur.len() - 1].0 + hop / 2.0, midi, curve: cur.iter().map(|c| (c.0, c.1)).collect(), level: cur.iter().fold(0.0f32, |m, c| m.max(c.2)) });
        }
        cur.clear();
    };
    for (t, p, lvl) in track {
        match p {
            Some(m) => {
                // A jump of more than ~0.8 semitone from the note so far starts a new note.
                let centre = if cur.is_empty() {
                    m
                } else {
                    let mut v: Vec<f32> = cur.iter().rev().take(8).map(|c| c.1).collect();
                    v.sort_by(f32::total_cmp);
                    v[v.len() / 2]
                };
                if !cur.is_empty() && (m - centre).abs() > 0.8 {
                    flush(&mut cur, &mut notes);
                }
                cur.push((t, m, lvl));
            }
            None => flush(&mut cur, &mut notes),
        }
    }
    flush(&mut cur, &mut notes);
    notes
}

/// The MIDI pitch of `note` at time `t` (from its curve).
fn curve_at(note: &NoteBlob, t: f64) -> f32 {
    match note.curve.iter().position(|(ct, _)| *ct >= t) {
        Some(0) => note.curve[0].1,
        Some(i) => {
            let (a, b) = (note.curve[i - 1], note.curve[i]);
            let f = ((t - a.0) / (b.0 - a.0).max(1e-9)) as f32;
            a.1 + (b.1 - a.1) * f
        }
        None => note.curve.last().map(|c| c.1).unwrap_or(note.midi),
    }
}

/// Renders the pitch edits: each edited note is re-pitched (granular, pitch-synchronous shifting) with a
/// smooth 15 ms move in and out; everything else stays untouched.
pub fn apply_pitch(data: &AudioData, notes: &[NoteBlob], edits: &[PitchEdit]) -> AudioData {
    let rate = data.rate as f32;
    let n = data.frames();
    // Semitones to move each sample by, and how much of the shifted signal to use.
    let mut offset = vec![0.0f32; n];
    let mut wet = vec![0.0f32; n];
    let ramp = (0.015 * rate) as usize;
    for e in edits {
        let Some(note) = notes.get(e.note) else { continue };
        if e.shift.abs() < 1e-3 && e.drift.abs() < 1e-3 {
            continue;
        }
        let (a, b) = (secs_to_frames(data, note.start), secs_to_frames(data, note.end));
        for i in a.saturating_sub(ramp)..(b + ramp).min(n) {
            let w = if i < a {
                1.0 - (a - i) as f32 / ramp as f32
            } else if i >= b {
                1.0 - (i - b) as f32 / ramp as f32
            } else {
                1.0
            };
            let t = i as f64 / rate as f64;
            let flatten = (note.midi - curve_at(note, t)) * e.drift;
            offset[i] = (e.shift + flatten) * w.max(0.0);
            wet[i] = wet[i].max(w.max(0.0));
        }
    }
    if wet.iter().all(|w| *w == 0.0) {
        return data.clone();
    }
    // Warm the shifter up on 0.2 s of silence, then line its output up with the dry signal.
    let pre = (0.2 * rate) as usize;
    per_channel(data, |x| {
        let mut sh = PitchShifter::new(rate);
        sh.set_period(None);
        for _ in 0..pre {
            sh.process(0.0, 1.0);
        }
        let delay = sh.delay().round() as usize + 1;
        let mut shifted = vec![0.0f32; n + delay];
        for (i, s) in shifted.iter_mut().enumerate() {
            let xi = if i < n { x[i] } else { 0.0 };
            let o = offset.get(i).copied().unwrap_or(0.0);
            *s = sh.process(xi, 2f32.powf(o / 12.0)) / 0.8;
        }
        (0..n).map(|i| x[i] + (shifted[i + delay] - x[i]) * wet[i]).collect()
    })
}

// ------------------------------------------------------------------------------------------------ files

/// Writes `data` as a 32-bit float stereo WAV.
pub fn write_wav(path: &std::path::Path, data: &AudioData) -> std::io::Result<()> {
    let spec = hound::WavSpec { channels: 2, sample_rate: data.rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = hound::WavWriter::create(path, spec).map_err(std::io::Error::other)?;
    for (l, r) in data.l.iter().zip(&data.r) {
        w.write_sample(*l).map_err(std::io::Error::other)?;
        w.write_sample(*r).map_err(std::io::Error::other)?;
    }
    w.finalize().map_err(std::io::Error::other)
}

/// Magnitudes (dB) for drawing a spectrogram: `cols` time columns × `rows` log-frequency rows
/// (20 Hz … Nyquist), from the mono mix.
pub fn spectrogram(data: &AudioData, cols: usize, rows: usize) -> Vec<Vec<f32>> {
    let x = mono(data);
    let rate = data.rate as f32;
    let spec = stft(&x);
    let cols = cols.max(1).min(spec.len().max(1));
    let rows = rows.max(1);
    let nyq = rate / 2.0;
    let row_bins: Vec<(usize, usize)> = (0..rows)
        .map(|r| {
            let f0 = 20.0 * (nyq / 20.0).powf(r as f32 / rows as f32);
            let f1 = 20.0 * (nyq / 20.0).powf((r + 1) as f32 / rows as f32);
            let (a, b) = ((f0 / rate * N as f32) as usize, ((f1 / rate * N as f32).ceil() as usize).max((f0 / rate * N as f32) as usize + 1));
            (a.min(N / 2), b.min(N / 2 + 1))
        })
        .collect();
    (0..cols)
        .map(|c| {
            let (fa, fb) = (c * spec.len() / cols, ((c + 1) * spec.len() / cols).max(c * spec.len() / cols + 1));
            row_bins
                .iter()
                .map(|(a, b)| {
                    let mut m = 0.0f32;
                    for frame in &spec[fa..fb.min(spec.len())] {
                        for v in &frame[*a..(*b).max(*a + 1).min(frame.len())] {
                            m = m.max(v.norm());
                        }
                    }
                    20.0 * (m / (N as f32 / 4.0)).max(1e-7).log10()
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn audio(f: impl Fn(f32) -> f32, secs: f32) -> AudioData {
        let n = (secs * RATE as f32) as usize;
        let l: Vec<f32> = (0..n).map(|i| f(i as f32 / RATE as f32)).collect();
        AudioData::from_planar(RATE, l.clone(), l)
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    fn tone_level(x: &[f32], hz: f32) -> f32 {
        let w = 2.0 * PI * hz / RATE as f32;
        let (mut re, mut im) = (0.0f32, 0.0f32);
        for (i, v) in x.iter().enumerate() {
            re += v * (w * i as f32).cos();
            im += v * (w * i as f32).sin();
        }
        2.0 * (re * re + im * im).sqrt() / x.len() as f32
    }

    fn noise(seed: u64) -> impl Fn(f32) -> f32 {
        move |t: f32| {
            let mut z = (t * RATE as f32) as u64 ^ seed;
            z = z.wrapping_mul(0x9E3779B97F4A7C15);
            z ^= z >> 29;
            z = z.wrapping_mul(0xBF58476D1CE4E5B9);
            ((z >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
        }
    }

    #[test]
    fn stft_round_trip_is_transparent() {
        let a = audio(|t| (2.0 * PI * 440.0 * t).sin() * 0.5 + 0.1 * (2.0 * PI * 3000.0 * t).sin(), 1.0);
        let y = istft(&stft(&a.l), a.frames());
        let err = a.l.iter().zip(&y).skip(N).take(a.frames() - 2 * N).fold(0.0f32, |m, (p, q)| m.max((p - q).abs()));
        assert!(err < 1e-4, "{err}");
    }

    #[test]
    fn denoise_removes_hiss_and_keeps_the_tone() {
        let n = noise(7);
        // One second of hiss alone, then the tone with the hiss.
        let a = audio(|t| 0.05 * n(t) + if t > 1.0 { 0.5 * (2.0 * PI * 440.0 * t).sin() } else { 0.0 }, 3.0);
        let prof = learn_noise(&a, Some((0.1, 0.9)));
        let y = denoise(&a, &prof, 30.0, 1.5);
        let hiss = |x: &[f32]| rms(&x[(0.2 * RATE as f32) as usize..(0.8 * RATE as f32) as usize]);
        assert!(hiss(&y.l) < hiss(&a.l) * 0.1, "hiss {} -> {}", hiss(&a.l), hiss(&y.l));
        let mid = &y.l[(1.5 * RATE as f32) as usize..(2.5 * RATE as f32) as usize];
        assert!((tone_level(mid, 440.0) - 0.5).abs() < 0.05, "tone {}", tone_level(mid, 440.0));
    }

    #[test]
    fn dehum_finds_and_removes_mains_hum() {
        let a = audio(|t| 0.3 * (2.0 * PI * 1000.0 * t).sin() + 0.2 * (2.0 * PI * 60.0 * t).sin() + 0.1 * (2.0 * PI * 180.0 * t).sin(), 2.0);
        assert_eq!(detect_hum(&a), 60.0);
        let y = dehum(&a, 0.0, 8, 30.0);
        let mid = &y.l[RATE as usize / 2..3 * RATE as usize / 2];
        assert!(tone_level(mid, 60.0) < 0.2 * 0.02, "60 Hz {}", tone_level(mid, 60.0));
        assert!(tone_level(mid, 180.0) < 0.1 * 0.05, "180 Hz {}", tone_level(mid, 180.0));
        assert!((tone_level(mid, 1000.0) - 0.3).abs() < 0.01);
    }

    #[test]
    fn declick_fixes_clicks_and_leaves_music_alone() {
        let clean = audio(|t| 0.4 * (2.0 * PI * 220.0 * t).sin() + 0.2 * (2.0 * PI * 1300.0 * t).sin(), 1.0);
        let mut dirty = clean.clone();
        for k in 0..12 {
            let i = 2000 + k * 3700;
            dirty.l[i] += 0.6;
            dirty.l[i + 1] -= 0.4;
            dirty.r[i] += 0.6;
            dirty.r[i + 1] -= 0.4;
        }
        let (y, n) = declick(&dirty, 5.0);
        assert!(n >= 12, "found {n}");
        let err = y.l.iter().zip(&clean.l).fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
        assert!(err < 0.05, "max error {err}");
        let (same, n0) = declick(&clean, 5.0);
        assert_eq!(n0, 0, "no false clicks on clean music");
        assert_eq!(same.l, clean.l);
    }

    #[test]
    fn declip_rebuilds_clipped_peaks() {
        let orig = audio(|t| 1.4 * (2.0 * PI * 200.0 * t).sin(), 0.5);
        let clipped = audio(|t| (1.4 * (2.0 * PI * 200.0 * t).sin()).clamp(-1.0, 1.0), 0.5);
        let (y, n) = declip(&clipped, 0.999);
        assert!(n > 100, "{n} runs");
        // Compare shapes (the result is turned down to fit): normalise both.
        let norm = |x: &[f32]| {
            let p = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            x.iter().map(|v| v / p).collect::<Vec<_>>()
        };
        let (o, c, r) = (norm(&orig.l), norm(&clipped.l), norm(&y.l));
        let err = |a: &[f32]| (a.iter().zip(&o).map(|(p, q)| (p - q).powi(2)).sum::<f32>() / o.len() as f32).sqrt();
        assert!(err(&r) < err(&c) * 0.3, "error {} -> {}", err(&c), err(&r));
        assert!(y.l.iter().all(|v| v.abs() <= 0.991));
    }

    #[test]
    fn distortion_repair_lowers_the_harmonics_of_soft_clipping() {
        let f0 = 150.0;
        let squashed = audio(|t| (2.5 * (2.0 * PI * f0 * t).sin()).tanh() * 0.9, 1.0);
        let thd = |x: &[f32]| {
            let h: f32 = (2..8).map(|k| tone_level(x, f0 * k as f32).powi(2)).sum::<f32>().sqrt();
            h / tone_level(x, f0)
        };
        let (y, n) = dedistort(&squashed, 1.0, 0.0);
        assert!(n > 50, "{n} peaks");
        assert!(thd(&y.l) < thd(&squashed.l) * 0.6, "THD {} -> {}", thd(&squashed.l), thd(&y.l));
    }

    #[test]
    fn dereverb_shortens_a_reverb_tail() {
        let n = noise(3);
        // Clicks of a short burst every 0.5 s with a 0.6 s exponential tail of noise.
        let a = audio(
            |t| {
                let ph = t % 0.5;
                let burst = if ph < 0.02 { 0.8 * (2.0 * PI * 700.0 * t).sin() } else { 0.0 };
                burst + 0.3 * n(t) * (-ph / 0.09).exp() * if ph > 0.02 { 1.0 } else { 0.0 }
            },
            2.0,
        );
        let y = dereverb(&a, 1.0, 0.6);
        // The tail between bursts (100 … 450 ms after each).
        let tail = |x: &[f32]| {
            let mut s = 0.0;
            for k in 1..3 {
                let a = ((k as f32 * 0.5 + 0.1) * RATE as f32) as usize;
                let b = ((k as f32 * 0.5 + 0.45) * RATE as f32) as usize;
                s += rms(&x[a..b]);
            }
            s
        };
        assert!(tail(&y.l) < tail(&a.l) * 0.5, "tail {} -> {}", tail(&a.l), tail(&y.l));
    }

    #[test]
    fn attenuate_takes_out_just_the_selection() {
        let a = audio(|t| 0.3 * (2.0 * PI * 1000.0 * t).sin() + 0.3 * (2.0 * PI * 5000.0 * t).sin(), 2.0);
        let y = attenuate(&a, 0.5, 1.5, 4000.0, 6000.0, -40.0);
        let part = |x: &[f32], t0: f32, t1: f32| x[(t0 * RATE as f32) as usize..(t1 * RATE as f32) as usize].to_vec();
        let inside = part(&y.l, 0.8, 1.2);
        assert!(tone_level(&inside, 5000.0) < 0.3 * 0.03, "5 kHz inside {}", tone_level(&inside, 5000.0));
        assert!((tone_level(&inside, 1000.0) - 0.3).abs() < 0.01, "1 kHz untouched");
        let outside = part(&y.l, 1.7, 1.95);
        assert!((tone_level(&outside, 5000.0) - 0.3).abs() < 0.01, "5 kHz outside the time range");
    }

    #[test]
    fn notes_are_found_and_shifted() {
        // Two notes: A4 for 0.5 s, then C5 for 0.5 s (with a little vibrato).
        let a = audio(
            |t| {
                let f = if t < 0.5 { 440.0 } else { 523.25 };
                // Vibrato of ±0.4 % at 5 Hz (the phase is the integral of the frequency).
                let ph = t + 0.004 * (1.0 - (2.0 * PI * 5.0 * t).cos()) / (2.0 * PI * 5.0);
                0.5 * (2.0 * PI * f * ph).sin()
            },
            1.0,
        );
        let notes = detect_notes(&a);
        assert_eq!(notes.len(), 2, "{notes:?}");
        assert!((notes[0].midi - 69.0).abs() < 0.3 && (notes[1].midi - 72.0).abs() < 0.3);
        // Move the first note up two semitones.
        let y = apply_pitch(&a, &notes, &[PitchEdit { note: 0, shift: 2.0, drift: 0.0 }]);
        let again = detect_notes(&y);
        assert!(again.iter().any(|n| (n.midi - 71.0).abs() < 0.35 && n.start < 0.2), "{again:?}");
        assert!(again.iter().any(|n| (n.midi - 72.0).abs() < 0.3 && n.start > 0.45), "the second note stays");
    }
}
