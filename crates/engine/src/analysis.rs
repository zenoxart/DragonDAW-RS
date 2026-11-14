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


// ---------------------------------------------------------------------------------------------------- audio → MIDI

/// How to transcribe audio into notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TranscribeMode {
    /// Monophonic melody (voice, bass, lead): pitch tracking.
    Melody,
    /// Drums: onsets, sorted into kick / snare / hat by brightness.
    Drums,
}

/// Transcribes `d` into notes (ticks at `bpm`, [`crate::model::PPQ`] per beat).
pub fn audio_to_notes(d: &AudioData, bpm: f64, mode: TranscribeMode) -> Vec<crate::model::Note> {
    use crate::effects::pitch::{detect_pitch, hz_to_midi};
    use crate::model::{Note, PPQ};
    let rate = d.rate as f32;
    let mono: Vec<f32> = d.l.iter().zip(&d.r).map(|(a, b)| (a + b) * 0.5).collect();
    let ticks = |secs: f64| (secs * bpm / 60.0 * PPQ as f64).round().max(0.0) as u32;
    let hop = 256usize;
    let frame = 2048usize;
    let rms_of = |s: &[f32]| (s.iter().map(|x| x * x).sum::<f32>() / s.len().max(1) as f32).sqrt();
    let peak_rms = mono.chunks(frame).map(rms_of).fold(0.0f32, f32::max).max(1e-6);
    let mut notes = Vec::new();
    match mode {
        TranscribeMode::Melody => {
            // Per frame: (pitch, rms). Consecutive frames with the same pitch become one note.
            let mut cur: Option<(i32, usize, f32)> = None; // (pitch, start frame, max rms)
            let mut pending: Option<(i32, usize)> = None; // pitch change must hold two frames
            let n_frames = mono.len().saturating_sub(frame) / hop;
            let close = |p: i32, start: usize, end: usize, vel: f32, notes: &mut Vec<Note>| {
                let (s, e) = (start as f64 * hop as f64 / rate as f64, end as f64 * hop as f64 / rate as f64);
                if e - s >= 0.06 {
                    let st = ticks(s);
                    notes.push(Note { pitch: p.clamp(0, 127) as u8, start: st, length: ticks(e).saturating_sub(st).max(1), velocity: (vel / peak_rms).sqrt().clamp(0.1, 1.0), ..Note::default() });
                }
            };
            for f in 0..n_frames {
                let w = &mono[f * hop..f * hop + frame];
                let rms = rms_of(w);
                let pitch = if rms > peak_rms * 0.05 { detect_pitch(w, rate, 50.0, 1500.0).filter(|p| p.1 > 0.85).map(|p| hz_to_midi(p.0).round() as i32) } else { None };
                match (cur, pitch) {
                    (Some((p, s, v)), Some(q)) if q == p => {
                        cur = Some((p, s, v.max(rms)));
                        pending = None;
                    }
                    (Some((p, s, v)), Some(q)) => match pending {
                        Some((pq, pf)) if pq == q => {
                            close(p, s, pf, v, &mut notes);
                            cur = Some((q, pf, rms));
                            pending = None;
                        }
                        _ => pending = Some((q, f)),
                    },
                    (Some((p, s, v)), None) => {
                        close(p, s, f, v, &mut notes);
                        cur = None;
                        pending = None;
                    }
                    (None, Some(q)) => cur = Some((q, f, rms)),
                    (None, None) => {}
                }
            }
            if let Some((p, s, v)) = cur {
                close(p, s, n_frames, v, &mut notes);
            }
        }
        TranscribeMode::Drums => {
            // Onsets on a fast energy envelope; brightness (zero-crossing rate) picks the drum.
            let h = 128usize;
            let env: Vec<f32> = mono.chunks(h).map(rms_of).collect();
            let mut last = -1000i64;
            for i in 1..env.len() {
                let rise = env[i] - env[i - 1];
                let i64i = i as i64;
                if rise > peak_rms * 0.15 && env[i] > peak_rms * 0.1 && i64i - last > (0.05 * rate / h as f32) as i64 {
                    last = i64i;
                    let start = i * h;
                    let seg = &mono[start..(start + 1024).min(mono.len())];
                    let zc = seg.windows(2).filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0)).count() as f32 / seg.len().max(1) as f32;
                    let hz = zc * rate / 2.0;
                    let pitch = if hz < 250.0 { 36 } else if hz < 2500.0 { 38 } else { 42 };
                    let peak = seg.iter().fold(0.0f32, |m, x| m.max(x.abs()));
                    notes.push(Note { pitch, start: ticks(start as f64 / rate as f64), length: PPQ / 4, velocity: (peak / peak_rms.max(1e-6) * 0.5).clamp(0.2, 1.0), ..Note::default() });
                }
            }
        }
    }
    notes
}


// ---------------------------------------------------------------------------------------------------- stems

