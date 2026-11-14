//! Audio analysis: tempo (onset envelope + autocorrelation) and key (chroma + Krumhansl–Schmuckler
//! profiles). Replaces the old placeholder "AI analyze" that returned random values.

use crate::samples::AudioData;
use rustfft::{num_complex::Complex, FftPlanner};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Analysis {
    pub bpm: Option<f64>,
    pub bpm_confidence: f32,
    pub key: Option<String>,
    pub key_confidence: f32,
    pub peak_db: f32,
    pub lufs: f32,
}

/// Mono onset-strength envelope (positive spectral-energy flux) with its frame rate.
fn onset_envelope(d: &AudioData) -> (Vec<f32>, f64) {
    let hop = 512;
    let win = 1024;
    let mono: Vec<f32> = d.l.iter().zip(&d.r).map(|(a, b)| (a + b) * 0.5).collect();
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(win);
    let window: Vec<f32> = (0..win).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / win as f32).cos()).collect();
    let mut prev = vec![0.0f32; win / 2];
    let mut env = Vec::new();
    let mut buf = vec![Complex::new(0.0, 0.0); win];
    let mut i = 0;
    while i + win <= mono.len() {
        for k in 0..win {
            buf[k] = Complex::new(mono[i + k] * window[k], 0.0);
        }
        fft.process(&mut buf);
        let mut flux = 0.0;
        for k in 1..win / 2 {
            let m = (buf[k].norm() + 1e-9).ln();
            flux += (m - prev[k]).max(0.0);
            prev[k] = m;
        }
        env.push(flux);
        i += hop;
    }
    // Remove the local mean so steady sections do not count as onsets.
    let w = 8;
    let smoothed: Vec<f32> = (0..env.len()).map(|k| {
        let a = k.saturating_sub(w);
        let b = (k + w).min(env.len());
        let mean = env[a..b].iter().sum::<f32>() / (b - a) as f32;
        (env[k] - mean).max(0.0)
    }).collect();
    (smoothed, d.rate as f64 / hop as f64)
}

/// Estimates the tempo between 70 and 180 BPM.
pub fn detect_bpm(d: &AudioData) -> (Option<f64>, f32) {
    if d.duration() < 4.0 {
        return (None, 0.0);
    }
    let (env, fps) = onset_envelope(d);
    if env.len() < 64 {
        return (None, 0.0);
    }
    let min_lag = (fps * 60.0 / 180.0) as usize;
    let max_lag = (fps * 60.0 / 70.0) as usize;
    let energy: f32 = env.iter().map(|x| x * x).sum::<f32>().max(1e-9);
    let mut best = (0usize, 0.0f32);
    let mut scores = Vec::new();
    for lag in min_lag..=max_lag.min(env.len() / 2) {
        let mut s = 0.0;
        for k in lag..env.len() {
            s += env[k] * env[k - lag];
        }
        // Reward lags whose double also correlates (metrical consistency).
        if 2 * lag < env.len() {
            let mut s2 = 0.0;
            for k in 2 * lag..env.len() {
                s2 += env[k] * env[k - 2 * lag];
            }
            s += 0.5 * s2;
        }
        scores.push(s);
        if s > best.1 {
            best = (lag, s);
        }
    }
    if best.0 == 0 {
        return (None, 0.0);
    }
    // Parabolic interpolation around the peak for sub-frame precision.
    let i = best.0 - min_lag;
    let lag = if i > 0 && i + 1 < scores.len() {
        let (a, b, c) = (scores[i - 1], scores[i], scores[i + 1]);
        let denom = a - 2.0 * b + c;
        best.0 as f64 + if denom.abs() > 1e-9 { 0.5 * (a - c) as f64 / denom as f64 } else { 0.0 }
    } else {
        best.0 as f64
    };
    let bpm = 60.0 * fps / lag;
    let mean = scores.iter().sum::<f32>() / scores.len() as f32;
    let confidence = ((best.1 - mean) / energy).clamp(0.0, 1.0);
    (Some((bpm * 10.0).round() / 10.0), confidence)
}

