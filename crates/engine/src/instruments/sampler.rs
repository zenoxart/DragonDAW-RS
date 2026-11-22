//! Sampler: plays one sample pitched across the keyboard (or slices of it, beat-slicer style) with
//! start/end, loop, reverse, fine tune, glide for slide notes and the channel's DAHDSR envelope.

use super::{note_ratio, InstrumentVoice, NoteOn};
use crate::dsp::{pan_law, EnvGen};
use crate::model::{Envelope, Instrument as Model, SamplerParams};
use crate::samples::{AudioData, SampleCache};
use std::sync::Arc;

struct Voice {
    note: u8,
    pos: f64,
    rate: f64,
    target_rate: f64,
    start: f64,
    end: f64,
    gain_l: f32,
    gain_r: f32,
    env: EnvGen,
    released: bool,
    done: bool,
}

pub struct Sampler {
    rate: f32,
    params: SamplerParams,
    data: Option<Arc<AudioData>>,
    voices: Vec<Voice>,
    envelope: Envelope,
    last_rate: f64,
}

const MAX_VOICES: usize = 32;

impl Sampler {
    pub fn new(rate: f32) -> Self {
        Sampler { rate, params: SamplerParams::default(), data: None, voices: Vec::with_capacity(MAX_VOICES + 1), envelope: Envelope::default(), last_rate: 1.0 }
    }

    /// Sets the amplitude envelope of the owning channel.
    pub fn set_envelope(&mut self, e: Envelope) {
        self.envelope = e;
    }

    fn region(&self, note: u8) -> Option<(f64, f64, f64)> {
        let d = self.data.as_ref()?;
        let n = d.frames() as f64;
        if n < 2.0 {
            return None;
        }
        let p = &self.params;
        if !p.slices.is_empty() {
            let idx = note as i32 - p.root_note as i32;
            if idx < 0 || idx as usize >= p.slices.len() {
                return None;
            }
            let i = idx as usize;
            let a = p.slices[i] as f64 * n;
            let b = p.slices.get(i + 1).map(|x| *x as f64 * n).unwrap_or(p.end as f64 * n);
            return Some((a, b.max(a + 1.0), 1.0));
        }
        let a = (p.start.clamp(0.0, 1.0) as f64 * n).min(n - 1.0);
        let b = (p.end.clamp(0.0, 1.0) as f64 * n).max(a + 1.0);
        let ratio = note_ratio(note as f32 - p.root_note as f32 + p.fine / 100.0);
        Some((a, b, ratio))
    }
}

impl InstrumentVoice for Sampler {
    fn note_on(&mut self, n: NoteOn) {
        let Some((start, end, ratio)) = self.region(n.note) else { return };
        let ratio = ratio * note_ratio(n.detune);
        if n.slide {
            if let Some(v) = self.voices.iter_mut().rev().find(|v| !v.released) {
                v.target_rate = ratio;
                v.note = n.note;
                return;
            }
        }
        if self.voices.len() >= MAX_VOICES {
            self.voices.remove(0);
        }
        let mut env = EnvGen::new(
            if self.envelope.enabled { self.envelope } else { Envelope { enabled: true, delay: 0.0, attack: 0.0, hold: 0.0, decay: 0.0, sustain: 1.0, release: 0.005, attack_tension: 0.0, release_tension: 0.0 } },
            self.rate,
        );
        env.trigger();
        let (pl, pr) = pan_law(n.pan);
        let vel = n.velocity.clamp(0.0, 1.0);
        let pos = if self.params.reverse { end - 1.0 } else { start };
        self.last_rate = ratio;
        self.voices.push(Voice {
            note: n.note,
            pos,
            rate: ratio,
            target_rate: ratio,
            start,
            end,
            gain_l: vel * pl * std::f32::consts::FRAC_1_SQRT_2,
            gain_r: vel * pr * std::f32::consts::FRAC_1_SQRT_2,
            env,
            released: false,
            done: false,
        });
    }

    fn note_off(&mut self, note: u8) {
        // One-shot samples without an envelope keep ringing (drum hits); with an envelope, release.
        if !self.envelope.enabled && !self.params.looped {
            return;
        }
        for v in self.voices.iter_mut().filter(|v| v.note == note && !v.released) {
            v.released = true;
            v.env.release();
        }
    }

