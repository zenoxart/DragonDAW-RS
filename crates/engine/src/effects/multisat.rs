//! "Dragon Multiband Saturator" (the FabFilter Saturn school): up to four bands split by Linkwitz–Riley
//! crossovers, each with its own saturation style, drive, dynamics (drive follows the band's level), tone,
//! level and mix; global mix and output. Per-band modulation comes from the effect modulation section.

use super::{Effect, EffectDef, ParamInfo};
use crate::dsp::{db_to_gain, Biquad, BiquadKind, Follower};

pub const SAT_STYLES: &[&str] = &["Clean tube", "Warm tube", "Tape", "Transformer", "Soft clip", "Hard clip", "Foldback", "Rectify"];
pub const BANDS: usize = 4;

macro_rules! band {
    ($n:literal) => {
        [
            ParamInfo::choice(concat!("b", $n, "_style"), concat!("Band ", $n, " style"), SAT_STYLES, 1.0),
            ParamInfo::lin(concat!("b", $n, "_drive"), concat!("Band ", $n, " drive"), 0.0, 36.0, 6.0, "dB"),
            ParamInfo::lin(concat!("b", $n, "_dyn"), concat!("Band ", $n, " dynamics"), -100.0, 100.0, 0.0, "%"),
            ParamInfo::lin(concat!("b", $n, "_tone"), concat!("Band ", $n, " tone"), -12.0, 12.0, 0.0, "dB"),
            ParamInfo::lin(concat!("b", $n, "_level"), concat!("Band ", $n, " level"), -24.0, 12.0, 0.0, "dB"),
            ParamInfo::lin(concat!("b", $n, "_mix"), concat!("Band ", $n, " mix"), 0.0, 100.0, 100.0, "%"),
        ]
    };
}
const PB: usize = 6;

const fn params() -> [ParamInfo; 4 + BANDS * PB] {
    let bands: [[ParamInfo; PB]; BANDS] = [band!("1"), band!("2"), band!("3"), band!("4")];
    let mut out = [ParamInfo::choice("bands", "Bands", &["1", "2", "3", "4"], 3.0); 4 + BANDS * PB];
    out[1] = ParamInfo::log("x1", "Crossover 1", 30.0, 1000.0, 150.0, "Hz");
    out[2] = ParamInfo::log("x2", "Crossover 2", 200.0, 6000.0, 1200.0, "Hz");
    out[3] = ParamInfo::log("x3", "Crossover 3", 1000.0, 16000.0, 6000.0, "Hz");
    let mut i = 0;
    while i < BANDS {
        let mut j = 0;
        while j < PB {
            out[4 + i * PB + j] = bands[i][j];
            j += 1;
        }
        i += 1;
    }
    out
}

static PARAMS: [ParamInfo; 4 + BANDS * PB] = params();
const EXTRA: &[ParamInfo] = &[ParamInfo::lin("mix", "Mix", 0.0, 100.0, 100.0, "%"), ParamInfo::lin("output", "Output", -24.0, 12.0, 0.0, "dB")];
static ALL: std::sync::OnceLock<Vec<ParamInfo>> = std::sync::OnceLock::new();

fn all_params() -> &'static [ParamInfo] {
    ALL.get_or_init(|| PARAMS.iter().chain(EXTRA.iter()).copied().collect())
}

/// Saturation curve of a style (the input already driven); output roughly level-matched.
pub fn shape(style: usize, x: f32) -> f32 {
    match style {
        0 => x.tanh(),
        // Warm: asymmetric (second harmonic); the DC it makes is removed after the shaper.
        1 => (x + 0.25 * x * x).tanh(),
        2 => (x * 0.8).tanh() * 1.1 / (1.0 + 0.08 * x.abs()),
        3 => x / (1.0 + x.abs()).sqrt() * (1.0 + 0.1 * (x * 3.0).sin() * (x.abs() * 0.3).min(0.3)),
        4 => {
            if x.abs() < 1.0 {
                1.5 * x - 0.5 * x * x * x
            } else {
                x.signum()
            }
        }
        5 => x.clamp(-1.0, 1.0),
        6 => (x * std::f32::consts::FRAC_PI_2).sin(),
        _ => 0.5 * (x.tanh() + x.abs().tanh() * 0.8),
    }
}

