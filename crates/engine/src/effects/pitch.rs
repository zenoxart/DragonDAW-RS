//! Pitch: YIN pitch detection (shared with audio-to-MIDI), a low-latency granular pitch shifter and the
//! "Dragon Tune" pitch-correction effect (Auto-Tune / Newtone style: snap a monophonic voice to a key).

use super::{Effect, EffectDef, ParamInfo};

/// YIN fundamental-frequency estimate of `buf` (mono). Returns `(hz, clarity 0 … 1)` when a period between
/// `min_hz` and `max_hz` is found with an aperiodicity below 0.2.
pub fn detect_pitch(buf: &[f32], rate: f32, min_hz: f32, max_hz: f32) -> Option<(f32, f32)> {
    let max_tau = ((rate / min_hz) as usize).min(buf.len() / 2);
    let min_tau = ((rate / max_hz) as usize).max(2);
    if max_tau <= min_tau + 2 {
        return None;
    }
    let w = buf.len() - max_tau;
    // Skip silence.
    let energy: f32 = buf[..w].iter().map(|x| x * x).sum::<f32>() / w as f32;
    if energy < 1e-6 {
        return None;
    }
    let mut d = vec![0.0f32; max_tau + 1];
    for tau in 1..=max_tau {
        let mut s = 0.0;
        for i in 0..w {
            let x = buf[i] - buf[i + tau];
            s += x * x;
        }
        d[tau] = s;
    }
    // Cumulative mean normalised difference.
    let mut cmnd = vec![1.0f32; max_tau + 1];
    let mut run = 0.0;
    for tau in 1..=max_tau {
        run += d[tau];
        cmnd[tau] = if run > 0.0 { d[tau] * tau as f32 / run } else { 1.0 };
    }
    let threshold = 0.15;
    let mut tau = min_tau;
    let mut found = None;
    while tau < max_tau {
        if cmnd[tau] < threshold {
            while tau + 1 < max_tau && cmnd[tau + 1] < cmnd[tau] {
                tau += 1;
            }
            found = Some(tau);
            break;
        }
        tau += 1;
    }
    let tau = match found {
        Some(t) => t,
        None => {
            // Fall back to the global minimum when it is clear enough.
            let (t, v) = (min_tau..max_tau).map(|t| (t, cmnd[t])).fold((0, f32::MAX), |a, b| if b.1 < a.1 { b } else { a });
            if v > 0.2 {
                return None;
            }
            t
        }
    };
    // Parabolic interpolation for a sub-sample period.
    let better = if tau > 1 && tau < max_tau {
        let (a, b, c) = (cmnd[tau - 1], cmnd[tau], cmnd[tau + 1]);
        let den = a + c - 2.0 * b;
        if den.abs() > 1e-9 {
            tau as f32 + 0.5 * (a - c) / den
        } else {
            tau as f32
        }
    } else {
        tau as f32
    };
    Some((rate / better, (1.0 - cmnd[tau]).clamp(0.0, 1.0)))
}

pub fn hz_to_midi(hz: f32) -> f32 {
    69.0 + 12.0 * (hz / 440.0).log2()
}

/// Granular pitch shifter: two crossfaded read taps moving through a delay line.
pub struct PitchShifter {
    buf: Vec<f32>,
    w: usize,
    phase: f32,
    window: f32,
}

impl PitchShifter {
    pub fn new(rate: f32) -> Self {
        let window = (rate * 0.03).max(64.0);
        PitchShifter { buf: vec![0.0; (window as usize) * 2 + 8], w: 0, phase: 0.0, window }
    }

    pub fn reset(&mut self) {
        self.buf.fill(0.0);
        self.phase = 0.0;
    }

    fn read(&self, delay: f32) -> f32 {
        let n = self.buf.len();
        let pos = self.w as f32 - delay;
        let pos = pos.rem_euclid(n as f32);
        let i = pos.floor() as usize;
        let f = pos - i as f32;
        self.buf[i % n] * (1.0 - f) + self.buf[(i + 1) % n] * f
    }

