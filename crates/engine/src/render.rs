//! The renderer: compiles a [`Project`] into an immutable [`Song`] and renders it block by block.
//!
//! The same code drives real-time playback (the device callback) and offline export, so what you hear is
//! what you bounce. Everything that keeps state between blocks – instruments and effects – lives in a
//! [`Pool`] keyed by model id, so recompiling after an edit (a fader move, a new note) never resets a reverb
//! tail or a held note.
//!
//! Timing: audio clips are placed by time (seconds from the tempo map), so they stay sample-accurate under
//! tempo changes. Pattern notes are scheduled at exact frame offsets inside the block. Automation is
//! evaluated per sub-block of at most [`AUTOMATION_BLOCK`] frames.

use crate::dsp::{balance, loudness::LoudnessMeter, DelayLine};
use crate::effects::{self, Effect};
use crate::instruments::{self, InstrumentVoice, NoteOn};
use crate::model::*;
use crate::samples::{AudioData, SampleCache};
use crate::time::TempoMap;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};

/// Frames per automation step (≈ 2.7 ms at 48 kHz).
pub const AUTOMATION_BLOCK: usize = 128;

pub type SharedEffect = Arc<Mutex<Box<dyn Effect>>>;
pub type SharedInstrument = Arc<Mutex<Box<dyn InstrumentVoice>>>;

/// Creates third-party plugin instances (implemented by [`crate::plugins`]).
pub trait PluginFactory: Send {
    fn create_effect(&mut self, plugin: &PluginRef, rate: f32, max_block: usize) -> Option<Box<dyn Effect>>;
    fn create_instrument(&mut self, plugin: &PluginRef, rate: f32, max_block: usize) -> Option<Box<dyn InstrumentVoice>>;
}

/// Stateful processors kept across recompiles.
#[derive(Default)]
pub struct Pool {
    effects: HashMap<Id, (String, SharedEffect)>,
    instruments: HashMap<Id, (String, SharedInstrument)>,
    pub plugins: Option<Box<dyn PluginFactory>>,
}

impl Pool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_plugins(f: Box<dyn PluginFactory>) -> Self {
        Pool { plugins: Some(f), ..Default::default() }
    }

    fn effect(&mut self, ins: &Insert, rate: f32) -> Option<SharedEffect> {
        let key = match &ins.plugin {
            Some(p) => format!("plugin:{}:{}", p.path.display(), p.id),
            None => ins.effect.clone(),
        };
        if let Some((k, e)) = self.effects.get(&ins.id) {
            if *k == key {
                return Some(e.clone());
            }
        }
        let e: Box<dyn Effect> = match &ins.plugin {
            Some(p) => self.plugins.as_mut()?.create_effect(p, rate, 4096)?,
            None => effects::create(&ins.effect, rate, &ins.params)?,
        };
        let shared = Arc::new(Mutex::new(e));
        self.effects.insert(ins.id, (key, shared.clone()));
        Some(shared)
    }

    fn instrument(&mut self, ch: &RackChannel, rate: f32, cache: &SampleCache) -> Option<SharedInstrument> {
        let key = match &ch.instrument {
            Instrument::Plugin(p) => format!("plugin:{}:{}", p.path.display(), p.id),
            other => other.kind_name().to_string(),
        };
        if let Some((k, i)) = self.instruments.get(&ch.id) {
            if *k == key {
                return Some(i.clone());
            }
        }
        let i: Box<dyn InstrumentVoice> = match &ch.instrument {
            Instrument::Plugin(p) => self.plugins.as_mut()?.create_instrument(p, rate, 4096)?,
            m => instruments::create(m, rate, cache)?,
        };
        let shared = Arc::new(Mutex::new(i));
        self.instruments.insert(ch.id, (key, shared.clone()));
        Some(shared)
    }

    /// Drops processors whose owner no longer exists.
    fn retain(&mut self, effects: &HashSet<Id>, instruments: &HashSet<Id>) {
        self.effects.retain(|k, _| effects.contains(k));
        self.instruments.retain(|k, _| instruments.contains(k));
    }

    pub fn effect_by_id(&self, id: Id) -> Option<SharedEffect> {
        self.effects.get(&id).map(|e| e.1.clone())
    }

    pub fn instrument_by_id(&self, id: Id) -> Option<SharedInstrument> {
        self.instruments.get(&id).map(|e| e.1.clone())
    }
}

// ---------------------------------------------------------------------------------------------------- song

#[derive(Clone)]
struct CClip {
    start_beat: f64,
    end_beat: f64,
    start_sec: f64,
    data: Arc<AudioData>,
    /// Source frame at the clip start.
    src_offset: f64,
    /// Source frames per output frame.
    step: f64,
    reverse: bool,
    gain_l: f32,
    gain_r: f32,
    fade_in: f64,
    fade_out: f64,
    curve: FadeCurve,
    gain_line: Vec<GainPoint>,
}

struct CTrack {
    strip: usize,
    clips: Vec<CClip>,
    input: Option<InputSource>,
    monitor: bool,
}

#[derive(Debug, Clone, Copy)]
struct SEvent {
    beat: f64,
    chan: usize,
    on: bool,
    note: u8,
    velocity: f32,
    pan: f32,
    detune: f32,
    slide: bool,
}

struct CChannel {
    #[allow(dead_code)]
    id: Id,
    strip: usize,
    volume: f32,
    pan: f32,
    pitch: f32,
    audible: bool,
    cut_group: u8,
    cut_by: u8,
    cut_self: bool,
    instrument: Option<SharedInstrument>,
}

struct CInsert {
    id: Id,
    effect: SharedEffect,
    enabled: bool,
    mix: f32,
    sidechain: Option<usize>,
}

struct CStrip {
    id: Id,
    volume: f32,
    pan: f32,
    width: f32,
    phase: bool,
    swap: bool,
    audible: bool,
    is_vca: bool,
    inserts: Vec<CInsert>,
    /// (target strip, level, pre-fader)
    sends: Vec<(usize, f32, bool)>,
    output: Option<usize>,
    input: Option<InputSource>,
    latency: usize,
}

/// An immutable, compiled snapshot of a project.
pub struct Song {
    pub rate: f32,
    pub tempo: TempoMap,
    pub base_bpm: f64,
    pub loop_range: Option<Range>,
    pub punch: Option<Range>,
    pub metronome: Metronome,
    pub length_beats: f64,
    signatures: Vec<TimeSignature>,
    tracks: Vec<CTrack>,
    channels: Vec<CChannel>,
    strips: Vec<CStrip>,
    order: Vec<usize>,
    master: usize,
    events: Vec<SEvent>,
    /// Events of each pattern for pattern mode, keyed by pattern id: (events, length in beats).
    pattern_events: HashMap<Id, (Vec<SEvent>, f64)>,
    automation: Vec<AutomationLane>,
    strip_index: HashMap<Id, usize>,
    channel_index: HashMap<Id, usize>,
    insert_index: HashMap<Id, (usize, usize)>,
    /// Edge delays for plugin delay compensation: (from strip, to strip, is_send) → samples.
    edge_delay: HashMap<(usize, usize, bool), usize>,
    /// Total latency of the master output (for the transport display).
    pub output_latency: usize,
    pub warnings: Vec<String>,
}

/// Cache of time-stretched / pitch-shifted clip audio.
#[derive(Default, Clone)]
pub struct StretchCache {
    inner: Arc<Mutex<HashMap<String, Arc<AudioData>>>>,
}

impl StretchCache {
    fn get(&self, key: &str, make: impl FnOnce() -> AudioData) -> Arc<AudioData> {
        if let Some(d) = self.inner.lock().unwrap().get(key) {
            return d.clone();
        }
        let d = Arc::new(make());
        self.inner.lock().unwrap().insert(key.to_string(), d.clone());
        d
    }
}

pub struct CompileContext<'a> {
    pub cache: &'a SampleCache,
    pub stretch: &'a StretchCache,
    pub pool: &'a mut Pool,
    pub rate: f32,
}

fn fade_gain(curve: FadeCurve, x: f64) -> f32 {
    let x = x.clamp(0.0, 1.0);
    (match curve {
        FadeCurve::Linear => x,
        FadeCurve::EqualPower => (x * std::f64::consts::FRAC_PI_2).sin(),
        FadeCurve::Exponential => x * x,
    }) as f32
}

