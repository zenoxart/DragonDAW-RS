//! Transmuter: a wavetable synth. Three wavetable oscillators (position, warp, up to 16 unison voices),
//! a sub and a noise oscillator, two filters (low / high / band pass, notch, comb, formant) in series or
//! parallel, four envelopes and four LFOs feeding a modulation matrix, macros, mono / legato / glide and an
//! FX rack of stock effects.
//!
//! Modulation is evaluated every 32 samples per voice and the values that move the sound (table
//! position, pitch, cutoff, levels) glide across those 32 samples, so nothing steps.

use super::{midi_to_hz, InstrumentVoice, NoteOn};
use crate::dsp::{EnvGen, Rng, Svf};
use crate::effects::{self, Effect};
use crate::model::Instrument as Model;
use crate::samples::SampleCache;
use crate::wt::*;
use std::sync::Arc;

const MAX_VOICES: usize = 32;
const MAX_UNISON: usize = 16;
const CHUNK: usize = 32;
const COMB: usize = 4096;

#[derive(Clone, Copy)]
struct OscChunk {
    on: bool,
    uni: usize,
    level: (f32, f32),
    pos: (f32, f32),
    warp: (f32, f32),
    ratio: (f64, f64),
    spread: [f64; MAX_UNISON],
    gains: [(f32, f32, f32); MAX_UNISON],
    inv_norm: f32,
    band: f32,
}

impl Default for OscChunk {
    fn default() -> Self {
        OscChunk { on: false, uni: 1, level: (0.0, 0.0), pos: (0.0, 0.0), warp: (0.0, 0.0), ratio: (1.0, 1.0), spread: [1.0; MAX_UNISON], gains: [(0.0, 0.0, 0.0); MAX_UNISON], inv_norm: 1.0, band: 0.0 }
    }
}

#[derive(Clone, Copy, Default)]
struct Mods {
    pos: [f32; 3],
    warp: [f32; 3],
    pitch: [f32; 3],
    level: [f32; 3],
    pan: [f32; 3],
    detune: [f32; 3],
    sub: f32,
    noise: f32,
    cutoff: [f32; 2],
    reso: [f32; 2],
    drive: [f32; 2],
    mix: [f32; 2],
    lfo_rate: [f32; 4],
    pitch_all: f32,
    master: f32,
}

/// One filter of one voice (both channels).
#[derive(Clone)]
struct VFilter {
    svf: [[Svf; 2]; 2],
    formant: [[Svf; 3]; 2],
    comb: Vec<[f32; 2]>,
    w: usize,
}

impl VFilter {
    fn new() -> Self {
        VFilter { svf: Default::default(), formant: Default::default(), comb: vec![[0.0; 2]; COMB], w: 0 }
    }

    fn reset(&mut self) {
        self.svf = Default::default();
        self.formant = Default::default();
        self.comb.iter_mut().for_each(|x| *x = [0.0; 2]);
    }

    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn process(&mut self, kind: FilterKind, x: [f32; 2], cutoff: f32, reso: f32, drive: f32, mix: f32, rate: f32) -> [f32; 2] {
        let mut out = [0.0; 2];
        let g = 1.0 + drive * 8.0;
        for c in 0..2 {
            let d = if drive > 0.001 { (x[c] * g).tanh() / g.sqrt() } else { x[c] };
            let y = match kind {
                FilterKind::Lp12 => self.svf[c][0].process(d, cutoff, reso, rate).0,
                FilterKind::Lp24 => {
                    let a = self.svf[c][0].process(d, cutoff, reso * 0.6, rate).0;
                    self.svf[c][1].process(a, cutoff, reso * 0.6, rate).0
                }
                FilterKind::Hp12 => self.svf[c][0].process(d, cutoff, reso, rate).2,
                FilterKind::Hp24 => {
                    let a = self.svf[c][0].process(d, cutoff, reso * 0.6, rate).2;
                    self.svf[c][1].process(a, cutoff, reso * 0.6, rate).2
                }
                FilterKind::Bp => self.svf[c][0].process(d, cutoff, reso, rate).1,
                FilterKind::Notch => {
                    let (l, _, h) = self.svf[c][0].process(d, cutoff, reso, rate);
                    l + h
                }
                FilterKind::CombPlus | FilterKind::CombMinus => {
                    let delay = (rate / cutoff.max(20.0)).clamp(2.0, (COMB - 2) as f32);
                    let r = (self.w as f32 - delay).rem_euclid(COMB as f32);
                    let i = r as usize;
                    let t = r - i as f32;
                    let a = self.comb[i % COMB][c];
                    let b = self.comb[(i + 1) % COMB][c];
                    let dl = a + (b - a) * t;
                    let fb = 0.2 + reso.clamp(0.0, 1.0) * 0.75;
                    let y = if kind == FilterKind::CombPlus { d + fb * dl } else { d - fb * dl };
                    self.comb[self.w][c] = y.clamp(-4.0, 4.0);
                    y * (1.0 - fb * 0.5)
                }
                FilterKind::Formant => {
                    const V: [[f32; 3]; 5] = [[800.0, 1150.0, 2900.0], [400.0, 1600.0, 2700.0], [270.0, 2100.0, 3000.0], [450.0, 800.0, 2830.0], [325.0, 700.0, 2530.0]];
                    // The cutoff (20 Hz … 20 kHz, log) walks through the vowels.
                    let x = ((cutoff.max(20.0) / 20.0).log2() / 10.0).clamp(0.0, 1.0) * 4.0;
                    let i = (x as usize).min(3);
                    let t = x - i as f32;
                    let q = 0.5 + reso * 0.45;
                    let mut s = 0.0;
                    for k in 0..3 {
                        let f = V[i][k] * (1.0 - t) + V[i + 1][k] * t;
                        s += self.formant[c][k].process(d, f, q, rate).1 * [1.0, 0.7, 0.4][k];
                    }
                    s * 1.6
                }
            };
            out[c] = x[c] + (y - x[c]) * mix;
        }
        if matches!(kind, FilterKind::CombPlus | FilterKind::CombMinus) {
            self.w = (self.w + 1) % COMB;
        }
        out
    }
}

