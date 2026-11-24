//! 2× / 4× oversampling with linear-phase half-band FIR stages (for the limiter).

const M: usize = 23; // taps = 2M + 1
const TAPS: usize = 2 * M + 1;

fn kernel() -> [f32; TAPS] {
    let mut h = [0.0f32; TAPS];
    let mut sum = 0.0;
    for (k, v) in h.iter_mut().enumerate() {
        let n = k as f32 - M as f32;
        let sinc = if n == 0.0 { 1.0 } else { (std::f32::consts::PI * n * 0.5).sin() / (std::f32::consts::PI * n * 0.5) };
        // Blackman window.
        let w = 0.42 - 0.5 * (std::f32::consts::TAU * k as f32 / (TAPS - 1) as f32).cos() + 0.08 * (2.0 * std::f32::consts::TAU * k as f32 / (TAPS - 1) as f32).cos();
        *v = 0.5 * sinc * w;
        sum += *v;
    }
    for v in h.iter_mut() {
        *v /= sum;
    }
    h
}

/// Taps of one polyphase branch: the half-band kernel's non-zero taps besides the centre (every other
/// one – a half-band filter is zero at every even offset from its centre).
const L: usize = M + 1;

/// One 2× stage (up and down filters with their own histories), polyphase: the half-band kernel is zero
/// at every even offset from its centre and the zero-stuffed samples contribute nothing, so of the
/// 2M + 1 taps each output needs only the L = M + 1 odd-offset ones (or just the centre). Same filter,
/// a quarter of the work, and no modulo per tap: the histories are written twice so the last L samples
/// are always one contiguous slice.
#[derive(Clone)]
struct Stage {
    /// h at the odd offsets from the centre (k = 0, 2, 4 … 2M), newest-first order.
    odd: [f32; L],
    /// The centre tap.
    centre: f32,
    up: [f32; 2 * L],
    up_pos: usize,
    /// Down: the second samples of each pair (they meet the odd taps) and the first ones (the centre).
    down1: [f32; 2 * L],
    down0: [f32; 2 * L],
    down_pos: usize,
}

impl Stage {
    fn new() -> Self {
        let h = kernel();
        let mut odd = [0.0; L];
        for (j, v) in odd.iter_mut().enumerate() {
            *v = h[2 * j];
        }
        Stage { odd, centre: h[M], up: [0.0; 2 * L], up_pos: 0, down1: [0.0; 2 * L], down0: [0.0; 2 * L], down_pos: 0 }
    }
    /// Pushes `x` into a mirrored history at `pos` (moving backwards): `buf[pos..pos + L]` is then the
    /// last L samples, newest first.
    fn push(buf: &mut [f32; 2 * L], pos: &mut usize, x: f32) {
        *pos = if *pos == 0 { L - 1 } else { *pos - 1 };
        buf[*pos] = x;
        buf[*pos + L] = x;
    }
    fn dot(a: &[f32], b: &[f32; L]) -> f32 {
        // Four partial sums (vectorises; the order differs from a plain sum only in rounding).
        let mut acc = [0.0f32; 4];
        for (x, h) in a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0) {
            for k in 0..4 {
                acc[k] += x[k] * h[k];
            }
        }
        // L is a multiple of four (checked at compile time), so there is no remainder.
        const _: () = assert!(L.is_multiple_of(4));
        acc[0] + acc[1] + acc[2] + acc[3]
    }
    /// One input sample → two output samples (zero-stuffed and filtered, gain 2).
    fn up(&mut self, x: f32) -> [f32; 2] {
        Self::push(&mut self.up, &mut self.up_pos, x);
        let p = self.up_pos;
        // The sample that sits at the kernel's centre for the second output: (M − 1) / 2 back.
        [2.0 * Self::dot(&self.up[p..p + L], &self.odd), 2.0 * self.centre * self.up[p + (M - 1) / 2]]
    }
    /// Two input samples → one output sample.
    fn down(&mut self, x: [f32; 2]) -> f32 {
        let mut p = self.down_pos;
        Self::push(&mut self.down0, &mut p, x[0]);
        let mut q = self.down_pos;
        Self::push(&mut self.down1, &mut q, x[1]);
        self.down_pos = q;
        Self::dot(&self.down1[q..q + L], &self.odd) + self.centre * self.down0[q + (M - 1) / 2]
    }
    fn reset(&mut self) {
        self.up = [0.0; 2 * L];
        self.down0 = [0.0; 2 * L];
        self.down1 = [0.0; 2 * L];
    }
}