impl Song {
    pub fn compile(p: &Project, cx: &mut CompileContext) -> Song {
        let rate = cx.rate;
        let tempo = tempo_map_with_automation(p);
        let mut warnings = Vec::new();

        // --- mixer strips
        let all: Vec<&MixerChannel> = p.mixer.all().collect();
        let strip_index: HashMap<Id, usize> = all.iter().enumerate().map(|(i, c)| (c.id, i)).collect();
        let master = 0usize;
        let any_solo = all.iter().any(|c| c.solo);
        let vca_gain = |c: &MixerChannel| -> f32 { c.vca.and_then(|v| p.mixer.get(v)).filter(|v| v.kind == ChannelKind::Vca).map(|v| if v.muted { 0.0 } else { v.volume }).unwrap_or(1.0) };
        let out_of = |c: &MixerChannel| -> Option<usize> {
            if c.id == MASTER {
                None
            } else {
                Some(c.output.and_then(|o| strip_index.get(&o).copied()).unwrap_or(master))
            }
        };
        // Strips that lead into a soloed strip (their audio is needed) or are soloed themselves.
        let mut solo_ok: HashSet<usize> = HashSet::new();
        if any_solo {
            let mut changed = true;
            for (i, c) in all.iter().enumerate() {
                if c.solo || c.solo_safe {
                    solo_ok.insert(i);
                }
            }
            // Upstream sources of soloed strips.
            while changed {
                changed = false;
                for (i, c) in all.iter().enumerate() {
                    if solo_ok.contains(&i) {
                        continue;
                    }
                    let targets: Vec<usize> = out_of(c).into_iter().chain(c.sends.iter().filter_map(|s| strip_index.get(&s.target).copied())).collect();
                    if targets.iter().any(|t| solo_ok.contains(t) && all[*t].solo) {
                        solo_ok.insert(i);
                        changed = true;
                    }
                }
            }
            solo_ok.insert(master);
        }
        let mut effect_ids = HashSet::new();
        let mut strips: Vec<CStrip> = Vec::with_capacity(all.len());
        for (i, c) in all.iter().enumerate() {
            let mut inserts = Vec::new();
            let mut latency = 0;
            for ins in &c.inserts {
                effect_ids.insert(ins.id);
                let Some(e) = cx.pool.effect(ins, rate) else {
                    warnings.push(format!("Effect '{}' on '{}' could not be loaded", ins.plugin.as_ref().map(|p| p.name.as_str()).unwrap_or(&ins.effect), c.name));
                    continue;
                };
                {
                    let mut g = e.lock().unwrap();
                    match &ins.plugin {
                        None => {
                            if let Some(def) = effects::find(&ins.effect) {
                                for prm in def.params {
                                    g.set_param(prm.id, effects::value(def, &ins.params, prm.id));
                                }
                            }
                        }
                        // Parameters edited in the host (generic editor, automation base values).
                        Some(pl) => {
                            for (id, v) in &pl.params {
                                g.set_param(&id.to_string(), *v as f32);
                            }
                        }
                    }
                    g.set_tempo(p.bpm);
                    if ins.enabled {
                        latency += g.latency();
                    }
                }
                inserts.push(CInsert { id: ins.id, effect: e, enabled: ins.enabled, mix: ins.mix.clamp(0.0, 1.0), sidechain: ins.sidechain.and_then(|s| strip_index.get(&s).copied()).filter(|&s| s != i) });
            }
            let audible = !c.muted && (!any_solo || solo_ok.contains(&i));
            strips.push(CStrip {
                id: c.id,
                volume: c.volume * vca_gain(c),
                pan: c.pan,
                width: c.width,
                phase: c.phase_invert,
                swap: c.swap_lr,
                audible,
                is_vca: c.kind == ChannelKind::Vca,
                inserts,
                sends: c.sends.iter().filter_map(|s| strip_index.get(&s.target).map(|&t| (t, s.level, s.pre_fader))).filter(|(t, _, _)| *t != i).collect(),
                output: out_of(c),
                input: c.input,
                latency,
            });
        }

        // --- processing order (sources before targets, sidechain sources first); drop cycle edges.
        let n = strips.len();
        let mut edges: Vec<Vec<usize>> = vec![Vec::new(); n];
        for (i, s) in strips.iter().enumerate() {
            if let Some(o) = s.output {
                edges[i].push(o);
            }
            for (t, _, _) in &s.sends {
                edges[i].push(*t);
            }
            for ins in &s.inserts {
                if let Some(sc) = ins.sidechain {
                    edges[sc].push(i);
                }
            }
        }
        let order = topo_order(&edges);
        let order = match order {
            Some(o) => o,
            None => {
                warnings.push("Routing contains a feedback loop; sends and sidechains forming it are ignored".into());
                for s in &mut strips {
                    s.sends.clear();
                    for ins in &mut s.inserts {
                        ins.sidechain = None;
                    }
                }
                let edges: Vec<Vec<usize>> = strips.iter().map(|s| s.output.into_iter().collect()).collect();
                topo_order(&edges).unwrap_or_else(|| (0..n).rev().collect())
            }
        };

        // --- plugin delay compensation: align every input of a strip to its slowest input.
        let mut out_latency = vec![0usize; n];
        let mut edge_delay = HashMap::new();
        {
            let mut incoming: Vec<Vec<(usize, bool)>> = vec![Vec::new(); n];
            for (i, s) in strips.iter().enumerate() {
                if let Some(o) = s.output {
                    incoming[o].push((i, false));
                }
                for (t, _, _) in &s.sends {
                    incoming[*t].push((i, true));
                }
            }
            for &i in &order {
                let in_lat = incoming[i].iter().map(|(s, _)| out_latency[*s]).max().unwrap_or(0);
                for &(s, is_send) in &incoming[i] {
                    let d = in_lat - out_latency[s];
                    if d > 0 {
                        edge_delay.insert((s, i, is_send), d);
                    }
                }
                out_latency[i] = in_lat + strips[i].latency;
            }
        }

        // --- channels
        let any_ch_solo = p.channels.iter().any(|c| c.solo);
        let mut instrument_ids = HashSet::new();
        let mut channels = Vec::new();
        for c in &p.channels {
            instrument_ids.insert(c.id);
            let inst = cx.pool.instrument(c, rate, cx.cache);
            match &inst {
                Some(i) => {
                    let mut g = i.lock().unwrap();
                    g.update(&c.instrument, cx.cache);
                    if let Instrument::Plugin(pl) = &c.instrument {
                        for (id, v) in &pl.params {
                            g.set_param(&id.to_string(), *v as f32);
                        }
                    }
                }
                None => warnings.push(format!("Instrument of '{}' could not be loaded", c.name)),
            }
            // The sampler takes the channel envelope.
            if let (Some(i), Instrument::Sampler(_)) = (&inst, &c.instrument) {
                // Envelope is applied through a dedicated hook on the sampler.
                let mut g = i.lock().unwrap();
                g.set_param("__env_enabled", if c.envelope.enabled { 1.0 } else { 0.0 });
                for (k, v) in [
                    ("__env_delay", c.envelope.delay),
                    ("__env_attack", c.envelope.attack),
                    ("__env_hold", c.envelope.hold),
                    ("__env_decay", c.envelope.decay),
                    ("__env_sustain", c.envelope.sustain),
                    ("__env_release", c.envelope.release),
                    ("__env_atens", c.envelope.attack_tension),
                    ("__env_rtens", c.envelope.release_tension),
                ] {
                    g.set_param(k, v);
                }
            }
            channels.push(CChannel {
                id: c.id,
                strip: strip_index.get(&c.mixer).copied().unwrap_or(master),
                volume: c.volume,
                pan: c.pan,
                pitch: c.pitch,
                audible: !c.muted && (!any_ch_solo || c.solo),
                cut_group: c.cut_group,
                cut_by: c.cut_by,
                cut_self: c.cut_self,
                instrument: inst,
            });
        }
        cx.pool.retain(&effect_ids, &instrument_ids);
        let channel_index: HashMap<Id, usize> = p.channels.iter().enumerate().map(|(i, c)| (c.id, i)).collect();

        // --- pattern events
        let pattern_events: HashMap<Id, (Vec<SEvent>, f64)> = p.patterns.iter().map(|pat| (pat.id, (pattern_to_events(pat, &channel_index, 0.0, pat.length_beats()), pat.length_beats()))).collect();

        // --- playlist: audio clips and pattern clips
        let any_track_solo = p.tracks.iter().any(|t| t.solo);
        let mut tracks = Vec::new();
        let mut events = Vec::new();
        for t in &p.tracks {
            let audible = !t.muted && (!any_track_solo || t.solo);
            let strip = strip_index.get(&t.mixer).copied().unwrap_or(master);
            let mut clips = Vec::new();
            for c in t.clips.iter().filter(|c| !c.muted && audible) {
                match &c.kind {
                    ClipKind::Audio(a) => match compile_clip(c, a, p, &tempo, cx) {
                        Ok(cc) => clips.push(cc),
                        Err(e) => warnings.push(format!("Clip '{}': {e}", c.name)),
                    },
                    ClipKind::Pattern { pattern } => {
                        if let Some(pat) = p.pattern(*pattern) {
                            let len = pat.length_beats().max(0.25);
                            let mut start = c.start;
                            while start < c.end() - 1e-9 {
                                let seg_end = (start + len).min(c.end());
                                events.extend(pattern_to_events(pat, &channel_index, start, seg_end - start));
                                start += len;
                            }
                        }
                    }
                }
            }
            clips.sort_by(|a, b| a.start_beat.total_cmp(&b.start_beat));
            tracks.push(CTrack { strip, clips, input: t.input, monitor: t.armed && t.monitor != MonitorMode::Off || t.monitor == MonitorMode::Input });
        }
        events.sort_by(|a, b| a.beat.total_cmp(&b.beat).then(a.on.cmp(&b.on)));

        let insert_index = strips.iter().enumerate().flat_map(|(si, s)| s.inserts.iter().enumerate().map(move |(ii, ins)| (ins.id, (si, ii)))).collect();
        let output_latency = out_latency[master];
        Song {
            rate,
            tempo,
            base_bpm: p.bpm,
            loop_range: if p.loop_enabled { p.loop_range.filter(|r| r.len() > 0.01) } else { None },
            punch: if p.punch_enabled { p.punch.filter(|r| r.len() > 0.01) } else { None },
            metronome: p.metronome,
            length_beats: p.song_length(),
            signatures: p.time_signatures.clone(),
            tracks,
            channels,
            strips,
            order,
            master,
            events,
            pattern_events,
            automation: p.automation.iter().filter(|a| a.mode != AutomationMode::Off && !a.points.is_empty()).cloned().collect(),
            strip_index,
            channel_index,
            insert_index,
            edge_delay,
            output_latency,
            warnings,
        }
    }

