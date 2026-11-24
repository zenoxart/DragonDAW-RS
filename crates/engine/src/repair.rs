//! Audio repair, offline, on a whole clip (or a part of it): a full repair toolset – spectral de-noise
//! with a learned noise profile, de-hum, de-click, de-clip, distortion repair (peaks flattened by soft
//! clipping), de-reverb, spectral attenuation of a time × frequency selection, gain – and note-based
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

/// Rebuilds clipped peaks: runs of samples stuck at the clip level (within `threshold` × the level where
/// the waveform runs flat, or × the peak) are
/// replaced by a cubic through the waveform's slope on either side; the result is turned down if the
/// restored peaks would clip again. Returns the audio and the number of repaired runs.
pub fn declip(data: &AudioData, threshold: f32) -> (AudioData, usize) {
    let peak = data.l.iter().chain(&data.r).fold(0.0f32, |m, v| m.max(v.abs())).max(1e-9);
    // The clip level: where the waveform runs flat (three identical samples up high) – a click louder
    // than the clipping must not hide it – else the peak.
    let flat = data.l.windows(3).chain(data.r.windows(3)).filter(|w| w[1].abs() > 0.2 * peak && (w[1] - w[0]).abs() < 1e-6 && (w[2] - w[1]).abs() < 1e-6).map(|w| w[1].abs()).fold(f32::MAX, f32::min);
    let base = if flat < f32::MAX { flat } else { peak };
    let level = base * threshold.clamp(0.5, 1.0);
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

/// A detected note : where it is, its centre pitch and how the pitch moves inside it.
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

/// How a corrected note is shaped beyond its pitch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteShape {
    /// Seconds after the note's detected end that still follow its move (a one-shot's ring-out, a held
    /// note's release), so the tail does not snap back to the old pitch.
    pub tail: f32,
    /// Seconds over which the pitch and level move in and out at the note's edges.
    pub transition: f32,
    /// Seconds over which the note fades in from silence at its start (0 = as recorded).
    pub fade_in: f32,
    /// Level change of the note, dB.
    pub gain_db: f32,
    /// Seconds the note moves (negative = earlier). The gaps before and after stretch or shrink to
    /// make room; see [`time_limits`].
    pub time_shift: f32,
    /// Length of the note relative to the recording (1 = as sung, 2 = twice as long).
    pub stretch: f32,
    /// Formant shift in semitones: the voice's timbre (its resonances) moves, the pitch stays.
    pub formant: f32,
}

impl Default for NoteShape {
    fn default() -> Self {
        NoteShape { tail: 0.0, transition: 0.015, fade_in: 0.0, gain_db: 0.0, time_shift: 0.0, stretch: 1.0, formant: 0.0 }
    }
}

impl NoteShape {
    /// Whether the shape changes anything by itself (tail and transition only shape a pitch move).
    pub fn changes_sound(&self) -> bool {
        self.fade_in > 1e-4 || self.gain_db.abs() > 1e-3 || self.resynthesises()
    }

    /// Whether the note's timing, length or formants change (rendered by the note resynthesis).
    pub fn resynthesises(&self) -> bool {
        self.time_shift.abs() > 1e-4 || (self.stretch - 1.0).abs() > 1e-3 || self.formant.abs() > 1e-3
    }
}

/// Where note `k` sounds after its timing edit (start, end in seconds): moved and stretched as far as
/// [`time_limits`] allows; where there is no room, where it was recorded.
pub fn placement(notes: &[NoteBlob], k: usize, sh: &NoteShape, total: f64) -> (f64, f64) {
    let n = &notes[k];
    if sh.time_shift.abs() <= 1e-4 && (sh.stretch - 1.0).abs() <= 1e-3 {
        return (n.start, n.end);
    }
    let len_in = (n.end - n.start).max(1e-3);
    let len = (len_in * sh.stretch.clamp(0.25, 4.0) as f64).max(0.02);
    let (_, _, lo, hi) = time_limits(notes, k, len, total);
    if hi >= lo {
        let s = (n.start + sh.time_shift as f64).clamp(lo, hi);
        (s, s + len)
    } else {
        (n.start, n.end)
    }
}

