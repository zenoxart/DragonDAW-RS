//! Soft Clipper: the sound of FL Studio's Fruity Soft Clipper (clean below the threshold, a smooth
//! tanh shoulder above it that never passes the ceiling, post gain) with a mastering clipper's extras:
//! input drive, shape (soft / hybrid / hard), up to 4× oversampling and a "clip only" listen.

use super::{Effect, EffectDef, ParamInfo};
use crate::dsp::oversample::Oversampler;
use crate::dsp::{db_to_gain, gain_to_db};

pub const CLIP_SHAPES: &[&str] = &["Soft", "Hybrid", "Hard"];
pub const CLIP_OVERSAMPLING: &[&str] = &["Off", "2×", "4×"];

const PARAMS: &[ParamInfo] = &[
    ParamInfo::lin("input", "Input", -12.0, 24.0, 0.0, "dB"),
    ParamInfo::lin("threshold", "Threshold", -24.0, 0.0, -6.0, "dB"),
    ParamInfo::lin("ceiling", "Ceiling", -12.0, 0.0, 0.0, "dB"),
    ParamInfo::choice("shape", "Shape", CLIP_SHAPES, 0.0),
    ParamInfo::choice("oversampling", "Oversampling", CLIP_OVERSAMPLING, 2.0),
    ParamInfo::lin("post", "Post gain", -24.0, 12.0, 0.0, "dB"),
    ParamInfo::toggle("listen", "Clip only", false),
];

/// The clipper's curve for one sample: linear up to `t`, then a shoulder towards `c` (both > 0, t ≤ c).
pub fn curve(x: f32, t: f32, c: f32, shape: usize) -> f32 {
    let a = x.abs();
    let y = if shape == 2 || c - t < 1e-6 {
        a.min(c)
    } else if a <= t {
        a
    } else {
        let u = (a - t) / (c - t);
        let s = if shape == 1 {
            // Hybrid: a harder knee (an algebraic sigmoid of order 4).
            u / (1.0 + u.powi(4)).powf(0.25)
        } else {
            u.tanh()
        };
        t + (c - t) * s
    };
    y.copysign(x)
}

pub struct SoftClipper {
    input: f32,
    threshold: f32,
    ceiling: f32,
    shape: usize,
    factor: usize,
    post: f32,
    listen: bool,
    os: [Oversampler; 2],
    /// Dry path delayed by the oversampler's latency (for "clip only").
    dry: [Vec<f32>; 2],
    dry_pos: [usize; 2],
    scratch: [f32; 4],
    /// Input peak, clipping (dB taken off), output peak — for the display.
    tele: [f32; 3],
    gr: f32,
}

impl SoftClipper {
    fn new() -> Self {
        let mut c = SoftClipper {
            input: 1.0,
            threshold: db_to_gain(-6.0),
            ceiling: 1.0,
            shape: 0,
            factor: 4,
            post: 1.0,
            listen: false,
            os: [Oversampler::new(4), Oversampler::new(4)],
            dry: [vec![0.0; 1], vec![0.0; 1]],
            dry_pos: [0; 2],
            scratch: [0.0; 4],
            tele: [-120.0, 0.0, -120.0],
            gr: 0.0,
        };
        c.set_factor(4);
        c
    }

    fn set_factor(&mut self, f: usize) {
        self.factor = f;
        self.os = [Oversampler::new(f), Oversampler::new(f)];
        let n = self.os[0].latency() + 1;
        self.dry = [vec![0.0; n], vec![0.0; n]];
        self.dry_pos = [0; 2];
    }
}

