//! Vacuum-tube stage models for the saturator and the channel strip: each tube type has its own transfer
//! curve (triode: asymmetric, mostly even harmonics; pentode: more symmetric, odd harmonics), grid
//! conduction on positive swings with a bias shift that recovers slowly ("blocking"), Miller-capacitance
//! high-frequency loss, and for power tubes supply sag (program-dependent gain drop).

/// Character of one tube type.
#[derive(Debug, Clone, Copy)]
pub struct TubeModel {
    pub name: &'static str,
    /// Gain into the curve (≈ amplification factor, scaled).
    pub k: f32,
    /// Operating-point offset: the larger, the more asymmetric (even harmonics).
    pub asym: f32,
    /// 0 = triode curve, 1 = pentode curve.
    pub pentode: f32,
    /// Supply sag amount (power tubes).
    pub sag: f32,
    /// Miller roll-off frequency (Hz).
    pub miller: f32,
    /// How fast grid conduction shifts the bias (0 … 1).
    pub blocking: f32,
}

pub const TUBES: &[TubeModel] = &[
    TubeModel { name: "12AX7 (high-mu triode)", k: 1.7, asym: 0.34, pentode: 0.0, sag: 0.0, miller: 9000.0, blocking: 0.6 },
    TubeModel { name: "12AT7 (medium-mu triode)", k: 1.25, asym: 0.26, pentode: 0.0, sag: 0.0, miller: 14000.0, blocking: 0.45 },
    TubeModel { name: "12AU7 (low-mu triode)", k: 0.85, asym: 0.18, pentode: 0.0, sag: 0.0, miller: 19000.0, blocking: 0.3 },
    TubeModel { name: "6SN7 (warm dual triode)", k: 0.95, asym: 0.24, pentode: 0.0, sag: 0.05, miller: 12000.0, blocking: 0.35 },
    TubeModel { name: "EF86 (small pentode)", k: 1.45, asym: 0.12, pentode: 0.6, sag: 0.0, miller: 15000.0, blocking: 0.4 },
    TubeModel { name: "EL34 (power pentode)", k: 1.25, asym: 0.06, pentode: 0.85, sag: 0.45, miller: 10000.0, blocking: 0.5 },
    TubeModel { name: "6L6 (beam tetrode)", k: 1.0, asym: 0.05, pentode: 0.7, sag: 0.3, miller: 13000.0, blocking: 0.4 },
    TubeModel { name: "KT88 (big beam tetrode)", k: 0.8, asym: 0.04, pentode: 0.6, sag: 0.15, miller: 16000.0, blocking: 0.3 },
    TubeModel { name: "300B (single-ended triode)", k: 1.05, asym: 0.45, pentode: 0.0, sag: 0.35, miller: 11000.0, blocking: 0.5 },
];

pub const TUBE_NAMES: &[&str] = &["12AX7", "12AT7", "12AU7", "6SN7", "EF86", "EL34", "6L6", "KT88", "300B"];

/// The static transfer curve of `t` with an extra `bias` (−1 … 1), for drawing and as the core of a stage.
pub fn tube_curve(t: &TubeModel, bias: f32, x: f32) -> f32 {
    let b = t.asym + bias * 0.4;
    // Triode: a soft odd curve plus a bounded even term (the plate curve's asymmetry): mostly 2nd
    // harmonic at moderate levels, grid conduction (harder positive knee) when driven hard.
    let tri = {
        let v = t.k * x;
        let odd = (v * 0.8).tanh() / 0.8;
        let even = b * (1.0 - (-v * v).exp());
        let grid = if v > 1.2 { -0.35 * ((v - 1.2) * 0.6).tanh() } else { 0.0 };
        (odd + even + grid) / t.k.max(0.3)
    };
    // Pentode: symmetric with a flatter top (odd harmonics).
    let pen = {
        let u = t.k * x;
        u / (1.0 + u.abs().powf(2.5)).powf(0.4) / t.k.max(0.3)
    };
    let y = tri * (1.0 - t.pentode) + pen * t.pentode;
    y * 1.15
}

/// A running tube stage (one channel).
#[derive(Debug, Clone, Copy)]
pub struct TubeStage {
    pub model: TubeModel,
    pub bias: f32,
    rate: f32,
    shift: f32,
    env: f32,
    lp: f32,
    lp_a: f32,
    /// DC blocker state (the even term shifts the average).
    dc_x: f32,
    dc_y: f32,
    /// The bias shift and the sag follow the level's peaks, held through a low-frequency wave's cycle:
    /// otherwise they ride the waveform of the bass and cancel its even harmonics.
    over_hold: super::PeakHold,
    sag_hold: super::PeakHold,
}

