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

use crate::dsp::{balance, db_to_gain, gain_to_db, loudness::LoudnessMeter, DelayLine};
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
    /// Per insert: the parameter hand-off to the audio thread and the values last sent.
    inboxes: HashMap<Id, (Arc<Inbox>, ParamSent)>,
    /// Per channel: the instrument settings last applied (an unchanged instrument is not locked).
    applied: HashMap<Id, (Instrument, Envelope)>,
}

/// Parameter values last sent to an effect.
#[derive(Default)]
struct ParamSent {
    builtin: HashMap<&'static str, f32>,
    plugin: HashMap<u32, f64>,
    tempo: f64,
}

/// Which parameter a queued change sets.
#[derive(Clone, Copy, Debug)]
pub enum PKey {
    /// A built-in effect's parameter id.
    Static(&'static str),
    /// A plugin parameter (numeric id).
    Num(u32),
    Tempo,
}

/// Parameter changes for a running effect, handed to the audio thread. The UI never locks a running
/// effect (if the audio thread had to wait for the UI – which the OS may pause at any time – the
/// block would be late: a crackle); the audio thread applies the queue right before it processes the
/// effect, and reports the effect's latency back.
pub struct Inbox {
    q: Mutex<Vec<(PKey, f64)>>,
    pending: std::sync::atomic::AtomicBool,
    latency: std::sync::atomic::AtomicUsize,
    /// The latency changed after a queued change (the song's delay compensation needs a recompile).
    latency_moved: std::sync::atomic::AtomicBool,
}

impl Inbox {
    fn new(latency: usize) -> Self {
        Inbox { q: Mutex::new(Vec::with_capacity(256)), pending: false.into(), latency: latency.into(), latency_moved: false.into() }
    }

    /// Queues changes (UI side).
    pub fn push(&self, changes: impl IntoIterator<Item = (PKey, f64)>) {
        let mut q = self.q.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        q.extend(changes);
        if !q.is_empty() {
            self.pending.store(true, std::sync::atomic::Ordering::Release);
        }
    }

    /// Applies queued changes to `e` (audio side, with the effect already locked). Never waits: if the
    /// UI is queuing right now, the changes land on the next block.
    #[inline]
    pub fn apply(&self, e: &mut dyn Effect) {
        use std::sync::atomic::Ordering;
        if !self.pending.load(Ordering::Acquire) {
            return;
        }
        let Ok(mut q) = self.q.try_lock() else { return };
        let mut buf = [0u8; 12];
        for (k, v) in q.drain(..) {
            match k {
                PKey::Static(id) => e.set_param(id, v as f32),
                PKey::Num(n) => e.set_param(num_str(n, &mut buf), v as f32),
                PKey::Tempo => e.set_tempo(v),
            }
        }
        self.pending.store(false, Ordering::Release);
        let l = e.latency();
        if self.latency.swap(l, Ordering::Relaxed) != l {
            self.latency_moved.store(true, Ordering::Relaxed);
        }
    }

