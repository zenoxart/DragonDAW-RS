//! **Jadefang** – a character EQ: six voicings of the same four bands (LF shelf, LMF and HMF bells, HF
//! shelf), an input that drives each voicing's iron or valve stage, and the two famous extremes of a
//! very broad "open" EQ: a SUB band that lifts the lowest octaves without boom and an AIR band – a
//! boost-only shelf so wide that its 20 and 40 kHz settings still open up the audible top gently.
//!
//! The voicings (the `model` choice) change how the bands behave, not just their sound:
//! - **Clean scale**: transparent, constant Q, no saturation.
//! - **Hoard**: console iron – broad bells, shelves with a small bump, a transformer stage (even
//!   harmonics) driven by the input, a little weight at 50 Hz and a softened top.
//! - **Ember**: valve program EQ – shelves with the dip next to them that makes a boost sound bigger
//!   without getting muddy, warm valve harmonics.
//! - **Talon**: punchy console – proportional Q (the more you boost or cut, the narrower the band),
//!   steeper shelves, light odd harmonics.
//! - **Skyfire**: the open one – extra-broad bells and shelves, the AIR band a little stronger, clean.
//! - **Ancient**: vintage passive – broad and gentle, a soft top roll-off, soft saturation.
//!
//! [`response_db`] gives the curve the audio gets (tests use it).

use super::{Effect, EffectDef, ParamInfo};
use crate::dsp::{db_to_gain, Biquad, BiquadKind};

pub const MODELS: &[&str] = &["Clean scale", "Hoard", "Ember", "Talon", "Skyfire", "Ancient"];
pub const AIR_FREQS: &[&str] = &["2.5 kHz", "5 kHz", "10 kHz", "20 kHz", "40 kHz"];
const AIR_HZ: [f32; 5] = [2500.0, 5000.0, 10000.0, 20000.0, 40000.0];

static PARAMS: &[ParamInfo] = &[
    ParamInfo::choice("model", "Voicing", MODELS, 1.0),
    ParamInfo::lin("input", "Input", -24.0, 24.0, 0.0, "dB"),
    ParamInfo::lin("sub", "Sub", -10.0, 10.0, 0.0, "dB"),
    ParamInfo::lin("lf_gain", "LF gain", -12.0, 12.0, 0.0, "dB"),
    ParamInfo::log("lf_freq", "LF freq", 40.0, 400.0, 100.0, "Hz"),
    ParamInfo::lin("lmf_gain", "LMF gain", -12.0, 12.0, 0.0, "dB"),
    ParamInfo::log("lmf_freq", "LMF freq", 200.0, 2000.0, 500.0, "Hz"),
    ParamInfo::lin("hmf_gain", "HMF gain", -12.0, 12.0, 0.0, "dB"),
    ParamInfo::log("hmf_freq", "HMF freq", 1000.0, 8000.0, 3000.0, "Hz"),
    ParamInfo::lin("hf_gain", "HF gain", -12.0, 12.0, 0.0, "dB"),
    ParamInfo::log("hf_freq", "HF freq", 4000.0, 16000.0, 10000.0, "Hz"),
    ParamInfo::lin("air", "Air", 0.0, 15.0, 0.0, "dB"),
    ParamInfo::choice("air_freq", "Air freq", AIR_FREQS, 3.0),
    ParamInfo::lin("output", "Output", -12.0, 12.0, 0.0, "dB"),
];

pub fn def() -> EffectDef {
    EffectDef {
        id: "jadefang",
        name: "Jadefang (character EQ)",
        category: "EQ",
        description: "Six voicings of a four-band EQ with a driven input stage, a very broad Sub band and a boost-only Air band.",
        params: PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(Jadefang::new(rate)),
    }
}

/// The settings that shape the curve and the colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub model: usize,
    pub input: f32,
    pub sub: f32,
    pub lf: (f32, f32),
    pub lmf: (f32, f32),
    pub hmf: (f32, f32),
    pub hf: (f32, f32),
    pub air: f32,
    pub air_freq: usize,
    pub output: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { model: 1, input: 0.0, sub: 0.0, lf: (0.0, 100.0), lmf: (0.0, 500.0), hmf: (0.0, 3000.0), hf: (0.0, 10000.0), air: 0.0, air_freq: 3, output: 0.0 }
    }
}

