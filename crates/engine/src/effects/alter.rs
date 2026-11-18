//! "Dragon Alter": real-time pitch and formant shifter for voices (the Little AlterBoy school). Pitch-
//! synchronous overlap-add (TD-PSOLA): two-period Hann grains are taken at the voice's pitch marks and laid
//! down at the new period (pitch), each grain resampled on the way (formant) – so pitch and formant move
//! independently. Controls follow Little AlterBoy's manual: Pitch ±12 semitones, Formant, Link (pitch and
//! formant move together, like a sample sped up), three modes – Transpose, Quantize (snaps the voice to the
//! nearest semitone as fast as possible) and Robot (one note, set by Pitch: 0 = one octave above middle C) –
//! tube Drive, Mix and Output.

#[cfg(test)]
use super::pitch::detect_pitch;
use super::{Effect, EffectDef, ParamInfo};

const RING: usize = 16384;
const ANALYSIS: usize = 1024;
const HOP: usize = 128;

pub const ALTER_NOTES: &[&str] = &["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
pub const ALTER_MODES: &[&str] = &["Transpose", "Quantize", "Robot"];
/// Robot mode's note at Pitch 0: C5, one octave above middle C (MIDI 72).
pub const ROBOT_BASE: f32 = 72.0;

const ALTER_PARAMS: &[ParamInfo] = &[
    ParamInfo::lin("pitch", "Pitch", -12.0, 12.0, 0.0, "st"),
    ParamInfo::lin("formant", "Formant", -12.0, 12.0, 0.0, "st"),
    ParamInfo::choice("mode", "Mode", ALTER_MODES, 0.0),
    ParamInfo::toggle("link", "Link", false),
    ParamInfo::lin("drive", "Drive", 0.0, 100.0, 0.0, "%"),
    ParamInfo::lin("mix", "Mix", 0.0, 100.0, 100.0, "%"),
    ParamInfo::lin("output", "Output", -18.0, 12.0, 0.0, "dB"),
];

pub struct Alter {
    rate: f32,
    pitch: f32,
    formant: f32,
    mode: usize,
    link: bool,
    drive: f32,
    mix: f32,
    out: f32,
    input: [Vec<f32>; 2],
    output: [Vec<f32>; 2],
    /// The summed grain windows at each output sample (the output is divided by it, so overlapping grains
    /// neither boost nor dip the level).
    weight: Vec<f32>,
    /// The longest grain half (two periods of the lowest voice: formant down an octave).
    max_half: f64,
    /// Smoothed mix, output gain and drive (no zipper noise when they move).
    mix_s: f32,
    out_s: f32,
    drive_s: f32,
    /// Total samples written (stream time).
    t: usize,
    /// Next synthesis mark (stream time, output side).
    next_out: f64,
    /// Current analysis mark (stream time, input side) and the input period.
    mark: f64,
    period: f32,
    since: usize,
    analysis: Vec<f32>,
    /// Scratch for the pitch detector (no allocation in the audio thread).
    yin: Vec<f32>,
    latency: usize,
    /// [detected Hz (0 = none), output Hz]
    tele: [f32; 2],
}

impl Alter {
    pub fn new(rate: f32) -> Self {
        // Look-ahead: a grain reaches up to two periods of the lowest voice (≈ 70 Hz) ahead of its centre in
        // the output (formant down an octave) and one period behind it in the input – three periods.
        let max_period = rate / 70.0;
        let latency = (max_period * 3.0) as usize + 64;
        Alter {
            rate,
            pitch: 0.0,
            formant: 0.0,
            mode: 0,
            link: false,
            drive: 0.0,
            mix: 1.0,
            out: 1.0,
            input: [vec![0.0; RING], vec![0.0; RING]],
            output: [vec![0.0; RING], vec![0.0; RING]],
            weight: vec![0.0; RING],
            max_half: (max_period * 2.0) as f64,
            mix_s: 1.0,
            out_s: 1.0,
            drive_s: 0.0,
            t: 0,
            next_out: 0.0,
            mark: 0.0,
            period: rate / 150.0,
            since: 0,
            analysis: vec![0.0; ANALYSIS],
            yin: Vec::with_capacity(ANALYSIS),
            latency,
            tele: [0.0; 2],
        }
    }

    fn analyse(&mut self) {
        let start = self.t + RING - ANALYSIS;
        for k in 0..ANALYSIS {
            let i = (start + k) % RING;
            self.analysis[k] = (self.input[0][i] + self.input[1][i]) * 0.5;
        }
        match super::pitch::detect_pitch_with(&self.analysis, self.rate, 70.0, 1000.0, &mut self.yin) {
            Some((hz, c)) if c > 0.75 => {
                self.period = self.rate / hz;
                self.tele[0] = hz;
            }
            _ => self.tele[0] = 0.0,
        }
    }

    /// Lays down one grain centred on stream time `at_out`, taken around input time `at_in`.
    fn grain(&mut self, at_in: f64, at_out: f64, period: f32, formant: f32) {
        let half_in = period as f64;
        // Formant up = the grain is read faster (shorter, higher resonances).
        let half_out = (half_in / formant as f64).clamp(4.0, self.max_half);
        let n = (half_out * 2.0) as usize;
        for j in 0..n {
            let u = j as f64 / n as f64;
            let w = 0.5 - 0.5 * (std::f64::consts::TAU * u).cos();
            let src = at_in - half_in + u * 2.0 * half_in;
            let dst = at_out - half_out + j as f64;
            if src < 0.0 || dst < 0.0 {
                continue;
            }
            let i0 = src.floor() as usize;
            let f = (src - i0 as f64) as f32;
            let o = dst.round() as usize % RING;
            let w = w as f32;
            self.weight[o] += w;
            for ch in 0..2 {
                let a = self.input[ch][i0 % RING];
                let b = self.input[ch][(i0 + 1) % RING];
                self.output[ch][o] += (a + (b - a) * f) * w;
            }
        }
    }
}

impl Effect for Alter {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "pitch" => self.pitch = v.clamp(-12.0, 12.0),
            "formant" => self.formant = v.clamp(-12.0, 12.0),
            "mode" => self.mode = (v.round().max(0.0) as usize).min(2),
            "link" => self.link = v >= 0.5,
            "drive" => self.drive = v / 100.0,
            "mix" => self.mix = v / 100.0,
            "output" => self.out = crate::dsp::db_to_gain(v),
            _ => {}
        }
    }
    fn reset(&mut self) {
        for b in self.input.iter_mut().chain(self.output.iter_mut()) {
            b.fill(0.0);
        }
        self.weight.fill(0.0);
        self.t = 0;
        self.next_out = 0.0;
        self.mark = 0.0;
    }
    fn latency(&self) -> usize {
        self.latency
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let ratio = 2f32.powf(self.pitch / 12.0);
        // Link: the formant follows the pitch (like a sample sped up), the Formant knob adds to it.
        let formant = 2f32.powf((self.formant + if self.link { self.pitch } else { 0.0 }) / 12.0).clamp(0.5, 2.0);
        let lat = self.latency;
        // Parameter smoothing (≈ 10 ms).
        let k = 1.0 - (-1.0 / (0.01 * self.rate)).exp();
        for i in 0..l.len() {
            let w = self.t % RING;
            self.input[0][w] = l[i];
            self.input[1][w] = r[i];
            self.since += 1;
            if self.since >= HOP {
                self.since = 0;
                self.analyse();
            }
            // Output period: the input period over the ratio, or the robot note's period.
            let hz_of = |midi: f32| 440.0 * 2f32.powf((midi - 69.0) / 12.0);
            let out_period = match self.mode {
                // Robot: one note, Pitch sets it.
                2 => self.rate / hz_of(ROBOT_BASE + self.pitch.round()),
                // Quantize: the sung pitch snapped to the nearest semitone (instantly), then transposed.
                1 => {
                    let hz = self.rate / self.period;
                    let midi = (69.0 + 12.0 * (hz / 440.0).log2()).round();
                    self.rate / hz_of(midi + self.pitch.round())
                }
                _ => self.period / ratio,
            };
            self.tele[1] = self.rate / out_period;
            // The output runs a fixed `lat` samples behind the input. Grains are laid down at their own
            // output time, centred where their source sits in the input, as soon as the sample being read
            // could reach them – so a grain is always complete before it is read, and changing periods
            // never move the read position (which is what used to crackle).
            let read = self.t as f64 - lat as f64;
            while self.next_out <= read + self.max_half {
                let c = self.next_out;
                let per = self.period as f64;
                // Analysis marks advance by whole input periods up to the grain's time.
                if self.mark > c || self.mark < c - per * 4.0 {
                    self.mark = c;
                }
                while self.mark + per <= c {
                    self.mark += per;
                }
                self.grain(self.mark, c, self.period, formant);
                self.next_out += out_period.max(16.0) as f64;
            }
            let rd = (self.t + RING - lat) % RING;
            // Divided by the summed windows (floored so gaps between sparse grains are not blown up).
            let norm = 1.0 / self.weight[rd].max(0.5);
            let (mut a, mut b) = (self.output[0][rd] * norm, self.output[1][rd] * norm);
            self.output[0][rd] = 0.0;
            self.output[1][rd] = 0.0;
            self.weight[rd] = 0.0;
            self.drive_s += (self.drive - self.drive_s) * k;
            self.mix_s += (self.mix - self.mix_s) * k;
            self.out_s += (self.out - self.out_s) * k;
            if self.drive_s > 1e-4 {
                let d = 1.0 + self.drive_s * 6.0;
                a = (a * d).tanh() / d.sqrt();
                b = (b * d).tanh() / d.sqrt();
            }
            // Dry is delayed by the same latency.
            let (da, db) = (self.input[0][rd], self.input[1][rd]);
            l[i] = (da + (a - da) * self.mix_s) * self.out_s;
            r[i] = (db + (b - db) * self.mix_s) * self.out_s;
            self.t += 1;
        }
    }
    fn telemetry(&self) -> &[f32] {
        &self.tele
    }
}