#[derive(Clone)]
struct Voice {
    on: bool,
    note: u8,
    vel: f32,
    rnd: f32,
    freq: f64,
    target: f64,
    age: u64,
    released: bool,
    phase: [[f64; MAX_UNISON]; 3],
    sub_phase: f64,
    noise_lp: [f32; 2],
    env: [EnvGen; 4],
    lfo_phase: [f64; 4],
    lfo_time: f32,
    lfo_hold: [f32; 4],
    lfo_prev: [f32; 4],
    lfo_done: [bool; 4],
    filter: [VFilter; 2],
    /// Last oscillator outputs (FM sources).
    last: [f32; 3],
    /// The modulated values at the start of the current chunk (they glide to the next).
    prev: Option<Mods>,
}

impl Voice {
    fn new(rate: f32) -> Self {
        Voice {
            on: false,
            note: 0,
            vel: 0.0,
            rnd: 0.0,
            freq: 440.0,
            target: 440.0,
            age: 0,
            released: false,
            phase: [[0.0; MAX_UNISON]; 3],
            sub_phase: 0.0,
            noise_lp: [0.0; 2],
            env: [EnvGen::new(Default::default(), rate); 4],
            lfo_phase: [0.0; 4],
            lfo_time: 0.0,
            lfo_hold: [0.0; 4],
            lfo_prev: [0.0; 4],
            lfo_done: [false; 4],
            filter: [VFilter::new(), VFilter::new()],
            last: [0.0; 3],
            prev: None,
        }
    }
}

pub struct Wavetable {
    rate: f32,
    p: WavetableParams,
    tables: [Arc<crate::wt::Wavetable>; 3],
    voices: Vec<Voice>,
    rng: Rng,
    counter: u64,
    bpm: f32,
    /// Free-running LFO phases (shared by all notes).
    free_phase: [f64; 4],
    fx: Vec<(String, Box<dyn Effect>)>,
    /// Samples the FX tails still run after the last note.
    tail: usize,
    /// Automated values that override the model: macros 1–4, positions A–C, cutoffs 1–2, master.
    auto_macro: [Option<f32>; 4],
    auto_pos: [Option<f32>; 3],
    auto_cut: [Option<f32>; 2],
    auto_master: Option<f32>,
    scratch: (Vec<f32>, Vec<f32>),
    held: Vec<u8>,
}

impl Wavetable {
    pub fn new(rate: f32) -> Self {
        let p = WavetableParams::default();
        let tables = [table(&p.osc[0].table), table(&p.osc[1].table), table(&p.osc[2].table)];
        Wavetable {
            rate,
            p,
            tables,
            voices: (0..MAX_VOICES).map(|_| Voice::new(rate)).collect(),
            rng: Rng::new(0xD4A6),
            counter: 0,
            bpm: 120.0,
            free_phase: [0.0; 4],
            fx: Vec::new(),
            tail: 0,
            auto_macro: [None; 4],
            auto_pos: [None; 3],
            auto_cut: [None; 2],
            auto_master: None,
            scratch: (vec![0.0; 4096], vec![0.0; 4096]),
            held: Vec::new(),
        }
    }

    fn lfo_hz(&self, k: usize, rate_mod: f32) -> f32 {
        let l = &self.p.lfo[k];
        let base = if l.sync { self.bpm / 60.0 / l.beats.max(1.0 / 64.0) } else { l.rate };
        base * 16f32.powf(rate_mod.clamp(-1.0, 1.0))
    }