impl Settings {
    pub fn from_params(get: impl Fn(&str) -> f32) -> Self {
        Settings {
            model: (get("model").round().max(0.0) as usize).min(MODELS.len() - 1),
            input: get("input"),
            sub: get("sub"),
            lf: (get("lf_gain"), get("lf_freq")),
            lmf: (get("lmf_gain"), get("lmf_freq")),
            hmf: (get("hmf_gain"), get("hmf_freq")),
            hf: (get("hf_gain"), get("hf_freq")),
            air: get("air").max(0.0),
            air_freq: (get("air_freq").round().max(0.0) as usize).min(AIR_HZ.len() - 1),
            output: get("output"),
        }
    }
}

/// How a voicing shapes the bands: bell Q, shelf Q, proportional Q, the shelf dips, the saturation
/// (amount, evenness) and its fixed colour bands.
struct Voicing {
    bell_q: f32,
    shelf_q: f32,
    proportional: bool,
    dips: bool,
    sat: f32,
    even: f32,
    air_k: f32,
    colour: &'static [(BiquadKind, f32, f32, f32)],
}

fn voicing(model: usize) -> Voicing {
    use BiquadKind::*;
    match model {
        1 => Voicing { bell_q: 0.7, shelf_q: 0.9, proportional: false, dips: false, sat: 0.3, even: 0.35, air_k: 1.0, colour: &[(Peak, 50.0, 0.7, 0.6), (HighShelf, 16000.0, 0.7, -0.6)] },
        2 => Voicing { bell_q: 0.8, shelf_q: 0.7, proportional: false, dips: true, sat: 0.35, even: 0.5, air_k: 1.0, colour: &[(Peak, 120.0, 0.6, 0.3)] },
        3 => Voicing { bell_q: 0.7, shelf_q: 1.2, proportional: true, dips: false, sat: 0.15, even: 0.0, air_k: 1.0, colour: &[] },
        4 => Voicing { bell_q: 0.45, shelf_q: 0.45, proportional: false, dips: false, sat: 0.0, even: 0.0, air_k: 1.2, colour: &[] },
        5 => Voicing { bell_q: 0.6, shelf_q: 0.6, proportional: false, dips: false, sat: 0.22, even: 0.25, air_k: 1.0, colour: &[(HighShelf, 14000.0, 0.6, -1.0)] },
        _ => Voicing { bell_q: 0.9, shelf_q: 0.7, proportional: false, dips: false, sat: 0.0, even: 0.0, air_k: 1.0, colour: &[] },
    }
}

/// Every filter of the curve: (kind, frequency, Q, gain dB). Bands at 0 dB are left out.
pub fn bands(s: &Settings, rate: f32) -> Vec<(BiquadKind, f32, f32, f32)> {
    use BiquadKind::*;
    let v = voicing(s.model);
    let nyq = rate * 0.45;
    let f = |x: f32| x.clamp(10.0, nyq);
    let bell_q = |g: f32| if v.proportional { v.bell_q + g.abs() / 12.0 * 1.6 } else { v.bell_q };
    let mut out = Vec::with_capacity(12);
    if s.sub.abs() > 0.01 {
        // A very broad shelf around 18 Hz: the lowest octaves move, the low mids don't.
        out.push((LowShelf, f(18.0), 0.5, s.sub));
    }
    if s.lf.0.abs() > 0.01 {
        out.push((LowShelf, f(s.lf.1), v.shelf_q, s.lf.0));
        if v.dips {
            out.push((Peak, f(s.lf.1 * 2.6), 1.0, -s.lf.0 * 0.35));
        }
    }
    if s.lmf.0.abs() > 0.01 {
        out.push((Peak, f(s.lmf.1), bell_q(s.lmf.0), s.lmf.0));
    }
    if s.hmf.0.abs() > 0.01 {
        out.push((Peak, f(s.hmf.1), bell_q(s.hmf.0), s.hmf.0));
    }
    if s.hf.0.abs() > 0.01 {
        out.push((HighShelf, f(s.hf.1), v.shelf_q, s.hf.0));
        if v.dips {
            out.push((Peak, f(s.hf.1 * 0.45), 1.0, -s.hf.0 * 0.3));
        }
    }
    if s.air > 0.01 {
        // Very broad: centred where it is set, even above what is heard (its slope reaches far down).
        let hz = AIR_HZ[s.air_freq];
        let centre = f(hz);
        let reach = if hz > nyq { (nyq / hz).sqrt() } else { 1.0 };
        out.push((HighShelf, centre * if hz > nyq { 0.55 } else { 1.0 }, 0.35, s.air * v.air_k * reach));
    }
    for &(k, hz, q, g) in v.colour {
        out.push((k, f(hz), q, g));
    }
    out
}