    pub fn beats_per_bar(&self, beat: f64) -> f64 {
        let mut s = (4, 4);
        for t in &self.signatures {
            if t.beat <= beat {
                s = (t.numerator.max(1), t.denominator.max(1));
            }
        }
        s.0 as f64 * 4.0 / s.1 as f64
    }

    /// Beat of the bar start at or before `beat`.
    fn bar_start(&self, beat: f64) -> (f64, f64) {
        // Walk signature regions.
        let mut region_start = 0.0;
        let mut bpb = 4.0;
        for t in &self.signatures {
            if t.beat <= beat {
                region_start = t.beat;
                bpb = t.numerator.max(1) as f64 * 4.0 / t.denominator.max(1) as f64;
            }
        }
        let bars = ((beat - region_start) / bpb).floor();
        (region_start + bars * bpb, bpb)
    }

    pub fn strip_of(&self, id: Id) -> Option<usize> {
        self.strip_index.get(&id).copied()
    }
}

fn tempo_map_with_automation(p: &Project) -> TempoMap {
    if let Some(lane) = p.automation.iter().find(|a| a.target == Target::Tempo && a.mode != AutomationMode::Off && !a.points.is_empty()) {
        // Sample the tempo lane every 1/4 beat into a step map.
        let mut q = p.clone();
        let end = lane.points.last().map(|x| x.beat).unwrap_or(0.0) + 1.0;
        q.tempo_points.clear();
        let mut b = 0.0;
        while b <= end {
            if let Some(v) = lane.value_at(b) {
                q.tempo_points.push(TempoPoint { beat: b, bpm: v as f64 });
            }
            b += 0.25;
        }
        return TempoMap::from_project(&q);
    }
    TempoMap::from_project(p)
}

fn compile_clip(c: &Clip, a: &AudioClip, p: &Project, tempo: &TempoMap, cx: &mut CompileContext) -> Result<CClip, String> {
    let rate = cx.rate;
    let raw = cx.cache.get(&a.source, rate as u32).map_err(|e| e.to_string())?;
    let pitch = a.pitch as f64;
    let (data, step) = match a.stretch {
        StretchMode::Resample => (raw.clone(), a.speed.max(0.01) * 2f64.powf(pitch / 12.0)),
        StretchMode::Stretch => {
            let tempo_factor = a.source_bpm.map(|sb| sb / p.bpm.max(1.0)).unwrap_or(1.0) / a.speed.max(0.01);
            if (tempo_factor - 1.0).abs() < 1e-4 && pitch.abs() < 1e-3 {
                (raw.clone(), 1.0)
            } else {
                let key = format!("{}|{}|{:.5}|{:.4}", cx.cache.resolve(&a.source).display(), rate, tempo_factor, pitch);
                let raw2 = raw.clone();
                let d = cx.stretch.get(&key, move || {
                    let s = crate::dsp::stretch::time_stretch(&raw2, tempo_factor);
                    crate::dsp::stretch::pitch_shift(&s, pitch)
                });
                // Offsets are given in source seconds; the stretched data runs `tempo_factor` times longer.
                (d, 1.0 / tempo_factor)
            }
        }
    };
    let (gl, gr) = balance(a.pan);
    let g = crate::dsp::db_to_gain(a.gain_db);
    // `step` converts output frames to frames of `data`; in Stretch mode `data` is already stretched, so the
    // source offset is scaled into stretched frames and playback advances 1:1.
    let (src_offset, play_step) = match a.stretch {
        StretchMode::Resample => (a.offset * rate as f64, step),
        StretchMode::Stretch => (a.offset * rate as f64 / step, 1.0),
    };
    Ok(CClip {
        start_beat: c.start,
        end_beat: c.end(),
        start_sec: tempo.beat_to_seconds(c.start),
        data,
        src_offset,
        step: play_step,
        reverse: a.reverse,
        gain_l: g * gl,
        gain_r: g * gr,
        fade_in: a.fade_in,
        fade_out: a.fade_out,
        curve: a.fade_curve,
        gain_line: a.gain_line.clone(),
    })
}

fn pattern_to_events(pat: &Pattern, channel_index: &HashMap<Id, usize>, start: f64, length: f64) -> Vec<SEvent> {
    let mut ev = Vec::new();
    let end = start + length;
    let step_beats = 0.25;
    for (cid, data) in &pat.channels {
        let Some(&chan) = channel_index.get(cid) else { continue };
        for (i, s) in data.steps.iter().enumerate().take(pat.steps as usize) {
            if !s.on {
                continue;
            }
            let swing = if i % 2 == 1 { pat.swing.clamp(0.0, 1.0) as f64 * step_beats * 0.5 } else { 0.0 };
            let b = start + i as f64 * step_beats + swing + s.shift.clamp(-0.5, 0.5) as f64 * step_beats;
            if b < start || b >= end {
                continue;
            }
            let base = SEvent { beat: b, chan, on: true, note: 60, velocity: s.velocity, pan: s.pan, detune: s.pitch, slide: false };
            ev.push(base);
            ev.push(SEvent { beat: (b + step_beats).min(end), on: false, ..base });
        }
        for nt in data.notes.iter().filter(|n| !n.muted) {
            let b = start + nt.start as f64 / PPQ as f64;
            if b >= end {
                continue;
            }
            let swing = if (nt.start / (PPQ / 4)) % 2 == 1 && nt.start % (PPQ / 4) == 0 { pat.swing.clamp(0.0, 1.0) as f64 * step_beats * 0.5 } else { 0.0 };
            let b = b + swing;
            let e = (start + nt.end() as f64 / PPQ as f64).min(end);
            let base = SEvent { beat: b, chan, on: true, note: nt.pitch, velocity: nt.velocity, pan: nt.pan, detune: 0.0, slide: nt.slide };
            ev.push(base);
            ev.push(SEvent { beat: e.max(b + 1e-6), on: false, ..base });
        }
    }
    ev.sort_by(|a, b| a.beat.total_cmp(&b.beat).then(a.on.cmp(&b.on)));
    ev
}

/// Kahn's topological sort; `None` on a cycle.
fn topo_order(edges: &[Vec<usize>]) -> Option<Vec<usize>> {
    let n = edges.len();
    let mut indeg = vec![0usize; n];
    for e in edges {
        for &t in e {
            indeg[t] += 1;
        }
    }
    let mut q: Vec<usize> = (0..n).filter(|&i| indeg[i] == 0).collect();
    let mut out = Vec::with_capacity(n);
    while let Some(i) = q.pop() {
        out.push(i);
        for &t in &edges[i] {
            indeg[t] -= 1;
            if indeg[t] == 0 {
                q.push(t);
            }
        }
    }
    (out.len() == n).then_some(out)
}

// ---------------------------------------------------------------------------------------------------- renderer

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum PlayMode {
    /// The playlist.
    #[default]
    Song,
    /// Loops one pattern (Channel Rack play button).
    Pattern(Id),
}

/// What the UI sees of the transport.
#[derive(Debug, Clone, Copy, Default)]
pub struct TransportState {
    pub playing: bool,
    pub recording: bool,
    pub mode: PlayMode,
    /// Playhead in beats (negative during count-in).
    pub beat: f64,
    pub seconds: f64,
    pub bpm: f64,
}

/// Peak meter values per strip (linear) and master loudness, read by the UI.
#[derive(Debug, Default, Clone)]
pub struct MeterValues {
    pub strips: HashMap<Id, (f32, f32)>,
    pub gain_reduction: HashMap<Id, f32>,
    pub momentary: f32,
    pub short_term: f32,
    pub integrated: f32,
    pub true_peak: f32,
    pub voices: usize,
    /// The most recent master samples (mono) for the spectrum analyser.
    pub scope: Vec<f32>,
    pub cpu_load: f32,
    /// Clip launcher: playing (lane, pattern) and queued (lane, pattern or stop).
    pub launched: Vec<(Id, Id)>,
    pub queued: Vec<(Id, Option<Id>)>,
}

#[derive(Clone, Default)]
pub struct Meters {
    inner: Arc<Mutex<MeterValues>>,
}

impl Meters {
    pub fn snapshot(&self) -> MeterValues {
        self.inner.lock().map(|m| m.clone()).unwrap_or_default()
    }
    /// Lets the meters fall (called by the UI each frame).
    pub fn decay(&self, factor: f32) {
        if let Ok(mut m) = self.inner.lock() {
            for v in m.strips.values_mut() {
                v.0 *= factor;
                v.1 *= factor;
            }
        }
    }
}

fn quantize_up(beat: f64, q: f64) -> f64 {
    if q <= 0.0 {
        beat
    } else {
        ((beat / q) - 1e-9).ceil() * q
    }
}

