//! Offline time-stretching (WSOLA – waveform-similarity overlap-add) and pitch shifting
//! (stretch + resample). Used for audio clips in *Stretch* mode, tempo warping and the clip editor.

use crate::samples::AudioData;

/// Stretches audio by `factor` (2.0 = twice as long, same pitch).
pub fn time_stretch(data: &AudioData, factor: f64) -> AudioData {
    let factor = factor.clamp(0.1, 10.0);
    if (factor - 1.0).abs() < 1e-4 || data.frames() < 4096 {
        if (factor - 1.0).abs() < 1e-4 {
            return data.clone();
        }
        return varispeed(data, 1.0 / factor);
    }
    let rate = data.rate as f64;
    let win = ((rate * 0.04) as usize).max(256) & !1; // 40 ms
    let hop_out = win / 2;
    let hop_in = hop_out as f64 / factor;
    let tolerance = (rate * 0.01) as isize; // ±10 ms search
    let window: Vec<f32> = (0..win).map(|i| (std::f32::consts::PI * i as f32 / win as f32).sin().powi(2)).collect();
    let n_in = data.frames();
    let n_out = (n_in as f64 * factor) as usize + win;
    let mut l = vec![0.0f32; n_out];
    let mut r = vec![0.0f32; n_out];
    let mut norm = vec![0.0f32; n_out];
    let mono: Vec<f32> = data.l.iter().zip(&data.r).map(|(a, b)| (a + b) * 0.5).collect();
    let mut prev_pos: isize = 0;
    let mut out_pos = 0usize;
    let mut k = 0usize;
    while out_pos + win < n_out {
        let nominal = (k as f64 * hop_in) as isize;
        if nominal as usize + win >= n_in {
            break;
        }
        // Find the offset whose start best continues the previous segment's natural continuation.
        let target = prev_pos + hop_out as isize;
        let mut best = nominal;
        if k > 0 {
            let mut best_score = f32::MIN;
            let step = 4;
            let mut d = -tolerance;
            while d <= tolerance {
                let cand = nominal + d;
                if cand >= 0 && (cand as usize) + win < n_in && target >= 0 && (target as usize) + hop_out < n_in {
                    let mut score = 0.0f32;
                    let mut i = 0;
                    while i < hop_out {
                        score += mono[cand as usize + i] * mono[target as usize + i];
                        i += 8;
                    }
                    if score > best_score {
                        best_score = score;
                        best = cand;
                    }
                }
                d += step;
            }
        }
        let start = best.max(0) as usize;
        for i in 0..win {
            let w = window[i];
            l[out_pos + i] += data.l[start + i] * w;
            r[out_pos + i] += data.r[start + i] * w;
            norm[out_pos + i] += w;
        }
        prev_pos = best;
        out_pos += hop_out;
        k += 1;
    }
    let len = ((n_in as f64 * factor) as usize).min(n_out);
    for i in 0..len {
        if norm[i] > 1e-3 {
            l[i] /= norm[i];
            r[i] /= norm[i];
        }
    }
    l.truncate(len);
    r.truncate(len);
    AudioData { rate: data.rate, l, r, source_channels: data.source_channels, source_rate: data.source_rate }
}

/// Changes speed and pitch together (tape style). `rate` 2.0 = twice as fast, one octave up.
pub fn varispeed(data: &AudioData, rate: f64) -> AudioData {
    let rate = rate.clamp(0.05, 20.0);
    let n = (data.frames() as f64 / rate) as usize;
    let mut l = Vec::with_capacity(n);
    let mut r = Vec::with_capacity(n);
    for i in 0..n {
        let (a, b) = data.sample_at(i as f64 * rate);
        l.push(a);
        r.push(b);
    }
    AudioData { rate: data.rate, l, r, source_channels: data.source_channels, source_rate: data.source_rate }
}

/// Shifts pitch by `semitones` keeping the length.
pub fn pitch_shift(data: &AudioData, semitones: f64) -> AudioData {
    if semitones.abs() < 1e-3 {
        return data.clone();
    }
    let ratio = 2f64.powf(semitones / 12.0);
    let stretched = time_stretch(data, ratio);
    let mut out = varispeed(&stretched, ratio);
    out.l.truncate(data.frames());
    out.r.truncate(data.frames());
    out
}

/// Reverses audio.
pub fn reverse(data: &AudioData) -> AudioData {
    let mut d = data.clone();
    d.l.reverse();
    d.r.reverse();
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, secs: f32, rate: u32) -> AudioData {
        let n = (secs * rate as f32) as usize;
        let s: Vec<f32> = (0..n).map(|i| (i as f32 * freq * std::f32::consts::TAU / rate as f32).sin() * 0.5).collect();
        AudioData::from_planar(rate, s.clone(), s)
    }

    fn zero_crossings(v: &[f32]) -> usize {
        v.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count()
    }

    #[test]
    fn stretch_keeps_pitch_and_changes_length() {
        let d = sine(440.0, 2.0, 44100);
        let s = time_stretch(&d, 1.5);
        assert!((s.frames() as f64 - d.frames() as f64 * 1.5).abs() < 10.0);
        // Same frequency → crossings per second unchanged (±3 %).
        let f_in = zero_crossings(&d.l) as f64 / d.duration();
        let f_out = zero_crossings(&s.l[2000..s.frames() - 2000]) as f64 / ((s.frames() - 4000) as f64 / 44100.0);
        assert!((f_out / f_in - 1.0).abs() < 0.03, "{f_in} {f_out}");
    }

    #[test]
    fn pitch_shift_octave_doubles_frequency() {
        let d = sine(220.0, 2.0, 44100);
        let s = pitch_shift(&d, 12.0);
        assert_eq!(s.frames(), d.frames());
        let f = zero_crossings(&s.l[4000..80000]) as f64 / (76000.0 / 44100.0);
        assert!((f / 440.0 - 1.0).abs() < 0.04, "{f}");
    }
}
