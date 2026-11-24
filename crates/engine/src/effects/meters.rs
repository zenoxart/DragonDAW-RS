//! Measuring inserts that pass the audio through unchanged (unless muted): the Scrying Glass analyzer
//! (spectrum, levels, loudness, stereo correlation and a vectorscope on any channel) and Siren's Fork, a
//! chromatic tuner.

use super::{Effect, EffectDef, ParamInfo};
use crate::dsp::loudness::LoudnessMeter;
use crate::dsp::spectrum::{self, Analyzer};

// ------------------------------------------------------------------------------------------- analyzer

pub const CHANNELS: &[&str] = &["Stereo", "Left", "Right", "Mid", "Side"];
pub const SPEEDS: &[&str] = &["Fast", "Medium", "Slow"];
pub const SLOPES: &[&str] = &["0 dB/oct", "3 dB/oct", "4.5 dB/oct"];
pub const RANGES: &[&str] = &["60 dB", "90 dB", "120 dB"];
/// Vectorscope points sent per block (x, y pairs).
pub const SCOPE_POINTS: usize = 64;
/// Telemetry layout: spectrum bands, then the meter values, then the scope.
pub const T_PEAK_L: usize = spectrum::BANDS;
pub const T_PEAK_R: usize = T_PEAK_L + 1;
pub const T_RMS_L: usize = T_PEAK_L + 2;
pub const T_RMS_R: usize = T_PEAK_L + 3;
pub const T_MOMENTARY: usize = T_PEAK_L + 4;
pub const T_SHORT: usize = T_PEAK_L + 5;
pub const T_CORR: usize = T_PEAK_L + 6;
pub const T_TRUE_PEAK: usize = T_PEAK_L + 7;
pub const T_SCOPE: usize = T_PEAK_L + 8;
const T_LEN: usize = T_SCOPE + 2 * SCOPE_POINTS;

const ANALYZER_PARAMS: &[ParamInfo] = &[
    ParamInfo::choice("channel", "Channel", CHANNELS, 0.0),
    ParamInfo::choice("speed", "Speed", SPEEDS, 1.0),
    ParamInfo::choice("slope", "Slope", SLOPES, 2.0),
    ParamInfo::choice("range", "Range", RANGES, 1.0),
    ParamInfo::toggle("hold", "Peak hold", true),
];

pub fn analyzer_def() -> EffectDef {
    EffectDef {
        id: "analyzer",
        name: "Scrying Glass (analyzer)",
        category: "Utility",
        description: "Analyzer insert: spectrum, peak and RMS levels, momentary and short-term loudness, true peak, stereo correlation and a vectorscope. The audio passes unchanged.",
        params: ANALYZER_PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(ScryingGlass::new(rate)),
    }
}

pub struct ScryingGlass {
    rate: f32,
    an: Analyzer,
    loud: LoudnessMeter,
    channel: usize,
    fall: f32,
    slope: f32,
    bands: [f32; spectrum::BANDS],
    rms: [f32; 2],
    peak: [f32; 2],
    corr: [f64; 3],
    scope: [f32; 2 * SCOPE_POINTS],
    scope_pos: usize,
    scope_skip: usize,
    tele: [f32; T_LEN],
}

impl ScryingGlass {
    pub fn new(rate: f32) -> Self {
        ScryingGlass {
            rate,
            an: Analyzer::new(rate),
            loud: LoudnessMeter::new(rate),
            channel: 0,
            fall: 0.25,
            slope: 4.5,
            bands: [spectrum::FLOOR; spectrum::BANDS],
            rms: [0.0; 2],
            peak: [0.0; 2],
            corr: [0.0; 3],
            scope: [0.0; 2 * SCOPE_POINTS],
            scope_pos: 0,
            scope_skip: 0,
            tele: [0.0; T_LEN],
        }
    }
}