/// Live events from the UI / MIDI input, applied at the next block.
#[derive(Debug, Clone)]
pub enum LiveEvent {
    NoteOn {
        channel: Id,
        note: u8,
        velocity: f32,
    },
    NoteOff {
        channel: Id,
        note: u8,
    },
    AllNotesOff,
    /// Plays a one-shot of an audio file (browser preview) through the master.
    Preview(Arc<AudioData>),
    StopPreview,
    /// Clip launcher: starts `pattern` looping on `lane` (or stops the lane with `None`) at the next multiple
    /// of `quantize` beats (0 = immediately). Launched patterns play on top of the song.
    Launch {
        lane: Id,
        pattern: Option<Id>,
        quantize: f64,
    },
    /// Stops every launched pattern at the next quantize boundary.
    StopAllLaunches {
        quantize: f64,
    },
}

/// A pattern looping in a launcher lane.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Launch {
    lane: Id,
    pattern: Id,
    start: f64,
    end: f64,
}

struct Preview {
    data: Arc<AudioData>,
    pos: usize,
}

pub struct Renderer {
    pub rate: f32,
    pub song: Arc<Song>,
    pub transport: TransportState,
    /// Beat where the next playback starts / where Stop returns to.
    pub start_beat: f64,
    seconds: f64,
    bufs: Vec<(Vec<f32>, Vec<f32>)>,
    pre_fader: Vec<(Vec<f32>, Vec<f32>)>,
    chan_buf: (Vec<f32>, Vec<f32>),
    edge_lines: HashMap<(Id, Id, bool), [DelayLine; 2]>,
    loudness: LoudnessMeter,
    pub meters: Meters,
    live: Vec<LiveEvent>,
    live_notes: Vec<(usize, u8)>,
    preview: Option<Preview>,
    click_phase: f32,
    click_left: usize,
    click_accent: bool,
    count_in: f64,
    scope: Vec<f32>,
    launches: Vec<Launch>,
    queued: Vec<(Id, Option<Id>, f64)>,
    /// When set, the renderer records which strips' signals to tap for recording (unused offline).
    pub last_block_beat: (f64, f64),
}

impl Renderer {
    pub fn new(song: Arc<Song>) -> Self {
        let rate = song.rate;
        Renderer {
            rate,
            transport: TransportState { bpm: song.base_bpm, ..Default::default() },
            song,
            start_beat: 0.0,
            seconds: 0.0,
            bufs: Vec::new(),
            pre_fader: Vec::new(),
            chan_buf: (vec![0.0; AUTOMATION_BLOCK], vec![0.0; AUTOMATION_BLOCK]),
            edge_lines: HashMap::new(),
            loudness: LoudnessMeter::new(rate),
            meters: Meters::default(),
            live: Vec::new(),
            live_notes: Vec::new(),
            preview: None,
            click_phase: 0.0,
            click_left: 0,
            click_accent: false,
            count_in: 0.0,
            scope: Vec::with_capacity(4096),
            launches: Vec::new(),
            queued: Vec::new(),
            last_block_beat: (0.0, 0.0),
        }
    }

    pub fn set_song(&mut self, song: Arc<Song>) {
        self.transport.bpm = song.base_bpm;
        self.song = song;
    }

    /// `true` while the recording count-in is running.
    pub fn counting_in(&self) -> bool {
        self.transport.recording && self.count_in > 0.0
    }

    pub fn push_live(&mut self, e: LiveEvent) {
        self.live.push(e);
    }

    pub fn play(&mut self, mode: PlayMode) {
        self.transport.mode = mode;
        if !self.transport.playing {
            self.transport.beat = match mode {
                PlayMode::Song => self.start_beat,
                PlayMode::Pattern(_) => 0.0,
            };
            self.transport.playing = true;
            self.loudness.reset();
        }
    }

    /// Starts recording; with a count-in the playhead starts `bars` before the start position.
    pub fn record(&mut self, count_in_bars: u32) {
        self.transport.mode = PlayMode::Song;
        let bpb = self.song.beats_per_bar(self.start_beat);
        self.count_in = count_in_bars as f64 * bpb;
        self.transport.beat = self.start_beat - self.count_in;
        self.transport.playing = true;
        self.transport.recording = true;
    }

    pub fn pause(&mut self) {
        self.transport.playing = false;
        self.transport.recording = false;
        self.all_notes_off(0.01);
    }

    pub fn stop(&mut self) {
        self.pause();
        self.transport.beat = self.start_beat;
        self.launches.clear();
        self.queued.clear();
    }

    pub fn seek(&mut self, beat: f64) {
        self.start_beat = beat.max(0.0);
        self.transport.beat = self.start_beat;
        self.all_notes_off(0.005);
    }

    fn all_notes_off(&mut self, fade: f32) {
        for c in &self.song.channels {
            if let Some(i) = &c.instrument {
                if let Ok(mut g) = i.lock() {
                    g.choke(fade);
                }
            }
        }
        self.live_notes.clear();
    }

    fn ensure_bufs(&mut self, frames: usize) {
        let n = self.song.strips.len();
        if self.bufs.len() != n || self.bufs.first().is_some_and(|b| b.0.len() < frames) {
            self.bufs = vec![(vec![0.0; frames], vec![0.0; frames]); n];
            self.pre_fader = vec![(vec![0.0; frames], vec![0.0; frames]); n];
        }
        if self.chan_buf.0.len() < frames {
            self.chan_buf = (vec![0.0; frames], vec![0.0; frames]);
        }
    }

    /// Renders `out_l.len()` frames. `input` holds the device input channels (planar) for monitoring.
    pub fn render(&mut self, out_l: &mut [f32], out_r: &mut [f32], input: &[&[f32]]) {
        let started = std::time::Instant::now();
        let total = out_l.len();
        let mut done = 0;
        while done < total {
            let n = (total - done).min(AUTOMATION_BLOCK);
            let ins: Vec<&[f32]> = input.iter().map(|c| if c.len() >= done + n { &c[done..done + n] } else { &c[0..0] }).collect();
            self.render_block(&mut out_l[done..done + n], &mut out_r[done..done + n], &ins);
            done += n;
        }
        let budget = total as f32 / self.rate;
        let load = started.elapsed().as_secs_f32() / budget.max(1e-6);
        if let Ok(mut m) = self.meters.inner.try_lock() {
            m.cpu_load = m.cpu_load * 0.9 + load * 0.1;
        }
    }