impl TubeStage {
    pub fn new(model: TubeModel, rate: f32) -> Self {
        let lp_a = (-std::f32::consts::TAU * model.miller / rate).exp();
        TubeStage { model, bias: 0.0, rate, shift: 0.0, env: 0.0, lp: 0.0, lp_a, dc_x: 0.0, dc_y: 0.0, over_hold: super::PeakHold::new(30.0, rate), sag_hold: super::PeakHold::new(30.0, rate) }
    }

    pub fn set_model(&mut self, m: TubeModel) {
        self.model = m;
        self.lp_a = (-std::f32::consts::TAU * m.miller / self.rate).exp();
    }

    pub fn reset(&mut self) {
        self.shift = 0.0;
        self.env = 0.0;
        self.lp = 0.0;
        self.dc_x = 0.0;
        self.dc_y = 0.0;
        self.over_hold.reset();
        self.sag_hold.reset();
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let m = self.model;
        // Miller capacitance: input high frequencies roll off.
        self.lp = x + (self.lp - x) * self.lp_a;
        let x = self.lp;
        // Grid conduction charges the coupling cap: positive peaks shift the bias down, recovering slowly.
        let over = self.over_hold.next((x * m.k + m.asym - 0.6).max(0.0));
        let attack = 0.002 * m.blocking;
        let release = 0.00003;
        self.shift += (over * 0.5 - self.shift) * if over * 0.5 > self.shift { attack } else { release };
        // Supply sag: sustained level pulls the gain down a little.
        let a = self.sag_hold.next(x);
        self.env += (a - self.env) * if a > self.env { 0.0005 } else { 0.00005 };
        let sag = 1.0 / (1.0 + m.sag * self.env * 1.5);
        let y = tube_curve(&m, self.bias - self.shift, x * sag);
        // Output coupling capacitor (≈ 5 Hz high-pass: blocks the DC, leaves the bass alone).
        let r = 1.0 - std::f32::consts::TAU * 5.0 / self.rate;
        self.dc_y = y - self.dc_x + r * self.dc_y;
        self.dc_x = y;
        self.dc_y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn harmonics(t: &TubeModel, drive: f32) -> (f32, f32, f32) {
        let rate = 48000.0;
        let f = 200.0;
        let mut s = TubeStage::new(*t, rate);
        let n = 48000;
        let y: Vec<f32> = (0..n).map(|i| s.process((std::f32::consts::TAU * f * i as f32 / rate).sin() * drive)).collect();
        let tail = &y[24000..];
        let mag = |h: f32| {
            let w = std::f32::consts::TAU * f * h / rate;
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for (i, v) in tail.iter().enumerate() {
                re += v * (w * i as f32).cos();
                im += v * (w * i as f32).sin();
            }
            (re * re + im * im).sqrt()
        };
        (mag(1.0), mag(2.0), mag(3.0))
    }

    /// Triodes make mostly 2nd harmonic, power pentodes mostly 3rd: the types sound different.
    #[test]
    fn triodes_are_even_pentodes_are_odd() {
        let (_, h2, h3) = harmonics(&TUBES[0], 0.4);
        assert!(h2 > h3, "12AX7: 2nd {h2} vs 3rd {h3}");
        let (_, h2, h3) = harmonics(&TUBES[5], 1.2);
        assert!(h3 > h2, "EL34: 3rd {h3} vs 2nd {h2}");
        // A low-mu triode is cleaner than a high-mu one at the same level.
        let (f1, a2, _) = harmonics(&TUBES[0], 0.5);
        let (g1, b2, _) = harmonics(&TUBES[2], 0.5);
        assert!(a2 / f1 > b2 / g1, "12AX7 dirtier than 12AU7");
    }

    #[test]
    fn stays_bounded_and_quiet_at_rest() {
        for t in TUBES {
            let mut s = TubeStage::new(*t, 48000.0);
            let mut last = 0.0;
            for _ in 0..4800 {
                last = s.process(0.0);
            }
            assert!(last.abs() < 1e-3, "{}: {last}", t.name);
            for i in 0..48000 {
                let y = s.process(((i as f32) * 0.01).sin() * 8.0);
                assert!(y.is_finite() && y.abs() < 4.0, "{}", t.name);
            }
        }
    }
}