/// A Linkwitz–Riley 4th-order low / high pair at one frequency.
#[derive(Clone, Copy, Default)]
struct Lr4 {
    lp: [Biquad; 2],
    hp: [Biquad; 2],
}

impl Lr4 {
    fn new(f: f32, rate: f32) -> Self {
        let lp = Biquad::new(BiquadKind::LowPass, f, std::f32::consts::FRAC_1_SQRT_2, 0.0, rate);
        let hp = Biquad::new(BiquadKind::HighPass, f, std::f32::consts::FRAC_1_SQRT_2, 0.0, rate);
        Lr4 { lp: [lp; 2], hp: [hp; 2] }
    }
    fn split(&mut self, x: f32) -> (f32, f32) {
        let a = self.lp[0].process(x);
        let lo = self.lp[1].process(a);
        let b = self.hp[0].process(x);
        let hi = self.hp[1].process(b);
        (lo, hi)
    }
}

pub struct MultibandSaturator {
    rate: f32,
    p: Vec<f32>,
    xo: [[Lr4; 3]; 2],
    tone: [[Biquad; BANDS]; 2],
    env: [Follower; BANDS],
    dc: [[(f32, f32); BANDS]; 2],
    /// Telemetry: per band its level (dB) and its current drive (dB).
    tele: [f32; BANDS * 2],
}

impl MultibandSaturator {
    pub fn new(rate: f32) -> Self {
        let mut s = MultibandSaturator {
            rate,
            p: all_params().iter().map(|p| p.default).collect(),
            xo: [[Lr4::default(); 3]; 2],
            tone: [[Biquad::default(); BANDS]; 2],
            env: std::array::from_fn(|_| Follower::new(5.0, 120.0, rate)),
            dc: [[(0.0, 0.0); BANDS]; 2],
            tele: [0.0; BANDS * 2],
        };
        s.update();
        s
    }
    fn v(&self, id: &str) -> f32 {
        all_params().iter().position(|p| p.id == id).map(|k| self.p[k]).unwrap_or(0.0)
    }
    fn band(&self, b: usize, key: &str) -> f32 {
        self.p[4 + b * PB + ["style", "drive", "dyn", "tone", "level", "mix"].iter().position(|k| *k == key).unwrap_or(0)]
    }
    fn update(&mut self) {
        let r = self.rate;
        let xs = [self.v("x1"), self.v("x2").max(self.v("x1") * 1.2), self.v("x3").max(self.v("x2") * 1.2)];
        for ch in 0..2 {
            for k in 0..3 {
                self.xo[ch][k] = Lr4::new(xs[k].min(r * 0.45), r);
            }
            for b in 0..BANDS {
                // Tone: a tilt around the band's centre (high shelf).
                let centre = match b {
                    0 => xs[0] * 0.5,
                    1 => (xs[0] * xs[1]).sqrt(),
                    2 => (xs[1] * xs[2]).sqrt(),
                    _ => (xs[2] * 2.0).min(r * 0.4),
                };
                self.tone[ch][b] = Biquad::new(BiquadKind::HighShelf, centre.min(r * 0.45), 0.6, self.band(b, "tone"), r);
            }
        }
    }
}