    fn render_block(&mut self, out_l: &mut [f32], out_r: &mut [f32], input: &[&[f32]]) {
        let n = out_l.len();
        self.ensure_bufs(n);
        let song = self.song.clone();
        for b in self.bufs.iter_mut().chain(self.pre_fader.iter_mut()) {
            b.0[..n].iter_mut().for_each(|x| *x = 0.0);
            b.1[..n].iter_mut().for_each(|x| *x = 0.0);
        }

        // ---- live events
        let live = std::mem::take(&mut self.live);
        for e in live {
            match e {
                LiveEvent::NoteOn { channel, note, velocity } => {
                    if let Some(&ci) = song.channel_index.get(&channel) {
                        self.trigger(&song, ci, NoteOn { note, velocity, pan: 0.0, detune: 0.0, slide: false });
                        self.live_notes.push((ci, note));
                    }
                }
                LiveEvent::NoteOff { channel, note } => {
                    if let Some(&ci) = song.channel_index.get(&channel) {
                        if let Some(i) = &song.channels[ci].instrument {
                            i.lock().unwrap().note_off(note);
                        }
                        self.live_notes.retain(|x| *x != (ci, note));
                    }
                }
                LiveEvent::AllNotesOff => self.all_notes_off(0.01),
                LiveEvent::Preview(d) => self.preview = Some(Preview { data: d, pos: 0 }),
                LiveEvent::StopPreview => self.preview = None,
                LiveEvent::Launch { lane, pattern, quantize } => {
                    let at = quantize_up(self.transport.beat.max(0.0), quantize);
                    self.queued.retain(|q| q.0 != lane);
                    self.queued.push((lane, pattern, at));
                }
                LiveEvent::StopAllLaunches { quantize } => {
                    let at = quantize_up(self.transport.beat.max(0.0), quantize);
                    let lanes: Vec<Id> = self.launches.iter().filter(|l| l.end.is_infinite()).map(|l| l.lane).collect();
                    self.queued.clear();
                    for lane in lanes {
                        self.queued.push((lane, None, at));
                    }
                }
            }
        }

        // ---- time span of this block (with loop wrap)
        let playing = self.transport.playing;
        let b0 = self.transport.beat;
        let t0 = if b0 >= 0.0 { song.tempo.beat_to_seconds(b0) } else { b0 * 60.0 / song.base_bpm };
        let t1 = t0 + n as f64 / self.rate as f64;
        let mut b1 = if b0 >= 0.0 { song.tempo.seconds_to_beat(t1) } else { b0 + (t1 - t0) * song.base_bpm / 60.0 };
        let (loop_start, loop_end) = match self.transport.mode {
            PlayMode::Pattern(id) => (0.0, song.pattern_events.get(&id).map(|e| e.1).unwrap_or(4.0)),
            PlayMode::Song => song.loop_range.map(|r| (r.start, r.end)).unwrap_or((f64::NAN, f64::NAN)),
        };
        let mut wrap_at: Option<usize> = None;
        if playing && loop_end.is_finite() && b0 >= 0.0 && b0 < loop_end && b1 >= loop_end {
            let t_end = song.tempo.beat_to_seconds(loop_end);
            wrap_at = Some((((t_end - t0) * self.rate as f64).round() as usize).min(n));
        }
        self.last_block_beat = (b0, b1);

        // ---- instruments (scheduled notes + live)
        if playing {
            let segments: Vec<(usize, usize, f64, f64)> = match wrap_at {
                Some(w) => {
                    let b_mid = loop_end;
                    let t_ls = song.tempo.beat_to_seconds(loop_start);
                    let rest = (n - w) as f64 / self.rate as f64;
                    let b_after = song.tempo.seconds_to_beat(t_ls + rest);
                    b1 = b_after;
                    vec![(0, w, b0, b_mid), (w, n, loop_start, b_after)]
                }
                None => vec![(0, n, b0, b1)],
            };
            for (seg_i, &(f0, f1, sb0, sb1)) in segments.iter().enumerate() {
                if seg_i == 1 {
                    // Loop wrap: release everything that was playing.
                    for c in &song.channels {
                        if let Some(i) = &c.instrument {
                            i.lock().unwrap().choke(0.004);
                        }
                    }
                }
                self.schedule_segment(&song, f0, f1, sb0, sb1);
            }
        } else {
            self.render_instruments_until(&song, 0, n, &[]);
        }

        // ---- audio clips and input monitoring
        if playing {
            let segs: Vec<(usize, usize, f64)> = match wrap_at {
                Some(w) => vec![(0, w, t0), (w, n, song.tempo.beat_to_seconds(loop_start))],
                None => vec![(0, n, t0)],
            };
            if self.transport.mode == PlayMode::Song {
                for (f0, f1, ts) in segs {
                    for tr in &song.tracks {
                        let (bl, br) = &mut self.bufs[tr.strip];
                        for c in &tr.clips {
                            mix_clip(c, &song.tempo, ts, &mut bl[f0..f1], &mut br[f0..f1], self.rate);
                        }
                    }
                }
            }
        }
        for tr in &song.tracks {
            if tr.monitor {
                if let Some(src) = tr.input {
                    add_input(&mut self.bufs[tr.strip], src, input, n);
                }
            }
        }
        for (i, s) in song.strips.iter().enumerate() {
            if let Some(src) = s.input {
                add_input(&mut self.bufs[i], src, input, n);
            }
        }

        // ---- automation values at the block start
        let auto_beat = if playing { b0.max(0.0) } else { self.transport.beat.max(0.0) };
        let mut strip_vol: Vec<f32> = song.strips.iter().map(|s| s.volume).collect();
        let mut strip_pan: Vec<f32> = song.strips.iter().map(|s| s.pan).collect();
        let mut strip_width: Vec<f32> = song.strips.iter().map(|s| s.width).collect();
        let mut strip_mute: Vec<bool> = song.strips.iter().map(|s| !s.audible).collect();
        let mut send_levels: HashMap<(usize, usize), f32> = HashMap::new();
        let mut insert_mix: HashMap<Id, f32> = HashMap::new();
        if playing {
            for lane in &song.automation {
                let Some(v) = lane.value_at(auto_beat) else { continue };
                match &lane.target {
                    Target::MixerVolume(id) => {
                        if let Some(&i) = song.strip_index.get(id) {
                            strip_vol[i] = v.max(0.0);
                        }
                    }
                    Target::MixerPan(id) => {
                        if let Some(&i) = song.strip_index.get(id) {
                            strip_pan[i] = v.clamp(-1.0, 1.0);
                        }
                    }
                    Target::MixerWidth(id) => {
                        if let Some(&i) = song.strip_index.get(id) {
                            strip_width[i] = v.clamp(0.0, 2.0);
                        }
                    }
                    Target::MixerMute(id) => {
                        if let Some(&i) = song.strip_index.get(id) {
                            strip_mute[i] = strip_mute[i] || v >= 0.5;
                        }
                    }
                    Target::SendLevel { channel, target } => {
                        if let (Some(&a), Some(&b)) = (song.strip_index.get(channel), song.strip_index.get(target)) {
                            send_levels.insert((a, b), v.max(0.0));
                        }
                    }
                    Target::InsertParam { insert, param, .. } => {
                        if let Some(&(si, ii)) = song.insert_index.get(insert) {
                            song.strips[si].inserts[ii].effect.lock().unwrap().set_param(param, v);
                        }
                    }
                    Target::InsertMix { insert, .. } => {
                        insert_mix.insert(*insert, v.clamp(0.0, 1.0));
                    }
                    Target::ChannelVolume(_) | Target::ChannelPan(_) | Target::ChannelPitch(_) => {}
                    Target::SynthCutoff(id) | Target::SynthResonance(id) => {
                        if let Some(&ci) = song.channel_index.get(id) {
                            if let Some(i) = &song.channels[ci].instrument {
                                i.lock().unwrap().set_param(if matches!(lane.target, Target::SynthCutoff(_)) { "cutoff" } else { "resonance" }, v);
                            }
                        }
                    }
                    Target::PluginParam { channel, insert, param } => {
                        let key = format!("{param}");
                        match insert.and_then(|i| song.insert_index.get(&i)) {
                            Some(&(si, ii)) => song.strips[si].inserts[ii].effect.lock().unwrap().set_param(&key, v),
                            None => {
                                if let Some(&ci) = song.channel_index.get(channel) {
                                    if let Some(i) = &song.channels[ci].instrument {
                                        i.lock().unwrap().set_param(&key, v);
                                    }
                                }
                            }
                        }
                    }
                    Target::Tempo => {}
                }
            }
        }

        // ---- mixer graph
        let mut meter_peaks: Vec<(Id, f32, f32)> = Vec::with_capacity(song.strips.len());
        let mut gr: Vec<(Id, f32)> = Vec::new();
        for &si in &song.order {
            let s = &song.strips[si];
            if s.is_vca {
                continue;
            }
            // Inserts.
            {
                let (bl, br) = &mut self.bufs[si];
                let (bl, br) = (&mut bl[..n], &mut br[..n]);
                for ins in &s.inserts {
                    if !ins.enabled {
                        continue;
                    }
                    let sc = ins.sidechain.map(|sc| (self.pre_fader[sc].0[..n].to_vec(), self.pre_fader[sc].1[..n].to_vec()));
                    let mix = insert_mix.get(&ins.id).copied().unwrap_or(ins.mix);
                    let mut e = ins.effect.lock().unwrap();
                    if mix >= 0.999 {
                        e.process(bl, br, sc.as_ref().map(|(a, b)| (a.as_slice(), b.as_slice())));
                    } else {
                        let dry: (Vec<f32>, Vec<f32>) = (bl.to_vec(), br.to_vec());
                        e.process(bl, br, sc.as_ref().map(|(a, b)| (a.as_slice(), b.as_slice())));
                        for k in 0..n {
                            bl[k] = dry.0[k] * (1.0 - mix) + bl[k] * mix;
                            br[k] = dry.1[k] * (1.0 - mix) + br[k] * mix;
                        }
                    }
                    let g = e.gain_reduction();
                    if g > 0.01 {
                        gr.push((ins.id, g));
                    }
                }
                self.pre_fader[si].0[..n].copy_from_slice(bl);
                self.pre_fader[si].1[..n].copy_from_slice(br);
            }
            // Pre-fader sends.
            for &(t, level, pre) in &s.sends {
                if pre {
                    let lvl = send_levels.get(&(si, t)).copied().unwrap_or(level);
                    self.route(si, t, true, lvl, n, true);
                }
            }
            // Fader, pan, width, polarity, swap, mute.
            {
                let (bl, br) = &mut self.bufs[si];
                let vol = if strip_mute[si] { 0.0 } else { strip_vol[si] };
                let (gl, gr_) = balance(strip_pan[si]);
                let w = strip_width[si];
                let mut pl = 0.0f32;
                let mut pr = 0.0f32;
                for k in 0..n {
                    let (mut a, mut b) = (bl[k], br[k]);
                    if s.swap {
                        std::mem::swap(&mut a, &mut b);
                    }
                    if s.phase {
                        a = -a;
                        b = -b;
                    }
                    if (w - 1.0).abs() > 1e-3 {
                        let m = (a + b) * 0.5;
                        let sd = (a - b) * 0.5 * w;
                        a = m + sd;
                        b = m - sd;
                    }
                    a *= vol * gl;
                    b *= vol * gr_;
                    bl[k] = a;
                    br[k] = b;
                    pl = pl.max(a.abs());
                    pr = pr.max(b.abs());
                }
                meter_peaks.push((s.id, pl, pr));
            }
            // Post-fader sends and output.
            for &(t, level, pre) in &s.sends {
                if !pre {
                    let lvl = send_levels.get(&(si, t)).copied().unwrap_or(level);
                    self.route(si, t, true, lvl, n, false);
                }
            }
            if let Some(o) = s.output {
                self.route(si, o, false, 1.0, n, false);
            }
        }

        // ---- master out, metronome, preview
        let m = song.master;
        out_l.copy_from_slice(&self.bufs[m].0[..n]);
        out_r.copy_from_slice(&self.bufs[m].1[..n]);
        self.loudness.process(out_l, out_r);
        for k in 0..n {
            self.scope.push((out_l[k] + out_r[k]) * 0.5);
        }
        if self.scope.len() > 4096 {
            let cut = self.scope.len() - 4096;
            self.scope.drain(..cut);
        }
        if playing {
            self.metronome(&song, b0, n, out_l, out_r);
        }
        if let Some(p) = &mut self.preview {
            for k in 0..n {
                if p.pos >= p.data.frames() {
                    break;
                }
                out_l[k] += p.data.l[p.pos] * 0.8;
                out_r[k] += p.data.r[p.pos] * 0.8;
                p.pos += 1;
            }
            if p.pos >= p.data.frames() {
                self.preview = None;
            }
        }

        // ---- advance transport
        if playing {
            self.transport.beat = b1;
            if self.transport.recording && self.count_in > 0.0 && b1 >= self.start_beat {
                self.count_in = 0.0;
            }
            if let PlayMode::Song = self.transport.mode {
                if song.loop_range.is_none() && !self.transport.recording && b1 > song.length_beats + 8.0 && song.length_beats > 0.0 {
                    // Ran past the end of the song: stop and return.
                    self.stop();
                }
            }
        }
        self.transport.seconds = if self.transport.beat >= 0.0 { song.tempo.beat_to_seconds(self.transport.beat) } else { 0.0 };
        self.transport.bpm = song.tempo.bpm_at(self.transport.beat.max(0.0));
        self.seconds = self.transport.seconds;

        // ---- meters
        if let Ok(mut mv) = self.meters.inner.try_lock() {
            for (id, l, r) in meter_peaks {
                let e = mv.strips.entry(id).or_insert((0.0, 0.0));
                e.0 = e.0.max(l);
                e.1 = e.1.max(r);
            }
            mv.gain_reduction.clear();
            for (id, g) in gr {
                mv.gain_reduction.insert(id, g);
            }
            mv.momentary = self.loudness.momentary();
            mv.short_term = self.loudness.short_term();
            mv.integrated = self.loudness.integrated();
            mv.true_peak = self.loudness.true_peak;
            mv.launched = self.launches.iter().filter(|l| l.end.is_infinite()).map(|l| (l.lane, l.pattern)).collect();
            mv.queued = self.queued.iter().map(|q| (q.0, q.1)).collect();
            mv.voices = song.channels.iter().filter_map(|c| c.instrument.as_ref()).map(|i| i.lock().map(|g| g.active_voices()).unwrap_or(0)).sum();
            mv.scope.clear();
            mv.scope.extend_from_slice(&self.scope);
        }
    }