impl Effect for ScryingGlass {
    fn set_param(&mut self, id: &str, v: f32) {
        let i = v.round().max(0.0) as usize;
        match id {
            "channel" => self.channel = i.min(CHANNELS.len() - 1),
            // The share of the distance a falling band covers per block.
            "speed" => self.fall = [0.5, 0.25, 0.08][i.min(2)],
            "slope" => self.slope = [0.0, 3.0, 4.5][i.min(2)],
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.an.reset();
        self.loud.reset();
        self.bands = [spectrum::FLOOR; spectrum::BANDS];
        self.corr = [0.0; 3];
    }

    fn telemetry(&self) -> &[f32] {
        &self.tele
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let n = l.len();
        if n == 0 {
            return;
        }
        // The analysed signal (the spectrum is mono: the chosen channel fed to both sides).
        match self.channel {
            0 => self.an.push(l, r),
            1 => self.an.push(l, l),
            2 => self.an.push(r, r),
            _ => {
                // Mid / side, sample by sample through a small stack buffer.
                let side = self.channel == 4;
                let mut buf = [0.0f32; 128];
                for (lc, rc) in l.chunks(128).zip(r.chunks(128)) {
                    for k in 0..lc.len() {
                        buf[k] = if side { 0.5 * (lc[k] - rc[k]) } else { 0.5 * (lc[k] + rc[k]) };
                    }
                    let b = &buf[..lc.len()];
                    self.an.push(b, b);
                }
            }
        }
        // The analyzer reads with a 4.5 dB/oct tilt; re-tilt to the chosen slope, then smooth.
        for b in 0..spectrum::BANDS {
            let f = spectrum::band_freq(b);
            let v = self.an.bands[b] - (4.5 - self.slope) * (f / 1000.0).log2();
            let cur = &mut self.bands[b];
            *cur = if v > *cur { v } else { *cur + (v - *cur) * self.fall };
        }
        self.loud.process(l, r);
        // Levels: RMS over ~300 ms, peaks with a 1.5 s fall.
        let k = (-(n as f32) / (0.3 * self.rate)).exp();
        let fall = (-(n as f32) / (1.5 * self.rate)).exp();
        for (c, x) in [&*l, &*r].into_iter().enumerate() {
            let (mut p, mut e) = (0.0f32, 0.0f32);
            for v in x {
                p = p.max(v.abs());
                e += v * v;
            }
            self.rms[c] = self.rms[c] * k + (e / n as f32) * (1.0 - k);
            self.peak[c] = (self.peak[c] * fall).max(p);
        }
        // Correlation E[lr] / sqrt(E[l²] E[r²]) over ~300 ms.
        let (mut lr, mut ll, mut rr) = (0.0f64, 0.0f64, 0.0f64);
        for i in 0..n {
            lr += (l[i] * r[i]) as f64;
            ll += (l[i] * l[i]) as f64;
            rr += (r[i] * r[i]) as f64;
        }
        let kd = k as f64;
        self.corr = [self.corr[0] * kd + lr * (1.0 - kd), self.corr[1] * kd + ll * (1.0 - kd), self.corr[2] * kd + rr * (1.0 - kd)];
        let den = (self.corr[1] * self.corr[2]).sqrt();
        let corr = if den > 1e-12 { (self.corr[0] / den) as f32 } else { 0.0 };
        // Vectorscope: every 4th sample, x = side, y = mid (rotated 45°: mono is a vertical line).
        for i in 0..n {
            self.scope_skip += 1;
            if self.scope_skip < 4 {
                continue;
            }
            self.scope_skip = 0;
            let s = std::f32::consts::FRAC_1_SQRT_2;
            self.scope[2 * self.scope_pos] = (r[i] - l[i]) * s;
            self.scope[2 * self.scope_pos + 1] = (l[i] + r[i]) * s;
            self.scope_pos = (self.scope_pos + 1) % SCOPE_POINTS;
        }
        let db = |x: f32| (20.0 * x.max(1e-6).log10()).max(-120.0);
        self.tele[..spectrum::BANDS].copy_from_slice(&self.bands);
        self.tele[T_PEAK_L] = db(self.peak[0]);
        self.tele[T_PEAK_R] = db(self.peak[1]);
        self.tele[T_RMS_L] = db(self.rms[0].sqrt());
        self.tele[T_RMS_R] = db(self.rms[1].sqrt());
        self.tele[T_MOMENTARY] = self.loud.momentary();
        self.tele[T_SHORT] = self.loud.short_term();
        self.tele[T_CORR] = corr;
        self.tele[T_TRUE_PEAK] = db(self.loud.true_peak);
        self.tele[T_SCOPE..].copy_from_slice(&self.scope);
    }
}

// ---------------------------------------------------------------------------------------------- tuner

pub const TUNER_RANGES: &[&str] = &["Guitar", "Bass", "Voice", "Wide"];
const RANGE_HZ: [(f32, f32); 4] = [(70.0, 1000.0), (28.0, 400.0), (70.0, 1200.0), (28.0, 2000.0)];
pub const DISPLAYS: &[&str] = &["Needle", "Strobe"];
/// Telemetry: frequency (Hz, 0 = none), note (MIDI, fractional, against the reference), cents off the
/// nearest note, confidence, input level (dB).
pub const T_FREQ: usize = 0;
pub const T_NOTE: usize = 1;
pub const T_CENTS: usize = 2;
pub const T_CONF: usize = 3;
pub const T_LEVEL: usize = 4;

const TUNER_PARAMS: &[ParamInfo] = &[
    ParamInfo::lin("reference", "A4", 430.0, 450.0, 440.0, "Hz"),
    ParamInfo::choice("range", "Range", TUNER_RANGES, 0.0),
    ParamInfo::choice("display", "Display", DISPLAYS, 0.0),
    ParamInfo::toggle("mute", "Mute", false),
];

pub fn tuner_def() -> EffectDef {
    EffectDef {
        id: "tuner",
        name: "Siren's Fork (tuner)",
        category: "Utility",
        description: "Chromatic tuner: the played note, how many cents it is off, a needle or a strobe. A4 reference 430 – 450 Hz; mute silences the output while you tune.",
        params: TUNER_PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(SirensFork::new(rate)),
    }
}

/// Analysis window (samples at 48 kHz scale with the rate) and hop.
const WIN: usize = 4096;
const HOP: usize = 1024;

pub struct SirensFork {
    rate: f32,
    reference: f32,
    range: usize,
    mute: bool,
    ring: [f32; WIN],
    pos: usize,
    since: usize,
    frame: Vec<f32>,
    scratch: Vec<f32>,
    freq: f32,
    conf: f32,
    level: f32,
    tele: [f32; 5],
}

impl SirensFork {
    pub fn new(rate: f32) -> Self {
        SirensFork {
            rate,
            reference: 440.0,
            range: 0,
            mute: false,
            ring: [0.0; WIN],
            pos: 0,
            since: 0,
            frame: vec![0.0; WIN],
            // The detector grows its scratch to the longest period; allocate that once, here.
            scratch: Vec::with_capacity(WIN + 2),
            freq: 0.0,
            conf: 0.0,
            level: -120.0,
            tele: [0.0; 5],
        }
    }

