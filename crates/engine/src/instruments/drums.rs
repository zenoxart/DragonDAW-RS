//! Drum pads (FPC style): each pad is a note with velocity layers, gain, pan, pitch and a choke group.

use super::{note_ratio, InstrumentVoice, NoteOn};
use crate::dsp::pan_law;
use crate::model::{Instrument as Model, Pad};
use crate::samples::{AudioData, SampleCache};
use std::sync::Arc;

struct Hit {
    pad: usize,
    data: Arc<AudioData>,
    pos: f64,
    step: f64,
    gl: f32,
    gr: f32,
    fade: f32,
    fade_step: f32,
}

pub struct DrumPads {
    rate: f32,
    pads: Vec<Pad>,
    layers: Vec<Vec<Option<Arc<AudioData>>>>,
    hits: Vec<Hit>,
}

impl DrumPads {
    pub fn new(rate: f32) -> Self {
        DrumPads { rate, pads: Vec::new(), layers: Vec::new(), hits: Vec::new() }
    }
}

/// The default 16-pad layout (GM drum notes).
pub fn default_pads() -> Vec<Pad> {
    let names = [("Kick", 36), ("Rim", 37), ("Snare", 38), ("Clap", 39), ("Snare 2", 40), ("Low tom", 41), ("Closed hat", 42), ("Mid tom", 43), ("Pedal hat", 44), ("Hi tom", 45), ("Open hat", 46), ("Tom 4", 47), ("Tom 5", 48), ("Crash", 49), ("Tom 6", 50), ("Ride", 51)];
    names.iter().map(|(n, k)| Pad { name: n.to_string(), note: *k, choke: if *k == 42 || *k == 44 || *k == 46 { 1 } else { 0 }, ..Pad::default() }).collect()
}

impl InstrumentVoice for DrumPads {
    fn note_on(&mut self, n: NoteOn) {
        let Some(pi) = self.pads.iter().position(|p| p.note == n.note) else { return };
        let pad = &self.pads[pi];
        let layer = pad.layers.iter().position(|l| n.velocity >= l.velocity_low && n.velocity <= l.velocity_high).or(if pad.layers.is_empty() { None } else { Some(pad.layers.len() - 1) });
        let Some(li) = layer else { return };
        let Some(data) = self.layers[pi].get(li).cloned().flatten() else { return };
        if pad.choke > 0 {
            let choke = pad.choke;
            let fade = 1.0 / (0.01 * self.rate);
            for h in &mut self.hits {
                if self.pads.get(h.pad).is_some_and(|p| p.choke == choke) {
                    h.fade_step = fade;
                }
            }
        }
        let (pl, pr) = pan_law((pad.pan + n.pan).clamp(-1.0, 1.0));
        let g = pad.gain * n.velocity;
        let step = note_ratio(pad.pitch + n.detune) * data.rate as f64 / self.rate as f64;
        self.hits.push(Hit { pad: pi, data, pos: 0.0, step, gl: g * pl * std::f32::consts::FRAC_1_SQRT_2, gr: g * pr * std::f32::consts::FRAC_1_SQRT_2, fade: 1.0, fade_step: 0.0 });
        if self.hits.len() > 64 {
            self.hits.remove(0);
        }
    }

    fn note_off(&mut self, _note: u8) {}

    fn choke(&mut self, seconds: f32) {
        let step = 1.0 / (seconds.max(0.001) * self.rate);
        for h in &mut self.hits {
            h.fade_step = step;
        }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        for h in &mut self.hits {
            for i in 0..l.len() {
                if h.fade <= 0.0 || h.pos as usize >= h.data.frames() {
                    break;
                }
                let (a, b) = h.data.sample_at(h.pos);
                l[i] += a * h.gl * h.fade;
                r[i] += b * h.gr * h.fade;
                h.pos += h.step;
                h.fade -= h.fade_step;
            }
        }
        self.hits.retain(|h| h.fade > 0.0 && (h.pos as usize) < h.data.frames());
    }

    fn update(&mut self, model: &Model, cache: &SampleCache) {
        if let Model::DrumPads(pads) = model {
            self.layers = pads.iter().map(|p| p.layers.iter().map(|l| cache.get(&l.path, self.rate as u32).ok()).collect()).collect();
            self.pads = pads.clone();
        }
    }

    fn active_voices(&self) -> usize {
        self.hits.len()
    }
}
