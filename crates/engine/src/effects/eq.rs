//! 7-band parametric EQ (bell, shelves, cuts, notch, band pass per band) with output gain.

use super::{Effect, EffectDef, ParamInfo};
use crate::dsp::spectrum::{Analyzer, BANDS};
use crate::dsp::{db_to_gain, Biquad, BiquadKind};

pub const EQ_BANDS: usize = 7;
const TYPES: &[&str] = &["Bell", "Low shelf", "High shelf", "Low pass", "High pass", "Band pass", "Notch"];

macro_rules! band {
    ($n:literal, $on:expr, $ty:expr, $f:expr) => {
        [
            ParamInfo::toggle(concat!("b", $n, "_on"), concat!("Band ", $n, " on"), $on),
            ParamInfo::choice(concat!("b", $n, "_type"), concat!("Band ", $n, " type"), TYPES, $ty),
            ParamInfo::log(concat!("b", $n, "_freq"), concat!("Band ", $n, " freq"), 20.0, 20000.0, $f, "Hz"),
            ParamInfo::lin(concat!("b", $n, "_gain"), concat!("Band ", $n, " gain"), -18.0, 18.0, 0.0, "dB"),
            ParamInfo::log(concat!("b", $n, "_q"), concat!("Band ", $n, " Q"), 0.1, 18.0, 0.9, ""),
        ]
    };
}

const fn flatten() -> [ParamInfo; EQ_BANDS * 5 + 1] {
    let bands: [[ParamInfo; 5]; EQ_BANDS] = [
        band!("1", false, 4.0, 30.0),
        band!("2", true, 1.0, 100.0),
        band!("3", true, 0.0, 300.0),
        band!("4", true, 0.0, 1000.0),
        band!("5", true, 0.0, 3000.0),
        band!("6", true, 2.0, 8000.0),
        band!("7", false, 3.0, 18000.0),
    ];
    let mut out = [ParamInfo::lin("output", "Output", -18.0, 18.0, 0.0, "dB"); EQ_BANDS * 5 + 1];
    let mut i = 0;
    while i < EQ_BANDS {
        let mut j = 0;
        while j < 5 {
            out[i * 5 + j] = bands[i][j];
            j += 1;
        }
        i += 1;
    }
    out
}

static PARAMS: [ParamInfo; EQ_BANDS * 5 + 1] = flatten();

#[derive(Debug, Clone, Copy)]
struct Band {
    on: bool,
    kind: BiquadKind,
    freq: f32,
    gain: f32,
    q: f32,
}

pub struct ParametricEq {
    rate: f32,
    bands: [Band; EQ_BANDS],
    filters: [[Biquad; 2]; EQ_BANDS],
    output: f32,
    dirty: bool,
    pre: Analyzer,
    post: Analyzer,
    /// Telemetry: the input spectrum, then the output spectrum ([`BANDS`] dB values each).
    tele: Vec<f32>,
}

impl ParametricEq {
    pub fn new(rate: f32) -> Self {
        let b = Band { on: false, kind: BiquadKind::Peak, freq: 1000.0, gain: 0.0, q: 0.9 };
        ParametricEq {
            rate,
            bands: [b; EQ_BANDS],
            filters: [[Biquad::default(); 2]; EQ_BANDS],
            output: 1.0,
            dirty: true,
            pre: Analyzer::new(rate),
            post: Analyzer::new(rate),
            tele: vec![crate::dsp::spectrum::FLOOR; BANDS * 2],
        }
    }

    fn update(&mut self) {
        for (i, b) in self.bands.iter().enumerate() {
            for f in &mut self.filters[i] {
                f.set(b.kind, b.freq, b.q, b.gain, self.rate);
            }
        }
        self.dirty = false;
    }

    /// Combined response in dB at a frequency, computed from a parameter map (for the UI curve).
    pub fn curve(params: &std::collections::BTreeMap<String, f32>, freq: f32, rate: f32) -> f32 {
        let d = def();
        let mut sum = 0.0;
        for n in 1..=EQ_BANDS {
            let get = |k: &str| super::value(&d, params, &format!("b{n}_{k}"));
            if get("on") < 0.5 {
                continue;
            }
            let b = Biquad::new(BiquadKind::from_index(get("type")), get("freq"), get("q"), get("gain"), rate);
            sum += b.response_db(freq, rate);
        }
        sum + super::value(&d, params, "output")
    }
}

impl Effect for ParametricEq {
    fn set_param(&mut self, id: &str, v: f32) {
        if id == "output" {
            self.output = db_to_gain(v);
            return;
        }
        let Some(rest) = id.strip_prefix('b') else { return };
        let Some((n, key)) = rest.split_once('_') else { return };
        let Ok(n) = n.parse::<usize>() else { return };
        if n == 0 || n > EQ_BANDS {
            return;
        }
        let b = &mut self.bands[n - 1];
        match key {
            "on" => b.on = v >= 0.5,
            "type" => b.kind = BiquadKind::from_index(v),
            "freq" => b.freq = v,
            "gain" => b.gain = v,
            "q" => b.q = v,
            _ => return,
        }
        self.dirty = true;
    }

    fn reset(&mut self) {
        for f in self.filters.iter_mut().flatten() {
            f.reset();
        }
        self.pre.reset();
        self.post.reset();
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        if self.dirty {
            self.update();
        }
        self.pre.push(l, r);
        for (i, b) in self.bands.iter().enumerate() {
            if !b.on {
                continue;
            }
            let [fl, fr] = &mut self.filters[i];
            for x in l.iter_mut() {
                *x = fl.process(*x);
            }
            for x in r.iter_mut() {
                *x = fr.process(*x);
            }
        }
        if (self.output - 1.0).abs() > 1e-6 {
            for x in l.iter_mut().chain(r.iter_mut()) {
                *x *= self.output;
            }
        }
        self.post.push(l, r);
        self.tele[..BANDS].copy_from_slice(&self.pre.bands);
        self.tele[BANDS..].copy_from_slice(&self.post.bands);
    }
    fn telemetry(&self) -> &[f32] {
        &self.tele
    }
}

pub fn def() -> EffectDef {
    EffectDef {
        id: "eq",
        name: "Parametric EQ",
        category: "EQ",
        description: "7-band parametric EQ with shelves, cuts and notch per band.",
        params: &PARAMS,
        uses_sidechain: false,
        create: |r| Box::new(ParametricEq::new(r)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curve_matches_band_gain() {
        let mut p = def().defaults();
        p.insert("b4_gain".into(), 6.0);
        let at = ParametricEq::curve(&p, 1000.0, 48000.0);
        assert!((at - 6.0).abs() < 0.3, "{at}");
    }
}
