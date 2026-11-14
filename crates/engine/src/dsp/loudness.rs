//! Loudness metering after ITU-R BS.1770-4 / EBU R128: K-weighting, momentary (400 ms), short-term (3 s),
//! gated integrated loudness, and true peak (4× oversampling).

use super::Biquad;

/// K-weighting pre-filter (high shelf + RLB high-pass) for any sample rate.
#[derive(Debug, Clone)]
pub struct KWeighting {
    shelf: [Biquad; 2],
    hp: [Biquad; 2],
}

fn k_filters(rate: f32) -> (Biquad, Biquad) {
    // Stage 1: high shelf, +4 dB above ~1.5 kHz (coefficients derived like libebur128).
    let f0 = 1681.974_5_f64;
    let g = 3.999_843_853_973_347_f64;
    let q = 0.707_175_236_955_419_6_f64;
    let k = (std::f64::consts::PI * f0 / rate as f64).tan();
    let vh = 10f64.powf(g / 20.0);
    let vb = vh.powf(0.499_666_774_154_541_6);
    let a0 = 1.0 + k / q + k * k;
    let shelf = raw((vh + vb * k / q + k * k) / a0, 2.0 * (k * k - vh) / a0, (vh - vb * k / q + k * k) / a0, 2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0);
    // Stage 2: RLB high-pass at ~38 Hz.
    let f0 = 38.135_470_876_024_44_f64;
    let q = 0.500_327_037_323_877_3_f64;
    let k = (std::f64::consts::PI * f0 / rate as f64).tan();
    let a0 = 1.0 + k / q + k * k;
    let hp = raw(1.0, -2.0, 1.0, 2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0);
    (shelf, hp)
}

fn raw(b0: f64, b1: f64, b2: f64, a1: f64, a2: f64) -> Biquad {
    Biquad::from_coefficients(b0 as f32, b1 as f32, b2 as f32, a1 as f32, a2 as f32)
}

impl KWeighting {
    pub fn new(rate: f32) -> Self {
        let (s, h) = k_filters(rate);
        KWeighting { shelf: [s, s], hp: [h, h] }
    }
    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        (self.hp[0].process(self.shelf[0].process(l)), self.hp[1].process(self.shelf[1].process(r)))
    }
}

/// 4× oversampling true-peak detector (windowed-sinc polyphase interpolation, 48 taps).
#[derive(Debug, Clone)]
pub struct TruePeak {
    hist: [[f32; 12]; 2],
    pos: usize,
    taps: [[f32; 12]; 4],
}

impl Default for TruePeak {
    fn default() -> Self {
        let mut taps = [[0f32; 12]; 4];
        for (phase, row) in taps.iter_mut().enumerate() {
            for (i, t) in row.iter_mut().enumerate() {
                let x = (i as f32 - 6.0) + phase as f32 / 4.0;
                let sinc = if x.abs() < 1e-6 { 1.0 } else { (std::f32::consts::PI * x).sin() / (std::f32::consts::PI * x) };
                let w = 0.5 + 0.5 * (std::f32::consts::PI * x / 6.5).cos();
                *t = sinc * w;
            }
        }
        TruePeak { hist: [[0.0; 12]; 2], pos: 0, taps }
    }
}

impl TruePeak {
    /// Feeds one frame, returns the largest interpolated |sample|.
    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> f32 {
        self.pos = (self.pos + 1) % 12;
        self.hist[0][self.pos] = l;
        self.hist[1][self.pos] = r;
        let mut m = 0.0f32;
        for ch in 0..2 {
            for ph in &self.taps {
                let mut acc = 0.0;
                for (i, t) in ph.iter().enumerate() {
                    acc += self.hist[ch][(self.pos + 12 - i) % 12] * t;
                }
                m = m.max(acc.abs());
            }
        }
        m
    }
}

/// Running loudness meter for the master bus.
#[derive(Debug, Clone)]
pub struct LoudnessMeter {
    rate: f32,
    k: KWeighting,
    tp: TruePeak,
    /// 100 ms block energies.
    blocks: Vec<f64>,
    acc: f64,
    acc_n: usize,
    block_len: usize,
    pub true_peak: f32,
}