    /// Adds strip `from` (post-fader, or pre-fader when `pre`) into strip `to`, through the PDC delay.
    fn route(&mut self, from: usize, to: usize, is_send: bool, level: f32, n: usize, pre: bool) {
        let song = &self.song;
        let delay = song.edge_delay.get(&(from, to, is_send)).copied().unwrap_or(0);
        let src = if pre { &self.pre_fader[from] } else { &self.bufs[from] };
        let (a, b) = (src.0[..n].to_vec(), src.1[..n].to_vec());
        let key = (song.strips[from].id, song.strips[to].id, is_send);
        let (tl, tr) = &mut self.bufs[to];
        if delay > 0 {
            let lines = self.edge_lines.entry(key).or_insert_with(|| [DelayLine::new(delay + 1), DelayLine::new(delay + 1)]);
            if lines[0].len() < delay {
                *lines = [DelayLine::new(delay + 1), DelayLine::new(delay + 1)];
            }
            for k in 0..n {
                lines[0].write(a[k]);
                lines[1].write(b[k]);
                tl[k] += lines[0].read(delay as f32) * level;
                tr[k] += lines[1].read(delay as f32) * level;
            }
        } else {
            for k in 0..n {
                tl[k] += a[k] * level;
                tr[k] += b[k] * level;
            }
        }
    }

    fn trigger(&mut self, song: &Song, ci: usize, on: NoteOn) {
        let c = &song.channels[ci];
        // Voice cutting: this channel's trigger chokes channels of its "by" group (and itself with cut-self).
        if c.cut_by > 0 {
            for (k, o) in song.channels.iter().enumerate() {
                if k != ci && o.cut_group == c.cut_by {
                    if let Some(i) = &o.instrument {
                        i.lock().unwrap().choke(0.01);
                    }
                }
            }
        }
        if let Some(i) = &c.instrument {
            let mut g = i.lock().unwrap();
            if c.cut_self && !on.slide {
                g.choke(0.005);
            }
            g.note_on(NoteOn { detune: on.detune + c.pitch, ..on });
        }
    }

    /// Schedules the pattern events of `[sb0, sb1)` at frames `[f0, f1)` and renders instruments.
    fn schedule_segment(&mut self, song: &Song, f0: usize, f1: usize, sb0: f64, sb1: f64) {
        let events: &[SEvent] = match self.transport.mode {
            PlayMode::Song => &song.events,
            PlayMode::Pattern(id) => song.pattern_events.get(&id).map(|e| e.0.as_slice()).unwrap_or(&[]),
        };
        let a = events.partition_point(|e| e.beat < sb0);
        let b = events.partition_point(|e| e.beat < sb1);
        let t_seg0 = song.tempo.beat_to_seconds(sb0.max(0.0));
        let mut sched: Vec<(usize, SEvent)> = Vec::with_capacity(b - a);
        let to_frame = |beat: f64| f0 + (((song.tempo.beat_to_seconds(beat) - t_seg0) * self.rate as f64).round().max(0.0) as usize).min(f1 - f0 - 1);
        for e in &events[a..b] {
            if sb0 < 0.0 {
                break;
            }
            sched.push((to_frame(e.beat), *e));
        }
        if sb0 >= 0.0 && self.transport.mode == PlayMode::Song {
            // Clip launcher: activate queued launches whose boundary falls in this segment.
            let due: Vec<(Id, Option<Id>, f64)> = self.queued.iter().filter(|q| q.2 < sb1).copied().collect();
            self.queued.retain(|q| q.2 >= sb1);
            for (lane, pattern, at) in due {
                for l in self.launches.iter_mut().filter(|l| l.lane == lane && l.end.is_infinite()) {
                    l.end = at;
                }
                if let Some(pattern) = pattern {
                    self.launches.push(Launch { lane, pattern, start: at, end: f64::INFINITY });
                }
            }
            for l in &self.launches {
                let Some((evs, len)) = song.pattern_events.get(&l.pattern) else { continue };
                // A lane that stops in this segment releases its channels.
                if l.end >= sb0 && l.end < sb1 {
                    let at = to_frame(l.end);
                    for e in evs.iter().filter(|e| e.on) {
                        sched.push((at, SEvent { on: false, beat: l.end, ..*e }));
                    }
                }
                if *len <= 0.0 || l.start >= sb1 || l.end <= sb0 {
                    continue;
                }
                let first = ((sb0 - l.start) / len).floor().max(0.0) as i64;
                let last = ((sb1 - l.start) / len).floor() as i64;
                for k in first..=last {
                    let off = l.start + k as f64 * len;
                    for e in evs {
                        let beat = e.beat + off;
                        // Note-offs are always delivered (so a stopped lane releases its notes); note-ons only
                        // while the launch is active.
                        if beat >= sb0 && beat < sb1 && (beat < l.end || !e.on) && beat >= l.start {
                            sched.push((to_frame(beat), SEvent { beat, ..*e }));
                        }
                    }
                }
            }
            self.launches.retain(|l| l.end >= sb1);
            sched.sort_by_key(|e| e.0);
        }
        self.render_instruments_until(song, f0, f1, &sched);
    }

    /// Renders every channel's instrument over `[f0, f1)`, applying `events` (frame, event) in order.
    fn render_instruments_until(&mut self, song: &Song, f0: usize, f1: usize, events: &[(usize, SEvent)]) {
        let mut cursor = f0;
        let mut i = 0;
        while cursor < f1 {
            let next = events.get(i).map(|e| e.0).unwrap_or(f1).max(cursor).min(f1);
            if next > cursor {
                self.render_channels(song, cursor, next);
                cursor = next;
            }
            while let Some(&(off, e)) = events.get(i) {
                if off > cursor {
                    break;
                }
                if song.channels[e.chan].audible {
                    if e.on {
                        self.trigger(song, e.chan, NoteOn { note: e.note, velocity: e.velocity, pan: e.pan, detune: e.detune, slide: e.slide });
                    } else if let Some(inst) = &song.channels[e.chan].instrument {
                        inst.lock().unwrap().note_off(e.note);
                    }
                }
                i += 1;
            }
            if i >= events.len() && cursor >= f1 {
                break;
            }
        }
    }