/// The EQ curve at `freq` (dB, input and output gain included, saturation not).
pub fn response_db(s: &Settings, rate: f32, freq: f32) -> f32 {
    bands(s, rate).into_iter().map(|(k, hz, q, g)| Biquad::new(k, hz, q, g, rate).response_db(freq, rate)).sum::<f32>() + s.output
}

const MAX_BANDS: usize = 12;

pub struct Jadefang {
    rate: f32,
    s: Settings,
    dirty: bool,
    filters: [[Biquad; MAX_BANDS]; 2],
    used: usize,
    /// DC blockers after the asymmetric stage.
    dc: [(f32, f32); 2],
    in_gain: f32,
    out_gain: f32,
    sat: f32,
    even: f32,
}

impl Jadefang {
    pub fn new(rate: f32) -> Self {
        let mut d = Jadefang { rate, s: Settings::default(), dirty: true, filters: Default::default(), used: 0, dc: [(0.0, 0.0); 2], in_gain: 1.0, out_gain: 1.0, sat: 0.0, even: 0.0 };
        d.update();
        d
    }

    fn update(&mut self) {
        let bands = bands(&self.s, self.rate);
        self.used = bands.len().min(MAX_BANDS);
        for (k, &(kind, hz, q, g)) in bands.iter().take(MAX_BANDS).enumerate() {
            for ch in 0..2 {
                let fl = &mut self.filters[ch][k];
                fl.set(kind, hz, q, g, self.rate);
            }
        }
        let v = voicing(self.s.model);
        self.in_gain = db_to_gain(self.s.input);
        self.sat = v.sat;
        self.even = v.even;
        // The driven stage adds level; the output undoes the input so the voicing is judged, not loudness.
        self.out_gain = db_to_gain(self.s.output - self.s.input * if v.sat > 0.0 { 0.85 } else { 1.0 });
        self.dirty = false;
    }

    #[inline]
    fn stage(&self, x: f32) -> f32 {
        if self.sat <= 0.0 {
            return x;
        }
        // Soft saturation with some even (asymmetric) content; level-matched for small signals.
        let shaped = (x + self.even * 0.3 * x * x).tanh();
        x + self.sat * (shaped - x)
    }
}

impl Effect for Jadefang {
    fn set_param(&mut self, id: &str, value: f32) {
        let s = &mut self.s;
        match id {
            "model" => s.model = (value.round().max(0.0) as usize).min(MODELS.len() - 1),
            "input" => s.input = value,
            "sub" => s.sub = value,
            "lf_gain" => s.lf.0 = value,
            "lf_freq" => s.lf.1 = value,
            "lmf_gain" => s.lmf.0 = value,
            "lmf_freq" => s.lmf.1 = value,
            "hmf_gain" => s.hmf.0 = value,
            "hmf_freq" => s.hmf.1 = value,
            "hf_gain" => s.hf.0 = value,
            "hf_freq" => s.hf.1 = value,
            "air" => s.air = value.max(0.0),
            "air_freq" => s.air_freq = (value.round().max(0.0) as usize).min(AIR_HZ.len() - 1),
            "output" => s.output = value,
            _ => return,
        }
        self.dirty = true;
    }