const MAJOR: [f32; 12] = [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88];
const MINOR: [f32; 12] = [6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17];
const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// Average chroma (12 pitch classes) of the file.
pub fn chroma(d: &AudioData) -> [f32; 12] {
    let win = 8192;
    let hop = 4096;
    let mono: Vec<f32> = d.l.iter().zip(&d.r).map(|(a, b)| (a + b) * 0.5).collect();
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(win);
    let mut c = [0.0f32; 12];
    let mut buf = vec![Complex::new(0.0, 0.0); win];
    let mut i = 0;
    let rate = d.rate as f32;
    while i + win <= mono.len() {
        for k in 0..win {
            let w = 0.5 - 0.5 * (std::f32::consts::TAU * k as f32 / win as f32).cos();
            buf[k] = Complex::new(mono[i + k] * w, 0.0);
        }
        fft.process(&mut buf);
        for (k, b) in buf.iter().enumerate().take(win / 2).skip(1) {
            let f = k as f32 * rate / win as f32;
            if !(55.0..=5000.0).contains(&f) {
                continue;
            }
            let midi = 69.0 + 12.0 * (f / 440.0).log2();
            let pc = (midi.round() as i32).rem_euclid(12) as usize;
            c[pc] += b.norm_sqr();
        }
        i += hop;
    }
    c
}

fn correlate(a: &[f32; 12], b: &[f32; 12], rot: usize) -> f32 {
    let ma = a.iter().sum::<f32>() / 12.0;
    let mb = b.iter().sum::<f32>() / 12.0;
    let (mut num, mut da, mut db) = (0.0, 0.0, 0.0);
    for i in 0..12 {
        let x = a[(i + rot) % 12] - ma;
        let y = b[i] - mb;
        num += x * y;
        da += x * x;
        db += y * y;
    }
    num / (da * db).sqrt().max(1e-9)
}

pub fn detect_key(d: &AudioData) -> (Option<String>, f32) {
    let c = chroma(d);
    if c.iter().sum::<f32>() < 1e-6 {
        return (None, 0.0);
    }
    let mut best = (String::new(), -1.0f32);
    for root in 0..12 {
        for (profile, mode) in [(&MAJOR, "major"), (&MINOR, "minor")] {
            let r = correlate(&c, profile, root);
            if r > best.1 {
                best = (format!("{} {mode}", NAMES[root]), r);
            }
        }
    }
    (Some(best.0), best.1.clamp(0.0, 1.0))
}

pub fn analyze(d: &AudioData) -> Analysis {
    let (bpm, bc) = detect_bpm(d);
    let (key, kc) = detect_key(d);
    let st = crate::dsp::loudness::measure(&d.l, &d.r, d.rate as f32);
    Analysis { bpm, bpm_confidence: bc, key, key_confidence: kc, peak_db: st.sample_peak_db, lufs: st.integrated }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click_track(bpm: f64, secs: f64, rate: u32) -> AudioData {
        let n = (secs * rate as f64) as usize;
        let mut l = vec![0.0f32; n];
        let period = (60.0 / bpm * rate as f64) as usize;
        let mut rng = crate::dsp::Rng::new(3);
        let mut t = 0;
        while t < n {
            for i in 0..1500.min(n - t) {
                l[t + i] = rng.bipolar() * (1.0 - i as f32 / 1500.0) * 0.8;
            }
            t += period;
        }
        AudioData::from_planar(rate, l.clone(), l)
    }

    #[test]
    fn finds_tempo() {
        for bpm in [90.0, 120.0, 140.0] {
            let d = click_track(bpm, 12.0, 44100);
            let (got, _) = detect_bpm(&d);
            let got = got.unwrap();
            assert!((got - bpm).abs() < 2.0 || (got * 2.0 - bpm).abs() < 2.0 || (got / 2.0 - bpm).abs() < 2.0, "{bpm} → {got}");
        }
    }

    #[test]
    fn finds_key_of_a_triad() {
        // A minor triad held: A, C, E.
        let rate = 44100;
        let n = rate * 4;
        let f = |m: f32| 440.0 * 2f32.powf((m - 69.0) / 12.0);
        let s: Vec<f32> = (0..n).map(|i| {
            let t = i as f32 / rate as f32;
            [57.0, 60.0, 64.0, 45.0].iter().map(|&m| (t * f(m) * std::f32::consts::TAU).sin()).sum::<f32>() * 0.2
        }).collect();
        let d = AudioData::from_planar(rate as u32, s.clone(), s);
        let (k, _) = detect_key(&d);
        assert!(k.as_deref() == Some("A minor") || k.as_deref() == Some("C major"), "{k:?}");
    }
}