/// Room around a note for a timing edit: the region `(from, to)` (seconds) whose content may stretch,
/// bounded by the neighbouring notes (and at most a quarter second away), and the earliest / latest
/// start the note can move to with its length `len` (seconds) so that both gaps keep 10 ms.
pub fn time_limits(notes: &[NoteBlob], k: usize, len: f64, total: f64) -> (f64, f64, f64, f64) {
    let n = &notes[k];
    let prev = if k > 0 { notes[k - 1].end } else { 0.0 };
    let next = notes.get(k + 1).map(|x| x.start).unwrap_or(total);
    let from = (n.start - 0.25).max(prev).max(0.0).min(n.start);
    let to = (n.end + 0.25).min(next).min(total).max(n.end);
    (from, to, from + 0.01, to - 0.01 - len)
}

/// What to do with a note: shift it, flatten its drift / vibrato and shape it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PitchEdit {
    /// Index into the detected notes.
    pub note: usize,
    /// Semitones (fractional allowed).
    pub shift: f32,
    /// 0 = keep the pitch movement, 1 = flatten it onto the note's centre.
    pub drift: f32,
    pub shape: NoteShape,
}

impl PitchEdit {
    pub fn new(note: usize, shift: f32, drift: f32) -> Self {
        PitchEdit { note, shift, drift, shape: NoteShape::default() }
    }
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
    if notes.is_empty() {
        notes.extend(one_shot(data));
    }
    notes
}

/// A one-shot (a single hit, pluck, 808 or sample too short or too percussive for the note tracker): the
/// whole sounding part as one note, at the pitch found in it – the tracker's steady frames with a looser
/// confidence, else the strongest spectral peak.
pub fn one_shot(data: &AudioData) -> Option<NoteBlob> {
    let x = mono(data);
    let rate = data.rate as f32;
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if peak < 1e-4 {
        return None;
    }
    // The sounding part: from the first to the last sample above −50 dB of the peak.
    let floor = peak * 0.00316;
    let first = x.iter().position(|v| v.abs() > floor)?;
    let last = x.iter().rposition(|v| v.abs() > floor)?;
    if last <= first {
        return None;
    }
    // Pitch: short windows (a one-shot may be shorter than the tracker's), loose confidence.
    let win = 1024.min(last - first + 1).max(256);
    let mut curve = Vec::new();
    let mut i = first;
    while i + win <= x.len() && i <= last {
        let w = &x[i..i + win];
        let lvl = w.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        if lvl > peak * 0.05 {
            if let Some((hz, _)) = detect_pitch(w, rate, 30.0, 2000.0).filter(|(_, c)| *c > 0.6) {
                curve.push(((i + win / 2) as f64 / rate as f64, hz_to_midi(hz)));
            }
        }
        i += win / 4;
    }
    let midi = if curve.is_empty() {
        // The strongest spectral peak of the loudest stretch.
        let n = 8192.min((last - first + 1).next_power_of_two()).max(1024);
        let mut buf: Vec<Complex<f32>> = (0..n).map(|k| Complex::new(x.get(first + k).copied().unwrap_or(0.0) * (0.5 - 0.5 * (2.0 * PI * k as f32 / n as f32).cos()), 0.0)).collect();
        FftPlanner::new().plan_fft_forward(n).process(&mut buf);
        let lo = (30.0 * n as f32 / rate) as usize;
        let (bin, _) = buf.iter().enumerate().take(n / 2).skip(lo.max(1)).fold((0, 0.0f32), |b, (k, c)| if c.norm() > b.1 { (k, c.norm()) } else { b });
        if bin == 0 {
            return None;
        }
        hz_to_midi(bin as f32 * rate / n as f32)
    } else {
        let mut v: Vec<f32> = curve.iter().map(|c| c.1).collect();
        v.sort_by(f32::total_cmp);
        v[v.len() / 2]
    };
    let start = first as f64 / rate as f64;
    let end = (last + 1) as f64 / rate as f64;
    if curve.is_empty() {
        curve = vec![(start, midi), (end, midi)];
    }
    Some(NoteBlob { start, end, midi, curve, level: peak })
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
    // Notes whose timing, length or formants change are resynthesised (with their pitch and level
    // edits); the others take the light path below.
    let (resynth, simple): (Vec<PitchEdit>, Vec<PitchEdit>) = edits.iter().partition(|e| e.shape.resynthesises() && notes.get(e.note).is_some());
    if !resynth.is_empty() {
        let mut out = apply_pitch(data, notes, &simple);
        for e in &resynth {
            out = resynth_note(&out, notes, e);
        }
        return out;
    }
    let rate = data.rate as f32;
    let n = data.frames();
    // Semitones to move each sample by, and how much of the shifted signal to use.
    let mut offset = vec![0.0f32; n];
    let mut wet = vec![0.0f32; n];
    // Level per sample (note volume and fade-in).
    let mut amp = vec![1.0f32; n];
    for e in edits {
        let Some(note) = notes.get(e.note) else { continue };
        let moves = e.shift.abs() >= 1e-3 || e.drift.abs() >= 1e-3;
        if !moves && !e.shape.changes_sound() {
            continue;
        }
        let ramp = ((e.shape.transition.max(0.001) * rate) as usize).max(1);
        let a = secs_to_frames(data, note.start);
        // The tail keeps following the note's move after its detected end.
        let b = secs_to_frames(data, note.end + e.shape.tail.max(0.0) as f64).min(n);
        let fade = (e.shape.fade_in.max(0.0) * rate) as usize;
        let g = 10f32.powf(e.shape.gain_db / 20.0);
        for i in a.saturating_sub(ramp)..(b + ramp).min(n) {
            let w = if i < a {
                1.0 - (a - i) as f32 / ramp as f32
            } else if i >= b {
                1.0 - (i - b) as f32 / ramp as f32
            } else {
                1.0
            }
            .max(0.0);
            if moves {
                let t = i as f64 / rate as f64;
                // Flattening follows the detected curve; in the tail only the move continues.
                let flatten = if t <= note.end { (note.midi - curve_at(note, t)) * e.drift } else { (note.midi - curve_at(note, note.end)) * e.drift };
                offset[i] = (e.shift + flatten) * w;
                wet[i] = wet[i].max(w);
            }
            // Fade-in from the note's start (silence before it inside the transition).
            let f = if fade == 0 {
                1.0
            } else if i < a {
                0.0
            } else {
                ((i - a) as f32 / fade as f32).min(1.0)
            };
            let target = g * if fade > 0 && i < a { 1.0 - w } else { f };
            amp[i] = 1.0 + (target - 1.0) * w;
        }
    }
    let scale = |y: Vec<f32>| -> Vec<f32> { y.into_iter().zip(&amp).map(|(v, a)| v * a).collect() };
    if wet.iter().all(|w| *w == 0.0) {
        if amp.iter().all(|a| *a == 1.0) {
            return data.clone();
        }
        return per_channel(data, |x| scale(x.to_vec()));
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
        scale((0..n).map(|i| x[i] + (shifted[i + delay] - x[i]) * wet[i]).collect())
    })
}