impl Effect for MultibandSaturator {
    fn set_param(&mut self, id: &str, v: f32) {
        if let Some(k) = all_params().iter().position(|p| p.id == id) {
            self.p[k] = v;
            self.update();
        }
    }
    fn reset(&mut self) {
        self.dc = [[(0.0, 0.0); BANDS]; 2];
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let nb = (self.v("bands").round() as usize + 1).clamp(1, BANDS);
        let mix = self.v("mix") / 100.0;
        let out = db_to_gain(self.v("output"));
        let bp: Vec<(usize, f32, f32, f32, f32)> =
            (0..BANDS).map(|b| (self.band(b, "style").round() as usize, self.band(b, "drive"), self.band(b, "dyn") / 100.0, db_to_gain(self.band(b, "level")), self.band(b, "mix") / 100.0)).collect();
        let dc_a = 1.0 - std::f32::consts::TAU * 10.0 / self.rate;
        let mut lvl = [0.0f32; BANDS];
        let mut drv = [0.0f32; BANDS];
        for i in 0..l.len() {
            let x = [l[i], r[i]];
            let mut split = [[0.0f32; BANDS]; 2];
            for ch in 0..2 {
                // Split into nb bands with cascaded crossovers.
                let mut rest = x[ch];
                for k in 0..nb - 1 {
                    let (lo, hi) = self.xo[ch][k].split(rest);
                    split[ch][k] = lo;
                    rest = hi;
                }
                split[ch][nb - 1] = rest;
            }
            for b in 0..nb {
                let e = self.env[b].next(split[0][b].abs().max(split[1][b].abs()));
                let e_db = crate::dsp::gain_to_db(e.max(1e-6));
                lvl[b] = lvl[b].max(e_db);
                let (style, drive, dynamics, level, bmix) = bp[b];
                // Dynamics: drive rises (or falls) with the band's level, ±12 dB over a 36 dB range.
                let d = drive + dynamics * 12.0 * ((e_db + 36.0) / 36.0).clamp(0.0, 1.0);
                drv[b] = d;
                let g = db_to_gain(d);
                let comp = 1.0 / g.sqrt();
                for ch in 0..2 {
                    let dry = split[ch][b];
                    let y = shape(style, dry * g) * comp;
                    // DC blocker (asymmetric styles).
                    let (px, py) = self.dc[ch][b];
                    let yd = y - px + dc_a * py;
                    self.dc[ch][b] = (y, yd);
                    let y = self.tone[ch][b].process(yd);
                    split[ch][b] = (dry + (y - dry) * bmix) * level;
                }
            }
            let yl: f32 = split[0][..nb].iter().sum();
            let yr: f32 = split[1][..nb].iter().sum();
            l[i] = (x[0] + (yl - x[0]) * mix) * out;
            r[i] = (x[1] + (yr - x[1]) * mix) * out;
        }
        for b in 0..BANDS {
            self.tele[b] = if b < nb { lvl[b] } else { -90.0 };
            self.tele[BANDS + b] = drv[b];
        }
    }
    fn telemetry(&self) -> &[f32] {
        &self.tele
    }
}

pub fn multiband_saturator_def() -> EffectDef {
    EffectDef {
        id: "multisat",
        name: "Multiband Saturator",
        category: "Distortion",
        description: "Saturn-style multiband saturation: up to four bands (Linkwitz–Riley crossovers), each with a style (clean / warm tube, tape, transformer, soft / hard clip, foldback, rectify), drive, dynamics, tone, level and mix.",
        params: all_params(),
        uses_sidechain: false,
        create: |rate| Box::new(MultibandSaturator::new(rate)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(f: f32, a: f32) -> Vec<f32> {
        (0..48000).map(|i| (std::f32::consts::TAU * f * i as f32 / 48000.0).sin() * a).collect()
    }
    fn harmonic(y: &[f32], f: f32) -> f32 {
        let w = std::f32::consts::TAU * f / 48000.0;
        let (mut re, mut im) = (0.0f32, 0.0f32);
        for (i, v) in y.iter().enumerate() {
            re += v * (w * i as f32).cos();
            im += v * (w * i as f32).sin();
        }
        (re * re + im * im).sqrt() / y.len() as f32
    }

    /// Bands saturate independently: driving only the low band adds harmonics to a bass tone but leaves a
    /// treble tone clean; clean settings pass the signal (flat crossovers).
    #[test]
    fn bands_saturate_independently() {
        let mut e = MultibandSaturator::new(48000.0);
        for b in 1..=4 {
            e.set_param(&format!("b{b}_drive"), 0.0);
            e.set_param(&format!("b{b}_style"), 5.0);
        }
        let run = |e: &mut MultibandSaturator, x: &[f32]| {
            let (mut l, mut r) = (x.to_vec(), x.to_vec());
            e.process(&mut l, &mut r, None);
            l
        };
        let lo = tone(60.0, 0.5);
        let flat = run(&mut e, &lo);
        assert!((harmonic(&flat[24000..], 60.0) / harmonic(&lo[24000..], 60.0) - 1.0).abs() < 0.1, "flat when clean");
        e.set_param("b1_drive", 24.0);
        let y = run(&mut e, &lo);
        assert!(harmonic(&y[24000..], 180.0) > harmonic(&y[24000..], 60.0) * 0.05, "low band distorts");
        let hi = tone(4000.0, 0.5);
        let yh = run(&mut e, &hi);
        assert!(harmonic(&yh[24000..], 12000.0) < harmonic(&yh[24000..], 4000.0) * 0.01, "treble untouched");
    }
}