    fn render_channels(&mut self, song: &Song, f0: usize, f1: usize) {
        let n = f1 - f0;
        if self.chan_buf.0.len() < n {
            self.chan_buf = (vec![0.0; n], vec![0.0; n]);
        }
        for c in &song.channels {
            let Some(inst) = &c.instrument else { continue };
            let (cl, cr) = (&mut self.chan_buf.0[..n], &mut self.chan_buf.1[..n]);
            cl.iter_mut().for_each(|x| *x = 0.0);
            cr.iter_mut().for_each(|x| *x = 0.0);
            {
                let mut g = inst.lock().unwrap();
                if g.active_voices() == 0 {
                    continue;
                }
                g.process(cl, cr);
            }
            let (gl, gr) = balance(c.pan);
            let (bl, br) = &mut self.bufs[c.strip];
            for k in 0..n {
                bl[f0 + k] += cl[k] * c.volume * gl;
                br[f0 + k] += cr[k] * c.volume * gr;
            }
        }
    }

    fn metronome(&mut self, song: &Song, b0: f64, n: usize, out_l: &mut [f32], out_r: &mut [f32]) {
        let m = song.metronome;
        let counting_in = self.transport.recording && b0 < self.start_beat;
        let active = counting_in || (m.enabled && (!m.only_when_recording || self.transport.recording));
        if !active {
            return;
        }
        let beat_secs = 60.0 / song.tempo.bpm_at(b0.max(0.0));
        for k in 0..n {
            let b = b0 + k as f64 / self.rate as f64 / beat_secs;
            let prev = b0 + (k as f64 - 1.0) / self.rate as f64 / beat_secs;
            if b.floor() != prev.floor() || (k == 0 && (b - b.round()).abs() < 1e-9) {
                let (bar_start, _) = song.bar_start(b.max(0.0));
                self.click_accent = (b.floor() - bar_start).abs() < 1e-6 || (b < 0.0 && (b - self.start_beat + self.count_in).rem_euclid(song.beats_per_bar(0.0)) < 1e-6);
                self.click_left = (self.rate * 0.03) as usize;
                self.click_phase = 0.0;
            }
            if self.click_left > 0 {
                let f = if self.click_accent { 1500.0 } else { 1000.0 };
                self.click_phase += f / self.rate;
                let env = self.click_left as f32 / (self.rate * 0.03);
                let s = (self.click_phase * std::f32::consts::TAU).sin() * env * env * m.volume * if self.click_accent { 0.9 } else { 0.6 };
                out_l[k] += s;
                out_r[k] += s;
                self.click_left -= 1;
            }
        }
    }
}

fn add_input(buf: &mut (Vec<f32>, Vec<f32>), src: InputSource, input: &[&[f32]], n: usize) {
    let l = input.get(src.left as usize).filter(|c| c.len() >= n);
    let r = src.right.and_then(|r| input.get(r as usize)).filter(|c| c.len() >= n);
    match (l, r) {
        (Some(a), Some(b)) => {
            for k in 0..n {
                buf.0[k] += a[k];
                buf.1[k] += b[k];
            }
        }
        (Some(a), None) => {
            for k in 0..n {
                buf.0[k] += a[k] * std::f32::consts::FRAC_1_SQRT_2;
                buf.1[k] += a[k] * std::f32::consts::FRAC_1_SQRT_2;
            }
        }
        _ => {}
    }
}

/// Mixes one audio clip into `[l, r)` for the time span starting at `t_start` seconds.
fn mix_clip(c: &CClip, tempo: &TempoMap, t_start: f64, l: &mut [f32], r: &mut [f32], rate: f32) {
    let n = l.len();
    let t_end = t_start + n as f64 / rate as f64;
    let clip_end_sec = tempo.beat_to_seconds(c.end_beat);
    if t_end <= c.start_sec || t_start >= clip_end_sec {
        return;
    }
    let len_beats = c.end_beat - c.start_beat;
    let frames = c.data.frames() as f64;
    for k in 0..n {
        let t = t_start + k as f64 / rate as f64;
        if t < c.start_sec || t >= clip_end_sec {
            continue;
        }
        let local = (t - c.start_sec) * rate as f64;
        let pos = if c.reverse { frames - 1.0 - (c.src_offset + local * c.step) } else { c.src_offset + local * c.step };
        if pos < 0.0 || pos >= frames - 1.0 {
            continue;
        }
        let (a, b) = c.data.sample_at(pos);
        let mut g = 1.0f32;
        if c.fade_in > 0.0 || c.fade_out > 0.0 || !c.gain_line.is_empty() {
            let beat = tempo.seconds_to_beat(t) - c.start_beat;
            if c.fade_in > 0.0 && beat < c.fade_in {
                g *= fade_gain(c.curve, beat / c.fade_in);
            }
            if c.fade_out > 0.0 && beat > len_beats - c.fade_out {
                g *= fade_gain(c.curve, (len_beats - beat) / c.fade_out);
            }
            if !c.gain_line.is_empty() {
                g *= crate::dsp::db_to_gain(gain_line_db(&c.gain_line, beat));
            }
        }
        l[k] += a * c.gain_l * g;
        r[k] += b * c.gain_r * g;
    }
}

fn gain_line_db(points: &[GainPoint], beat: f64) -> f32 {
    if beat <= points[0].beat {
        return points[0].db;
    }
    let i = points.partition_point(|p| p.beat <= beat);
    if i >= points.len() {
        return points[points.len() - 1].db;
    }
    let (a, b) = (points[i - 1], points[i]);
    let t = ((beat - a.beat) / (b.beat - a.beat).max(1e-9)) as f32;
    a.db + (b.db - a.db) * t
}

/// Builds a fresh renderer for offline work.
pub fn offline_renderer(p: &Project, cache: &SampleCache, rate: f32, pool: &mut Pool) -> Renderer {
    let stretch = StretchCache::default();
    let song = Song::compile(p, &mut CompileContext { cache, stretch: &stretch, pool, rate });
    Renderer::new(Arc::new(song))
}

