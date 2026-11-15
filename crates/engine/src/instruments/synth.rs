//! Subtractive synth: 3 band-limited oscillators (PolyBLEP saw/square, triangle, sine, noise) with unison,
//! a resonant multimode filter with its own envelope, an amp envelope, an LFO and glide for slide notes.

use super::{midi_to_hz, InstrumentVoice, NoteOn};
use crate::dsp::{pan_law, EnvGen, Rng, Svf};
use crate::model::{FilterMode, Instrument as Model, SynthParams, Waveform};
use crate::samples::SampleCache;

#[inline]
fn poly_blep(t: f64, dt: f64) -> f64 {
    if t < dt {
        let t = t / dt;
        t + t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

#[derive(Clone)]
struct Voice {
    note: u8,
    freq: f64,
    target_freq: f64,
    phases: Vec<[f64; 3]>,
    vel: f32,
    gl: f32,
    gr: f32,
    amp: EnvGen,
    fenv: EnvGen,
    filter: [Svf; 2],
    released: bool,
    age: u64,
}

pub struct Synth {
    rate: f32,
    p: SynthParams,
    voices: Vec<Voice>,
    lfo_phase: f32,
    rng: Rng,
    counter: u64,
    cutoff_mod: f32,
    res_mod: Option<f32>,
    /// Smoothed cutoff, resonance and gain (knob turns and automation glide over ~10 ms instead of stepping).
    smooth: Option<[f32; 3]>,
}

impl Synth {
    pub fn new(rate: f32) -> Self {
        Synth { rate, p: SynthParams::default(), voices: Vec::new(), lfo_phase: 0.0, rng: Rng::new(1234), counter: 0, cutoff_mod: 0.0, res_mod: None, smooth: None }
    }

    #[inline]
    fn osc(wave: Waveform, phase: f64, dt: f64, pw: f64, rng: &mut Rng) -> f64 {
        match wave {
            Waveform::Saw => 2.0 * phase - 1.0 - poly_blep(phase, dt),
            Waveform::Square => {
                let mut v = if phase < pw { 1.0 } else { -1.0 };
                v += poly_blep(phase, dt);
                v -= poly_blep((phase - pw).rem_euclid(1.0), dt);
                v
            }
            Waveform::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
            Waveform::Sine => (phase * std::f64::consts::TAU).sin(),
            Waveform::Noise => rng.bipolar() as f64,
        }
    }
}

impl InstrumentVoice for Synth {
    fn note_on(&mut self, n: NoteOn) {
        let freq = midi_to_hz(n.note as f32 + n.detune) as f64;
        if n.slide || self.p.polyphony <= 1 {
            if let Some(v) = self.voices.iter_mut().filter(|v| !v.released).max_by_key(|v| v.age) {
                v.target_freq = freq;
                v.note = n.note;
                if !n.slide {
                    v.amp.trigger();
                    v.fenv.trigger();
                }
                return;
            }
        }
        while self.voices.len() >= self.p.polyphony.max(1) as usize {
            // Steal the oldest voice.
            let oldest = self.voices.iter().enumerate().min_by_key(|(_, v)| v.age).map(|(i, _)| i).unwrap_or(0);
            self.voices.remove(oldest);
        }
        let unison = self.p.unison.clamp(1, 8) as usize;
        let mut phases = Vec::with_capacity(unison);
        for _ in 0..unison {
            phases.push([self.rng.next_f32() as f64, self.rng.next_f32() as f64, self.rng.next_f32() as f64]);
        }
        let (gl, gr) = pan_law(n.pan);
        let mut amp = EnvGen::new(self.p.amp_adsr, self.rate);
        let mut fenv = EnvGen::new(self.p.filter_adsr, self.rate);
        amp.trigger();
        fenv.trigger();
        self.counter += 1;
        self.voices.push(Voice { note: n.note, freq, target_freq: freq, phases, vel: n.velocity, gl, gr, amp, fenv, filter: [Svf::default(); 2], released: false, age: self.counter });
    }

    fn note_off(&mut self, note: u8) {
        for v in self.voices.iter_mut().filter(|v| v.note == note && !v.released) {
            v.released = true;
            v.amp.release();
            v.fenv.release();
        }
    }

    fn choke(&mut self, seconds: f32) {
        for v in &mut self.voices {
            v.released = true;
            v.amp.choke(seconds);
        }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let p = &self.p;
        let rate = self.rate as f64;
        let glide = if p.glide > 0.0 { 1.0 - (-1.0 / (p.glide as f64 * rate)).exp() } else { 1.0 };
        let detune_cents = p.detune as f64;
        let rng = &mut self.rng;
        let unison_norm = 1.0 / (p.voices_norm());
        let targets = [(p.cutoff + self.cutoff_mod).max(1.0), self.res_mod.unwrap_or(p.resonance), p.gain];
        let mut sm = self.smooth.unwrap_or(targets);
        let k_smooth = 1.0 - (-1.0 / (0.01 * self.rate)).exp();
        for i in 0..l.len() {
            for (c, t) in sm.iter_mut().zip(targets) {
                *c += (t - *c) * k_smooth;
            }
            let [sm_cut, sm_res, sm_gain] = sm;
            self.lfo_phase = (self.lfo_phase + p.lfo_rate / self.rate).fract();
            let lfo = (self.lfo_phase * std::f32::consts::TAU).sin();
            let pitch_mod = 2f64.powf((lfo * p.lfo_pitch) as f64 / 12.0);
            for v in &mut self.voices {
                let a = v.amp.next();
                if v.amp.is_idle() {
                    continue;
                }
                v.freq += (v.target_freq - v.freq) * glide;
                let f_env = v.fenv.next();
                let n_uni = v.phases.len();
                let mut sl = 0.0f64;
                let mut sr = 0.0f64;
                for (u, ph) in v.phases.iter_mut().enumerate() {
                    let spread = if n_uni > 1 { (u as f64 / (n_uni - 1) as f64) * 2.0 - 1.0 } else { 0.0 };
                    let uni = 2f64.powf(spread * detune_cents / 1200.0);
                    let mut s = 0.0;
                    for (k, o) in p.osc.iter().enumerate() {
                        if o.level <= 0.0 {
                            continue;
                        }
                        let f = v.freq * uni * pitch_mod * 2f64.powf((o.coarse as f64 + o.fine as f64 / 100.0) / 12.0);
                        let dt = (f / rate).min(0.45);
                        s += Synth::osc(o.wave, ph[k], dt, o.pulse_width.clamp(0.05, 0.95) as f64, rng) * o.level as f64;
                        ph[k] += dt;
                        if ph[k] >= 1.0 {
                            ph[k] -= 1.0;
                        }
                    }
                    // Spread unison voices across the stereo field.
                    let pan = (spread * 0.5 + 0.5).clamp(0.0, 1.0);
                    sl += s * (1.0 - pan * 0.6);
                    sr += s * (0.4 + pan * 0.6);
                }
                let cutoff = sm_cut * 2f32.powf(p.filter_env * f_env + lfo * p.lfo_filter) * (0.6 + 0.4 * v.vel);
                let res = sm_res;
                let mut out = [0.0f32; 2];
                for (ch, x) in [sl as f32, sr as f32].into_iter().enumerate() {
                    let (lp, bp, hp) = v.filter[ch].process(x * unison_norm, cutoff, res, self.rate);
                    out[ch] = match p.filter {
                        FilterMode::LowPass => lp,
                        FilterMode::HighPass => hp,
                        FilterMode::BandPass => bp,
                    };
                }
                let g = a * v.vel * sm_gain;
                l[i] += out[0] * g * v.gl;
                r[i] += out[1] * g * v.gr;
            }
        }
        self.smooth = Some(sm);
        self.voices.retain(|v| !v.amp.is_idle());
    }

    fn update(&mut self, model: &Model, _cache: &SampleCache) {
        if let Model::Synth(p) = model {
            self.p = p.clone();
            for v in &mut self.voices {
                v.amp.set(p.amp_adsr);
                v.fenv.set(p.filter_adsr);
            }
        }
    }

    fn active_voices(&self) -> usize {
        self.voices.len()
    }

    fn set_param(&mut self, id: &str, value: f32) {
        match id {
            "cutoff" => self.cutoff_mod = value - self.p.cutoff,
            "resonance" => self.res_mod = Some(value),
            _ => {}
        }
    }
}

trait UnisonNorm {
    fn voices_norm(&self) -> f32;
}

impl UnisonNorm for SynthParams {
    fn voices_norm(&self) -> f32 {
        (self.unison.clamp(1, 8) as f32).sqrt()
    }
}