impl LoudnessMeter {
    pub fn new(rate: f32) -> Self {
        LoudnessMeter { rate, k: KWeighting::new(rate), tp: TruePeak::default(), blocks: Vec::new(), acc: 0.0, acc_n: 0, block_len: (rate * 0.1) as usize, true_peak: 0.0 }
    }

    pub fn reset(&mut self) {
        *self = LoudnessMeter::new(self.rate);
    }

    pub fn process(&mut self, l: &[f32], r: &[f32]) {
        for i in 0..l.len() {
            let (a, b) = self.k.process(l[i], r[i]);
            self.acc += (a * a + b * b) as f64;
            self.acc_n += 1;
            self.true_peak = self.true_peak.max(self.tp.process(l[i], r[i]));
            if self.acc_n >= self.block_len {
                self.blocks.push(self.acc / self.acc_n as f64);
                self.acc = 0.0;
                self.acc_n = 0;
                // Keep at most ~2 hours of blocks.
                if self.blocks.len() > 72_000 {
                    self.blocks.drain(..36_000);
                }
            }
        }
    }

    fn lufs(energy: f64) -> f32 {
        if energy <= 1e-12 {
            -70.0
        } else {
            (-0.691 + 10.0 * energy.log10()) as f32
        }
    }

    fn window(&self, blocks: usize) -> f32 {
        let n = self.blocks.len();
        if n == 0 {
            return -70.0;
        }
        let take = blocks.min(n);
        Self::lufs(self.blocks[n - take..].iter().sum::<f64>() / take as f64)
    }

    /// Momentary loudness (400 ms).
    pub fn momentary(&self) -> f32 {
        self.window(4)
    }

    /// Short-term loudness (3 s).
    pub fn short_term(&self) -> f32 {
        self.window(30)
    }

    /// Gated integrated loudness over everything measured.
    pub fn integrated(&self) -> f32 {
        integrated_from_blocks(&self.blocks)
    }
}

fn integrated_from_blocks(blocks: &[f64]) -> f32 {
    // 400 ms windows with 75 % overlap = 4 consecutive 100 ms blocks.
    if blocks.len() < 4 {
        return -70.0;
    }
    let windows: Vec<f64> = blocks.windows(4).map(|w| w.iter().sum::<f64>() / 4.0).collect();
    let abs: Vec<f64> = windows.iter().copied().filter(|&e| LoudnessMeter::lufs(e) > -70.0).collect();
    if abs.is_empty() {
        return -70.0;
    }
    let rel = LoudnessMeter::lufs(abs.iter().sum::<f64>() / abs.len() as f64) - 10.0;
    let gated: Vec<f64> = abs.into_iter().filter(|&e| LoudnessMeter::lufs(e) > rel).collect();
    if gated.is_empty() {
        return -70.0;
    }
    LoudnessMeter::lufs(gated.iter().sum::<f64>() / gated.len() as f64)
}

/// Offline measurement of a whole buffer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoudnessStats {
    pub integrated: f32,
    pub true_peak_db: f32,
    pub sample_peak_db: f32,
}

pub fn measure(l: &[f32], r: &[f32], rate: f32) -> LoudnessStats {
    let mut m = LoudnessMeter::new(rate);
    m.process(l, r);
    let peak = l.iter().chain(r.iter()).fold(0.0f32, |a, s| a.max(s.abs()));
    LoudnessStats { integrated: m.integrated(), true_peak_db: super::gain_to_db(m.true_peak.max(peak)), sample_peak_db: super::gain_to_db(peak) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_1k_minus_20_dbfs_is_about_minus_20_lufs() {
        // A 1 kHz sine at −20 dBFS on both channels reads about −20 LUFS.
        let rate = 48000.0;
        let a = super::super::db_to_gain(-20.0);
        let n = (rate * 5.0) as usize;
        let s: Vec<f32> = (0..n).map(|i| (i as f32 * 1000.0 * std::f32::consts::TAU / rate).sin() * a).collect();
        let st = measure(&s, &s, rate);
        assert!((st.integrated + 20.0).abs() < 0.6, "{}", st.integrated);
        assert!(st.true_peak_db < -19.5 && st.true_peak_db > -20.5, "{}", st.true_peak_db);
        let silent = vec![0.0f32; n];
        assert_eq!(measure(&silent, &silent, rate).integrated, -70.0);
    }
}