    /// The modulation of a voice now (sources → matrix → destinations).
    fn modulate(&mut self, v: &mut Voice, lfo_rate_mod: [f32; 4], dt: f32) -> Mods {
        // LFOs advance by the chunk.
        let mut lfo = [0.0f32; 4];
        for k in 0..4 {
            let l = self.p.lfo[k].clone();
            let hz = self.lfo_hz(k, lfo_rate_mod[k]) as f64;
            let ph = match l.mode {
                LfoMode::Free => self.free_phase[k],
                _ => v.lfo_phase[k],
            };
            let wrapped = ph.rem_euclid(1.0) as f32;
            let mut val = match l.shape {
                LfoShape::Random => v.lfo_hold[k],
                LfoShape::Smooth => v.lfo_prev[k] + (v.lfo_hold[k] - v.lfo_prev[k]) * (wrapped * wrapped * (3.0 - 2.0 * wrapped)),
                s => s.at(wrapped),
            };
            if l.mode == LfoMode::OneShot && v.lfo_done[k] {
                val = l.shape.at(0.999);
            }
            // Delay, then rise.
            let t = v.lfo_time - l.delay;
            let gain = if t < 0.0 {
                0.0
            } else if l.rise > 0.0 {
                (t / l.rise).min(1.0)
            } else {
                1.0
            };
            lfo[k] = if l.unipolar { (val * 0.5 + 0.5) * gain } else { val * gain };
            // Advance.
            let next = ph + hz * dt as f64;
            if next.floor() > ph.floor() {
                v.lfo_prev[k] = v.lfo_hold[k];
                v.lfo_hold[k] = self.rng.next_f32() * 2.0 - 1.0;
                if l.mode == LfoMode::OneShot {
                    v.lfo_done[k] = true;
                }
            }
            if l.mode != LfoMode::Free {
                v.lfo_phase[k] = next;
            }
        }
        v.lfo_time += dt;
        let env = [v.env[0].level(), v.env[1].level(), v.env[2].level(), v.env[3].level()];
        let macros: [f32; 4] = std::array::from_fn(|k| self.auto_macro[k].unwrap_or(self.p.macros[k]));
        let mut m = Mods::default();
        for s in &self.p.matrix {
            if !s.on {
                continue;
            }
            let src = match s.source {
                ModSource::Env(k) => env[k as usize % 4],
                ModSource::Lfo(k) => lfo[k as usize % 4],
                ModSource::Velocity => v.vel,
                ModSource::Note => ((v.note as f32 - 60.0) / 60.0).clamp(-1.0, 1.0),
                ModSource::Random => v.rnd,
                ModSource::Macro(k) => macros[k as usize % 4],
            };
            let a = s.amount * src;
            match s.dest {
                ModDest::OscPos(k) => m.pos[k as usize % 3] += a,
                ModDest::OscWarp(k) => m.warp[k as usize % 3] += a,
                ModDest::OscPitch(k) => m.pitch[k as usize % 3] += a * 24.0,
                ModDest::OscLevel(k) => m.level[k as usize % 3] += a,
                ModDest::OscPan(k) => m.pan[k as usize % 3] += a,
                ModDest::OscDetune(k) => m.detune[k as usize % 3] += a,
                ModDest::SubLevel => m.sub += a,
                ModDest::NoiseLevel => m.noise += a,
                ModDest::FilterCutoff(k) => m.cutoff[k as usize % 2] += a * 8.0,
                ModDest::FilterReso(k) => m.reso[k as usize % 2] += a,
                ModDest::FilterDrive(k) => m.drive[k as usize % 2] += a,
                ModDest::FilterMix(k) => m.mix[k as usize % 2] += a,
                ModDest::LfoRate(k) => m.lfo_rate[k as usize % 4] += a,
                ModDest::Pitch => m.pitch_all += a * 24.0,
                ModDest::Master => m.master += a,
            }
        }
        m
    }

    fn start_voice(&mut self, idx: usize, n: NoteOn, retrigger: bool) {
        let freq = midi_to_hz(n.note as f32 + n.detune) as f64;
        let rnd = self.rng.next_f32();
        let mut ph = [[0.0f64; MAX_UNISON]; 3];
        for (o, row) in ph.iter_mut().enumerate() {
            let osc = &self.p.osc[o];
            for x in row.iter_mut() {
                *x = (osc.phase + osc.random_phase * self.rng.next_f32()) as f64;
            }
        }
        let glide = self.p.glide > 0.0;
        let counter = self.counter;
        let env_p = self.p.env;
        let v = &mut self.voices[idx];
        let was_on = v.on && !v.released;
        v.note = n.note;
        v.vel = n.velocity;
        v.target = freq;
        if !(glide && was_on) {
            v.freq = freq;
        }
        v.released = false;
        v.age = counter;
        if retrigger || !was_on {
            v.rnd = rnd;
            v.phase = ph;
            v.sub_phase = 0.0;
            v.lfo_phase = [0.0; 4];
            v.lfo_time = 0.0;
            v.lfo_done = [false; 4];
            for (k, e) in v.env.iter_mut().enumerate() {
                e.set(env_p[k]);
                e.trigger();
            }
            if !v.on {
                v.filter.iter_mut().for_each(VFilter::reset);
                v.prev = None;
            }
        }
        v.on = true;
    }

    fn render_voice(&mut self, v: &mut Voice, l: &mut [f32], r: &mut [f32]) {
        let rate = self.rate;
        let n = l.len();
        let mut start = 0;
        while start < n {
            let len = CHUNK.min(n - start);
            let dt = len as f32 / rate;
            let lfo_mod = v.prev.map(|p| p.lfo_rate).unwrap_or_default();
            let m = self.modulate(v, lfo_mod, dt);
            let m0 = v.prev.unwrap_or(m);
            self.render_chunk(v, &m0, &m, &mut l[start..start + len], &mut r[start..start + len]);
            v.prev = Some(m);
            start += len;
            if v.env[0].is_idle() {
                v.on = false;
                break;
            }
        }
    }