pub fn alter_def() -> EffectDef {
    EffectDef {
        id: "alter",
        name: "Alter (pitch & formant)",
        category: "Pitch",
        description: "Real-time voice pitch and formant shifter after Little AlterBoy: pitch ±12 semitones, formant, link, Transpose / Quantize / Robot modes, tube drive, mix and output.",
        params: ALTER_PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(Alter::new(rate)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buzz(hz: f32, secs: f32) -> Vec<f32> {
        // A glottal-ish buzz with a formant around 800 Hz.
        let n = (48000.0 * secs) as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / 48000.0;
                (1..30)
                    .map(|h| {
                        let f = h as f32 * hz;
                        let formant = 1.0 / (1.0 + ((f - 800.0) / 300.0).powi(2));
                        (std::f32::consts::TAU * f * t).sin() * formant / h as f32
                    })
                    .sum::<f32>()
                    * 0.3
            })
            .collect()
    }

    fn run(params: &[(&str, f32)], x: &[f32]) -> Vec<f32> {
        let mut e = Alter::new(48000.0);
        for (k, v) in params {
            e.set_param(k, *v);
        }
        let (mut l, mut r) = (x.to_vec(), x.to_vec());
        for (a, b) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
            e.process(a, b, None);
        }
        l
    }

    #[test]
    fn shifts_pitch_by_semitones() {
        let x = buzz(150.0, 1.0);
        for (st, want) in [(12.0f32, 300.0f32), (-5.0, 150.0 * 2f32.powf(-5.0 / 12.0)), (7.0, 150.0 * 2f32.powf(7.0 / 12.0))] {
            let y = run(&[("pitch", st)], &x);
            let (f, _) = detect_pitch(&y[30000..31024 + 1024], 48000.0, 60.0, 1000.0).unwrap();
            assert!((f / want - 1.0).abs() < 0.03, "{st} st: {f} Hz, want {want}");
        }
    }

    #[test]
    fn formant_moves_without_changing_pitch() {
        let x = buzz(150.0, 1.0);
        let y = run(&[("formant", 7.0)], &x);
        let (f, _) = detect_pitch(&y[30000..32048], 48000.0, 60.0, 1000.0).unwrap();
        assert!((f / 150.0 - 1.0).abs() < 0.03, "pitch kept: {f}");
        // Energy around 800 Hz moves up (towards ~1.2 kHz).
        let band = |y: &[f32], lo: f32, hi: f32| {
            let seg = &y[30000..34096];
            let mut e = 0.0;
            let mut f = lo;
            while f < hi {
                let w = std::f32::consts::TAU * f / 48000.0;
                let (mut re, mut im) = (0.0f32, 0.0f32);
                for (i, v) in seg.iter().enumerate() {
                    re += v * (w * i as f32).cos();
                    im += v * (w * i as f32).sin();
                }
                e += re * re + im * im;
                f += 150.0;
            }
            e
        };
        let ratio_in = band(&x, 1050.0, 1500.0) / band(&x, 600.0, 900.0);
        let ratio_out = band(&y, 1050.0, 1500.0) / band(&y, 600.0, 900.0);
        assert!(ratio_out > ratio_in * 1.5, "formant up: {ratio_in} → {ratio_out}");
    }

    /// Robot (manual): one note set by Pitch, 0 = one octave above middle C (C5, 523 Hz); −12 = C4.
    #[test]
    fn robot_sings_one_note() {
        let x = buzz(170.0, 1.0);
        let y = run(&[("mode", 2.0), ("pitch", -12.0)], &x);
        let (f, _) = detect_pitch(&y[30000..32048], 48000.0, 60.0, 1000.0).unwrap();
        assert!((f / 261.63 - 1.0).abs() < 0.03, "{f}");
    }

    /// Quantize snaps a voice between semitones to the nearest one.
    #[test]
    fn quantize_snaps_to_semitones() {
        // 158 Hz is 27 cents above D#3 (155.6 Hz): the output must land on D#3.
        let x = buzz(158.0, 1.0);
        let y = run(&[("mode", 1.0)], &x);
        let (f, _) = detect_pitch(&y[30000..32048], 48000.0, 60.0, 1000.0).unwrap();
        let cents = 1200.0 * (f / 440.0).log2();
        let off = (cents / 100.0 - (cents / 100.0).round()).abs() * 100.0;
        assert!(off < 15.0, "{f} Hz is {off} cents off a semitone");
        assert!((f / 155.56 - 1.0).abs() < 0.02, "nearest semitone D#3: {f}");
    }

    /// Link moves the formant with the pitch (like a sped-up sample).
    #[test]
    fn link_moves_formant_with_pitch() {
        let x = buzz(150.0, 1.0);
        let a = run(&[("pitch", 7.0)], &x);
        let b = run(&[("pitch", 7.0), ("link", 1.0)], &x);
        let c = run(&[("pitch", 7.0), ("formant", 7.0)], &x);
        let d = |p: &[f32], q: &[f32]| p[30000..34000].iter().zip(&q[30000..34000]).map(|(x, y)| (x - y).abs()).sum::<f32>();
        assert!(d(&b, &c) < d(&a, &b) * 0.2, "link = formant following pitch");
    }

    /// No crackle: with a voice that glides and wobbles (so the detected period keeps changing), the output
    /// is as smooth as the input – no sample-to-sample jumps – in every mode.
    #[test]
    fn a_gliding_voice_comes_out_without_crackles() {
        let sr = 48000.0f32;
        let mut ph = 0.0f32;
        let x: Vec<f32> = (0..96000)
            .map(|i| {
                let t = i as f32 / sr;
                let hz = 150.0 + 60.0 * (t * 0.7 * std::f32::consts::TAU).sin() + 6.0 * (t * 5.5 * std::f32::consts::TAU).sin();
                ph += hz / sr;
                (1..12).map(|h| (std::f32::consts::TAU * ph * h as f32).sin() / h as f32).sum::<f32>() * 0.3
            })
            .collect();
        // The largest second difference: a click is a spike far above the signal's own curvature.
        let roughness = |y: &[f32]| y[8000..].windows(3).map(|w| (w[2] - 2.0 * w[1] + w[0]).abs()).fold(0.0f32, f32::max);
        let input = roughness(&x);
        for params in [
            vec![("pitch", 0.0)],
            vec![("pitch", 4.0)],
            vec![("pitch", -5.0), ("formant", 3.0)],
            vec![("mode", 1.0)],
            vec![("mode", 2.0)],
            vec![("pitch", 12.0)],
            vec![("pitch", -12.0)],
            vec![("pitch", 7.0), ("link", 1.0)],
        ] {
            let y = run(&params, &x);
            let r = roughness(&y);
            assert!(r < input * 2.5, "{params:?}: roughness {r} vs input {input}");
        }
    }

    /// Turning Mix and Output while it plays does not click.
    #[test]
    fn moving_mix_and_output_is_smooth() {
        let x = buzz(150.0, 1.0);
        let mut e = Alter::new(48000.0);
        e.set_param("pitch", 5.0);
        let (mut l, mut r) = (x.clone(), x.clone());
        for (k, (a, b)) in l.chunks_mut(128).zip(r.chunks_mut(128)).enumerate() {
            if k % 20 == 0 {
                e.set_param("mix", if (k / 20) % 2 == 0 { 0.0 } else { 100.0 });
                e.set_param("output", if (k / 20) % 2 == 0 { -12.0 } else { 6.0 });
            }
            e.process(a, b, None);
        }
        let jump = l[4000..].windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        let natural = x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        assert!(jump < natural * 2.5, "{jump} vs {natural}");
    }
}