    /// Processes one sample at the pitch `ratio` (2.0 = an octave up).
    #[inline]
    pub fn process(&mut self, x: f32, ratio: f32) -> f32 {
        let n = self.buf.len();
        self.buf[self.w] = x;
        self.phase = (self.phase + (1.0 - ratio) / self.window).rem_euclid(1.0);
        let p2 = (self.phase + 0.5).rem_euclid(1.0);
        let g1 = (std::f32::consts::PI * self.phase).sin();
        let g2 = (std::f32::consts::PI * p2).sin();
        let y = self.read(self.phase * self.window + 1.0) * g1 + self.read(p2 * self.window + 1.0) * g2;
        self.w = (self.w + 1) % n;
        // sin² + cos² crossfade keeps the level close to constant.
        y * 0.8
    }
}

pub const KEYS: &[&str] = &["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
pub const TUNE_SCALES: &[(&str, &[u8])] = &[("Chromatic", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]), ("Major", &[0, 2, 4, 5, 7, 9, 11]), ("Minor", &[0, 2, 3, 5, 7, 8, 10]), ("Major pentatonic", &[0, 2, 4, 7, 9]), ("Minor pentatonic", &[0, 3, 5, 7, 10]), ("Harmonic minor", &[0, 2, 3, 5, 7, 8, 11])];
const SCALE_NAMES: &[&str] = &["Chromatic", "Major", "Minor", "Major pentatonic", "Minor pentatonic", "Harmonic minor"];

/// Nearest note (fractional MIDI → integer MIDI) allowed by `root` + `scale`.
pub fn nearest_in_scale(midi: f32, root: u8, scale: &[u8]) -> f32 {
    let base = midi.round() as i32;
    let mut best = base as f32;
    let mut dist = f32::MAX;
    for cand in base - 6..=base + 6 {
        let pc = (cand - root as i32).rem_euclid(12) as u8;
        if scale.contains(&pc) {
            let d = (cand as f32 - midi).abs();
            if d < dist {
                dist = d;
                best = cand as f32;
            }
        }
    }
    best
}

const TUNE_PARAMS: &[ParamInfo] = &[
    ParamInfo::choice("key", "Key", KEYS, 0.0),
    ParamInfo::choice("scale", "Scale", SCALE_NAMES, 0.0),
    ParamInfo::log("speed", "Retune speed", 1.0, 400.0, 25.0, "ms"),
    ParamInfo::lin("amount", "Amount", 0.0, 1.0, 1.0, ""),
    ParamInfo::lin("mix", "Mix", 0.0, 1.0, 1.0, ""),
];

const HOP: usize = 256;
const FRAME: usize = 2048;

pub struct PitchCorrect {
    rate: f32,
    key: u8,
    scale: usize,
    speed_ms: f32,
    amount: f32,
    mix: f32,
    ring: Vec<f32>,
    pos: usize,
    analysis: Vec<f32>,
    fill: usize,
    since: usize,
    target_ratio: f32,
    ratio: f32,
    sl: PitchShifter,
    sr: PitchShifter,
    /// Last detected pitch (MIDI) for displays and tests.
    pub detected: Option<f32>,
}

impl PitchCorrect {
    pub fn new(rate: f32) -> Self {
        PitchCorrect { rate, key: 0, scale: 0, speed_ms: 25.0, amount: 1.0, mix: 1.0, ring: vec![0.0; FRAME], pos: 0, analysis: vec![0.0; FRAME], fill: 0, since: 0, target_ratio: 1.0, ratio: 1.0, sl: PitchShifter::new(rate), sr: PitchShifter::new(rate), detected: None }
    }

    fn analyse(&mut self) {
        // Unroll the ring, oldest sample first.
        let (a, b) = self.ring.split_at(self.pos);
        self.analysis[..b.len()].copy_from_slice(b);
        self.analysis[b.len()..].copy_from_slice(a);
        match detect_pitch(&self.analysis, self.rate, 70.0, 1100.0) {
            Some((hz, clarity)) if clarity > 0.8 => {
                let m = hz_to_midi(hz);
                self.detected = Some(m);
                let target = nearest_in_scale(m, self.key, TUNE_SCALES[self.scale].1);
                let semis = (target - m) * self.amount;
                self.target_ratio = 2f32.powf(semis / 12.0);
            }
            _ => {
                self.detected = None;
                self.target_ratio = 1.0;
            }
        }
    }
}