    fn choke(&mut self, seconds: f32) {
        for v in &mut self.voices {
            v.released = true;
            v.env.choke(seconds);
        }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let Some(d) = self.data.clone() else { return };
        let glide = if self.params.glide > 0.0 { 1.0 - (-1.0 / (self.params.glide as f64 * self.rate as f64)).exp() } else { 1.0 };
        let rate_scale = d.rate as f64 / self.rate as f64;
        let (looped, reverse) = (self.params.looped, self.params.reverse);
        for v in &mut self.voices {
            for i in 0..l.len() {
                if v.done {
                    break;
                }
                let e = v.env.next();
                if v.env.is_idle() {
                    v.done = true;
                    break;
                }
                let (a, b) = d.sample_at(v.pos);
                l[i] += a * v.gain_l * e;
                r[i] += b * v.gain_r * e;
                v.rate += (v.target_rate - v.rate) * glide;
                let step = v.rate * rate_scale;
                if reverse {
                    v.pos -= step;
                    if v.pos < v.start {
                        if looped {
                            v.pos = v.end - 1.0;
                        } else {
                            v.done = true;
                        }
                    }
                } else {
                    v.pos += step;
                    if v.pos >= v.end - 1.0 {
                        if looped {
                            v.pos = v.start + (v.pos - (v.end - 1.0));
                        } else {
                            v.done = true;
                        }
                    }
                }
            }
        }
        self.voices.retain(|v| !v.done);
    }

    fn update(&mut self, model: &Model, cache: &SampleCache) {
        if let Model::Sampler(p) = model {
            if p.path != self.params.path || self.data.is_none() {
                self.data = if p.path.as_os_str().is_empty() { None } else { cache.get(&p.path, self.rate as u32).ok() };
            }
            self.params = p.clone();
        }
    }

    fn active_voices(&self) -> usize {
        self.voices.len()
    }

    fn set_param(&mut self, id: &str, v: f32) {
        let e = &mut self.envelope;
        match id {
            "__env_enabled" => e.enabled = v >= 0.5,
            "__env_delay" => e.delay = v,
            "__env_attack" => e.attack = v,
            "__env_hold" => e.hold = v,
            "__env_decay" => e.decay = v,
            "__env_sustain" => e.sustain = v,
            "__env_release" => e.release = v,
            "__env_atens" => e.attack_tension = v,
            "__env_rtens" => e.release_tension = v,
            _ => {}
        }
    }
}

/// Finds slice points by transient detection (for the beat slicer): returns fractions 0 … 1.
pub fn detect_slices(data: &AudioData, sensitivity: f32, max_slices: usize) -> Vec<f32> {
    let n = data.frames();
    if n == 0 {
        return vec![0.0];
    }
    let hop = 512;
    let mut energy = Vec::with_capacity(n / hop + 1);
    let mut i = 0;
    while i + hop <= n {
        let e: f32 = (i..i + hop).map(|k| data.l[k] * data.l[k] + data.r[k] * data.r[k]).sum::<f32>() / hop as f32;
        energy.push(e.sqrt());
        i += hop;
    }
    let mut slices = vec![0.0f32];
    let min_gap = (data.rate as usize / 10) / hop; // 100 ms
    let mut last = 0usize;
    let thr = 1.5 + (1.0 - sensitivity.clamp(0.0, 1.0)) * 4.0;
    for k in 4..energy.len() {
        let avg = energy[k - 4..k].iter().sum::<f32>() / 4.0 + 1e-4;
        if energy[k] / avg > thr && k - last >= min_gap.max(1) {
            slices.push((k * hop) as f32 / n as f32);
            last = k;
            if slices.len() >= max_slices {
                break;
            }
        }
    }
    slices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slices_found_on_clicks() {
        let rate = 44100u32;
        let mut l = vec![0.0f32; rate as usize * 2];
        for k in 0..4 {
            let at = k * rate as usize / 2;
            for i in 0..2000 {
                l[at + i] = (1.0 - i as f32 / 2000.0) * if i % 2 == 0 { 0.8 } else { -0.8 };
            }
        }
        let d = AudioData::from_planar(rate, l.clone(), l);
        let s = detect_slices(&d, 0.5, 16);
        assert_eq!(s.len(), 4, "{s:?}");
    }
}