impl Effect for SoftClipper {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "input" => self.input = db_to_gain(v),
            "threshold" => self.threshold = db_to_gain(v),
            "ceiling" => self.ceiling = db_to_gain(v),
            "shape" => self.shape = (v.round() as usize).min(2),
            "oversampling" => {
                let f = [1, 2, 4][(v.round() as usize).min(2)];
                if f != self.factor {
                    self.set_factor(f);
                }
            }
            "post" => self.post = db_to_gain(v),
            "listen" => self.listen = v > 0.5,
            _ => {}
        }
    }

    fn reset(&mut self) {
        for o in &mut self.os {
            o.reset();
        }
        for d in &mut self.dry {
            d.fill(0.0);
        }
        self.tele = [-120.0, 0.0, -120.0];
        self.gr = 0.0;
    }

    fn latency(&self) -> usize {
        self.os[0].latency()
    }

    fn gain_reduction(&self) -> f32 {
        self.gr
    }

    fn telemetry(&self) -> &[f32] {
        &self.tele
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let t = self.threshold.min(self.ceiling);
        let (c, shape, f) = (self.ceiling, self.shape, self.factor);
        let (mut pin, mut pout) = (0.0f32, 0.0f32);
        let n = self.dry[0].len();
        for (ch, buf) in [l, r].into_iter().enumerate() {
            let mut pos = self.dry_pos[ch];
            for v in buf.iter_mut() {
                let x = *v * self.input;
                pin = pin.max(x.abs());
                let y = if f == 1 {
                    curve(x, t, c, shape)
                } else {
                    self.os[ch].up(x, &mut self.scratch[..f]);
                    for s in self.scratch[..f].iter_mut() {
                        *s = curve(*s, t, c, shape);
                    }
                    self.os[ch].down(&self.scratch[..f])
                };
                // The dry signal delayed like the oversampled path (a ring of latency + 1 samples).
                self.dry[ch][pos] = x;
                pos = (pos + 1) % n;
                let lined = self.dry[ch][pos];
                let out = if self.listen { (lined - y) * self.post } else { y * self.post };
                pout = pout.max(out.abs());
                *v = out;
            }
            self.dry_pos[ch] = pos;
        }
        // How much the loudest input sample was clipped by.
        let red = if pin > 1e-6 { (gain_to_db(pin) - gain_to_db(curve(pin, t, c, shape).max(1e-9))).max(0.0) } else { 0.0 };
        self.gr = red;
        self.tele = [gain_to_db(pin.max(1e-6)), red, gain_to_db(pout.max(1e-6))];
    }
}

pub fn soft_clipper_def() -> EffectDef {
    EffectDef {
        id: "clipper",
        name: "Soft Clipper",
        category: "Distortion",
        description: "Soft clipper with the sound of FL Studio's Fruity Soft Clipper: clean below the threshold, a smooth shoulder up to the ceiling, post gain; plus input drive, soft / hybrid / hard shapes, up to 4× oversampling and a clip-only listen.",
        params: PARAMS,
        uses_sidechain: false,
        create: |_| Box::new(SoftClipper::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_curve_is_clean_below_the_threshold_and_never_passes_the_ceiling() {
        let (t, c) = (0.5, 1.0);
        for shape in 0..3 {
            assert!((curve(0.3, t, c, shape) - 0.3).abs() < 1e-6);
            assert!((curve(-0.4, t, c, shape) + 0.4).abs() < 1e-6);
            for x in [0.6f32, 1.0, 2.0, 10.0, 100.0] {
                let y = curve(x, t, c, shape);
                assert!(y <= c + 1e-6 && y >= t, "shape {shape}: f({x}) = {y}");
                assert!(curve(-x, t, c, shape) == -y);
            }
        }
        // Soft is smooth: it keeps rising above the threshold.
        assert!(curve(0.8, t, c, 0) > curve(0.7, t, c, 0));
    }

    #[test]
    fn loud_input_is_held_under_the_ceiling() {
        for os in [0.0, 1.0, 2.0] {
            let mut fx = SoftClipper::new();
            fx.set_param("oversampling", os);
            fx.set_param("input", 18.0);
            fx.set_param("ceiling", -1.0);
            let mut l: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.05).sin()).collect();
            let mut r = l.clone();
            fx.process(&mut l, &mut r, None);
            let peak = l[200..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
            // Oversampling filters may ring a little over the ceiling.
            assert!(peak < db_to_gain(-1.0) * 1.12, "oversampling {os}: peak {peak}");
            assert!(fx.gain_reduction() > 6.0);
        }
    }

    #[test]
    fn clip_only_hears_just_what_was_taken_off() {
        let mut fx = SoftClipper::new();
        fx.set_param("oversampling", 0.0);
        fx.set_param("listen", 1.0);
        let mut l: Vec<f32> = (0..480).map(|i| 0.2 * (i as f32 * 0.05).sin()).collect();
        let mut r = l.clone();
        fx.process(&mut l, &mut r, None);
        assert!(l.iter().all(|v| v.abs() < 1e-6), "nothing is clipped below the threshold");
    }
}