/// An oversampler of factor 1, 2 or 4 for one channel.
#[derive(Clone)]
pub struct Oversampler {
    pub factor: usize,
    stages: Vec<Stage>,
}

impl Oversampler {
    pub fn new(factor: usize) -> Self {
        let n = match factor {
            4 => 2,
            2 => 1,
            _ => 0,
        };
        Oversampler { factor: 1 << n, stages: (0..n).map(|_| Stage::new()).collect() }
    }
    /// Delay of an up + down round trip, in base-rate samples.
    pub fn latency(&self) -> usize {
        // Each stage's up and down filter delays M samples at its own rate.
        let mut d = 0.0;
        for k in 0..self.stages.len() {
            d += 2.0 * M as f32 / (2u32 << k) as f32;
        }
        d.round() as usize
    }
    /// Upsamples one sample into `out` (`factor` samples).
    pub fn up(&mut self, x: f32, out: &mut [f32]) {
        match self.stages.len() {
            0 => out[0] = x,
            1 => out[..2].copy_from_slice(&self.stages[0].up(x)),
            _ => {
                let a = self.stages[0].up(x);
                let b = self.stages[1].up(a[0]);
                let c = self.stages[1].up(a[1]);
                out[..4].copy_from_slice(&[b[0], b[1], c[0], c[1]]);
            }
        }
    }
    /// Downsamples `factor` samples into one.
    pub fn down(&mut self, x: &[f32]) -> f32 {
        match self.stages.len() {
            0 => x[0],
            1 => self.stages[0].down([x[0], x[1]]),
            _ => {
                let a = self.stages[1].down([x[0], x[1]]);
                let b = self.stages[1].down([x[2], x[3]]);
                self.stages[0].down([a, b])
            }
        }
    }
    pub fn reset(&mut self) {
        self.stages.iter_mut().for_each(Stage::reset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The polyphase stage is the same filter as the plain zero-stuffed FIR it replaced (up to rounding).
    #[test]
    fn polyphase_equals_the_plain_fir() {
        let h = kernel();
        let fir = |buf: &[f32], t: usize| -> f32 { (0..TAPS).map(|k| if t >= k { h[k] * buf[t - k] } else { 0.0 }).sum() };
        let mut seed = 5u32;
        let x: Vec<f32> = (0..600)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                (seed as f32 / u32::MAX as f32) * 2.0 - 1.0
            })
            .collect();
        // Up: zero-stuffed stream filtered, gain 2.
        let stuffed: Vec<f32> = x.iter().flat_map(|v| [*v, 0.0]).collect();
        let mut st = Stage::new();
        for (m, v) in x.iter().enumerate() {
            let y = st.up(*v);
            for j in 0..2 {
                let want = 2.0 * fir(&stuffed, 2 * m + j);
                assert!((y[j] - want).abs() < 1e-5, "up {m}/{j}: {} vs {want}", y[j]);
            }
        }
        // Down: every second output of the filtered stream.
        let mut st = Stage::new();
        for m in 0..300 {
            let y = st.down([x[2 * m], x[2 * m + 1]]);
            let want = fir(&x, 2 * m + 1);
            assert!((y - want).abs() < 1e-5, "down {m}: {y} vs {want}");
        }
    }

    /// A tone survives the round trip (delayed by the reported latency); content above the base Nyquist
    /// created at the high rate is removed on the way down.
    #[test]
    fn round_trip_keeps_the_signal() {
        for f in [2usize, 4] {
            let mut o = Oversampler::new(f);
            let x: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
            let mut buf = [0.0f32; 4];
            let y: Vec<f32> = x
                .iter()
                .map(|v| {
                    o.up(*v, &mut buf);
                    o.down(&buf[..f])
                })
                .collect();
            let lat = o.latency();
            let err = (1000..4000).map(|i| (y[i] - x[i - lat]).abs()).fold(0.0f32, f32::max);
            // (4×: the delay is 34.5 samples, so half a sample of the error is the rounding.)
            assert!(err < 0.04, "factor {f}: latency {lat}, error {err}");
        }
    }
}