    pub fn latency(&self) -> usize {
        self.latency.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// `n` as decimal text in `buf`, without allocating.
fn num_str(n: u32, buf: &mut [u8; 12]) -> &str {
    let mut i = buf.len();
    let mut n = n;
    loop {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    std::str::from_utf8(&buf[i..]).unwrap_or("0")
}

impl Pool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_plugins(f: Box<dyn PluginFactory>) -> Self {
        Pool { plugins: Some(f), ..Default::default() }
    }

    /// The insert's effect and whether it was just created (not yet seen by the audio thread).
    fn effect(&mut self, ins: &Insert, rate: f32) -> Option<(SharedEffect, bool)> {
        let key = match &ins.plugin {
            Some(p) => format!("plugin:{}:{}", p.path.display(), p.id),
            None => ins.effect.clone(),
        };
        if let Some((k, e)) = self.effects.get(&ins.id) {
            if *k == key {
                return Some((e.clone(), false));
            }
        }
        let e: Box<dyn Effect> = match &ins.plugin {
            Some(p) => self.plugins.as_mut()?.create_effect(p, rate, 4096)?,
            None => effects::create(&ins.effect, rate, &ins.params)?,
        };
        let shared = Arc::new(Mutex::new(e));
        self.effects.insert(ins.id, (key, shared.clone()));
        self.inboxes.remove(&ins.id);
        Some((shared, true))
    }

    /// Sends an effect its parameters: straight in while only this thread knows it, otherwise only the
    /// changed values through its inbox. Returns the inbox and the effect's latency.
    fn send_params(&mut self, ins: &Insert, e: &SharedEffect, fresh: bool, bpm: f64) -> (Arc<Inbox>, usize) {
        let mut changes: Vec<(PKey, f64)> = Vec::new();
        let entry = self.inboxes.entry(ins.id).or_insert_with(|| (Arc::new(Inbox::new(0)), ParamSent::default()));
        let sent = &mut entry.1;
        match &ins.plugin {
            None => {
                if let Some(def) = effects::find(&ins.effect) {
                    for prm in def.params {
                        let v = effects::value(def, &ins.params, prm.id);
                        if fresh || sent.builtin.get(prm.id) != Some(&v) {
                            sent.builtin.insert(prm.id, v);
                            changes.push((PKey::Static(prm.id), v as f64));
                        }
                    }
                }
            }
            // Parameters edited in the host (generic editor, automation base values).
            Some(pl) => {
                for (id, v) in &pl.params {
                    if fresh || sent.plugin.get(id) != Some(v) {
                        sent.plugin.insert(*id, *v);
                        changes.push((PKey::Num(*id), *v));
                    }
                }
            }
        }
        if fresh || sent.tempo != bpm {
            sent.tempo = bpm;
            changes.push((PKey::Tempo, bpm));
        }
        let inbox = entry.0.clone();
        if fresh {
            // Nobody else has it yet: set everything directly and measure its latency.
            let mut g = e.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut buf = [0u8; 12];
            for (k, v) in changes {
                match k {
                    PKey::Static(id) => g.set_param(id, v as f32),
                    PKey::Num(n) => g.set_param(num_str(n, &mut buf), v as f32),
                    PKey::Tempo => g.set_tempo(v),
                }
            }
            let l = g.latency();
            inbox.latency.store(l, std::sync::atomic::Ordering::Relaxed);
            (inbox, l)
        } else {
            if !changes.is_empty() {
                inbox.push(changes);
            }
            let l = inbox.latency();
            (inbox, l)
        }
    }

    /// Queues built-in parameter values for a running insert (live knob moves). Only changed values go.
    pub fn push_builtin(&mut self, ins: Id, def: &effects::EffectDef, params: &std::collections::BTreeMap<String, f32>) {
        let Some((inbox, sent)) = self.inboxes.get_mut(&ins) else { return };
        let mut changes = Vec::new();
        for prm in def.params {
            let v = effects::value(def, params, prm.id);
            if sent.builtin.get(prm.id) != Some(&v) {
                sent.builtin.insert(prm.id, v);
                changes.push((PKey::Static(prm.id), v as f64));
            }
        }
        if !changes.is_empty() {
            inbox.push(changes);
        }
    }

    /// Whether an effect's latency changed since the last call (the song needs a recompile).
    pub fn latency_moved(&self) -> bool {
        self.inboxes.values().fold(false, |any, (i, _)| i.latency_moved.swap(false, std::sync::atomic::Ordering::Relaxed) | any)
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
        self.applied.remove(&ch.id);
        Some(shared)
    }

    /// Drops processors whose owner no longer exists.
    fn retain(&mut self, effects: &HashSet<Id>, instruments: &HashSet<Id>) {
        self.effects.retain(|k, _| effects.contains(k));
        self.inboxes.retain(|k, _| effects.contains(k));
        self.instruments.retain(|k, _| instruments.contains(k));
        self.applied.retain(|k, _| instruments.contains(k));
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
    inbox: Arc<Inbox>,
    enabled: bool,
    mix: f32,
    sidechain: Option<usize>,
    mods: Vec<CMod>,
}

/// A compiled modulation route of a built-in effect.
struct CMod {
    source: crate::model::ModSource,
    /// The XY pad value at compile time (live changes come through automation into `Renderer::xy`).
    xy: f32,
    info: effects::ParamInfo,
    /// The parameter's own value, normalized.
    base: f32,
    depth: f32,
    slew: f32,
}

/// The XY pad parameter of an axis.
pub fn xy_param(axis: u8) -> &'static str {
    if axis == 0 {
        "__xy_x"
    } else {
        "__xy_y"
    }
}

/// Running state of one modulation route (LFO phase, follower level, random step).
#[derive(Default, Clone, Copy)]
struct ModRun {
    phase: f32,
    env: f32,
    step: u64,
    /// The slewed output of the route, and whether it has started.
    out: f32,
    started: bool,
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
    /// Strips in processing order, grouped by dependency level (sources before targets).
    order: Vec<usize>,
    /// `order[a..b]` per dependency level: strips of one level do not feed each other, so their insert
    /// chains can run in parallel.
    levels: Vec<(usize, usize)>,
    /// Per strip: its first insert's index in a flat list of all inserts.
    ins_base: Vec<usize>,
    ins_total: usize,
    master: usize,
    events: Vec<SEvent>,
    /// Events of each pattern for pattern mode, keyed by pattern id: (events, length in beats).
    pattern_events: HashMap<Id, (Vec<SEvent>, f64)>,
    automation: Vec<AutomationLane>,
    /// Automation clips per lane: (start, end) in beats. Lanes listed here follow their clips.
    automation_clips: HashMap<Id, Vec<(f64, f64)>>,
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
        if let Some(d) = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(key) {
            return d.clone();
        }
        let d = Arc::new(make());
        self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(key.to_string(), d.clone());
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
    /// Every note-on of the arrangement: (beat, channel index, note). For checks and analysis.
    pub fn note_ons(&self) -> Vec<(f64, usize, u8)> {
        self.events.iter().filter(|e| e.on).map(|e| (e.beat, e.chan, e.note)).collect()
    }

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
            if c.id == MASTER || !c.main_out {
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
                let Some((e, fresh)) = cx.pool.effect(ins, rate) else {
                    warnings.push(format!("Effect '{}' on '{}' could not be loaded", ins.plugin.as_ref().map(|p| p.name.as_str()).unwrap_or(&ins.effect), c.name));
                    continue;
                };
                // Parameters reach a running effect through its inbox (never by locking it here).
                let (inbox, lat) = cx.pool.send_params(ins, &e, fresh, p.bpm);
                if ins.enabled {
                    latency += lat;
                }
                let mods = match (&ins.plugin, effects::find(&ins.effect)) {
                    (None, Some(def)) => ins
                        .mods
                        .iter()
                        .filter_map(|m| {
                            let info = *def.params.iter().find(|p| p.id == m.target)?;
                            let xy = match m.source {
                                ModSource::Xy { axis } => ins.params.get(xy_param(axis)).copied().unwrap_or(0.5),
                                _ => 0.0,
                            };
                            Some(CMod { source: m.source, xy, info, base: info.normalize(effects::value(def, &ins.params, info.id)), depth: m.depth.clamp(-1.0, 1.0), slew: m.slew.max(0.0) })
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                inserts.push(CInsert { id: ins.id, effect: e, inbox, enabled: ins.enabled, mix: ins.mix.clamp(0.0, 1.0), sidechain: ins.sidechain.and_then(|s| strip_index.get(&s).copied()).filter(|&s| s != i), mods });
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
                // Sidechain-only routes are not heard: the target's effects read the source as their sidechain.
                sends: c.sends.iter().filter(|s| !s.sidechain_only).filter_map(|s| strip_index.get(&s.target).map(|&t| (t, s.level, s.pre_fader))).filter(|(t, _, _)| *t != i).collect(),
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

        // --- dependency levels: a strip's level is one more than the highest level feeding it (outputs,
        // sends and sidechains), so a level's strips are independent of each other.
        let (order, levels) = {
            let mut lvl = vec![0usize; n];
            // In topological order every source is final before the strips it feeds are visited.
            for &i in &order {
                // A sidechain source must be done before the strip that listens to it.
                for ins in &strips[i].inserts {
                    if let Some(sc) = ins.sidechain {
                        if lvl[i] <= lvl[sc] {
                            lvl[i] = lvl[sc] + 1;
                        }
                    }
                }
                let s = &strips[i];
                let li = lvl[i];
                if let Some(o) = s.output {
                    lvl[o] = lvl[o].max(li + 1);
                }
                for (t, _, _) in &s.sends {
                    lvl[*t] = lvl[*t].max(li + 1);
                }
            }
            let mut sorted = order.clone();
            // Stable: within a level the topological order is kept.
            sorted.sort_by_key(|&i| lvl[i]);
            let mut levels = Vec::new();
            let mut a = 0;
            for k in 1..=sorted.len() {
                if k == sorted.len() || lvl[sorted[k]] != lvl[sorted[a]] {
                    levels.push((a, k));
                    a = k;
                }
            }
            (sorted, levels)
        };
        let mut ins_base = Vec::with_capacity(n);
        let mut ins_total = 0;
        for s in &strips {
            ins_base.push(ins_total);
            ins_total += s.inserts.len();
        }

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
            // Only an instrument whose settings changed is locked (the audio thread may be playing it).
            let changed = cx.pool.applied.get(&c.id).is_none_or(|(i, e)| *i != c.instrument || *e != c.envelope);
            match &inst {
                Some(i) if changed => {
                    let mut g = i.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    g.update(&c.instrument, cx.cache);
                    if let Instrument::Plugin(pl) = &c.instrument {
                        for (id, v) in &pl.params {
                            g.set_param(&id.to_string(), *v as f32);
                        }
                    }
                }
                Some(_) => {}
                None => warnings.push(format!("Instrument of '{}' could not be loaded", c.name)),
            }
            // The sampler takes the channel envelope.
            if let (Some(i), Instrument::Sampler(_), true) = (&inst, &c.instrument, changed) {
                // Envelope is applied through a dedicated hook on the sampler.
                let mut g = i.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
            if changed && inst.is_some() {
                cx.pool.applied.insert(c.id, (c.instrument.clone(), c.envelope));
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
                    // Automation clips drive their lane (see `automation_clips`); they make no sound.
                    ClipKind::Automation { .. } => {}
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
            levels,
            ins_base,
            ins_total,
            master,
            events,
            pattern_events,
            automation: p.automation.iter().filter(|a| a.mode != AutomationMode::Off && !a.points.is_empty()).cloned().collect(),
            automation_clips: {
                let mut m: HashMap<Id, Vec<(f64, f64)>> = HashMap::new();
                for t in p.tracks.iter().filter(|t| !t.muted) {
                    for c in t.clips.iter().filter(|c| !c.muted) {
                        if let ClipKind::Automation { lane } = c.kind {
                            m.entry(lane).or_default().push((c.start, c.end()));
                        }
                    }
                }
                m
            },
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
    /// Live display values of effects that report them ([`crate::effects::Effect::telemetry`]).
    pub fx_data: HashMap<Id, Vec<f32>>,
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

/// Writes `v` in decimal into `buf` and returns it as text (no allocation).
fn u32_str(mut v: u32, buf: &mut [u8; 10]) -> &str {
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    std::str::from_utf8(&buf[i..]).unwrap_or("0")
}

/// Per-block working buffers, kept between blocks so the audio thread does not allocate once they have
/// reached their size (an allocation can block on the allocator's lock: a dropout / stutter).
#[derive(Default)]
struct Scratch {
    strip_vol: Vec<f32>,
    strip_pan: Vec<f32>,
    strip_width: Vec<f32>,
    strip_mute: Vec<bool>,
    send_levels: HashMap<(usize, usize), f32>,
    insert_mix: HashMap<Id, f32>,
    meter_peaks: Vec<(Id, f32, f32)>,
    gr: Vec<(Id, f32)>,
    /// Telemetry: (insert, start, len) into `tele_data`.
    tele: Vec<(Id, usize, usize)>,
    tele_data: Vec<f32>,
    sc: (Vec<f32>, Vec<f32>),
    dry: (Vec<f32>, Vec<f32>),
    /// Parallel insert processing: per insert (flat) its (mix, mix at block start); per strip whether it
    /// runs in the parallel pass of the current level, and its own result slot.
    ins_mix: Vec<(f32, f32)>,
    par: Vec<bool>,
    par_list: Vec<usize>,
    slots: Vec<StripSlot>,
}

/// Raw pointers to the per-strip buffers and result slots, for the parallel pass (each job touches only
/// its own strip, so the accesses are disjoint).
#[derive(Clone, Copy)]
struct StripPtrs {
    bufs: *mut (Vec<f32>, Vec<f32>),
    slots: *mut StripSlot,
}
// SAFETY: used only inside `MixPool::run`, where every index (strip) is handled by exactly one thread.
unsafe impl Sync for StripPtrs {}
unsafe impl Send for StripPtrs {}

impl StripPtrs {
    /// SAFETY: `si` must be in range of both vectors and handled by one thread at a time.
    #[allow(clippy::mut_from_ref)]
    unsafe fn strip(&self, si: usize) -> (&mut (Vec<f32>, Vec<f32>), &mut StripSlot) {
        (&mut *self.bufs.add(si), &mut *self.slots.add(si))
    }
}

/// What one strip's insert chain reports when it runs on a worker thread.
#[derive(Default)]
struct StripSlot {
    gr: Vec<(Id, f32)>,
    tele: Vec<(Id, usize, usize)>,
    tele_data: Vec<f32>,
    dry: (Vec<f32>, Vec<f32>),
}

/// Telemetry floats one strip's chain may report per block (the largest displays send a few thousand).
const TELE_ROOM: usize = 16384;

impl StripSlot {
    fn with_room() -> Self {
        StripSlot { gr: Vec::with_capacity(64), tele: Vec::with_capacity(64), tele_data: Vec::with_capacity(TELE_ROOM), dry: (Vec::with_capacity(AUTOMATION_BLOCK), Vec::with_capacity(AUTOMATION_BLOCK)) }
    }
}

pub struct Renderer {
    /// Modulation state per (insert, route).
    mod_runs: HashMap<(Id, usize), ModRun>,
    /// Automated XY pad values per (insert, axis).
    xy: HashMap<(Id, u8), f32>,
    pub rate: f32,
    pub song: Arc<Song>,
    pub transport: TransportState,
    /// Beat where the next playback starts / where Stop returns to.
    pub start_beat: f64,
    seconds: f64,
    bufs: Vec<(Vec<f32>, Vec<f32>)>,
    pre_fader: Vec<(Vec<f32>, Vec<f32>)>,
    chan_buf: (Vec<f32>, Vec<f32>),
    scratch: Scratch,
    /// `route`'s copy of the source (kept between calls).
    route_tmp: (Vec<f32>, Vec<f32>),
    /// Offline analysis: a loudness meter per strip, fed each strip's pre-fader signal (auto-balance).
    strip_meters: Option<Vec<LoudnessMeter>>,
    /// The mixer stage's load (share of the block time) and whether it runs on the worker threads.
    mix_load: f32,
    mix_parallel: bool,
    edge_lines: HashMap<(Id, Id, bool), [DelayLine; 2]>,
    loudness: LoudnessMeter,
    pub meters: Meters,
    live: Vec<LiveEvent>,
    live_notes: Vec<(usize, u8)>,
    /// The last note triggered anywhere (pitch, velocity) – the MIDI note / velocity modulation sources.
    last_note: (u8, f32),
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
    ramps: Ramps,
}

/// Gains applied in the last block, per processor (strip fader, channel, send, insert mix), so a changed value
/// ramps linearly across the next block instead of stepping (a step is an audible click).
#[derive(Default)]
struct Ramps(HashMap<(u8, Id, Id), [f32; 2]>);

impl Ramps {
    const STRIP: u8 = 0;
    const CHANNEL: u8 = 1;
    const SEND: u8 = 2;
    const MIX: u8 = 3;
    const WIDTH: u8 = 4;

    /// Returns the previous gains and stores `target` for the next block.
    fn start(&mut self, key: (u8, Id, Id), target: [f32; 2]) -> [f32; 2] {
        match self.0.get_mut(&key) {
            Some(v) => std::mem::replace(v, target),
            None => {
                self.0.insert(key, target);
                target
            }
        }
    }
}

/// Linear interpolation from `a` to `b` at sample `k` of `n` (reaching `b` on the last sample).
#[inline]
fn ramp(a: f32, b: f32, k: usize, n: usize) -> f32 {
    if a == b {
        b
    } else {
        a + (b - a) * (k + 1) as f32 / n as f32
    }
}

impl Renderer {
    pub fn new(song: Arc<Song>) -> Self {
        let rate = song.rate;
        Renderer {
            mod_runs: HashMap::new(),
            xy: HashMap::new(),
            rate,
            transport: TransportState { bpm: song.base_bpm, ..Default::default() },
            song,
            start_beat: 0.0,
            seconds: 0.0,
            bufs: Vec::new(),
            pre_fader: Vec::new(),
            chan_buf: (vec![0.0; AUTOMATION_BLOCK], vec![0.0; AUTOMATION_BLOCK]),
            scratch: Scratch::default(),
            route_tmp: Default::default(),
            strip_meters: None,
            mix_load: 0.0,
            mix_parallel: false,
            edge_lines: HashMap::new(),
            ramps: Ramps::default(),
            loudness: LoudnessMeter::new(rate),
            meters: Meters::default(),
            live: Vec::new(),
            live_notes: Vec::new(),
            last_note: (60, 0.0),
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

    /// Switches to a new compiled song; returns the old one so the caller can drop it off the audio thread
    /// (freeing a whole song there can block).
    pub fn set_song(&mut self, song: Arc<Song>) -> Arc<Song> {
        self.transport.bpm = song.base_bpm;
        // The song is compiled for the rate the audio actually runs at (the sound card's): the clock must
        // follow it, or playback runs at song.rate / old rate speed (a 48 kHz clock on a 44.1 kHz card
        // plays 8 % slow, so exports sounded faster than the DAW).
        if (song.rate - self.rate).abs() > 0.5 {
            self.rate = song.rate;
            self.loudness = LoudnessMeter::new(song.rate);
        }
        std::mem::replace(&mut self.song, song)
    }

    /// `true` while the recording count-in is running.
    pub fn counting_in(&self) -> bool {
        self.transport.recording && self.count_in > 0.0
    }

    pub fn push_live(&mut self, e: LiveEvent) {
        self.live.push(e);
    }

    pub fn play(&mut self, mode: PlayMode) {
        // Starting the song or a pattern cuts a running browser preview (FL behaviour).
        self.preview = None;
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
        // Denormals in decaying tails cost 10 – 100× (random CPU overloads): flush them on this thread.
        crate::dsp::denormal::flush_to_zero();
        crate::fault::set_current(None);
        let started = std::time::Instant::now();
        let total = out_l.len();
        let mut done = 0;
        while done < total {
            let n = (total - done).min(AUTOMATION_BLOCK);
            // Input channel slices for this chunk, without allocating (up to 32 channels).
            let mut ins: [&[f32]; 32] = [&[]; 32];
            let k = input.len().min(32);
            for (slot, c) in ins.iter_mut().zip(input.iter()) {
                *slot = if c.len() >= done + n { &c[done..done + n] } else { &c[0..0] };
            }
            self.render_block(&mut out_l[done..done + n], &mut out_r[done..done + n], &ins[..k]);
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
                            i.lock().unwrap_or_else(std::sync::PoisonError::into_inner).note_off(note);
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
            let segments: ([(usize, usize, f64, f64); 2], usize) = match wrap_at {
                Some(w) => {
                    let b_mid = loop_end;
                    let t_ls = song.tempo.beat_to_seconds(loop_start);
                    let rest = (n - w) as f64 / self.rate as f64;
                    let b_after = song.tempo.seconds_to_beat(t_ls + rest);
                    b1 = b_after;
                    ([(0, w, b0, b_mid), (w, n, loop_start, b_after)], 2)
                }
                None => ([(0, n, b0, b1), (0, 0, 0.0, 0.0)], 1),
            };
            for (seg_i, &(f0, f1, sb0, sb1)) in segments.0[..segments.1].iter().enumerate() {
                if seg_i == 1 {
                    // Loop wrap: release everything that was playing.
                    for c in &song.channels {
                        if let Some(i) = &c.instrument {
                            i.lock().unwrap_or_else(std::sync::PoisonError::into_inner).choke(0.004);
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
            let segs: ([(usize, usize, f64); 2], usize) = match wrap_at {
                Some(w) => ([(0, w, t0), (w, n, song.tempo.beat_to_seconds(loop_start))], 2),
                None => ([(0, n, t0), (0, 0, 0.0)], 1),
            };
            if self.transport.mode == PlayMode::Song {
                for &(f0, f1, ts) in &segs.0[..segs.1] {
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
        let mut sc = std::mem::take(&mut self.scratch);
        sc.strip_vol.clear();
        sc.strip_vol.extend(song.strips.iter().map(|s| s.volume));
        sc.strip_pan.clear();
        sc.strip_pan.extend(song.strips.iter().map(|s| s.pan));
        sc.strip_width.clear();
        sc.strip_width.extend(song.strips.iter().map(|s| s.width));
        sc.strip_mute.clear();
        sc.strip_mute.extend(song.strips.iter().map(|s| !s.audible));
        sc.send_levels.clear();
        sc.insert_mix.clear();
        let (strip_vol, strip_pan, strip_width, strip_mute) = (&mut sc.strip_vol, &mut sc.strip_pan, &mut sc.strip_width, &mut sc.strip_mute);
        let (send_levels, insert_mix) = (&mut sc.send_levels, &mut sc.insert_mix);
        if playing {
            for lane in &song.automation {
                let v = match song.automation_clips.get(&lane.id) {
                    Some(clips) => lane.value_in_clips(clips, auto_beat),
                    None => lane.value_at(auto_beat),
                };
                let Some(v) = v else { continue };
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
                    Target::InsertParam { insert, param, .. } if param.starts_with("__xy") => {
                        self.xy.insert((*insert, if param.ends_with('y') { 1 } else { 0 }), v);
                    }
                    Target::InsertParam { insert, param, .. } => {
                        if let Some(&(si, ii)) = song.insert_index.get(insert) {
                            song.strips[si].inserts[ii].effect.lock().unwrap_or_else(std::sync::PoisonError::into_inner).set_param(param, v);
                        }
                    }
                    Target::InsertMix { insert, .. } => {
                        insert_mix.insert(*insert, v.clamp(0.0, 1.0));
                    }
                    Target::ChannelVolume(_) | Target::ChannelPan(_) | Target::ChannelPitch(_) => {}
                    Target::SynthCutoff(id) | Target::SynthResonance(id) => {
                        if let Some(&ci) = song.channel_index.get(id) {
                            if let Some(i) = &song.channels[ci].instrument {
                                i.lock().unwrap_or_else(std::sync::PoisonError::into_inner).set_param(if matches!(lane.target, Target::SynthCutoff(_)) { "cutoff" } else { "resonance" }, v);
                            }
                        }
                    }
                    Target::PluginParam { channel, insert, param } => {
                        // The id as text without allocating (u32: at most 10 digits).
                        let mut digits = [0u8; 10];
                        let key = u32_str(*param, &mut digits);
                        match insert.and_then(|i| song.insert_index.get(&i)) {
                            Some(&(si, ii)) => song.strips[si].inserts[ii].effect.lock().unwrap_or_else(std::sync::PoisonError::into_inner).set_param(key, v),
                            None => {
                                if let Some(&ci) = song.channel_index.get(channel) {
                                    if let Some(i) = &song.channels[ci].instrument {
                                        i.lock().unwrap_or_else(std::sync::PoisonError::into_inner).set_param(key, v);
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
        sc.meter_peaks.clear();
        sc.gr.clear();
        sc.tele.clear();
        sc.tele_data.clear();
        // Sizes grow once per song (not per block): room for every strip's reports.
        if sc.tele_data.capacity() < TELE_ROOM * 4 {
            sc.tele_data.reserve(TELE_ROOM * 4);
            sc.tele.reserve(1024);
            sc.gr.reserve(1024);
        }
        if sc.ins_mix.len() < song.ins_total {
            sc.ins_mix.resize(song.ins_total, (1.0, 1.0));
        }
        if sc.par.len() < song.strips.len() {
            sc.par.resize(song.strips.len(), false);
            // Room for everything a chain reports, so no block ever grows these on the audio thread.
            sc.slots.resize_with(song.strips.len(), StripSlot::with_room);
            sc.par_list.reserve(song.strips.len());
        }
        let pool = crate::mixpool::MixPool::global();
        let threads = pool.threads();
        let mix_started = std::time::Instant::now();
        for &(la, lb) in &song.levels {
            let level = &song.order[la..lb];
            // ---- parallel pass: plain insert chains (no sidechain, no modulation) of this level run on all
            // cores; they touch only their own strip. Chains with a sidechain or modulation share state and
            // run in order below.
            let mut eligible = 0;
            sc.par_list.clear();
            for &si in level {
                let s = &song.strips[si];
                let ok = !s.is_vca && s.inserts.iter().any(|i| i.enabled) && s.inserts.iter().all(|i| i.sidechain.is_none() && i.mods.is_empty());
                sc.par[si] = ok;
                if ok {
                    sc.par_list.push(si);
                }
                eligible += ok as usize;
            }
            // Only under load: for a light project the audio thread does it alone and never waits on
            // another thread.
            let parallel = threads > 1 && n >= 128 && eligible >= 2 && (self.mix_parallel || pool.always());
            if parallel {
                for &si in level {
                    if !sc.par[si] {
                        continue;
                    }
                    for (ii, ins) in song.strips[si].inserts.iter().enumerate() {
                        let mix = insert_mix.get(&ins.id).copied().unwrap_or(ins.mix);
                        let mix0 = self.ramps.start((Ramps::MIX, ins.id, 0), [mix, 0.0])[0];
                        sc.ins_mix[song.ins_base[si] + ii] = (mix, mix0);
                    }
                }
                {
                    let (strips, base, mixes, list) = (&song.strips, &song.ins_base, &sc.ins_mix, &sc.par_list);
                    let ptrs = StripPtrs { bufs: self.bufs.as_mut_ptr(), slots: sc.slots.as_mut_ptr() };
                    pool.run(list.len(), &|k| {
                        let si = list[k];
                        // SAFETY: `list` holds distinct strip indices within both vectors, and each index is
                        // handled by exactly one thread; nothing else touches these strips meanwhile.
                        let ((bl, br), slot) = unsafe { ptrs.strip(si) };
                        slot.gr.clear();
                        slot.tele.clear();
                        slot.tele_data.clear();
                        let (bl, br) = (&mut bl[..n], &mut br[..n]);
                        for (ii, ins) in strips[si].inserts.iter().enumerate() {
                            if !ins.enabled {
                                continue;
                            }
                            let (mix, mix0) = mixes[base[si] + ii];
                            crate::fault::set_current(Some(ins.id));
                            let mut e = ins.effect.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                            ins.inbox.apply(&mut **e);
                            e.set_position(b0, playing);
                            if mix >= 0.999 && mix0 >= 0.999 {
                                e.process(bl, br, None);
                            } else {
                                slot.dry.0.clear();
                                slot.dry.0.extend_from_slice(bl);
                                slot.dry.1.clear();
                                slot.dry.1.extend_from_slice(br);
                                e.process(bl, br, None);
                                for k in 0..n {
                                    let m = ramp(mix0, mix, k, n);
                                    bl[k] = slot.dry.0[k] * (1.0 - m) + bl[k] * m;
                                    br[k] = slot.dry.1[k] * (1.0 - m) + br[k] * m;
                                }
                            }
                            guard_finite(&mut **e, ins.id, bl, br);
                            let g = e.gain_reduction();
                            if g > 0.01 {
                                slot.gr.push((ins.id, g));
                            }
                            let t = e.telemetry();
                            if !t.is_empty() {
                                slot.tele.push((ins.id, slot.tele_data.len(), t.len()));
                                slot.tele_data.extend_from_slice(t);
                            }
                        }
                    });
                }
                for &si in level {
                    if !sc.par[si] {
                        continue;
                    }
                    let slot = &sc.slots[si];
                    sc.gr.extend_from_slice(&slot.gr);
                    for &(id, start, len) in &slot.tele {
                        sc.tele.push((id, sc.tele_data.len(), len));
                        sc.tele_data.extend_from_slice(&slot.tele_data[start..start + len]);
                    }
                    self.pre_fader[si].0[..n].copy_from_slice(&self.bufs[si].0[..n]);
                    self.pre_fader[si].1[..n].copy_from_slice(&self.bufs[si].1[..n]);
                }
            }
            let (meter_peaks, gr, tele, tele_data) = (&mut sc.meter_peaks, &mut sc.gr, &mut sc.tele, &mut sc.tele_data);
            let (sc_buf, dry_buf, par_mark) = (&mut sc.sc, &mut sc.dry, &sc.par);
            for &si in level {
                let s = &song.strips[si];
                if s.is_vca {
                    continue;
                }
                // Inserts (unless the parallel pass did them).
                if !(parallel && par_mark[si]) {
                    let (bl, br) = &mut self.bufs[si];
                    let (bl, br) = (&mut bl[..n], &mut br[..n]);
                    for ins in &s.inserts {
                        if !ins.enabled {
                            continue;
                        }
                        let has_sc = if let Some(k) = ins.sidechain {
                            sc_buf.0.clear();
                            sc_buf.0.extend_from_slice(&self.pre_fader[k].0[..n]);
                            sc_buf.1.clear();
                            sc_buf.1.extend_from_slice(&self.pre_fader[k].1[..n]);
                            true
                        } else {
                            false
                        };
                        let sc = has_sc.then_some((sc_buf.0.as_slice(), sc_buf.1.as_slice()));
                        let mix = insert_mix.get(&ins.id).copied().unwrap_or(ins.mix);
                        let mix0 = self.ramps.start((Ramps::MIX, ins.id, 0), [mix, 0.0])[0];
                        let mut e = ins.effect.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                        // Modulation: each route moves its parameter once per block.
                        if !ins.mods.is_empty() {
                            let dt = n as f32 / self.rate;
                            let peak = bl.iter().chain(br.iter()).fold(0.0f32, |m, x| m.max(x.abs()));
                            let bpm = self.transport.bpm as f32;
                            for (k, m) in ins.mods.iter().enumerate() {
                                let run = self.mod_runs.entry((ins.id, k)).or_default();
                                let v = match m.source {
                                    crate::model::ModSource::Lfo { rate, shape, sync } => {
                                        let hz = if sync { rate * bpm / 60.0 } else { rate };
                                        run.phase += hz * dt;
                                        if run.phase >= 1.0 {
                                            run.phase -= run.phase.floor();
                                            run.step = run.step.wrapping_add(1);
                                        }
                                        crate::model::ModSource::lfo_shape(shape, run.phase, run.step)
                                    }
                                    crate::model::ModSource::Envelope { attack, release, gain } => {
                                        let x = peak * db_to_gain(gain);
                                        let tau = if x > run.env { attack } else { release }.max(0.1) * 0.001;
                                        run.env += (x - run.env) * (1.0 - (-dt / tau).exp());
                                        ((gain_to_db(run.env.max(1e-6)) + 60.0) / 60.0).clamp(0.0, 1.0)
                                    }
                                    crate::model::ModSource::Xy { axis } => self.xy.get(&(ins.id, axis)).copied().unwrap_or(m.xy).clamp(0.0, 1.0),
                                    crate::model::ModSource::MidiNote { lo, hi } => {
                                        let (lo, hi) = (lo.min(hi) as f32, lo.max(hi).max(lo.min(hi) + 1) as f32);
                                        ((self.last_note.0 as f32 - lo) / (hi - lo)).clamp(0.0, 1.0)
                                    }
                                    crate::model::ModSource::MidiVelocity => self.last_note.1.clamp(0.0, 1.0),
                                };
                                // Slew: a one-pole glide toward the source value.
                                let v = if m.slew > 0.0 && run.started { run.out + (v - run.out) * (1.0 - (-dt / (m.slew * 0.001)).exp()) } else { v };
                                run.out = v;
                                run.started = true;
                                let nv = (m.base + m.depth * v).clamp(0.0, 1.0);
                                e.set_param(m.info.id, m.info.denormalize(nv));
                            }
                        }
                        ins.inbox.apply(&mut **e);
                        e.set_position(b0, playing);
                        crate::fault::set_current(Some(ins.id));
                        if mix >= 0.999 && mix0 >= 0.999 {
                            e.process(bl, br, sc);
                        } else {
                            dry_buf.0.clear();
                            dry_buf.0.extend_from_slice(bl);
                            dry_buf.1.clear();
                            dry_buf.1.extend_from_slice(br);
                            e.process(bl, br, sc);
                            for k in 0..n {
                                let m = ramp(mix0, mix, k, n);
                                bl[k] = dry_buf.0[k] * (1.0 - m) + bl[k] * m;
                                br[k] = dry_buf.1[k] * (1.0 - m) + br[k] * m;
                            }
                        }
                        guard_finite(&mut **e, ins.id, bl, br);
                        let g = e.gain_reduction();
                        if g > 0.01 {
                            gr.push((ins.id, g));
                        }
                        let t = e.telemetry();
                        if !t.is_empty() {
                            tele.push((ins.id, tele_data.len(), t.len()));
                            tele_data.extend_from_slice(t);
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
                    let (gl, gr_) = (vol * gl, vol * gr_);
                    let [gl0, gr0] = self.ramps.start((Ramps::STRIP, s.id, 0), [gl, gr_]);
                    let w1 = strip_width[si];
                    let w0 = self.ramps.start((Ramps::WIDTH, s.id, 0), [w1, 0.0])[0];
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
                        let w = ramp(w0, w1, k, n);
                        if (w - 1.0).abs() > 1e-3 {
                            let m = (a + b) * 0.5;
                            let sd = (a - b) * 0.5 * w;
                            a = m + sd;
                            b = m - sd;
                        }
                        a *= ramp(gl0, gl, k, n);
                        b *= ramp(gr0, gr_, k, n);
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
        }

        // ---- parallel mixer on / off by load (with hysteresis): the stage's time as a share of the
        // block, scaled up while it runs spread over the workers.
        {
            let budget = n as f32 / self.rate;
            let spread = if self.mix_parallel { (threads as f32 * 0.6).max(1.0) } else { 1.0 };
            let load = mix_started.elapsed().as_secs_f32() / budget.max(1e-6) * spread;
            self.mix_load = self.mix_load * 0.9 + load * 0.1;
            if self.mix_load > 0.30 {
                self.mix_parallel = true;
            } else if self.mix_load < 0.12 {
                self.mix_parallel = false;
            }
        }

        // ---- offline analysis: per-strip pre-fader loudness
        if let Some(ms) = &mut self.strip_meters {
            for (k, m) in ms.iter_mut().enumerate().take(self.pre_fader.len()) {
                m.process(&self.pre_fader[k].0[..n], &self.pre_fader[k].1[..n]);
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
            for &(id, l, r) in sc.meter_peaks.iter() {
                let e = mv.strips.entry(id).or_insert((0.0, 0.0));
                e.0 = e.0.max(l);
                e.1 = e.1.max(r);
            }
            mv.gain_reduction.clear();
            for &(id, g) in sc.gr.iter() {
                mv.gain_reduction.insert(id, g);
            }
            for &(id, start, len) in sc.tele.iter() {
                // Reuse each insert's vector (no allocation once it exists).
                let v = mv.fx_data.entry(id).or_default();
                v.clear();
                v.extend_from_slice(&sc.tele_data[start..start + len]);
            }
            mv.momentary = self.loudness.momentary();
            mv.short_term = self.loudness.short_term();
            mv.integrated = self.loudness.integrated();
            mv.true_peak = self.loudness.true_peak;
            mv.launched.clear();
            mv.launched.extend(self.launches.iter().filter(|l| l.end.is_infinite()).map(|l| (l.lane, l.pattern)));
            mv.queued.clear();
            mv.queued.extend(self.queued.iter().map(|q| (q.0, q.1)));
            mv.voices = song.channels.iter().filter_map(|c| c.instrument.as_ref()).map(|i| i.lock().map(|g| g.active_voices()).unwrap_or(0)).sum();
            mv.scope.clear();
            mv.scope.extend_from_slice(&self.scope);
        }
        self.scratch = sc;
    }

    /// Adds strip `from` (post-fader, or pre-fader when `pre`) into strip `to`, through the PDC delay.
    fn route(&mut self, from: usize, to: usize, is_send: bool, level: f32, n: usize, pre: bool) {
        let song = &self.song;
        let delay = song.edge_delay.get(&(from, to, is_send)).copied().unwrap_or(0);
        let src = if pre { &self.pre_fader[from] } else { &self.bufs[from] };
        let (mut a, mut b) = std::mem::take(&mut self.route_tmp);
        a.clear();
        a.extend_from_slice(&src.0[..n]);
        b.clear();
        b.extend_from_slice(&src.1[..n]);
        let key = (song.strips[from].id, song.strips[to].id, is_send);
        let level0 = if is_send { self.ramps.start((Ramps::SEND, key.0, key.1), [level, 0.0])[0] } else { level };
        let (tl, tr) = &mut self.bufs[to];
        if delay > 0 {
            let lines = self.edge_lines.entry(key).or_insert_with(|| [DelayLine::new(delay + 1), DelayLine::new(delay + 1)]);
            if lines[0].len() < delay {
                *lines = [DelayLine::new(delay + 1), DelayLine::new(delay + 1)];
            }
            for k in 0..n {
                lines[0].write(a[k]);
                lines[1].write(b[k]);
                let g = ramp(level0, level, k, n);
                tl[k] += lines[0].read(delay as f32) * g;
                tr[k] += lines[1].read(delay as f32) * g;
            }
        } else {
            for k in 0..n {
                let g = ramp(level0, level, k, n);
                tl[k] += a[k] * g;
                tr[k] += b[k] * g;
            }
        }
        self.route_tmp = (a, b);
    }

    fn trigger(&mut self, song: &Song, ci: usize, on: NoteOn) {
        self.last_note = (on.note, on.velocity);
        let c = &song.channels[ci];
        // Voice cutting: this channel's trigger chokes channels of its "by" group (and itself with cut-self).
        if c.cut_by > 0 {
            for (k, o) in song.channels.iter().enumerate() {
                if k != ci && o.cut_group == c.cut_by {
                    if let Some(i) = &o.instrument {
                        i.lock().unwrap_or_else(std::sync::PoisonError::into_inner).choke(0.01);
                    }
                }
            }
        }
        if let Some(i) = &c.instrument {
            let mut g = i.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
                        inst.lock().unwrap_or_else(std::sync::PoisonError::into_inner).note_off(e.note);
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
                let mut g = inst.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                if g.active_voices() == 0 {
                    continue;
                }
                g.process(cl, cr);
            }
            let (gl, gr) = balance(c.pan);
            let (gl, gr) = (c.volume * gl, c.volume * gr);
            let [gl0, gr0] = self.ramps.start((Ramps::CHANNEL, c.id, 0), [gl, gr]);
            let (bl, br) = &mut self.bufs[c.strip];
            for k in 0..n {
                bl[f0 + k] += cl[k] * ramp(gl0, gl, k, n);
                br[f0 + k] += cr[k] * ramp(gr0, gr, k, n);
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

/// Length of the automatic de-click fades at audio clip edges (seconds).
const DECLICK: f64 = 0.003;

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
    // De-click only where the clip cuts into the audio: a start inside the file (slipped, trimmed or
    // reversed), an end before the file's end. A sample's own first frame is kept (drum attacks).
    let start_pos = if c.reverse { frames - 1.0 - c.src_offset } else { c.src_offset };
    let end_local = (clip_end_sec - c.start_sec) * rate as f64;
    let end_pos = if c.reverse { frames - 1.0 - (c.src_offset + end_local * c.step) } else { c.src_offset + end_local * c.step };
    let cut_in = if c.reverse { start_pos < frames - 2.0 } else { start_pos > 0.5 };
    let cut_out = if c.reverse { end_pos > 0.5 } else { end_pos < frames - 2.0 };
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
        // De-click (like FL's default): 3 ms raised-cosine fades at both clip edges, so a clip that starts
        // or ends mid-waveform does not jump.
        let edge = (if cut_in { t - c.start_sec } else { f64::MAX }).min(if cut_out { clip_end_sec - t } else { f64::MAX });
        let mut g = if edge < DECLICK { (0.5 - 0.5 * (std::f64::consts::PI * edge / DECLICK).cos()) as f32 } else { 1.0 };
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

/// After an insert: if its output is not finite (NaN / infinity – a blown-up filter or feedback loop),
/// silence the block and reset the effect so it cannot poison everything after it (and every later
/// block), and report the insert.
#[inline]
fn guard_finite(e: &mut dyn Effect, id: Id, l: &mut [f32], r: &mut [f32]) {
    // NaN fails every comparison, infinity fails `< BIG`: one pass, vectorised.
    const BIG: f32 = 1.0e20;
    let ok = l.iter().chain(r.iter()).fold(true, |ok, x| ok & (x.abs() < BIG));
    if !ok {
        l.fill(0.0);
        r.fill(0.0);
        e.reset();
        crate::fault::report_nan(id);
    }
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

/// Pre-fader integrated loudness (LUFS) of every audio strip over the song (or `max_secs` of it), from one
/// offline render: what the mixer's auto-balance sets the faders from.
pub fn measure_strip_loudness(p: &Project, cache: &SampleCache, rate: f32, pool: &mut Pool, max_secs: f64, progress: &mut dyn FnMut(f32)) -> Vec<(Id, f32)> {
    let mut r = offline_renderer(p, cache, rate, pool);
    r.strip_meters = Some((0..r.song.strips.len()).map(|_| LoudnessMeter::exact(rate)).collect());
    let end = p.song_length().max(4.0);
    let secs = r.song.tempo.beat_to_seconds(end).min(max_secs).max(1.0);
    let frames = (secs * rate as f64) as usize;
    r.seek(0.0);
    r.play(PlayMode::Song);
    if let Some(song) = Arc::get_mut(&mut r.song) {
        song.loop_range = None;
    }
    let (mut l, mut rr) = (vec![0.0f32; 1024], vec![0.0f32; 1024]);
    let mut done = 0;
    while done < frames {
        let n = 1024.min(frames - done);
        if !r.transport.playing {
            r.transport.playing = true;
        }
        r.render(&mut l[..n], &mut rr[..n], &[]);
        done += n;
        progress(done as f32 / frames as f32);
    }
    let ms = r.strip_meters.take().unwrap_or_default();
    r.song.strips.iter().zip(ms.iter()).filter(|(s, _)| !s.is_vca).map(|(s, m)| (s.id, m.integrated())).collect()
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

    /// The tempo and the sample speed do not depend on the sample rate the song is rendered at (the
    /// device's rate for playback, the export's render rate): beat 2 is at the same second, and a
    /// 44.1 kHz sample lasts as long, at 44.1, 48 and 96 kHz.
    #[test]
    fn timing_is_the_same_at_every_render_rate() {
        let dir = tempfile::tempdir().unwrap();
        let click = dir.path().join("c.wav");
        // 4410 frames at 44.1 kHz = exactly 100 ms of signal.
        let s: Vec<f32> = (0..4410).map(|i| (i as f32 * 1000.0 * std::f32::consts::TAU / 44100.0).sin() * 0.8).collect();
        write_wav(&click, &s, &s, 44100, BitDepth::Float32, false, false).unwrap();
        let mut p = Project::new_default();
        p.bpm = 140.0;
        let ch = p.add_channel("Click", Instrument::Sampler(SamplerParams { path: click, ..Default::default() }));
        if let Some(c) = p.channels.iter_mut().find(|c| c.id == ch) {
            c.envelope.enabled = false;
        }
        let pat = p.patterns[0].id;
        let mut steps = vec![Step::default(); 16];
        steps[0].on = true;
        steps[8].on = true;
        p.patterns[0].channels.insert(ch, PatternChannel { steps, notes: vec![] });
        let id = p.next_id();
        p.tracks[0].clips.push(Clip { id, name: "p".into(), start: 0.0, length: 4.0, muted: false, color: Rgb::default(), kind: ClipKind::Pattern { pattern: pat } });
        let beat2 = 2.0 * 60.0 / 140.0;
        for rate in [44100.0f32, 48000.0, 96000.0] {
            let cache = SampleCache::default();
            let mut pool = Pool::new();
            let mut r = offline_renderer(&p, &cache, rate, &mut pool);
            r.play(PlayMode::Song);
            let n = (rate * 1.2) as usize;
            let (mut l, mut rr) = (vec![0.0; n], vec![0.0; n]);
            // Odd block sizes like a sound card's callback.
            let mut done = 0;
            for blk in [441usize, 512, 128, 1000].iter().cycle() {
                if done >= n {
                    break;
                }
                let k = (*blk).min(n - done);
                r.render(&mut l[done..done + k], &mut rr[done..done + k], &[]);
                done += k;
            }
            let loud: Vec<usize> = l.iter().enumerate().filter(|(_, x)| x.abs() > 0.05).map(|(i, _)| i).collect();
            let first_end = loud.iter().take_while(|i| (**i as f32) < rate * 0.5).max().copied().unwrap();
            let second = loud.iter().find(|i| (**i as f32) > rate * 0.5).copied().unwrap();
            let len_s = first_end as f64 / rate as f64;
            let at_s = second as f64 / rate as f64;
            assert!((at_s - beat2).abs() < 0.0015, "{rate}: beat 2 at {at_s:.4} s, expected {beat2:.4} s");
            assert!((len_s - 0.1).abs() < 0.003, "{rate}: the 100 ms sample lasts {len_s:.4} s");
        }
    }

    /// The app starts the engine with a song compiled at 48 kHz and then sends one compiled for the sound
    /// card's rate: the engine's clock must follow (it used to keep 48 kHz, so on a 44.1 kHz card the DAW
    /// played 8 % slow and the export sounded faster).
    #[test]
    fn the_clock_follows_the_rate_of_a_new_song() {
        let mut p = Project::new_default();
        p.bpm = 120.0;
        let cache = SampleCache::default();
        let mut pool = Pool::new();
        let s48 = Song::compile(&p, &mut CompileContext { cache: &cache, stretch: &StretchCache::default(), pool: &mut pool, rate: 48000.0 });
        let s44 = Song::compile(&p, &mut CompileContext { cache: &cache, stretch: &StretchCache::default(), pool: &mut pool, rate: 44100.0 });
        let mut r = Renderer::new(Arc::new(s48));
        r.set_song(Arc::new(s44));
        r.play(PlayMode::Song);
        let (mut l, mut rr) = (vec![0.0; 441], vec![0.0; 441]);
        for _ in 0..100 {
            r.render(&mut l, &mut rr, &[]);
        }
        // One second at 120 BPM = beat 2.
        assert!((r.transport.beat - 2.0).abs() < 1e-3, "beat {}", r.transport.beat);
    }

    /// Starting playback cuts the browser preview.
    #[test]
    fn playing_cuts_the_preview() {
        let p = Project::new_default();
        let cache = SampleCache::default();
        let mut pool = Pool::new();
        let mut r = offline_renderer(&p, &cache, 48000.0, &mut pool);
        let tone = AudioData::from_planar(48000, vec![0.5; 48000], vec![0.5; 48000]);
        r.push_live(LiveEvent::Preview(Arc::new(tone)));
        let (mut l, mut rr) = (vec![0.0; 256], vec![0.0; 256]);
        r.render(&mut l, &mut rr, &[]);
        assert!(l.iter().any(|x| x.abs() > 0.1), "preview audible");
        r.play(PlayMode::Song);
        let (mut l, mut rr) = (vec![0.0; 256], vec![0.0; 256]);
        r.render(&mut l, &mut rr, &[]);
        assert!(l.iter().all(|x| x.abs() < 0.1), "preview cut when playback starts");
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

    /// An automation clip plays its lane relative to the clip start and holds the last value afterwards.
    #[test]
    fn automation_clips_drive_their_lane() {
        let lane =
            AutomationLane { id: 9, target: Target::Tempo, points: vec![AutoPoint { beat: 0.0, value: 0.0, tension: 0.0 }, AutoPoint { beat: 4.0, value: 1.0, tension: 0.0 }], mode: AutomationMode::Read, visible: true };
        let clips = [(8.0, 12.0), (20.0, 22.0)];
        assert_eq!(lane.value_in_clips(&clips, 4.0), None, "before the first clip");
        assert!((lane.value_in_clips(&clips, 10.0).unwrap() - 0.5).abs() < 1e-6, "relative to the clip start");
        assert!((lane.value_in_clips(&clips, 15.0).unwrap() - 1.0).abs() < 1e-3, "holds after the clip");
        assert!((lane.value_in_clips(&clips, 21.0).unwrap() - 0.25).abs() < 1e-6, "second clip restarts the curve");
        assert!((lane.value_in_clips(&clips, 30.0).unwrap() - 0.5).abs() < 1e-3, "holds the end of the (shorter) second clip");
        assert_eq!(lane.length_beats(), 4.0);
    }

    /// Moving a fader while playing (a recompile with a new volume) must not step the output.
    #[test]
    fn fader_moves_do_not_click() {
        let dir = tempfile::tempdir().unwrap();
        let f = sine_file(dir.path(), "a.wav", 1.0);
        let mut p = Project::new_default();
        let id = p.next_id();
        p.tracks[0].clips.push(Clip { id, name: "a".into(), start: 0.0, length: 4.0, muted: false, color: Rgb::default(), kind: ClipKind::Audio(AudioClip { source: f, ..Default::default() }) });
        let cache = SampleCache::default();
        let stretch = StretchCache::default();
        let mut pool = Pool::new();
        let mut r = Renderer::new(Arc::new(Song::compile(&p, &mut CompileContext { cache: &cache, stretch: &stretch, pool: &mut pool, rate: 44100.0 })));
        r.play(PlayMode::Song);
        let mut out = Vec::new();
        for k in 0..40 {
            if k == 20 {
                let ch = p.tracks[0].mixer;
                let c = p.mixer.get_mut(ch).unwrap();
                c.volume = 0.05;
                c.pan = 1.0;
                r.set_song(Arc::new(Song::compile(&p, &mut CompileContext { cache: &cache, stretch: &stretch, pool: &mut pool, rate: 44100.0 })));
            }
            let (mut l, mut rr) = (vec![0.0; 512], vec![0.0; 512]);
            r.render(&mut l, &mut rr, &[]);
            out.extend(l);
        }
        // A 440 Hz sine at this level moves < 0.05 per sample; a hard volume step would jump by ~0.3.
        let step = out[512 * 19..512 * 22].windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        assert!(step < 0.06, "step {step}");
        assert!(out[512 * 30..].iter().fold(0.0f32, |m, x| m.max(x.abs())) < 0.05, "volume applied");
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
        p.mixer.get_mut(bus).unwrap().solo = false;
        // FL-style: no main route, a send into a second bus only.
        let b2 = p.add_mixer_channel("Bus 2");
        {
            let c = p.mixer.get_mut(ch).unwrap();
            c.main_out = false;
            c.output = None;
            c.sends.push(MixerSend { target: b2, level: 1.0, pre_fader: false, sidechain_only: false });
        }
        p.mixer.get_mut(b2).unwrap().volume = 0.5;
        let routed = render_secs(&p, 0.3).0.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!((routed / base - 0.5).abs() < 0.02, "{routed} {base}");
        p.mixer.get_mut(ch).unwrap().sends.clear();
        let none = render_secs(&p, 0.3).0.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(none < 1e-6, "{none}");
    }

    /// A sidechain-only route (FL's "Sidechain to this track") is not heard in the target, but the
    /// target's compressor reacts to it: a tone on a bus is ducked by a kick track that only sidechains.
    #[test]
    fn sidechain_only_route_ducks_without_being_heard() {
        let dir = tempfile::tempdir().unwrap();
        let tone = sine_file(dir.path(), "tone.wav", 0.3);
        let kick = sine_file(dir.path(), "kick.wav", 1.0);
        let mut p = Project::empty();
        let t_tone = p.add_track("Tone");
        let t_kick = p.add_track("Kick");
        for (t, f) in [(t_tone, tone), (t_kick, kick)] {
            let id = p.next_id();
            p.track_mut(t).unwrap().clips.push(Clip { id, name: "c".into(), start: 0.0, length: 4.0, muted: false, color: Rgb::default(), kind: ClipKind::Audio(AudioClip { source: f, ..Default::default() }) });
        }
        let (tone_ch, kick_ch) = (p.track(t_tone).unwrap().mixer, p.track(t_kick).unwrap().mixer);
        // The kick: no main output, only sidechain material for the tone's channel.
        {
            let k = p.mixer.get_mut(kick_ch).unwrap();
            k.main_out = false;
            k.sends.push(MixerSend { target: tone_ch, level: 1.0, pre_fader: false, sidechain_only: true });
        }
        let level = |p: &Project| render_secs(p, 0.5).0[12000..].iter().fold(0.0f32, |m, x| m.max(x.abs()));
        let alone = level(&p);
        assert!(alone > 0.2 && alone < 0.4, "only the tone is heard ({alone})");
        // A compressor on the tone's channel listening to the kick ducks it.
        let iid = p.next_id();
        let mut params = effects::find("compressor").unwrap().defaults();
        params.insert("threshold".into(), -30.0);
        params.insert("ratio".into(), 8.0);
        params.insert("sidechain".into(), 1.0);
        p.mixer.get_mut(tone_ch).unwrap().inserts.push(Insert { id: iid, effect: "compressor".into(), enabled: true, mix: 1.0, params, sidechain: Some(kick_ch), plugin: None, preset: String::new(), mods: Vec::new() });
        let ducked = level(&p);
        assert!(ducked < alone * 0.6, "ducked by the kick: {ducked} vs {alone}");
    }

    /// FabFilter-style modulation: a square LFO on a utility's gain makes the level jump between two
    /// values; an envelope follower opens a gain with the input.
    #[test]
    fn modulation_moves_effect_parameters() {
        let dir = tempfile::tempdir().unwrap();
        let tone = dir.path().join("t.wav");
        let s: Vec<f32> = (0..44100 * 2).map(|i| (i as f32 * 0.06).sin() * 0.25).collect();
        write_wav(&tone, &s, &s, 44100, BitDepth::Float32, false, false).unwrap();
        let mut p = Project::new_default();
        let id = p.next_id();
        p.tracks[0].clips.push(Clip { id, name: "t".into(), start: 0.0, length: 4.0, muted: false, color: Rgb::default(), kind: ClipKind::Audio(AudioClip { source: tone, ..Default::default() }) });
        let m1 = p.tracks[0].mixer;
        let ins = p.next_id();
        p.mixer.get_mut(m1).unwrap().inserts.push(Insert {
            id: ins,
            effect: "utility".into(),
            enabled: true,
            mix: 1.0,
            params: Default::default(),
            sidechain: None,
            plugin: None,
            preset: String::new(),
            mods: vec![Modulation { source: ModSource::Lfo { rate: 4.0, shape: 3, sync: false }, target: "gain".into(), depth: 0.5, slew: 0.0 }],
        });
        let (l, _) = render_secs(&p, 1.0);
        // RMS in 20 ms windows: two clearly different levels.
        let win: Vec<f32> = l.chunks(882).skip(2).map(|c| (c.iter().map(|x| x * x).sum::<f32>() / c.len() as f32).sqrt()).collect();
        let (lo, hi) = win.iter().fold((f32::MAX, 0.0f32), |(a, b), x| (a.min(*x), b.max(*x)));
        assert!(hi > lo * 3.0, "{lo} {hi}");
    }

    /// The EQ is audible through the whole render path: +12 dB at 1 kHz on a 1 kHz tone, in every
    /// processing mode, and the insert's mix knob blends it.
    #[test]
    fn eq_is_audible_in_a_render() {
        let dir = tempfile::tempdir().unwrap();
        let tone = dir.path().join("t.wav");
        let s: Vec<f32> = (0..44100 * 3).map(|i| (i as f32 * std::f32::consts::TAU * 1000.0 / 44100.0).sin() * 0.1).collect();
        write_wav(&tone, &s, &s, 44100, BitDepth::Float32, false, false).unwrap();
        let level = |eq: Option<(f32, f32)>| {
            let mut p = Project::new_default();
            let id = p.next_id();
            p.tracks[0].clips.push(Clip { id, name: "t".into(), start: 0.0, length: 6.0, muted: false, color: Rgb::default(), kind: ClipKind::Audio(AudioClip { source: tone.clone(), ..Default::default() }) });
            if let Some((phase, mix)) = eq {
                let def = crate::effects::find("eq").unwrap();
                let mut params = def.defaults();
                params.insert("b4_on".into(), 1.0);
                params.insert("b4_freq".into(), 1000.0);
                params.insert("b4_gain".into(), 12.0);
                params.insert("phase".into(), phase);
                let m1 = p.tracks[0].mixer;
                let ins = p.next_id();
                p.mixer.get_mut(m1).unwrap().inserts.push(Insert { id: ins, effect: "eq".into(), enabled: true, mix, params, sidechain: None, plugin: None, preset: String::new(), mods: Vec::new() });
            }
            let (l, _) = render_secs(&p, 2.0);
            let seg = &l[44100..88200];
            (seg.iter().map(|x| x * x).sum::<f32>() / seg.len() as f32).sqrt()
        };
        let dry = level(None);
        assert!(dry > 0.01, "the tone plays: {dry}");
        for phase in [0.0, 1.0, 2.0] {
            let wet = level(Some((phase, 1.0)));
            let db = 20.0 * (wet / dry).log10();
            assert!((db - 12.0).abs() < 1.5, "phase mode {phase}: {db:.1} dB");
        }
        let half = level(Some((0.0, 0.5)));
        assert!(half > dry * 1.5 && half < dry * 3.5, "mix 50 %: {}", half / dry);
    }

    /// MIDI velocity as a modulation source (FabFilter's MIDI sources), with slew: a hard note pulls the
    /// utility's gain down; with 300 ms slew the change glides in instead of jumping.
    #[test]
    fn midi_velocity_modulates_with_slew() {
        let dir = tempfile::tempdir().unwrap();
        let tone = dir.path().join("t.wav");
        let s: Vec<f32> = (0..48000 * 3).map(|i| (i as f32 * 0.06).sin() * 0.25).collect();
        write_wav(&tone, &s, &s, 48000, BitDepth::Float32, false, false).unwrap();
        let run = |slew: f32| {
            let mut p = Project::new_default();
            let id = p.next_id();
            p.tracks[0].clips.push(Clip { id, name: "t".into(), start: 0.0, length: 6.0, muted: false, color: Rgb::default(), kind: ClipKind::Audio(AudioClip { source: tone.clone(), ..Default::default() }) });
            let ch = p.add_channel("Key", Instrument::Synth(SynthParams::default()));
            p.channel_mut(ch).unwrap().volume = 0.0;
            let m1 = p.tracks[0].mixer;
            let ins = p.next_id();
            p.mixer.get_mut(m1).unwrap().inserts.push(Insert {
                id: ins,
                effect: "utility".into(),
                enabled: true,
                mix: 1.0,
                params: Default::default(),
                sidechain: None,
                plugin: None,
                preset: String::new(),
                mods: vec![Modulation { source: ModSource::MidiVelocity, target: "gain".into(), depth: -0.3, slew }],
            });
            let cache = SampleCache::default();
            let mut pool = Pool::new();
            let mut r = offline_renderer(&p, &cache, 48000.0, &mut pool);
            r.play(PlayMode::Song);
            let block = |r: &mut Renderer, secs: f32| {
                let n = (secs * 48000.0) as usize;
                let (mut l, mut rr) = (vec![0.0f32; n], vec![0.0f32; n]);
                for (a, b) in l.chunks_mut(256).zip(rr.chunks_mut(256)) {
                    r.render(a, b, &[]);
                }
                (l.iter().map(|x| x * x).sum::<f32>() / n as f32).sqrt()
            };
            let before = block(&mut r, 0.3);
            r.push_live(LiveEvent::NoteOn { channel: ch, note: 60, velocity: 1.0 });
            let soon = block(&mut r, 0.03);
            block(&mut r, 1.6);
            let late = block(&mut r, 0.2);
            (before, soon, late)
        };
        let (b, soon, late) = run(0.0);
        assert!(late < b * 0.7 && soon < b * 0.75, "a hard note lowers the gain at once: {b} {soon} {late}");
        let (b2, soon2, late2) = run(300.0);
        assert!((late2 - late).abs() < late * 0.05, "same end point: {late} {late2}");
        assert!(soon2 > b2 * 0.9, "slew glides in: {b2} {soon2}");
    }

    /// The XY pad as a modulation source: its X value moves the target.
    #[test]
    fn xy_pad_modulates() {
        let dir = tempfile::tempdir().unwrap();
        let tone = dir.path().join("t.wav");
        let s: Vec<f32> = (0..44100).map(|i| (i as f32 * 0.06).sin() * 0.25).collect();
        write_wav(&tone, &s, &s, 44100, BitDepth::Float32, false, false).unwrap();
        let level = |x: f32| {
            let mut p = Project::new_default();
            let id = p.next_id();
            p.tracks[0].clips.push(Clip { id, name: "t".into(), start: 0.0, length: 2.0, muted: false, color: Rgb::default(), kind: ClipKind::Audio(AudioClip { source: tone.clone(), ..Default::default() }) });
            let m1 = p.tracks[0].mixer;
            let ins = p.next_id();
            p.mixer.get_mut(m1).unwrap().inserts.push(Insert {
                id: ins,
                effect: "utility".into(),
                enabled: true,
                mix: 1.0,
                params: [(xy_param(0).to_string(), x)].into_iter().collect(),
                sidechain: None,
                plugin: None,
                preset: String::new(),
                mods: vec![Modulation { source: ModSource::Xy { axis: 0 }, target: "gain".into(), depth: 0.3, slew: 0.0 }],
            });
            let (l, _) = render_secs(&p, 0.5);
            l[4410..].iter().fold(0.0f32, |m, v| m.max(v.abs()))
        };
        assert!(level(1.0) > level(0.0) * 4.0, "{} {}", level(0.0), level(1.0));
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
            mods: Vec::new(),
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