    fn render_chunk(&mut self, v: &mut Voice, m0: &Mods, m1: &Mods, l: &mut [f32], r: &mut [f32]) {
        let rate = self.rate;
        let p = &self.p;
        let len = l.len();
        let glide_k = if p.glide > 0.0 { 1.0 - (-1.0 / (p.glide as f64 * rate as f64)).exp() } else { 1.0 };
        // Per oscillator constants for this chunk.
        let pos_base: [f32; 3] = std::array::from_fn(|k| self.auto_pos[k].unwrap_or(p.osc[k].pos));
        let warp_base: [f32; 3] = std::array::from_fn(|k| p.osc[k].warp_amount);
        let cut_base: [f32; 2] = std::array::from_fn(|k| self.auto_cut[k].unwrap_or(p.filters[k].cutoff));
        let mut rng = std::mem::replace(&mut self.rng, Rng::new(1));
        let key = (v.note as f32 - 60.0) / 12.0;
        let vel_gain = 1.0 - p.velocity * (1.0 - v.vel);
        // Per oscillator for this chunk: levels, positions, warps and pitch ratios at its start and end
        // (glided per sample), the unison spread and gains.
        let oc: [OscChunk; 3] = std::array::from_fn(|o| {
            let osc = &p.osc[o];
            let mut c = OscChunk { on: osc.on, uni: (osc.unison as usize).clamp(1, MAX_UNISON), ..OscChunk::default() };
            if !osc.on {
                return c;
            }
            c.level = ((osc.level + m0.level[o]).clamp(0.0, 1.5), (osc.level + m1.level[o]).clamp(0.0, 1.5));
            if c.level.0 <= 0.0 && c.level.1 <= 0.0 {
                c.on = false;
                return c;
            }
            c.pos = ((pos_base[o] + m0.pos[o]).clamp(0.0, 1.0), (pos_base[o] + m1.pos[o]).clamp(0.0, 1.0));
            c.warp = ((warp_base[o] + m0.warp[o]).clamp(0.0, 1.0), (warp_base[o] + m1.warp[o]).clamp(0.0, 1.0));
            let semis = |m: &Mods| osc.octave as f32 * 12.0 + osc.semi as f32 + osc.fine / 100.0 + m.pitch[o] + m.pitch_all;
            c.ratio = (super::note_ratio(semis(m0)), super::note_ratio(semis(m1)));
            let detune = (osc.detune + m1.detune[o]).clamp(0.0, 1.0);
            let pan = (osc.pan + m1.pan[o]).clamp(-1.0, 1.0);
            let mut norm = 0.0f32;
            for u in 0..c.uni {
                let off = if c.uni == 1 { 0.0 } else { u as f32 / (c.uni - 1) as f32 * 2.0 - 1.0 };
                c.spread[u] = super::note_ratio(off * detune * 0.5);
                // The centre voice(s) against the outer ones (blend).
                let w = if c.uni == 1 || off.abs() < 0.34 { 1.0 - osc.blend * 0.5 } else { osc.blend };
                norm += w * w;
                let up = (pan + off * osc.width).clamp(-1.0, 1.0);
                c.gains[u] = (w * (1.0 - up).min(1.0), w * (1.0 + up).min(1.0), w);
            }
            c.inv_norm = 1.0 / norm.sqrt().max(1e-6);
            let top = (v.target.max(v.freq) * c.ratio.0.max(c.ratio.1) * (1.0 + detune as f64 * 0.03)) as f32;
            let sync = if osc.warp == Warp::Sync { 1.0 + c.warp.0.max(c.warp.1) * 7.0 } else { 1.0 };
            c.band = level_for(top * sync, rate);
            c
        });
        for i in 0..len {
            let t = (i as f32 + 1.0) / len as f32;
            let lerp = |a: f32, b: f32| a + (b - a) * t;
            v.freq += (v.target - v.freq) * glide_k;
            let amp = v.env[0].next();
            for e in v.env[1..].iter_mut() {
                e.next();
            }
            let pitch_all = lerp(m0.pitch_all, m1.pitch_all);
            // Sources into the two filter buses and the direct bus.
            let mut bus = [[0.0f32; 2]; 3];
            let mut add = |route: Route, s: [f32; 2]| {
                let (a, b, d) = match route {
                    Route::FilterA => (1.0, 0.0, 0.0),
                    Route::FilterB => (0.0, 1.0, 0.0),
                    Route::Both => (0.5, 0.5, 0.0),
                    Route::Direct => (0.0, 0.0, 1.0),
                };
                for c in 0..2 {
                    bus[0][c] += s[c] * a;
                    bus[1][c] += s[c] * b;
                    bus[2][c] += s[c] * d;
                }
            };
            // Oscillators in the order B, A, C so FM can use A ← B, B ← A (last sample), C ← B.
            for &o in &[1usize, 0, 2] {
                let c = &oc[o];
                if !c.on {
                    continue;
                }
                let osc = &p.osc[o];
                let level = c.level.0 + (c.level.1 - c.level.0) * t;
                let table = &self.tables[o];
                let pos = c.pos.0 + (c.pos.1 - c.pos.0) * t;
                let warp = c.warp.0 + (c.warp.1 - c.warp.0) * t;
                let f0 = v.freq * (c.ratio.0 + (c.ratio.1 - c.ratio.0) * t as f64) / rate as f64;
                let fm_src = match o {
                    0 => v.last[1],
                    1 => v.last[0],
                    _ => v.last[1],
                };
                let mut sum = [0.0f32; 2];
                let mut mono = 0.0f32;
                for u in 0..c.uni {
                    let inc = f0 * c.spread[u];
                    let ph = v.phase[o][u];
                    let next = ph + inc;
                    v.phase[o][u] = if next >= 1.0 { next - 1.0 } else { next };
                    let wp = match osc.warp {
                        Warp::Sync => (ph * (1.0 + warp as f64 * 7.0)).rem_euclid(1.0),
                        Warp::BendPlus => ph.powf(1.0 - warp as f64 * 0.85),
                        Warp::BendMinus => 1.0 - (1.0 - ph).powf(1.0 - warp as f64 * 0.85),
                        Warp::Pwm => {
                            let w = 0.5 * (1.0 - warp as f64 * 0.95);
                            if ph < w {
                                ph / (2.0 * w).max(1e-6)
                            } else {
                                0.5 + (ph - w) / (2.0 * (1.0 - w))
                            }
                        }
                        Warp::Fm => ph + warp as f64 * 0.6 * fm_src as f64,
                        _ => ph,
                    };
                    let mut s = table.sample(pos, c.band, wp);
                    match osc.warp {
                        Warp::Asym => s = s * (1.0 - warp) - table.sample(pos, c.band, wp + 0.5) * warp,
                        Warp::Fold => s = (s * (1.0 + warp * 6.0) * std::f32::consts::FRAC_PI_2).sin(),
                        Warp::Crush => {
                            let steps = 2f32.powf(1.0 + (1.0 - warp) * 13.0);
                            s = (s * steps).round() / steps;
                        }
                        _ => {}
                    }
                    let (gl, gr, w) = c.gains[u];
                    sum[0] += s * gl;
                    sum[1] += s * gr;
                    mono += s * w;
                }
                let g = level * c.inv_norm;
                v.last[o] = mono * c.inv_norm;
                add(osc.route, [sum[0] * g, sum[1] * g]);
            }
            if p.sub.on {
                let lv = (p.sub.level + lerp(m0.sub, m1.sub)).clamp(0.0, 1.5);
                let f = v.freq * super::note_ratio(p.sub.octave as f32 * 12.0 + pitch_all);
                let dt = f / rate as f64;
                let ph = v.sub_phase;
                v.sub_phase = (ph + dt).rem_euclid(1.0);
                let s = match p.sub.shape {
                    SubShape::Sine => (ph * std::f64::consts::TAU).sin(),
                    SubShape::Triangle => 1.0 - 4.0 * ((ph + 0.25).rem_euclid(1.0) - 0.5).abs(),
                    SubShape::Saw => 2.0 * ph - 1.0 - blep(ph, dt),
                    SubShape::Square => (if ph < 0.5 { 1.0 } else { -1.0 }) + blep(ph, dt) - blep((ph + 0.5).rem_euclid(1.0), dt),
                } as f32
                    * lv;
                let (gl, gr) = ((1.0 - p.sub.pan).min(1.0), (1.0 + p.sub.pan).min(1.0));
                add(p.sub.route, [s * gl, s * gr]);
            }
            if p.noise.on {
                let lv = (p.noise.level + lerp(m0.noise, m1.noise)).clamp(0.0, 1.5);
                let c = p.noise.color;
                // Dark: low-passed; bright: the low-passed part taken away.
                let k = 0.02 + 0.9 * c.min(0.5) * 2.0;
                let mut out = [0.0; 2];
                for (ch, o) in out.iter_mut().enumerate() {
                    let w = rng.next_f32() * 2.0 - 1.0;
                    v.noise_lp[ch] += (w - v.noise_lp[ch]) * k;
                    *o = if c <= 0.5 { v.noise_lp[ch] * (1.0 + (0.5 - c) * 4.0) } else { w - v.noise_lp[ch] * (c - 0.5) * 2.0 } * lv;
                }
                let (gl, gr) = ((1.0 - p.noise.pan).min(1.0), (1.0 + p.noise.pan).min(1.0));
                add(p.noise.route, [out[0] * gl, out[1] * gr]);
            }
            // Filters.
            let mut run = |k: usize, x: [f32; 2]| -> [f32; 2] {
                let f = &p.filters[k];
                if !f.on {
                    return x;
                }
                let cutoff = (cut_base[k] * 2f32.powf(lerp(m0.cutoff[k], m1.cutoff[k]) + key * f.keytrack)).clamp(20.0, 20000.0);
                let reso = (f.resonance + lerp(m0.reso[k], m1.reso[k])).clamp(0.0, 0.99);
                let drive = (f.drive + lerp(m0.drive[k], m1.drive[k])).clamp(0.0, 1.0);
                let mix = (f.mix + lerp(m0.mix[k], m1.mix[k])).clamp(0.0, 1.0);
                v.filter[k].process(f.kind, x, cutoff, reso, drive, mix, rate)
            };
            let out = match p.filter_routing {
                FilterRouting::Serial => {
                    let a = run(0, bus[0]);
                    let b = run(1, [a[0] + bus[1][0], a[1] + bus[1][1]]);
                    [b[0] + bus[2][0], b[1] + bus[2][1]]
                }
                FilterRouting::Parallel => {
                    let a = run(0, bus[0]);
                    let b = run(1, bus[1]);
                    [a[0] + b[0] + bus[2][0], a[1] + b[1] + bus[2][1]]
                }
            };
            let g = amp * vel_gain * (1.0 + lerp(m0.master, m1.master)).max(0.0) * 0.5;
            l[i] += out[0] * g;
            r[i] += out[1] * g;
        }
        self.rng = rng;
    }
}