/// Groups mixer channels by id for quick lookup in the UI.
pub fn mixer_map(p: &Project) -> BTreeMap<Id, &MixerChannel> {
    p.mixer.all().map(|c| (c.id, c)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::samples::{write_wav, BitDepth};

    fn sine_file(dir: &std::path::Path, name: &str, secs: f32) -> std::path::PathBuf {
        let p = dir.join(name);
        let n = (44100.0 * secs) as usize;
        let s: Vec<f32> = (0..n).map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 44100.0).sin() * 0.5).collect();
        write_wav(&p, &s, &s, 44100, BitDepth::Float32, false, false).unwrap();
        p
    }

    fn render_secs(p: &Project, secs: f32) -> (Vec<f32>, Vec<f32>) {
        let cache = SampleCache::default();
        let mut pool = Pool::new();
        let mut r = offline_renderer(p, &cache, 44100.0, &mut pool);
        r.play(PlayMode::Song);
        let n = (44100.0 * secs) as usize;
        let mut l = vec![0.0; n];
        let mut rr = vec![0.0; n];
        for (a, b) in l.chunks_mut(512).zip(rr.chunks_mut(512)) {
            r.render(a, b, &[]);
        }
        (l, rr)
    }

    #[test]
    fn audio_clip_plays_at_its_position() {
        let dir = tempfile::tempdir().unwrap();
        let f = sine_file(dir.path(), "a.wav", 1.0);
        let mut p = Project::new_default();
        p.bpm = 120.0; // 1 beat = 0.5 s
        let id = p.next_id();
        p.tracks[0].clips.push(Clip { id, name: "a".into(), start: 2.0, length: 1.0, muted: false, color: Rgb::default(), kind: ClipKind::Audio(AudioClip { source: f, ..Default::default() }) });
        let (l, _) = render_secs(&p, 2.0);
        // Silent before beat 2 (1.0 s), sound between 1.0 and 1.5 s, silent after.
        let rms = |a: usize, b: usize| (l[a..b].iter().map(|x| x * x).sum::<f32>() / (b - a) as f32).sqrt();
        assert!(rms(0, 44000) < 1e-4);
        assert!(rms(44200, 66000) > 0.1);
        assert!(rms(66300, 88000) < 1e-4);
    }

    #[test]
    fn moving_a_clip_moves_the_sound() {
        let dir = tempfile::tempdir().unwrap();
        let f = sine_file(dir.path(), "a.wav", 1.0);
        let mut p = Project::new_default();
        p.bpm = 120.0;
        let id = p.next_id();
        p.tracks[0].clips.push(Clip { id, name: "a".into(), start: 0.0, length: 0.5, muted: false, color: Rgb::default(), kind: ClipKind::Audio(AudioClip { source: f, offset: 0.0, ..Default::default() }) });
        let (l, _) = render_secs(&p, 1.0);
        assert!(l[100..10000].iter().any(|x| x.abs() > 0.1));
        p.tracks[0].clips[0].start = 1.0;
        let (l2, _) = render_secs(&p, 1.0);
        assert!(l2[100..20000].iter().all(|x| x.abs() < 1e-4));
        assert!(l2[23000..30000].iter().any(|x| x.abs() > 0.1));
        // Muting the clip silences it.
        p.tracks[0].clips[0].muted = true;
        let (l3, _) = render_secs(&p, 1.0);
        assert!(l3.iter().all(|x| x.abs() < 1e-6));
    }

    #[test]
    fn pattern_steps_trigger_samples_on_time() {
        let dir = tempfile::tempdir().unwrap();
        // A short click as the sample.
        let click = dir.path().join("c.wav");
        let s: Vec<f32> = (0..200).map(|i| if i < 100 { 0.9 } else { 0.0 }).collect();
        write_wav(&click, &s, &s, 44100, BitDepth::Float32, false, false).unwrap();
        let mut p = Project::new_default();
        p.bpm = 120.0;
        let ch = p.add_channel("Kick", Instrument::Sampler(SamplerParams { path: click, ..Default::default() }));
        let pat = p.patterns[0].id;
        let mut steps = vec![Step::default(); 16];
        steps[0].on = true;
        steps[4].on = true;
        p.patterns[0].channels.insert(ch, PatternChannel { steps, notes: vec![] });
        let id = p.next_id();
        p.tracks[0].clips.push(Clip { id, name: "p".into(), start: 0.0, length: 4.0, muted: false, color: Rgb::default(), kind: ClipKind::Pattern { pattern: pat } });
        let (l, _) = render_secs(&p, 1.0);
        // Step 0 at 0 s, step 4 (beat 1) at 0.5 s = 22050 frames.
        let first = l.iter().position(|x| x.abs() > 0.05).unwrap();
        assert!(first < 3, "{first}");
        let second = l[1000..].iter().position(|x| x.abs() > 0.05).unwrap() + 1000;
        assert!((second as i64 - 22050).abs() <= 2, "{second}");
    }

    #[test]
    fn swing_delays_odd_steps() {
        let pat = Pattern {
            id: 1,
            name: "p".into(),
            color: Rgb::default(),
            steps: 16,
            swing: 1.0,
            channels: [(9, PatternChannel { steps: (0..16).map(|i| Step { on: i < 2, ..Step::default() }).collect(), notes: vec![] })].into_iter().collect(),
        };
        let idx: HashMap<Id, usize> = [(9, 0)].into_iter().collect();
        let ev = pattern_to_events(&pat, &idx, 0.0, 4.0);
        let ons: Vec<f64> = ev.iter().filter(|e| e.on).map(|e| e.beat).collect();
        assert_eq!(ons[0], 0.0);
        assert!((ons[1] - 0.375).abs() < 1e-9, "{:?}", ons);
    }

    #[test]
    fn mixer_routing_sends_and_solo() {
        let dir = tempfile::tempdir().unwrap();
        let f = sine_file(dir.path(), "a.wav", 1.0);
        let mut p = Project::new_default();
        let id = p.next_id();
        p.tracks[0].clips.push(Clip { id, name: "a".into(), start: 0.0, length: 4.0, muted: false, color: Rgb::default(), kind: ClipKind::Audio(AudioClip { source: f, ..Default::default() }) });
        let base = render_secs(&p, 0.3).0.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        // Route track 1's mixer channel into a bus at half volume.
        let bus = p.add_mixer_channel("Bus");
        let ch = p.tracks[0].mixer;
        p.mixer.get_mut(ch).unwrap().output = Some(bus);
        p.mixer.get_mut(bus).unwrap().volume = 0.4;
        let bused = render_secs(&p, 0.3).0.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!((bused / base - 0.4).abs() < 0.02, "{bused} {base}");
        // Soloing another channel mutes this path.
        let other = p.tracks[1].mixer;
        p.mixer.get_mut(other).unwrap().solo = true;
        let solo = render_secs(&p, 0.3).0.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(solo < 1e-6);
        // Soloing the bus keeps its source audible.
        p.mixer.get_mut(other).unwrap().solo = false;
        p.mixer.get_mut(bus).unwrap().solo = true;
        let s2 = render_secs(&p, 0.3).0.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!((s2 - bused).abs() < 1e-3);
    }

    #[test]
    fn plugin_delay_compensation_aligns_parallel_paths() {
        let dir = tempfile::tempdir().unwrap();
        let click = dir.path().join("c.wav");
        let mut s = vec![0.0f32; 4410];
        s[0] = 0.5;
        write_wav(&click, &s, &s, 44100, BitDepth::Float32, false, false).unwrap();
        let mut p = Project::new_default();
        let id = p.next_id();
        p.tracks[0].clips.push(Clip { id, name: "a".into(), start: 0.0, length: 1.0, muted: false, color: Rgb::default(), kind: ClipKind::Audio(AudioClip { source: click.clone(), ..Default::default() }) });
        let id2 = p.next_id();
        p.tracks[1].clips.push(Clip { id: id2, name: "b".into(), start: 0.0, length: 1.0, muted: false, color: Rgb::default(), kind: ClipKind::Audio(AudioClip { source: click, ..Default::default() }) });
        // A limiter (look-ahead latency) on track 1 only.
        let m1 = p.tracks[0].mixer;
        let ins = p.next_id();
        p.mixer.get_mut(m1).unwrap().inserts.push(Insert {
            id: ins,
            effect: "limiter".into(),
            enabled: true,
            mix: 1.0,
            params: [("ceiling".to_string(), 0.0)].into_iter().collect(),
            sidechain: None,
            plugin: None,
            preset: String::new(),
        });
        let (l, _) = render_secs(&p, 0.1);
        let peaks: Vec<usize> = l.iter().enumerate().filter(|(_, x)| x.abs() > 0.05).map(|(i, _)| i).collect();
        // Both clicks arrive at the same (delayed) frame – one combined spike.
        assert!(!peaks.is_empty());
        assert!(peaks.iter().all(|&i| (i as i64 - peaks[0] as i64).abs() <= 2), "{peaks:?}");
        assert!(peaks[0] > 50);
    }

    #[test]
    fn automation_moves_the_fader() {
        let dir = tempfile::tempdir().unwrap();
        let f = sine_file(dir.path(), "a.wav", 2.0);
        let mut p = Project::new_default();
        p.bpm = 120.0;
        let id = p.next_id();
        p.tracks[0].clips.push(Clip { id, name: "a".into(), start: 0.0, length: 4.0, muted: false, color: Rgb::default(), kind: ClipKind::Audio(AudioClip { source: f, ..Default::default() }) });
        let ch = p.tracks[0].mixer;
        let lid = p.next_id();
        p.automation.push(AutomationLane {
            id: lid,
            target: Target::MixerVolume(ch),
            points: vec![AutoPoint { beat: 0.0, value: 0.0, tension: 0.0 }, AutoPoint { beat: 2.0, value: 0.0, tension: 0.0 }, AutoPoint { beat: 2.01, value: 1.0, tension: 0.0 }],
            mode: AutomationMode::Read,
            visible: true,
        });
        let (l, _) = render_secs(&p, 1.5);
        assert!(l[1000..40000].iter().all(|x| x.abs() < 1e-6));
        assert!(l[46000..60000].iter().any(|x| x.abs() > 0.1));
    }

    #[test]
    fn loop_wraps_the_playhead() {
        let mut p = Project::new_default();
        p.bpm = 120.0;
        p.loop_range = Some(Range::new(0.0, 1.0));
        p.loop_enabled = true;
        let cache = SampleCache::default();
        let mut pool = Pool::new();
        let mut r = offline_renderer(&p, &cache, 44100.0, &mut pool);
        r.play(PlayMode::Song);
        let mut l = vec![0.0; 512];
        let mut rr = vec![0.0; 512];
        for _ in 0..100 {
            r.render(&mut l, &mut rr, &[]);
            assert!(r.transport.beat >= 0.0 && r.transport.beat < 1.0 + 1e-6, "{}", r.transport.beat);
        }
    }

    #[test]
    fn clip_launcher_starts_on_the_bar_and_stops() {
        let mut p = Project::new_default();
        p.bpm = 120.0;
        let ch = p.add_channel("Synth", Instrument::Synth(SynthParams::default()));
        let pat = p.patterns[0].id;
        p.patterns[0].channels.insert(ch, PatternChannel { steps: vec![], notes: vec![Note { pitch: 57, start: 0, length: PPQ * 4, ..Note::default() }] });
        // No clips on the playlist: only the launcher makes sound.
        let cache = SampleCache::default();
        let mut pool = Pool::new();
        let mut r = offline_renderer(&p, &cache, 48000.0, &mut pool);
        r.play(PlayMode::Song);
        let block = |r: &mut Renderer, secs: f64| {
            let n = (secs * 48000.0) as usize;
            let (mut l, mut rr) = (vec![0.0f32; n], vec![0.0f32; n]);
            for (a, b) in l.chunks_mut(512).zip(rr.chunks_mut(512)) {
                r.render(a, b, &[]);
            }
            l.iter().fold(0.0f32, |m, x| m.max(x.abs()))
        };
        assert!(block(&mut r, 0.25) < 1e-4, "silent before launching");
        // Launch quantised to the bar (4 beats = 2 s at 120 BPM): nothing until beat 4.
        r.push_live(LiveEvent::Launch { lane: 1, pattern: Some(pat), quantize: 4.0 });
        assert!(block(&mut r, 1.5) < 1e-4, "waits for the bar");
        assert!(block(&mut r, 0.5) > 0.05, "plays from the bar");
        assert_eq!(r.meters.snapshot().launched, vec![(1, pat)]);
        // Stop immediately: the note is released.
        r.push_live(LiveEvent::Launch { lane: 1, pattern: None, quantize: 0.0 });
        block(&mut r, 1.0);
        assert!(block(&mut r, 0.3) < 1e-3, "stopped");
        assert!(r.meters.snapshot().launched.is_empty());
    }
}