impl Effect for PitchCorrect {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "key" => self.key = (v.round() as u8).min(11),
            "scale" => self.scale = (v.round() as usize).min(TUNE_SCALES.len() - 1),
            "speed" => self.speed_ms = v.max(0.5),
            "amount" => self.amount = v.clamp(0.0, 1.0),
            "mix" => self.mix = v.clamp(0.0, 1.0),
            _ => {}
        }
    }
    fn reset(&mut self) {
        self.sl.reset();
        self.sr.reset();
        self.ring.fill(0.0);
        self.fill = 0;
        self.ratio = 1.0;
        self.target_ratio = 1.0;
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let coef = (-1.0 / (self.speed_ms * 0.001 * self.rate)).exp();
        for i in 0..l.len() {
            // Analysis ring (most recent FRAME samples, oldest first).
            self.ring[self.pos] = (l[i] + r[i]) * 0.5;
            self.pos = (self.pos + 1) % FRAME;
            self.fill = (self.fill + 1).min(FRAME);
            self.since += 1;
            if self.since >= HOP && self.fill == FRAME {
                self.since = 0;
                self.analyse();
            }
            self.ratio = self.target_ratio + (self.ratio - self.target_ratio) * coef;
            let (dl, dr) = (l[i], r[i]);
            let wl = self.sl.process(dl, self.ratio);
            let wr = self.sr.process(dr, self.ratio);
            l[i] = dl * (1.0 - self.mix) + wl * self.mix;
            r[i] = dr * (1.0 - self.mix) + wr * self.mix;
        }
    }
}

pub fn pitch_correct_def() -> EffectDef {
    EffectDef { id: "tune", name: "Dragon Tune (pitch correction)", category: "Pitch", description: "Real-time pitch correction: snaps a monophonic voice or instrument to the notes of a key.", params: TUNE_PARAMS, uses_sidechain: false, create: |rate| Box::new(PitchCorrect::new(rate)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f32, secs: f32) -> Vec<f32> {
        (0..(48000.0 * secs) as usize).map(|i| (std::f32::consts::TAU * hz * i as f32 / 48000.0).sin() * 0.5).collect()
    }

    #[test]
    fn yin_finds_the_fundamental() {
        for hz in [82.4f32, 220.0, 440.0, 659.3] {
            let (f, c) = detect_pitch(&sine(hz, 0.05), 48000.0, 60.0, 1100.0).unwrap();
            assert!((f - hz).abs() / hz < 0.005, "{hz}: {f}");
            assert!(c > 0.9);
        }
        assert!(detect_pitch(&vec![0.0; 2048], 48000.0, 60.0, 1000.0).is_none());
    }

    #[test]
    fn scale_snapping() {
        // 61.4 (between C#4 and D4) snaps to D in C major and to C# chromatically.
        assert_eq!(nearest_in_scale(61.4, 0, TUNE_SCALES[0].1), 61.0);
        assert_eq!(nearest_in_scale(61.4, 0, TUNE_SCALES[1].1), 62.0);
    }

    #[test]
    fn shifter_moves_pitch_by_the_ratio() {
        let mut s = PitchShifter::new(48000.0);
        let x = sine(300.0, 1.0);
        let y: Vec<f32> = x.iter().map(|v| s.process(*v, 1.5)).collect();
        let (f, _) = detect_pitch(&y[30000..32048], 48000.0, 60.0, 1100.0).unwrap();
        assert!((f - 450.0).abs() < 6.0, "{f}");
    }

    #[test]
    fn tune_pulls_a_sharp_note_to_the_scale() {
        // 452 Hz is ~47 cents sharp of A4 (440 Hz): chromatic correction brings it to 440.
        let mut e = PitchCorrect::new(48000.0);
        e.set_param("speed", 5.0);
        let x = sine(452.0, 1.0);
        let (mut l, mut r) = (x.clone(), x);
        for (a, b) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
            e.process(a, b, None);
        }
        let (f, _) = detect_pitch(&l[36000..38048], 48000.0, 60.0, 1100.0).unwrap();
        assert!((f - 440.0).abs() < 3.0, "corrected to {f} Hz");
    }
}