    fn analyse(&mut self) {
        let (lo, hi) = RANGE_HZ[self.range];
        // Low notes need the whole window; higher ranges use the newest half (faster response).
        let len = if lo < 60.0 { WIN } else { WIN / 2 };
        for i in 0..len {
            self.frame[i] = self.ring[(self.pos + WIN - len + i) % WIN];
        }
        match super::pitch::detect_pitch_with(&self.frame[..len], self.rate, lo, hi, &mut self.scratch) {
            Some((f, c)) if c > 0.6 => {
                // Glide towards a steady reading; jump when the note changes.
                if self.freq > 0.0 && (f / self.freq).log2().abs() < 0.04 {
                    self.freq += (f - self.freq) * 0.5;
                } else {
                    self.freq = f;
                }
                self.conf = c;
            }
            _ => self.conf *= 0.7,
        }
    }
}

impl Effect for SirensFork {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "reference" => self.reference = v.clamp(430.0, 450.0),
            "range" => self.range = (v.round().max(0.0) as usize).min(TUNER_RANGES.len() - 1),
            "mute" => self.mute = v >= 0.5,
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.ring = [0.0; WIN];
        self.freq = 0.0;
        self.conf = 0.0;
    }

    fn telemetry(&self) -> &[f32] {
        &self.tele
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let mut peak = 0.0f32;
        for i in 0..l.len() {
            let x = 0.5 * (l[i] + r[i]);
            peak = peak.max(x.abs());
            self.ring[self.pos] = x;
            self.pos = (self.pos + 1) % WIN;
            self.since += 1;
            if self.since >= HOP {
                self.since = 0;
                self.analyse();
            }
            if self.mute {
                l[i] = 0.0;
                r[i] = 0.0;
            }
        }
        let db = 20.0 * peak.max(1e-6).log10();
        self.level = if db > self.level { db } else { self.level - 0.5 };
        let shown = self.conf > 0.3 && self.level > -60.0;
        let note = if self.freq > 0.0 { 69.0 + 12.0 * (self.freq / self.reference).log2() } else { 0.0 };
        self.tele = [if shown { self.freq } else { 0.0 }, note, (note - note.round()) * 100.0, self.conf, self.level];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tuner_reads_a_flat_e() {
        let rate = 48000.0;
        let mut t = SirensFork::new(rate);
        // E2 (82.41 Hz) 10 cents flat.
        let f = 82.4069 * 2f32.powf(-10.0 / 1200.0);
        let n = 48000;
        let mut l: Vec<f32> = (0..n).map(|i| (std::f32::consts::TAU * f * i as f32 / rate).sin() * 0.5).collect();
        let mut r = l.clone();
        for (a, b) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
            t.process(a, b, None);
        }
        let tele = t.telemetry();
        assert!((tele[T_NOTE].round() - 40.0).abs() < 0.5, "note {}", tele[T_NOTE]);
        assert!((tele[T_CENTS] + 10.0).abs() < 2.0, "cents {}", tele[T_CENTS]);
    }

    #[test]
    fn analyzer_reads_correlation() {
        let rate = 48000.0;
        let mut a = ScryingGlass::new(rate);
        let n = 48000;
        let mut l: Vec<f32> = (0..n).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        let mut r: Vec<f32> = l.iter().map(|v| -v).collect();
        for (x, y) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
            a.process(x, y, None);
        }
        assert!(a.telemetry()[T_CORR] < -0.95, "{}", a.telemetry()[T_CORR]);
    }
}
