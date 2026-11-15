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

const EXTRA: usize = 3;

const fn flatten() -> [ParamInfo; EQ_BANDS * 5 + EXTRA] {
    let bands: [[ParamInfo; 5]; EQ_BANDS] = [
        band!("1", false, 4.0, 30.0),
        band!("2", true, 1.0, 100.0),
        band!("3", true, 0.0, 300.0),
        band!("4", true, 0.0, 1000.0),
        band!("5", true, 0.0, 3000.0),
        band!("6", true, 2.0, 8000.0),
        band!("7", false, 3.0, 18000.0),
    ];
    let mut out = [ParamInfo::lin("output", "Output", -18.0, 18.0, 0.0, "dB"); EQ_BANDS * 5 + EXTRA];
    out[EQ_BANDS * 5 + 1] = ParamInfo::toggle("auto_gain", "Auto gain", false);
    out[EQ_BANDS * 5 + 2] = ParamInfo::choice("solo", "Solo band (listen)", &["Off", "1", "2", "3", "4", "5", "6", "7"], 0.0);
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

static PARAMS: [ParamInfo; EQ_BANDS * 5 + EXTRA] = flatten();

/// Gain that keeps the loudness about the same: minus the curve's average boost over 50 Hz–12 kHz,
/// weighted towards the midrange (FabFilter-style auto gain).
fn auto_gain(curve: impl Fn(f32) -> f32) -> f32 {
    let (mut sum, mut wsum) = (0.0, 0.0);
    for k in 0..40 {
        let f = 50.0 * 240f32.powf(k as f32 / 39.0);
        let w = 1.0 / (1.0 + ((f / 1500.0).log2() / 2.5).powi(2));
        sum += curve(f) * w;
        wsum += w;
    }
    -(sum / wsum).clamp(-18.0, 18.0)
}

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
    auto_gain: bool,
    solo: usize,
    solo_f: [Biquad; 2],
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
            auto_gain: false,
            solo: 0,
            solo_f: [Biquad::default(); 2],
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
        if self.solo > 0 {
            let b = self.bands[self.solo - 1];
            for f in &mut self.solo_f {
                f.set(BiquadKind::BandPass, b.freq, b.q.max(0.3), 0.0, self.rate);
            }
        }
        self.dirty = false;
    }

    fn auto_gain_db(&self) -> f32 {
        auto_gain(|f| self.bands.iter().zip(&self.filters).filter(|(b, _)| b.on).map(|(_, fl)| fl[0].response_db(f, self.rate)).sum())
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

    /// The auto-gain compensation (dB) for a parameter map, for the UI.
    pub fn auto_gain_for(params: &std::collections::BTreeMap<String, f32>, rate: f32) -> f32 {
        let d = def();
        let filters: Vec<Biquad> = (1..=EQ_BANDS)
            .filter(|n| super::value(&d, params, &format!("b{n}_on")) >= 0.5)
            .map(|n| {
                let get = |k: &str| super::value(&d, params, &format!("b{n}_{k}"));
                Biquad::new(BiquadKind::from_index(get("type")), get("freq"), get("q"), get("gain"), rate)
            })
            .collect();
        auto_gain(|f| filters.iter().map(|b| b.response_db(f, rate)).sum())
    }
}

impl Effect for ParametricEq {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "output" => {
                self.output = db_to_gain(v);
                return;
            }
            "auto_gain" => {
                self.auto_gain = v >= 0.5;
                return;
            }
            "solo" => {
                self.solo = (v.round().max(0.0) as usize).min(EQ_BANDS);
                self.dirty = true;
                return;
            }
            _ => {}
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
        let out = self.output * if self.auto_gain { db_to_gain(self.auto_gain_db()) } else { 1.0 };
        if self.solo > 0 {
            // Listen to the soloed band's region only, like holding a band's headphones in Pro-Q.
            let [fl, fr] = &mut self.solo_f;
            for x in l.iter_mut() {
                *x = fl.process(*x);
            }
            for x in r.iter_mut() {
                *x = fr.process(*x);
            }
        }
        if (out - 1.0).abs() > 1e-6 {
            for x in l.iter_mut().chain(r.iter_mut()) {
                *x *= out;
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

    #[test]
    fn auto_gain_offsets_a_broad_boost_and_solo_isolates_the_band() {
        let mut p = def().defaults();
        p.insert("b4_gain".into(), 6.0);
        p.insert("b4_q".into(), 0.3);
        let g = ParametricEq::auto_gain_for(&p, 48000.0);
        assert!(g < -2.0 && g > -6.5, "{g}");
        // Solo band 4 (1 kHz): a 100 Hz tone nearly vanishes, a 1 kHz tone passes.
        let mut e = ParametricEq::new(48000.0);
        for (k, v) in &def().defaults() {
            e.set_param(k, *v);
        }
        e.set_param("solo", 4.0);
        let level = |e: &mut ParametricEq, f: f32| {
            let mut l: Vec<f32> = (0..48000).map(|i| (std::f32::consts::TAU * f * i as f32 / 48000.0).sin()).collect();
            let mut r = l.clone();
            e.process(&mut l, &mut r, None);
            l[24000..].iter().fold(0.0f32, |m, x| m.max(x.abs()))
        };
        assert!(level(&mut e, 1000.0) > 0.7);
        assert!(level(&mut e, 100.0) < 0.15);
    }
}