#[inline]
fn blep(t: f64, dt: f64) -> f64 {
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

impl InstrumentVoice for Wavetable {
    fn note_on(&mut self, n: NoteOn) {
        self.counter += 1;
        self.held.retain(|x| *x != n.note);
        self.held.push(n.note);
        if self.p.mono || self.p.polyphony <= 1 {
            // One voice: glide to the new note; legato keeps the envelopes going while a note is held.
            let idx = self.voices.iter().position(|v| v.on).unwrap_or(0);
            let held = self.voices[idx].on && !self.voices[idx].released;
            let retrigger = !(self.p.legato && held) && !n.slide;
            self.start_voice(idx, n, retrigger);
            for (k, v) in self.voices.iter_mut().enumerate() {
                if k != idx && v.on {
                    v.env[0].choke(0.01);
                    v.released = true;
                }
            }
            return;
        }
        let poly = (self.p.polyphony as usize).clamp(1, MAX_VOICES);
        let active = self.voices.iter().filter(|v| v.on).count();
        let idx = if let Some(i) = self.voices.iter().position(|v| !v.on).filter(|_| active < poly) {
            i
        } else {
            // Steal: the oldest released voice, else the oldest.
            self.voices.iter().enumerate().filter(|(_, v)| v.on).min_by_key(|(_, v)| (!v.released, v.age)).map(|(i, _)| i).unwrap_or(0)
        };
        if self.voices[idx].on {
            // A stolen voice starts clean (its old tail would click).
            self.voices[idx].on = false;
        }
        self.start_voice(idx, n, true);
    }

    fn note_off(&mut self, note: u8) {
        self.held.retain(|x| *x != note);
        if (self.p.mono || self.p.polyphony <= 1) && !self.held.is_empty() {
            // Back to the last note still held.
            let back = *self.held.last().unwrap_or(&note);
            if let Some(v) = self.voices.iter_mut().find(|v| v.on && !v.released) {
                v.note = back;
                v.target = midi_to_hz(back as f32) as f64;
                if self.p.glide <= 0.0 {
                    v.freq = v.target;
                }
            }
            return;
        }
        for v in self.voices.iter_mut().filter(|v| v.on && !v.released && v.note == note) {
            v.released = true;
            for e in v.env.iter_mut() {
                e.release();
            }
        }
    }

    fn choke(&mut self, seconds: f32) {
        self.held.clear();
        for v in self.voices.iter_mut().filter(|v| v.on) {
            v.released = true;
            v.env[0].choke(seconds);
        }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len();
        if self.scratch.0.len() < n {
            self.scratch = (vec![0.0; n], vec![0.0; n]);
        }
        let (mut sl, mut sr) = std::mem::take(&mut self.scratch);
        sl[..n].iter_mut().for_each(|x| *x = 0.0);
        sr[..n].iter_mut().for_each(|x| *x = 0.0);
        // Free LFOs run on whether notes play or not.
        for k in 0..4 {
            let hz = self.lfo_hz(k, 0.0) as f64;
            self.free_phase[k] += hz * n as f64 / self.rate as f64;
        }
        let mut any = false;
        let mut voices = std::mem::take(&mut self.voices);
        for v in voices.iter_mut().filter(|v| v.on) {
            any = true;
            self.render_voice(v, &mut sl[..n], &mut sr[..n]);
        }
        self.voices = voices;
        let master = self.auto_master.unwrap_or(self.p.master);
        for i in 0..n {
            sl[i] *= master;
            sr[i] *= master;
        }
        if any {
            self.tail = (self.rate * 6.0) as usize;
        } else {
            self.tail = self.tail.saturating_sub(n);
        }
        for (_, fx) in self.fx.iter_mut() {
            fx.process(&mut sl[..n], &mut sr[..n], None);
        }
        // The preset's level trim, after the effects so it doesn't change how they sound.
        let trim = 10f32.powf(self.p.trim_db.clamp(-36.0, 36.0) / 20.0);
        for i in 0..n {
            l[i] += sl[i] * trim;
            r[i] += sr[i] * trim;
        }
        self.scratch = (sl, sr);
    }

    fn update(&mut self, model: &Model, _cache: &SampleCache) {
        let Model::Wavetable(p) = model else { return };
        for k in 0..3 {
            if p.osc[k].table != self.p.osc[k].table || self.tables[k].is_empty() {
                self.tables[k] = table(&p.osc[k].table);
            }
        }
        // FX: rebuild when the chain changes, else pass the parameters on.
        let ids: Vec<&str> = p.fx.iter().filter(|f| f.on).map(|f| f.id.as_str()).collect();
        let have: Vec<&str> = self.fx.iter().map(|f| f.0.as_str()).collect();
        if ids != have {
            self.fx = p.fx.iter().filter(|f| f.on).filter_map(|f| effects::create(&f.id, self.rate, &f.params).map(|e| (f.id.clone(), e))).collect();
        } else {
            for ((_, e), f) in self.fx.iter_mut().zip(p.fx.iter().filter(|f| f.on)) {
                for (k, v) in &f.params {
                    e.set_param(k, *v);
                }
            }
        }
        for v in self.voices.iter_mut() {
            for (k, e) in v.env.iter_mut().enumerate() {
                if !v.released {
                    e.set(p.env[k]);
                }
            }
        }
        self.p = (**p).clone();
    }

    fn active_voices(&self) -> usize {
        let v = self.voices.iter().filter(|v| v.on).count();
        // While the FX tails ring the instrument stays awake.
        if v == 0 && self.tail > 0 && !self.fx.is_empty() {
            1
        } else {
            v
        }
    }

    fn set_param(&mut self, id: &str, value: f32) {
        let digit = |prefix: &str, n: usize| id.strip_prefix(prefix).and_then(|d| d.parse::<usize>().ok()).filter(|d| (1..=n).contains(d)).map(|d| d - 1);
        if id == "__bpm" {
            self.bpm = value.max(1.0);
        } else if id == "cutoff" {
            self.auto_cut[0] = Some(value);
        } else if id == "master" {
            self.auto_master = Some(value);
        } else if let Some(k) = digit("macro", 4) {
            self.auto_macro[k] = Some(value);
        } else if let Some(k) = digit("pos", 3) {
            self.auto_pos[k] = Some(value);
        } else if let Some(k) = digit("cutoff", 2) {
            self.auto_cut[k] = Some(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(p: WavetableParams, note: u8, secs: f32) -> (Vec<f32>, Vec<f32>) {
        let rate = 48000.0;
        let mut s = Wavetable::new(rate);
        s.update(&Model::Wavetable(Box::new(p)), &SampleCache::default());
        s.note_on(NoteOn { note, velocity: 1.0, pan: 0.0, detune: 0.0, slide: false });
        let n = (secs * rate) as usize;
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            s.process(a, b);
        }
        (l, r)
    }

    /// The loudest 50 ms of a note (dBFS RMS, both channels), its peak and its biggest jump.
    fn loudness(p: WavetableParams, note: u8, secs: f32) -> (f32, f32, f32) {
        let rate = 48000.0;
        let mut s = Wavetable::new(rate);
        s.set_param("__bpm", 120.0);
        s.update(&Model::Wavetable(Box::new(p)), &SampleCache::default());
        s.note_on(NoteOn { note, velocity: 0.8, pan: 0.0, detune: 0.0, slide: false });
        let n = (secs * rate) as usize;
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            s.process(a, b);
        }
        let win = 2400;
        let mut best = 0.0f32;
        for k in (0..n.saturating_sub(win)).step_by(win / 2) {
            let e: f32 = l[k..k + win].iter().chain(&r[k..k + win]).map(|v| v * v).sum::<f32>() / (2 * win) as f32;
            best = best.max(e);
        }
        let peak = l.iter().chain(&r).fold(0.0f32, |m, v| m.max(v.abs()));
        let jump = l.windows(2).chain(r.windows(2)).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        (10.0 * best.max(1e-12).log10(), peak, jump)
    }

    const TARGET_DB: f32 = -17.0;

    fn audition_secs(c: crate::wt_library::Category) -> f32 {
        use crate::wt_library::Category::*;
        match c {
            Pad | Atmosphere => 5.0,
            Fx => 3.0,
            _ => 1.5,
        }
    }

    #[test]
    fn library_presets_play_cleanly_at_an_even_level() {
        let lib = crate::wt_library::library();
        for p in lib.iter().filter(|p| p.category != crate::wt_library::Category::Signature).step_by(23) {
            let (db, peak, jump) = loudness(p.params.clone(), p.category.audition_note(), audition_secs(p.category));
            assert!(peak.is_finite() && peak < 3.0, "{}: peak {peak}", p.name);
            assert!(jump < 1.5, "{}: jump {jump}", p.name);
            assert!((db - TARGET_DB).abs() < 4.0, "{} ({}): {db:.1} dB", p.name, p.kind);
        }
    }

    /// Measures every generated preset and writes the level corrections (run in release).
    #[test]
    #[ignore]
    fn measure_preset_levels() {
        let lib = crate::wt_library::library();
        let gen: Vec<_> = lib.iter().filter(|p| p.category != crate::wt_library::Category::Signature).collect();
        let old = crate::wt_library::LEVELS;
        let threads = 4;
        let chunk = gen.len().div_ceil(threads);
        let results: Vec<(usize, f32, f32, f32)> = std::thread::scope(|sc| {
            let hs: Vec<_> = gen
                .chunks(chunk)
                .enumerate()
                .map(|(ci, c)| {
                    sc.spawn(move || {
                        c.iter()
                            .enumerate()
                            .map(|(j, p)| {
                                let (db, pk, jp) = loudness(p.params.clone(), p.category.audition_note(), audition_secs(p.category));
                                (ci * chunk + j, db, pk, jp)
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            hs.into_iter().flat_map(|h| h.join().unwrap()).collect()
        });
        let mut levels = vec![0.0f32; gen.len()];
        for (i, db, pk, jp) in results {
            let prev = old.get(i).copied().unwrap_or(0.0);
            let corr = (prev + TARGET_DB - db).clamp(-36.0, 36.0);
            levels[i] = (corr * 10.0).round() / 10.0;
            if pk > 2.0 || jp > 1.0 || db < -50.0 || (prev + TARGET_DB - db).abs() > 36.0 {
                println!("CHECK {} ({}): {db:.1} dB peak {pk:.2} jump {jp:.2}", gen[i].name, gen[i].kind);
            }
        }
        let text = format!("[{}]\n", levels.iter().map(|v| format!("{v:.1}")).collect::<Vec<_>>().join(", "));
        std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/src/wt_levels.in"), text).unwrap();
    }

    fn pitch(x: &[f32], rate: f32) -> f32 {
        let zc = x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        zc as f32 / (x.len() as f32 / rate)
    }

    #[test]
    fn plays_the_note_in_tune_and_stops() {
        let mut p = WavetableParams::default();
        p.osc[0].pos = 0.0; // sine
        let (l, _) = play(p.clone(), 69, 1.0);
        let f = pitch(&l[4800..], 48000.0);
        assert!((f - 440.0).abs() < 3.0, "{f}");
        // Released: silent after the release.
        let mut s = Wavetable::new(48000.0);
        s.update(&Model::Wavetable(Box::new(p)), &SampleCache::default());
        s.note_on(NoteOn { note: 60, velocity: 1.0, pan: 0.0, detune: 0.0, slide: false });
        let mut a = vec![0.0; 4800];
        let mut b = vec![0.0; 4800];
        s.process(&mut a, &mut b);
        s.note_off(60);
        let mut a = vec![0.0; 48000];
        let mut b = vec![0.0; 48000];
        s.process(&mut a, &mut b);
        assert_eq!(s.active_voices(), 0);
        assert!(a[40000..].iter().all(|x| x.abs() < 1e-5));
    }

    #[test]
    fn high_notes_do_not_alias() {
        // A saw at 4 kHz: everything above Nyquist must be gone, so no energy far below the fundamental
        // except DC-free: measure the spectrum between 0 and 3.5 kHz.
        let mut p = WavetableParams::default();
        p.osc[0].pos = 0.66;
        let (l, _) = play(p, 107, 0.5); // B7 ≈ 3951 Hz
        let x = &l[4800..4800 + 16384];
        let mut planner = rustfft::FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(x.len());
        let mut s: Vec<rustfft::num_complex::Complex<f32>> =
            x.iter().enumerate().map(|(i, v)| rustfft::num_complex::Complex::new(v * (0.5 - 0.5 * (i as f32 / x.len() as f32 * std::f32::consts::TAU).cos()), 0.0)).collect();
        fft.process(&mut s);
        let bin = |hz: f32| (hz / 48000.0 * x.len() as f32) as usize;
        let total: f32 = s[1..x.len() / 2].iter().map(|c| c.norm_sqr()).sum();
        let below: f32 = s[bin(100.0)..bin(3500.0)].iter().map(|c| c.norm_sqr()).sum();
        assert!(below / total < 1e-4, "aliasing below the fundamental: {}", below / total);
    }

    #[test]
    fn unison_spreads_into_stereo_and_position_changes_the_tone() {
        let mut p = WavetableParams::default();
        p.osc[0].unison = 7;
        p.osc[0].detune = 0.5;
        let (l, r) = play(p.clone(), 57, 0.5);
        let diff: f32 = l.iter().zip(&r).map(|(a, b)| (a - b).abs()).sum();
        assert!(diff > 10.0, "stereo unison {diff}");
        // Sine vs saw position: the saw is brighter (more of its energy in the differences).
        let bright = |pos: f32| {
            let mut q = WavetableParams::default();
            q.osc[0].pos = pos;
            let (x, _) = play(q, 45, 0.3);
            let d: f32 = x.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum();
            d / x.iter().map(|v| v * v).sum::<f32>()
        };
        assert!(bright(0.66) > bright(0.0) * 3.0, "{} vs {}", bright(0.66), bright(0.0));
    }

    #[test]
    fn the_matrix_moves_the_filter_and_every_preset_plays_cleanly() {
        let mut p = WavetableParams::default();
        p.filters[0].on = true;
        p.filters[0].cutoff = 200.0;
        let (dark, _) = play(p.clone(), 48, 0.4);
        p.matrix.push(ModSlot { source: ModSource::Velocity, dest: ModDest::FilterCutoff(0), amount: 0.6, on: true });
        let (bright, _) = play(p, 48, 0.4);
        let hf = |x: &[f32]| x.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>();
        assert!(hf(&bright) > hf(&dark) * 2.0, "{} vs {}", hf(&bright), hf(&dark));
        for (name, preset) in presets() {
            let (l, r) = play(preset, 48, 1.0);
            let peak = l.iter().chain(&r).fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(peak.is_finite() && peak > 0.005 && peak < 4.0, "{name}: peak {peak}");
            let jump = l.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
            assert!(jump < 1.5, "{name}: jump {jump}");
        }
    }

    #[test]
    fn mono_legato_glides_and_returns_to_the_held_note() {
        let mut p = WavetableParams::default();
        p.osc[0].pos = 0.0;
        p.mono = true;
        p.legato = true;
        p.glide = 0.02;
        let mut s = Wavetable::new(48000.0);
        s.update(&Model::Wavetable(Box::new(p)), &SampleCache::default());
        let on = |n: u8| NoteOn { note: n, velocity: 1.0, pan: 0.0, detune: 0.0, slide: false };
        s.note_on(on(57));
        s.note_on(on(69));
        let mut a = vec![0.0; 24000];
        let mut b = vec![0.0; 24000];
        s.process(&mut a, &mut b);
        assert!((pitch(&a[12000..], 48000.0) - 440.0).abs() < 4.0);
        s.note_off(69);
        let mut a = vec![0.0; 24000];
        let mut b = vec![0.0; 24000];
        s.process(&mut a, &mut b);
        assert!((pitch(&a[12000..], 48000.0) - 220.0).abs() < 3.0, "back to the held note");
        assert_eq!(s.voices.iter().filter(|v| v.on).count(), 1);
    }
}