    fn reset(&mut self) {
        for ch in &mut self.filters {
            for f in ch.iter_mut() {
                f.reset();
            }
        }
        self.dc = [(0.0, 0.0); 2];
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sidechain: Option<(&[f32], &[f32])>) {
        if self.dirty {
            self.update();
        }
        let r_dc = 1.0 - 2.0 * std::f32::consts::PI * 8.0 / self.rate;
        for (ch, buf) in [l, r].into_iter().enumerate() {
            for x in buf.iter_mut() {
                let mut y = self.stage(*x * self.in_gain);
                if self.even > 0.0 && self.sat > 0.0 {
                    let (px, py) = self.dc[ch];
                    let o = y - px + r_dc * py;
                    self.dc[ch] = (y, o);
                    y = o;
                }
                for f in self.filters[ch][..self.used].iter_mut() {
                    y = f.process(y);
                }
                *x = y * self.out_gain;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone_gain(e: &mut Jadefang, hz: f32) -> f32 {
        let rate = 48000.0;
        let n = 9600;
        let mut l: Vec<f32> = (0..n).map(|i| (i as f32 * hz * std::f32::consts::TAU / rate).sin() * 0.05).collect();
        let mut r = l.clone();
        e.process(&mut l, &mut r, None);
        let rms = |v: &[f32]| (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt();
        20.0 * (rms(&l[4800..]) / (0.05 / 2f32.sqrt())).log10()
    }

    #[test]
    fn flat_at_zero_and_the_curve_matches_the_audio() {
        let mut e = Jadefang::new(48000.0);
        e.set_param("model", 0.0);
        assert!(tone_gain(&mut e, 1000.0).abs() < 0.1, "Clean scale at 0 dB is flat");
        e.set_param("lmf_gain", 9.0);
        e.set_param("lmf_freq", 1000.0);
        let got = tone_gain(&mut e, 1000.0);
        let want = response_db(&e.s, 48000.0, 1000.0);
        assert!((got - want).abs() < 0.3 && (want - 9.0).abs() < 0.3, "{got} vs {want}");
    }

    #[test]
    fn proportional_q_narrows_big_boosts() {
        let s = |g: f32| Settings { model: 3, lmf: (g, 1000.0), ..Settings::default() };
        // A small boost spreads further (relative to its peak) than a big one.
        let spread = |g: f32| response_db(&s(g), 48000.0, 2500.0) / g;
        assert!(spread(3.0) > spread(12.0) + 0.05, "{} vs {}", spread(3.0), spread(12.0));
    }

    #[test]
    fn ember_shelves_dip_next_to_a_boost() {
        let mut s = Settings { model: 2, lf: (8.0, 60.0), ..Settings::default() };
        let dip_ember = response_db(&s, 48000.0, 160.0);
        s.model = 0;
        let dip_clean = response_db(&s, 48000.0, 160.0);
        assert!(dip_ember < dip_clean - 1.0, "{dip_ember} vs {dip_clean}");
    }

    #[test]
    fn air_is_broad_and_still_audible_at_40k() {
        let s = Settings { model: 0, air: 10.0, air_freq: 4, ..Settings::default() };
        let at = |f| response_db(&s, 48000.0, f);
        assert!(at(12000.0) > 2.0, "the 40 kHz air still opens the audible top: {}", at(12000.0));
        assert!(at(500.0) < 0.5, "and leaves the mids: {}", at(500.0));
        let s5 = Settings { air_freq: 1, ..s };
        assert!(response_db(&s5, 48000.0, 2500.0) > 1.5, "5 kHz air reaches down into the presence range");
    }

    #[test]
    fn input_drives_the_voicing_but_level_stays_close() {
        let mut e = Jadefang::new(48000.0);
        e.set_param("model", 1.0);
        e.set_param("input", 12.0);
        let g = tone_gain(&mut e, 1000.0);
        assert!(g.abs() < 3.0, "the output follows the input back: {g}");
    }
}