/// Offline pitch-synchronous overlap-add of one note with its timing, length, pitch, formant and level
/// edits. Grains two periods long (the period from the note's pitch curve; 10 ms grains outside it)
/// are cut around analysis marks one period apart and laid down at synthesis marks spaced by the new
/// period. A time map (output → input) picks which grain plays where: the region between the
/// neighbouring notes stretches so the note can move and change length. A formant shift resamples each
/// grain (shorter grains = higher resonances) while the grain spacing – the pitch – stays.
fn resynth_note(data: &AudioData, notes: &[NoteBlob], e: &PitchEdit) -> AudioData {
    let note = &notes[e.note];
    let rate = data.rate as f64;
    let total = data.frames() as f64 / rate;
    let sh = e.shape;
    let len_in = (note.end - note.start).max(1e-3);
    let (start_out, end_out) = placement(notes, e.note, &sh, total);
    let len_out = end_out - start_out;
    let (from, to, _, _) = time_limits(notes, e.note, len_out, total);
    // Knots (output seconds, input seconds) of the time map.
    let knots = [(from, from), (start_out, note.start), (start_out + len_out, note.end), (to, to)];
    let map = |t: f64| -> f64 {
        for w in knots.windows(2) {
            if t <= w[1].0 {
                let f = ((t - w[0].0) / (w[1].0 - w[0].0).max(1e-9)).clamp(0.0, 1.0);
                return w[0].1 + (w[1].1 - w[0].1) * f;
            }
        }
        t
    };
    let inside = |t_in: f64| t_in >= note.start && t_in <= note.end;
    // Period (samples) at an input time: from the pitch curve inside the note, else unvoiced grains.
    let unvoiced = 0.01 * rate;
    let period = |t_in: f64| -> f64 {
        if inside(t_in) {
            let hz = 440.0 * 2f64.powf((curve_at(note, t_in) as f64 - 69.0) / 12.0);
            (rate / hz).clamp(32.0, 1200.0)
        } else {
            unvoiced
        }
    };
    let ramp = (sh.transition.max(0.005) as f64).min(len_in / 2.0);
    // How much of the edit applies at an input time (ramped at the note's edges).
    let weight = |t_in: f64| -> f64 {
        if t_in < note.start - ramp || t_in > note.end + ramp {
            0.0
        } else if t_in < note.start {
            1.0 - (note.start - t_in) / ramp
        } else if t_in > note.end {
            1.0 - (t_in - note.end) / ramp
        } else {
            1.0
        }
    };
    let g = 10f64.powf(sh.gain_db as f64 / 20.0);
    let x = mono(data);
    let a = ((from - 0.03) * rate).max(0.0) as usize;
    let b = (((to + 0.03) * rate) as usize).min(x.len());
    // Analysis marks over the region, voiced ones snapped to the waveform's (positive) peak: the same
    // point of every period, so consecutive grains line up.
    let mut marks: Vec<f64> = Vec::new();
    let mut m = a as f64;
    while m < b as f64 {
        let p = period(m / rate);
        let mut at = m;
        if inside(m / rate) {
            let r = (p / 4.0) as isize;
            let c = m as isize;
            let best = (c - r..=c + r).filter(|i| *i >= 0 && (*i as usize) < x.len()).max_by(|i, j| x[*i as usize].total_cmp(&x[*j as usize]));
            if let Some(bi) = best {
                at = bi as f64;
            }
        }
        if marks.last().is_none_or(|l| at > *l + 8.0) {
            marks.push(at);
        }
        m = at.max(m) + p;
    }
    if marks.is_empty() {
        return data.clone();
    }
    let (oa, ob) = ((from * rate) as usize, ((to * rate) as usize).min(x.len()));
    let hann = |u: f64| if u.abs() >= 1.0 { 0.0 } else { 0.5 + 0.5 * (std::f64::consts::PI * u).cos() };
    let sample = |ch: &[f32], pos: f64| -> f64 {
        if pos < 0.0 || pos >= (ch.len() - 1) as f64 {
            return 0.0;
        }
        let i = pos as usize;
        let f = pos - i as f64;
        ch[i] as f64 * (1.0 - f) + ch[i + 1] as f64 * f
    };
    let render = |ch: &[f32]| -> Vec<f32> {
        let mut acc = vec![0.0f64; ob.saturating_sub(oa) + 1];
        let mut wsum = vec![0.0f64; acc.len()];
        let mut t = oa as f64;
        while t < ob as f64 {
            let t_in = map(t / rate) * rate;
            // The analysis mark nearest the mapped time.
            let k = marks.partition_point(|mk| *mk < t_in);
            let mk = match (k.checked_sub(1).map(|i| marks[i]), marks.get(k).copied()) {
                (Some(p), Some(n)) => {
                    if t_in - p < n - t_in {
                        p
                    } else {
                        n
                    }
                }
                (Some(p), None) => p,
                (None, Some(n)) => n,
                (None, None) => break,
            };
            let w = weight(mk / rate);
            let p = period(mk / rate);
            let semis = if inside(mk / rate) || w > 0.0 {
                let c = curve_at(note, (mk / rate).clamp(note.start, note.end));
                (e.shift + (note.midi - c) * e.drift) as f64 * w
            } else {
                0.0
            };
            let ratio = 2f64.powf(semis / 12.0);
            let phi = 2f64.powf(sh.formant as f64 * w / 12.0);
            let level = 1.0 + (g - 1.0) * w;
            let fade = if sh.fade_in > 1e-4 { ((mk / rate - note.start) / sh.fade_in as f64).clamp(0.0, 1.0) * w + (1.0 - w) } else { 1.0 };
            // Grain: two periods around the mark, resampled by the formant factor.
            let half = p / phi;
            let lo = (-half).ceil() as isize;
            let hi = half.floor() as isize;
            for j in lo..=hi {
                let o = t as isize + j - oa as isize;
                if o < 0 || o as usize >= acc.len() {
                    continue;
                }
                let win = hann(j as f64 / half);
                acc[o as usize] += sample(ch, mk + j as f64 * phi) * win * level * fade;
                wsum[o as usize] += win;
            }
            let hop = if inside(mk / rate) { p / ratio } else { unvoiced / 2.0 };
            t += hop.max(8.0);
        }
        acc.iter().zip(&wsum).map(|(v, w)| (v / w.max(0.35)) as f32).collect()
    };
    let x_fade = (0.005 * rate) as usize;
    per_channel(data, |ch| {
        let wet = render(ch);
        let mut out = ch.to_vec();
        let n = ob.saturating_sub(oa);
        for i in 0..n {
            // Crossfade into and out of the resynthesised region (where the map is the identity).
            let edge = i.min(n - 1 - i);
            let w = (edge as f32 / x_fade.max(1) as f32).min(1.0);
            out[oa + i] = ch[oa + i] * (1.0 - w) + wet[i] * w;
        }
        out
    })
}

