//! Real-time spectrum analyzer for effect displays (FabFilter-style analyzer behind an EQ curve).
//!
//! Collects a mono sum, runs a Hann-windowed FFT every [`HOP`] samples and folds the bins into
//! [`BANDS`] log-spaced bands from 20 Hz to 20 kHz, in dB, with a fast attack and a slow fall.

use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::sync::Arc;

/// Number of log-spaced bands reported.
pub const BANDS: usize = 96;
/// The lowest reading (dB); silent bands sit here.
pub const FLOOR: f32 = -90.0;
const SIZE: usize = 4096;
const HOP: usize = 1024;

pub struct Analyzer {
    rate: f32,
    fft: Arc<dyn Fft<f32>>,
    ring: Vec<f32>,
    pos: usize,
    since: usize,
    window: Vec<f32>,
    buf: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    /// Smoothed band levels in dB (length [`BANDS`]).
    pub bands: [f32; BANDS],
}

/// Center frequency of a band.
pub fn band_freq(b: usize) -> f32 {
    20.0 * 1000f32.powf((b as f32 + 0.5) / BANDS as f32)
}

impl Analyzer {
    pub fn new(rate: f32) -> Self {
        let fft = FftPlanner::new().plan_fft_forward(SIZE);
        let scratch = vec![Complex::default(); fft.get_inplace_scratch_len()];
        let window = (0..SIZE).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / SIZE as f32).cos()).collect();
        Analyzer { rate, fft, ring: vec![0.0; SIZE], pos: 0, since: 0, window, buf: vec![Complex::default(); SIZE], scratch, bands: [FLOOR; BANDS] }
    }

    /// Feeds a stereo block (summed to mono).
    pub fn push(&mut self, l: &[f32], r: &[f32]) {
        for (a, b) in l.iter().zip(r) {
            self.ring[self.pos] = 0.5 * (a + b);
            self.pos = (self.pos + 1) % SIZE;
            self.since += 1;
            if self.since >= HOP {
                self.since = 0;
                self.analyze();
            }
        }
    }

    fn analyze(&mut self) {
        for i in 0..SIZE {
            self.buf[i] = Complex::new(self.ring[(self.pos + i) % SIZE] * self.window[i], 0.0);
        }
        self.fft.process_with_scratch(&mut self.buf, &mut self.scratch);
        let bin_hz = self.rate / SIZE as f32;
        // Hann gain 0.5, one-sided spectrum: a full-scale sine reads ~0 dB.
        let norm = 4.0 / SIZE as f32;
        let fall = 2.5; // dB per analysis (~ 115 dB/s at 48 kHz)
        for b in 0..BANDS {
            let lo = 20.0 * 1000f32.powf(b as f32 / BANDS as f32);
            let hi = 20.0 * 1000f32.powf((b + 1) as f32 / BANDS as f32);
            let (i0, i1) = ((lo / bin_hz).floor() as usize, ((hi / bin_hz).ceil() as usize).max((lo / bin_hz).floor() as usize + 1));
            let mut peak = 0.0f32;
            for i in i0.max(1)..i1.min(SIZE / 2) {
                peak = peak.max(self.buf[i].norm());
            }
            if i0 >= SIZE / 2 {
                peak = 0.0;
            }
            // Tilt +4.5 dB/oct around 1 kHz so music reads flat, like most analyzers.
            let tilt = 4.5 * (band_freq(b) / 1000.0).log2();
            let db = (20.0 * (peak * norm).max(1e-9).log10() + tilt).max(FLOOR);
            let cur = &mut self.bands[b];
            *cur = if db > *cur { db } else { (*cur - fall).max(db) };
        }
    }

    pub fn reset(&mut self) {
        self.ring.fill(0.0);
        self.bands = [FLOOR; BANDS];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sine_peaks_in_its_band() {
        let rate = 48000.0;
        let mut a = Analyzer::new(rate);
        let x: Vec<f32> = (0..SIZE * 2).map(|i| (std::f32::consts::TAU * 1000.0 * i as f32 / rate).sin() * 0.5).collect();
        a.push(&x, &x);
        let top = (0..BANDS).max_by(|p, q| a.bands[*p].total_cmp(&a.bands[*q])).unwrap();
        let f = band_freq(top);
        assert!((900.0..1120.0).contains(&f), "peak at {f} Hz");
        assert!((a.bands[top] + 6.0).abs() < 3.0, "level {}", a.bands[top]);
        assert!(a.bands[5] < a.bands[top] - 40.0);
    }
}