/// Stems from [`separate_stems`].
pub struct Stems {
    /// Percussive part (drums, transients).
    pub drums: AudioData,
    /// Sustained, harmonic part outside the centre (instruments).
    pub harmonic: AudioData,
    /// Harmonic content panned to the centre (usually lead vocals, bass, kick body).
    pub center: AudioData,
}

fn median(v: &mut [f32]) -> f32 {
    let m = v.len() / 2;
    *v.select_nth_unstable_by(m, |a, b| a.total_cmp(b)).1
}

/// Offline stem separation with classic DSP (no neural network):
/// harmonic/percussive separation by median filtering of the spectrogram (Fitzgerald 2010), then the
/// harmonic part is split into centre and sides by per-bin left/right similarity. The three stems add
/// back up to the input (soft masks), so nothing is lost.
pub fn separate_stems(d: &AudioData) -> Stems {
    let win = 2048usize;
    let hop = win / 4;
    let bins = win / 2 + 1;
    let n = d.frames();
    let window: Vec<f32> = (0..win).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / win as f32).cos()).collect();
    let mut planner = FftPlanner::<f32>::new();
    let fwd = planner.plan_fft_forward(win);
    let inv = planner.plan_fft_inverse(win);
    let frames = n.div_ceil(hop) + 1;
    // STFT of both channels.
    let stft = |x: &[f32]| -> Vec<Vec<Complex<f32>>> {
        (0..frames)
            .map(|f| {
                let start = f as isize * hop as isize - (win / 2) as isize;
                let mut buf: Vec<Complex<f32>> = (0..win)
                    .map(|i| {
                        let k = start + i as isize;
                        let v = if k >= 0 && (k as usize) < x.len() { x[k as usize] } else { 0.0 };
                        Complex::new(v * window[i], 0.0)
                    })
                    .collect();
                fwd.process(&mut buf);
                buf.truncate(bins);
                buf
            })
            .collect()
    };
    let sl = stft(&d.l);
    let sr = stft(&d.r);
    let mag: Vec<Vec<f32>> = sl.iter().zip(&sr).map(|(a, b)| a.iter().zip(b).map(|(x, y)| (x.norm() + y.norm()) * 0.5).collect()).collect();
    // Median filters: across time (harmonic) and across frequency (percussive).
    let (ht, pf) = (17usize, 17usize);
    let mut harm = vec![vec![0.0f32; bins]; frames];
    let mut perc = vec![vec![0.0f32; bins]; frames];
    let mut tmp = Vec::with_capacity(ht.max(pf));
    for f in 0..frames {
        for b in 0..bins {
            tmp.clear();
            for k in f.saturating_sub(ht / 2)..(f + ht / 2 + 1).min(frames) {
                tmp.push(mag[k][b]);
            }
            harm[f][b] = median(&mut tmp);
            tmp.clear();
            for k in b.saturating_sub(pf / 2)..(b + pf / 2 + 1).min(bins) {
                tmp.push(mag[f][k]);
            }
            perc[f][b] = median(&mut tmp);
        }
    }
    // Soft masks (Wiener, power 2) and the centre mask on the harmonic part.
    let mut outs: [Vec<Vec<Complex<f32>>>; 6] = Default::default(); // drums L/R, harmonic L/R, centre L/R
    for o in outs.iter_mut() {
        *o = vec![vec![Complex::new(0.0, 0.0); bins]; frames];
    }
    for f in 0..frames {
        for b in 0..bins {
            let (h2, p2) = (harm[f][b].powi(2), perc[f][b].powi(2));
            let mp = if h2 + p2 > 1e-20 { p2 / (h2 + p2) } else { 0.5 };
            let mh = 1.0 - mp;
            let (l, r) = (sl[f][b], sr[f][b]);
            // Centre: how similar L and R are in this bin (1 = identical).
            let diff = (l - r).norm();
            let sum = (l + r).norm().max(1e-12);
            let mc = (1.0 - diff / sum).clamp(0.0, 1.0).powi(2);
            outs[0][f][b] = l * mp;
            outs[1][f][b] = r * mp;
            outs[2][f][b] = l * mh * (1.0 - mc);
            outs[3][f][b] = r * mh * (1.0 - mc);
            outs[4][f][b] = l * mh * mc;
            outs[5][f][b] = r * mh * mc;
        }
    }
    // Inverse STFT with overlap-add (Hann² sums to 1.5 at 75 % overlap).
    let istft = |spec: &Vec<Vec<Complex<f32>>>| -> Vec<f32> {
        let mut out = vec![0.0f32; n + win];
        let mut norm = vec![0.0f32; n + win];
        let mut buf = vec![Complex::new(0.0, 0.0); win];
        for (f, frame) in spec.iter().enumerate() {
            buf[..bins].copy_from_slice(frame);
            for k in 1..win / 2 {
                buf[win - k] = frame[k].conj();
            }
            inv.process(&mut buf);
            let start = f as isize * hop as isize - (win / 2) as isize;
            for i in 0..win {
                let k = start + i as isize;
                if k >= 0 && (k as usize) < out.len() {
                    out[k as usize] += buf[i].re / win as f32 * window[i];
                    norm[k as usize] += window[i] * window[i];
                }
            }
        }
        out.truncate(n);
        for (o, w) in out.iter_mut().zip(norm) {
            if w > 1e-6 {
                *o /= w;
            }
        }
        out
    };
    let mk = |a: &Vec<Vec<Complex<f32>>>, b: &Vec<Vec<Complex<f32>>>| AudioData { rate: d.rate, l: istft(a), r: istft(b), ..Default::default() };
    Stems { drums: mk(&outs[0], &outs[1]), harmonic: mk(&outs[2], &outs[3]), center: mk(&outs[4], &outs[5]) }
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

    #[test]
    fn melody_and_drums_to_midi() {
        use super::{audio_to_notes, TranscribeMode};
        let rate = 48000usize;
        // A4, C5, E5 for 0.5 s each at 120 BPM = one beat each.
        let mut l = Vec::new();
        for hz in [440.0f32, 523.25, 659.26] {
            l.extend((0..rate / 2).map(|i| (std::f32::consts::TAU * hz * i as f32 / rate as f32).sin() * 0.5));
        }
        let d = crate::samples::AudioData { rate: rate as u32, r: l.clone(), l, ..Default::default() };
        let notes = audio_to_notes(&d, 120.0, TranscribeMode::Melody);
        let pitches: Vec<u8> = notes.iter().map(|n| n.pitch).collect();
        assert_eq!(pitches, vec![69, 72, 76], "{notes:?}");
        assert!(notes[1].start.abs_diff(crate::model::PPQ) < 12, "{}", notes[1].start);
        // Drums: a low thump at 0 s and a noise burst at 0.5 s.
        let mut rng = crate::dsp::Rng::new(3);
        let mut x = vec![0.0f32; rate];
        for i in 0..4000 {
            x[i] = (std::f32::consts::TAU * 60.0 * i as f32 / rate as f32).sin() * (1.0 - i as f32 / 4000.0);
            x[rate / 2 + i] = rng.bipolar() * 0.8 * (1.0 - i as f32 / 4000.0);
        }
        let d = crate::samples::AudioData { rate: rate as u32, r: x.clone(), l: x, ..Default::default() };
        let hits = audio_to_notes(&d, 120.0, TranscribeMode::Drums);
        assert_eq!(hits.len(), 2, "{hits:?}");
        assert_eq!(hits[0].pitch, 36);
        assert_eq!(hits[1].pitch, 42);
        assert!(hits[1].start.abs_diff(crate::model::PPQ) < 6);
    }

    #[test]
    fn stems_split_drums_tones_and_centre() {
        let rate = 44100usize;
        let n = rate * 2;
        // A centred 330 Hz tone, a 220 Hz tone hard left, and a click every 0.25 s in both channels.
        let mut l = vec![0.0f32; n];
        let mut r = vec![0.0f32; n];
        for i in 0..n {
            let t = i as f32 / rate as f32;
            let c = (std::f32::consts::TAU * 330.0 * t).sin() * 0.3;
            l[i] = c + (std::f32::consts::TAU * 220.0 * t).sin() * 0.3;
            r[i] = c;
            if i % (rate / 4) < 30 {
                let k = 1.0 - (i % (rate / 4)) as f32 / 30.0;
                l[i] += 0.9 * k;
                r[i] += 0.9 * k;
            }
        }
        let d = AudioData { rate: rate as u32, l: l.clone(), r: r.clone(), ..Default::default() };
        let s = super::separate_stems(&d);
        let energy = |a: &AudioData| a.l.iter().chain(&a.r).map(|x| x * x).sum::<f32>();
        // The stems add back up to the input.
        let err: f32 = (0..n).map(|i| (s.drums.l[i] + s.harmonic.l[i] + s.center.l[i] - l[i]).powi(2)).sum();
        assert!(err / energy(&d) < 1e-3, "reconstruction error {}", err / energy(&d));
        // Click positions carry the drum stem; between clicks it is quiet.
        let click = (rate / 4) * 2 + 10;
        let between = (rate / 4) * 2 + rate / 8;
        let win = |a: &[f32], c: usize| a[c - 64..c + 64].iter().map(|x| x.abs()).fold(0.0f32, f32::max);
        assert!(win(&s.drums.l, click) > 0.3, "click in drums");
        assert!(win(&s.drums.l, between) < 0.1, "no tone in drums: {}", win(&s.drums.l, between));
        // The centred tone lands in the centre stem; the left-only tone in the harmonic (sides) stem.
        let rms = |a: &[f32]| (a[rate / 2..rate].iter().map(|x| x * x).sum::<f32>() / (rate / 2) as f32).sqrt();
        assert!(rms(&s.center.r) > 0.15, "centre tone: {}", rms(&s.center.r));
        assert!(rms(&s.harmonic.r) < 0.05, "right side has no off-centre tone: {}", rms(&s.harmonic.r));
        assert!(rms(&s.harmonic.l) > rms(&s.harmonic.r) * 3.0);
    }
}