// ------------------------------------------------------------------------------------------------ diagnosis

/// What a recording seems to suffer from (the repair view's "check the recording").
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Diagnosis {
    /// Mains hum: its frequency and how far it stands out of its surroundings (dB).
    pub hum: Option<(f32, f32)>,
    /// Clicks found.
    pub clicks: usize,
    /// Places where the waveform is cut flat.
    pub clipping: usize,
    /// How far the quiet parts (noise) sit below the loud ones (dB).
    pub snr_db: f32,
    /// The loudest sample (dBFS).
    pub peak_db: f32,
}

fn goertzel(x: &[f32], f: f32, rate: f32) -> f32 {
    let w = 2.0 * PI * f / rate;
    let c = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f32, 0.0f32);
    for v in x {
        let s0 = v + c * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    (s1 * s1 + s2 * s2 - c * s1 * s2).max(0.0) / (x.len().max(1) as f32).powi(2)
}

/// Looks the recording over: hum, clicks, clipping, background noise.
pub fn diagnose(data: &AudioData) -> Diagnosis {
    let rate = data.rate as f32;
    let x = mono(data);
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-9);
    // Hum: the mains frequency and its first harmonics against tones a little beside them.
    let probe = &x[..x.len().min((rate * 20.0) as usize)];
    let hum = [50.0f32, 60.0]
        .iter()
        .map(|&b| {
            let on: f32 = (1..=3).map(|k| goertzel(probe, b * k as f32, rate)).sum();
            let off: f32 = (1..=3).map(|k| goertzel(probe, b * k as f32 + 13.0, rate) + goertzel(probe, b * k as f32 - 13.0, rate)).sum::<f32>() / 2.0;
            (b, 10.0 * ((on + 1e-20) / (off + 1e-20)).log10(), on)
        })
        .filter(|(_, above, on)| *above > 12.0 && on.sqrt() > peak * 0.003)
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(b, above, _)| (b, above));
    // Clipping: flat tops – three or more identical samples up near the top of the waveform.
    let top = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let mut clipping = 0;
    let mut run = 0;
    for w in x.windows(2) {
        if w[1].abs() > 0.3 * top && w[1].abs() > 0.2 && (w[1] - w[0]).abs() < 1e-6 {
            run += 1;
            if run == 2 {
                clipping += 1;
            }
        } else {
            run = 0;
        }
    }
    // Clicks, counted on a de-clipped copy (the corners of clipped peaks are not clicks).
    let (_, clicks) = if clipping > 0 { declick(&declip(data, 0.995).0, 5.0) } else { declick(data, 5.0) };
    // Noise: the quietest tenth of 20 ms frames against the loudest half.
    let frame = (rate * 0.02) as usize;
    let mut e: Vec<f32> = x.chunks(frame.max(1)).map(|c| c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).collect();
    e.sort_by(f32::total_cmp);
    let n = e.len().max(1);
    let quiet = e[..(n / 10).max(1)].iter().sum::<f32>() / (n / 10).max(1) as f32;
    let loud = e[n / 2..].iter().sum::<f32>() / (n - n / 2).max(1) as f32;
    let snr_db = 10.0 * ((loud + 1e-20) / (quiet + 1e-20)).log10();
    // Channels are counted separately by the repairs; report places, not channel hits.
    let ch = if data.l == data.r { 2 } else { 1 };
    Diagnosis { hum, clicks: clicks / ch, clipping, snr_db, peak_db: 20.0 * peak.log10() }
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
    fn one_shots_are_one_note() {
        // A decaying 808-like tone, 0.4 s at 55 Hz, then silence.
        let a = audio(|t| if t < 0.4 { 0.8 * (2.0 * PI * 55.0 * t).sin() * (-t * 6.0).exp() } else { 0.0 }, 0.6);
        let notes = detect_notes(&a);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!((notes[0].midi - 33.0).abs() < 0.5, "A1 = 33: {}", notes[0].midi);
        // A short pluck too short for the tracker.
        let b = audio(|t| 0.6 * (2.0 * PI * 440.0 * t).sin() * (-t * 40.0).exp(), 0.12);
        let nb = detect_notes(&b);
        assert_eq!(nb.len(), 1);
        assert!((nb[0].midi - 69.0).abs() < 0.5, "{}", nb[0].midi);
    }

    /// Moving a note later: its tone starts later, the gap before it stretches, nothing outside the
    /// region changes; a formant shift keeps the pitch.
    #[test]
    fn notes_move_in_time_and_shift_formants() {
        // Silence, a 220 Hz note 0.4 – 0.8 s, silence.
        let a = audio(|t| if (0.4..0.8).contains(&t) { 0.5 * (2.0 * PI * 220.0 * t).sin() } else { 0.0 }, 1.4);
        let note = NoteBlob { start: 0.4, end: 0.8, midi: 57.0, curve: vec![(0.4, 57.0), (0.8, 57.0)], level: 0.5 };
        let (_, to, _, hi) = time_limits(std::slice::from_ref(&note), 0, 0.4, 1.4);
        assert!((to - 1.05).abs() < 1e-9 && hi > 0.6);
        let mut e = PitchEdit::new(0, 0.0, 0.0);
        e.shape.time_shift = 0.15;
        let y = apply_pitch(&a, std::slice::from_ref(&note), &[e]);
        let seg = |d: &AudioData, t0: f32, t1: f32| rms(&d.l[(t0 * RATE as f32) as usize..(t1 * RATE as f32) as usize]);
        assert!(seg(&y, 0.42, 0.52) < 0.05, "the note no longer starts at 0.4 s: {}", seg(&y, 0.42, 0.52));
        assert!(seg(&y, 0.6, 0.9) > 0.25, "it sounds from 0.55 s on: {}", seg(&y, 0.6, 0.9));
        assert!(seg(&y, 0.98, 1.4) < 0.05, "and ends before the region does");
        let mid = &y.l[(0.65 * RATE as f32) as usize..(0.85 * RATE as f32) as usize];
        assert!(tone_level(mid, 220.0) > 0.3, "same pitch: {}", tone_level(mid, 220.0));
        // Twice as long.
        let mut s = PitchEdit::new(0, 0.0, 0.0);
        s.shape.stretch = 1.5;
        let y = apply_pitch(&a, std::slice::from_ref(&note), &[s]);
        assert!(seg(&y, 0.85, 0.95) > 0.25, "longer: {}", seg(&y, 0.85, 0.95));
        // A formant shift: the pitch stays at 220 Hz, the spectrum's balance moves.
        // Harmonics of 220 Hz under one resonance at 660 Hz (the 3rd harmonic).
        let peak = |h: f32| (-((h * 220.0 - 660.0) / 250.0).powi(2)).exp();
        let rich = audio(|t| if (0.4..0.8).contains(&t) { (1..10).map(|h| (2.0 * PI * 220.0 * h as f32 * t).sin() * (0.05 + peak(h as f32))).sum::<f32>() * 0.3 } else { 0.0 }, 1.4);
        let mut f = PitchEdit::new(0, 0.0, 0.0);
        f.shape.formant = 5.0;
        let y = apply_pitch(&rich, std::slice::from_ref(&note), &[f]);
        let win = |d: &AudioData| d.l[(0.5 * RATE as f32) as usize..(0.7 * RATE as f32) as usize].to_vec();
        let (w0, w1) = (win(&rich), win(&y));
        // The resonance keeps its height (within 2 dB) at its new place.
        assert!(tone_level(&w1, 880.0) > 0.8 * tone_level(&w0, 660.0), "{} vs {}", tone_level(&w1, 880.0), tone_level(&w0, 660.0));
        // +5 semitones moves the resonance from 660 Hz towards 880 Hz: the 4th harmonic gains on the 3rd.
        let tilt = |w: &[f32]| tone_level(w, 880.0) / tone_level(w, 660.0);
        assert!(tilt(&w1) > tilt(&w0) * 3.0, "the resonance moved up: {} vs {}", tilt(&w1), tilt(&w0));
        assert!(tone_level(&w1, 330.0) < 0.01 && tone_level(&w1, 550.0) < 0.01, "no new pitch in between");
        assert!(seg(&y, 0.0, 0.1) < 1e-6 && seg(&y, 1.2, 1.4) < 1e-6, "outside untouched");
    }

    #[test]
    fn note_shape_volume_fade_and_tail() {
        let a = audio(|t| 0.5 * (2.0 * PI * 220.0 * t).sin(), 1.0);
        let note = NoteBlob { start: 0.2, end: 0.6, midi: 57.0, curve: vec![(0.2, 57.0), (0.6, 57.0)], level: 0.5 };
        let mut e = PitchEdit::new(0, 0.0, 0.0);
        e.shape.gain_db = -6.0;
        e.shape.fade_in = 0.1;
        let y = apply_pitch(&a, std::slice::from_ref(&note), &[e]);
        let seg = |d: &AudioData, t0: f32, t1: f32| rms(&d.l[(t0 * RATE as f32) as usize..(t1 * RATE as f32) as usize]);
        assert!((seg(&y, 0.0, 0.18) - seg(&a, 0.0, 0.18)).abs() < 1e-4, "before the note untouched");
        assert!(seg(&y, 0.2, 0.22) < seg(&a, 0.2, 0.22) * 0.2, "fades in from silence");
        let r = seg(&y, 0.4, 0.58) / seg(&a, 0.4, 0.58);
        assert!((r - 0.5).abs() < 0.03, "−6 dB: {r}");
        assert!((seg(&y, 0.7, 1.0) - seg(&a, 0.7, 1.0)).abs() < 1e-4, "after the note untouched");
        // A tail: the move continues 0.2 s after the note's end.
        let mut t = PitchEdit::new(0, 12.0, 0.0);
        t.shape.tail = 0.2;
        let y = apply_pitch(&a, std::slice::from_ref(&note), &[t]);
        let at = |d: &AudioData, t0: f32, hz: f32| tone_level(&d.l[(t0 * RATE as f32) as usize..((t0 + 0.1) * RATE as f32) as usize], hz);
        assert!(at(&y, 0.66, 440.0) > at(&y, 0.66, 220.0), "the tail is up an octave too");
        assert!(at(&y, 0.85, 220.0) > at(&y, 0.85, 440.0), "after the tail the original pitch");
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
        let y = apply_pitch(&a, &notes, &[PitchEdit::new(0, 2.0, 0.0)]);
        let again = detect_notes(&y);
        assert!(again.iter().any(|n| (n.midi - 71.0).abs() < 0.35 && n.start < 0.2), "{again:?}");
        assert!(again.iter().any(|n| (n.midi - 72.0).abs() < 0.3 && n.start > 0.45), "the second note stays");
    }

    #[test]
    fn diagnosis_finds_what_is_wrong_and_nothing_else() {
        let n = noise(11);
        // A clean, loud tone with gaps between phrases (10 ms fades: an abrupt start would be a click).
        let gate = |t: f32| ((t % 1.0) / 0.01).min((0.8 - (t % 1.0)) / 0.01).clamp(0.0, 1.0);
        let clean = audio(move |t| gate(t) * 0.5 * (2.0 * PI * 330.0 * t).sin(), 3.0);
        let d = diagnose(&clean);
        assert_eq!(d.hum, None);
        assert_eq!(d.clicks, 0);
        assert_eq!(d.clipping, 0);
        assert!(d.snr_db > 60.0, "{}", d.snr_db);
        // The same with hum, hiss, clicks and clipping.
        let mut dirty = audio(move |t| (gate(t) * 1.3 * (2.0 * PI * 330.0 * t).sin() + 0.02 * (2.0 * PI * 50.0 * t).sin() + 0.01 * n(t)).clamp(-1.0, 1.0), 3.0);
        for k in 0..6 {
            let i = (0.85 + k as f32 * 0.37) as usize * RATE as usize + 900 * k;
            dirty.l[i] += 0.5;
            dirty.r[i] += 0.5;
        }
        let d = diagnose(&dirty);
        assert_eq!(d.hum.map(|h| h.0), Some(50.0), "{d:?}");
        assert!(d.clicks >= 3, "{d:?}");
        assert!(d.clipping > 100, "{d:?}");
        assert!(d.snr_db < 45.0, "{d:?}");
    }
}
