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

/// One 2× stage (up and down filters with their own histories).
#[derive(Clone)]
struct Stage {
    h: [f32; TAPS],
    up: [f32; TAPS],
    up_pos: usize,
    down: [f32; TAPS],
    down_pos: usize,
}

impl Stage {
    fn new() -> Self {
        Stage { h: kernel(), up: [0.0; TAPS], up_pos: 0, down: [0.0; TAPS], down_pos: 0 }
    }
    fn fir(buf: &[f32; TAPS], pos: usize, h: &[f32; TAPS]) -> f32 {
        let mut acc = 0.0;
        for (k, c) in h.iter().enumerate() {
            acc += c * buf[(pos + TAPS - k) % TAPS];
        }
        acc
    }
    /// One input sample → two output samples (zero-stuffed and filtered, gain 2).
    fn up(&mut self, x: f32) -> [f32; 2] {
        let mut out = [0.0; 2];
        for (j, v) in [x, 0.0].into_iter().enumerate() {
            self.up_pos = (self.up_pos + 1) % TAPS;
            self.up[self.up_pos] = v;
            out[j] = 2.0 * Self::fir(&self.up, self.up_pos, &self.h);
        }
        out
    }
    /// Two input samples → one output sample.
    fn down(&mut self, x: [f32; 2]) -> f32 {
        let mut y = 0.0;
        for (j, v) in x.into_iter().enumerate() {
            self.down_pos = (self.down_pos + 1) % TAPS;
            self.down[self.down_pos] = v;
            if j == 1 {
                y = Self::fir(&self.down, self.down_pos, &self.h);
            }
        }
        y
    }
    fn reset(&mut self) {
        self.up = [0.0; TAPS];
        self.down = [0.0; TAPS];
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
