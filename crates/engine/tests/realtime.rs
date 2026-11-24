//! Real-time safety and load: what the audio callback does per block.
//!
//! - No heap allocation or freeing inside `Renderer::render` once playing (an allocation can block on a
//!   lock and cause a dropout / stutter).
//! - 200 stock effect instances at the largest buffer load and render faster than real time.

use dragon_engine::model::*;
use dragon_engine::render::{offline_renderer, PlayMode, Pool};
use dragon_engine::samples::{write_wav, BitDepth, SampleCache};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
thread_local! {
    static WATCH: Cell<bool> = const { Cell::new(false) };
    static INSIDE: Cell<bool> = const { Cell::new(false) };
}
static TRACES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
static TRACE_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// With DRAGON_ALLOC_TRACE set, the first few allocations inside the watched region are recorded with
/// their backtrace (to find them).
fn trace() {
    if !TRACE_ON.load(Ordering::Relaxed) || INSIDE.with(|i| i.get()) {
        return;
    }
    INSIDE.with(|i| i.set(true));
    WATCH.with(|w| w.set(false));
    if let Ok(mut t) = TRACES.lock() {
        if t.len() < 6 {
            t.push(std::backtrace::Backtrace::force_capture().to_string());
        }
    }
    WATCH.with(|w| w.set(true));
    INSIDE.with(|i| i.set(false));
}

// SAFETY: forwards to the system allocator; only counts.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if WATCH.with(|w| w.get()) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            trace();
        }
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if WATCH.with(|w| w.get()) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            trace();
        }
        System.dealloc(p, l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        if WATCH.with(|w| w.get()) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            trace();
        }
        System.realloc(p, l, n)
    }
}

#[global_allocator]
static A: Counting = Counting;

/// A busy project: `tracks` tracks with an audio clip each, `per` stock effects on each track's channel
/// (cycling through every effect type), a synth with notes.
fn busy_project(dir: &std::path::Path, tracks: usize, per: usize) -> Project {
    let tone = dir.join("t.wav");
    let s: Vec<f32> = (0..48000 * 4).map(|i| (i as f32 * 0.031).sin() * 0.2 + ((i * 7919) % 1000) as f32 * 1e-4 - 0.05).collect();
    write_wav(&tone, &s, &s, 48000, BitDepth::Float32, false, false).unwrap();
    let mut p = Project::new_default();
    let defs: Vec<_> = dragon_engine::effects::registry().iter().filter(|d| d.id != "tuner").collect();
    let mut k = 0;
    for t in 0..tracks {
        let tid = if t < p.tracks.len() { p.tracks[t].id } else { p.add_track(&format!("T{t}")) };
        let id = p.next_id();
        let track = p.tracks.iter_mut().find(|x| x.id == tid).unwrap();
        track.clips.push(Clip { id, name: "t".into(), start: 0.0, length: 64.0, muted: false, color: Rgb::default(), kind: ClipKind::Audio(AudioClip { source: tone.clone(), ..Default::default() }) });
        let m = track.mixer;
        for _ in 0..per {
            let def = defs[k % defs.len()];
            k += 1;
            let iid = p.next_id();
            p.mixer.get_mut(m).unwrap().inserts.push(Insert {
                id: iid,
                effect: def.id.into(),
                enabled: true,
                mix: 1.0,
                params: def.defaults(),
                sidechain: None,
                plugin: None,
                preset: String::new(),
                mods: Vec::new(),
                file: None,
            });
        }
    }
    p
}

#[test]
fn render_does_not_allocate_while_playing() {
    let dir = tempfile::tempdir().unwrap();
    let p = busy_project(dir.path(), 6, 8);
    let cache = SampleCache::default();
    let mut pool = Pool::new();
    let mut r = offline_renderer(&p, &cache, 48000.0, &mut pool);
    r.play(PlayMode::Song);
    let (mut l, mut rr) = (vec![0.0f32; 4096], vec![0.0f32; 4096]);
    // Warm-up: first blocks may size buffers.
    for _ in 0..40 {
        r.render(&mut l[..1024], &mut rr[..1024], &[]);
    }
    // With two input channels (a connected recording device), as the live callback passes them.
    let input = (vec![0.01f32; 4096], vec![-0.01f32; 4096]);
    TRACE_ON.store(std::env::var_os("DRAGON_ALLOC_TRACE").is_some(), Ordering::Relaxed);
    ALLOCS.store(0, Ordering::Relaxed);
    WATCH.with(|w| w.set(true));
    for k in 0..200 {
        let n = [64, 256, 1024, 4096][k % 4];
        r.render(&mut l[..n], &mut rr[..n], &[&input.0[..n], &input.1[..n]]);
    }
    WATCH.with(|w| w.set(false));
    let n = ALLOCS.load(Ordering::Relaxed);
    for t in TRACES.lock().unwrap().iter() {
        let frames: Vec<&str> = t
            .lines()
            .filter(|l| l.trim_start().chars().next().is_some_and(|c| c.is_ascii_digit()))
            .filter(|l| !l.contains("realtime::") && !l.contains("std::") && !l.contains("alloc::") && !l.contains("core::"))
            .take(5)
            .collect();
        println!("--- allocation at:\n{}", frames.join("\n"));
    }
    assert_eq!(n, 0, "the audio thread allocated / freed {n} times in 200 blocks");
}

/// Instruments (every stock kind) playing patterns from the playlist, a launched clip-launcher lane, a
/// monitored input and the per-insert meters: still no allocation per block.
#[test]
fn instruments_launcher_and_monitoring_do_not_allocate() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = busy_project(dir.path(), 2, 3);
    let tone = dir.path().join("t.wav");
    let pads: Vec<Pad> = (0..4).map(|k| Pad { note: 48 + k, layers: vec![PadLayer { path: tone.clone(), velocity_low: 0.0, velocity_high: 1.0 }], ..Default::default() }).collect();
    let kinds = vec![
        Instrument::Sampler(SamplerParams { path: tone.clone(), ..Default::default() }),
        Instrument::DrumPads(pads),
        Instrument::Synth(Default::default()),
        Instrument::Wavetable(Box::default()),
        Instrument::Guitar(Box::default()),
        Instrument::Piano(Box::default()),
        Instrument::Guitar(Box::new(dragon_engine::gtr::GuitarParams { mode: dragon_engine::gtr::PlayMode::Strum, ..Default::default() })),
        Instrument::Guitar(Box::new(dragon_engine::gtr::GuitarParams { mode: dragon_engine::gtr::PlayMode::Chord, ..Default::default() })),
        Instrument::Guitar(Box::new(dragon_engine::gtr::GuitarParams { mode: dragon_engine::gtr::PlayMode::Pattern, ..Default::default() })),
    ];
    let pat = p.add_pattern();
    let launch_pat = p.add_pattern();
    for (k, ins) in kinds.into_iter().enumerate() {
        let ch = p.add_channel(&format!("I{k}"), ins);
        for pid in [pat, launch_pat] {
            let pt = p.patterns.iter_mut().find(|x| x.id == pid).unwrap();
            let notes =
                (0..16).map(|n| Note { pitch: 48 + (n * 5 % 24) as u8 + k as u8, start: n * PPQ / 2, length: PPQ / 3, velocity: 0.8, pan: 0.0, release: 0.5, slide: false, muted: false, ..Default::default() }).collect();
            pt.channels.entry(ch).or_default().notes = notes;
        }
    }
    let tid = p.tracks[0].id;
    let cid = p.next_id();
    let t = p.tracks.iter_mut().find(|t| t.id == tid).unwrap();
    t.clips.push(Clip { id: cid, name: "pat".into(), start: 0.0, length: 64.0, muted: false, color: Rgb::default(), kind: ClipKind::Pattern { pattern: pat } });
    // A monitored, armed input track.
    t.armed = true;
    t.input = Some(InputSource { left: 0, right: Some(1) });
    t.monitor = MonitorMode::Input;
    let cache = SampleCache::default();
    let mut pool = Pool::new();
    let mut r = offline_renderer(&p, &cache, 48000.0, &mut pool);
    r.play(PlayMode::Song);
    let (mut l, mut rr) = (vec![0.0f32; 4096], vec![0.0f32; 4096]);
    let input = (vec![0.01f32; 4096], vec![-0.01f32; 4096]);
    r.push_live(dragon_engine::render::LiveEvent::Launch { lane: 1, pattern: Some(launch_pat), quantize: 0.0 });
    // Warm up with the parallel mixer on, then watch with it on and off: the renderer switches between
    // the two by load, and neither path may grow a buffer the other one never touched.
    let _turn = TIMING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let pool = dragon_engine::mixpool::MixPool::global();
    pool.set_always(true);
    for _ in 0..80 {
        r.render(&mut l[..1024], &mut rr[..1024], &[&input.0[..1024], &input.1[..1024]]);
    }
    TRACE_ON.store(std::env::var_os("DRAGON_ALLOC_TRACE").is_some(), Ordering::Relaxed);
    ALLOCS.store(0, Ordering::Relaxed);
    WATCH.with(|w| w.set(true));
    for k in 0..200 {
        pool.set_always(k < 100);
        let n = [64, 256, 1024, 512][k % 4];
        r.render(&mut l[..n], &mut rr[..n], &[&input.0[..n], &input.1[..n]]);
    }
    WATCH.with(|w| w.set(false));
    pool.set_always(false);
    let n = ALLOCS.load(Ordering::Relaxed);
    for t in TRACES.lock().unwrap().iter() {
        let frames: Vec<&str> = t
            .lines()
            .filter(|l| l.trim_start().chars().next().is_some_and(|c| c.is_ascii_digit()))
            .filter(|l| !l.contains("realtime::") && !l.contains("std::") && !l.contains("alloc::") && !l.contains("core::"))
            .take(6)
            .collect();
        println!("--- allocation at:\n{}", frames.join("\n"));
    }
    assert_eq!(n, 0, "the audio thread allocated / freed {n} times in 200 blocks");
}

#[test]
fn two_hundred_stock_effects_run_in_real_time() {
    let _turn = TIMING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    let p = busy_project(dir.path(), 20, 10);
    let count: usize = p.mixer.all().map(|m| m.inserts.len()).sum();
    assert!(count >= 200, "{count} effects");
    let cache = SampleCache::default();
    let mut pool = Pool::new();
    let t0 = std::time::Instant::now();
    let mut r = offline_renderer(&p, &cache, 48000.0, &mut pool);
    let load = t0.elapsed();
    r.play(PlayMode::Song);
    let block = 4096; // the largest buffer
    let (mut l, mut rr) = (vec![0.0f32; block], vec![0.0f32; block]);
    let blocks = 48000 * 5 / block; // 5 s of audio
    let t1 = std::time::Instant::now();
    let mut worst = std::time::Duration::ZERO;
    for _ in 0..blocks {
        let b = std::time::Instant::now();
        r.render(&mut l, &mut rr, &[]);
        worst = worst.max(b.elapsed());
        assert!(l.iter().chain(rr.iter()).all(|x| x.is_finite()));
    }
    let took = t1.elapsed().as_secs_f64();
    let audio = (blocks * block) as f64 / 48000.0;
    let budget = block as f64 / 48000.0;
    println!("{count} effects: loaded in {load:?}; {audio:.1} s rendered in {took:.2} s ({:.0} % CPU of one core); worst block {:.1} ms of {:.1} ms", took / audio * 100.0, worst.as_secs_f64() * 1000.0, budget * 1000.0);
    // Debug builds are ~10-30x slower; the real-time bound is checked in release, on machines with at
    // least four cores (the mixer runs independent strips on all of them).
    if !cfg!(debug_assertions) && std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) >= 4 {
        // Shared CI machines are noisy: the average must leave headroom, single blocks may hiccup.
        assert!(took < audio * 0.9, "200 effects need {:.0} % of real time", took / audio * 100.0);
        assert!(worst.as_secs_f64() < budget * 3.0, "worst block {worst:?} of {budget:.3} s");
    }
}

/// Diagnostic (run with --ignored --nocapture): per stock effect, heap operations and CPU per second of audio.
#[test]
#[ignore]
fn per_effect_cost() {
    let mut rows = Vec::new();
    for def in dragon_engine::effects::registry() {
        let mut e = dragon_engine::effects::create(def.id, 48000.0, &def.defaults()).unwrap();
        let x: Vec<f32> = (0..4096).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
        let (mut l, mut r) = (x.clone(), x.clone());
        for _ in 0..12 {
            l.copy_from_slice(&x);
            r.copy_from_slice(&x);
            e.process(&mut l, &mut r, None);
        }
        ALLOCS.store(0, Ordering::Relaxed);
        let t = std::time::Instant::now();
        WATCH.with(|w| w.set(true));
        for _ in 0..24 {
            l.copy_from_slice(&x);
            r.copy_from_slice(&x);
            e.process(&mut l, &mut r, None);
        }
        WATCH.with(|w| w.set(false));
        let secs = t.elapsed().as_secs_f64();
        let audio = 24.0 * 4096.0 / 48000.0;
        rows.push((secs / audio * 100.0, ALLOCS.load(Ordering::Relaxed), def.id));
    }
    rows.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (cpu, allocs, id) in rows {
        println!("{id:24} {cpu:7.2} % CPU  {allocs:6} heap ops");
    }
}

/// The parallel mixer (strips of one dependency level on several threads) renders exactly what the
/// sequential mixer renders.
#[test]
fn parallel_mixer_matches_sequential() {
    let dir = tempfile::tempdir().unwrap();
    let p = busy_project(dir.path(), 8, 4);
    let render = |parallel: bool| {
        dragon_engine::mixpool::MixPool::global().set_enabled(parallel);
        dragon_engine::mixpool::MixPool::global().set_always(parallel);
        let cache = SampleCache::default();
        let mut fx = Pool::new();
        let mut r = offline_renderer(&p, &cache, 48000.0, &mut fx);
        r.play(PlayMode::Song);
        let (mut l, mut rr) = (vec![0.0f32; 48000], vec![0.0f32; 48000]);
        for (a, b) in l.chunks_mut(512).zip(rr.chunks_mut(512)) {
            r.render(a, b, &[]);
        }
        dragon_engine::mixpool::MixPool::global().set_enabled(true);
        dragon_engine::mixpool::MixPool::global().set_always(false);
        (l, rr)
    };
    let (sl, sr) = render(false);
    let (pl, pr) = render(true);
    assert!(sl.iter().any(|x| x.abs() > 1e-4), "something plays");
    let d = sl.iter().zip(&pl).chain(sr.iter().zip(&pr)).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
    assert!(d < 1e-6, "parallel and sequential differ by {d}");
}

/// Diagnostic (run with --ignored --nocapture): the first process() call per effect vs a later one.
#[test]
#[ignore]
fn first_block_cost() {
    for def in dragon_engine::effects::registry() {
        let mut e = dragon_engine::effects::create(def.id, 48000.0, &def.defaults()).unwrap();
        let x: Vec<f32> = (0..4096).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
        let (mut l, mut r) = (x.clone(), x.clone());
        let t = std::time::Instant::now();
        e.process(&mut l, &mut r, None);
        let first = t.elapsed();
        let t = std::time::Instant::now();
        e.process(&mut l, &mut r, None);
        let second = t.elapsed();
        if first > second * 5 && first.as_micros() > 500 {
            println!("{:20} first {:?} later {:?}", def.id, first, second);
        }
    }
}

/// Diagnostic (run with --ignored --nocapture): per effect, cost per block while audio plays vs in the
/// silent tail after it (where decaying filters and reverbs produce denormal numbers).
#[test]
#[ignore]
fn silent_tail_cost() {
    if std::env::var_os("DRAGON_FTZ").is_some() {
        dragon_engine::dsp::denormal::flush_to_zero();
    }
    let rate = 48000.0;
    let n = 256;
    for def in dragon_engine::effects::registry() {
        let mut e = dragon_engine::effects::create(def.id, rate, &def.defaults()).unwrap();
        let mut seed = 1u32;
        let mut noise = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5
        };
        let (mut l, mut r) = (vec![0.0f32; n], vec![0.0f32; n]);
        let mut busy = std::time::Duration::ZERO;
        for _ in 0..40 {
            for k in 0..n {
                l[k] = noise() * 0.5;
                r[k] = noise() * 0.5;
            }
            let t = std::time::Instant::now();
            e.process(&mut l, &mut r, None);
            busy += t.elapsed();
        }
        // 20 s of silence; measure the last 2 s.
        let blocks = (20.0 * rate) as usize / n;
        let mut tail = std::time::Duration::ZERO;
        for b in 0..blocks {
            l.fill(0.0);
            r.fill(0.0);
            let t = std::time::Instant::now();
            e.process(&mut l, &mut r, None);
            if b >= blocks - (2.0 * rate) as usize / n {
                tail += t.elapsed();
            }
        }
        let tail_blocks = (2.0 * rate) as usize / n;
        let busy_per = busy.as_secs_f64() / 40.0;
        let tail_per = tail.as_secs_f64() / tail_blocks as f64;
        println!("{:16} busy {:7.1} us  tail {:7.1} us  x{:.1}", def.id, busy_per * 1e6, tail_per * 1e6, tail_per / busy_per.max(1e-9));
    }
}

/// With denormals flushed (as every render does), no effect's silent tail costs much more than processing
/// audio: the CPU stays flat when a sound stops.
#[test]
fn silent_tails_do_not_spike_the_cpu() {
    let _turn = TIMING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if cfg!(debug_assertions) {
        return;
    }
    std::thread::spawn(|| {
        dragon_engine::dsp::denormal::flush_to_zero();
        let rate = 48000.0;
        let n = 256;
        let mut slow = Vec::new();
        for def in dragon_engine::effects::registry() {
            let mut e = dragon_engine::effects::create(def.id, rate, &def.defaults()).unwrap();
            let mut seed = 1u32;
            let (mut l, mut r) = (vec![0.0f32; n], vec![0.0f32; n]);
            // The fastest block of each phase: a denormal slowdown hits every block, preemption by other
            // threads (tests run side by side) does not.
            let mut busy = std::time::Duration::MAX;
            for _ in 0..40 {
                for k in 0..n {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    l[k] = ((seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5) * 0.5;
                    r[k] = -l[k];
                }
                let t = std::time::Instant::now();
                e.process(&mut l, &mut r, None);
                busy = busy.min(t.elapsed());
            }
            let blocks = (12.0 * rate) as usize / n;
            let last = (2.0 * rate) as usize / n;
            let mut tail = std::time::Duration::MAX;
            for b in 0..blocks {
                l.fill(0.0);
                r.fill(0.0);
                let t = std::time::Instant::now();
                e.process(&mut l, &mut r, None);
                if b >= blocks - last {
                    tail = tail.min(t.elapsed());
                }
            }
            let (busy, tail) = (busy.as_secs_f64(), tail.as_secs_f64());
            if tail > busy * 3.0 + 20e-6 {
                slow.push(format!("{} busy {:.1} us tail {:.1} us", def.id, busy * 1e6, tail * 1e6));
            }
        }
        assert!(slow.is_empty(), "tails slower than audio: {slow:?}");
    })
    .join()
    .unwrap();
}

/// Timing tests take turns (run side by side they would measure each other).
static TIMING: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct Boom {
    blocks: usize,
}

impl dragon_engine::effects::Effect for Boom {
    fn set_param(&mut self, _: &str, _: f32) {}
    fn reset(&mut self) {}
    fn process(&mut self, _l: &mut [f32], _r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        self.blocks += 1;
        if self.blocks == 10 {
            panic!("boom in a plugin");
        }
    }
}

struct BoomFactory;

impl dragon_engine::render::PluginFactory for BoomFactory {
    fn create_effect(&mut self, _: &PluginRef, _: f32, _: usize) -> Option<Box<dyn dragon_engine::effects::Effect>> {
        Some(Box::new(Boom { blocks: 0 }))
    }
    fn create_instrument(&mut self, _: &PluginRef, _: f32, _: usize) -> Option<Box<dyn dragon_engine::instruments::InstrumentVoice>> {
        None
    }
}

/// An effect that panics inside the render (here a plugin, on its 10th block) is caught where the audio
/// callback catches it, the failing insert is reported by id, and – with the mixer running in parallel or
/// not – nothing hangs; the poisoned effect lock does not panic again on the next render.
#[test]
fn a_panicking_effect_is_caught_and_named() {
    let dir = tempfile::tempdir().unwrap();
    for parallel in [false, true] {
        let mut p = busy_project(dir.path(), 8, 1);
        let m = p.tracks[3].mixer;
        let bad = p.next_id();
        let plugin = PluginRef { format: PluginFormat::Clap, path: "boom.clap".into(), id: "boom".into(), name: "Boom".into(), state: Vec::new(), params: Default::default() };
        p.mixer.get_mut(m).unwrap().inserts.push(Insert {
            id: bad,
            effect: String::new(),
            enabled: true,
            mix: 1.0,
            params: Default::default(),
            sidechain: None,
            plugin: Some(plugin),
            preset: String::new(),
            mods: Vec::new(),
            file: None,
        });
        let (tx, rx) = std::sync::mpsc::channel();
        let dir = dir.path().to_path_buf();
        std::thread::spawn(move || {
            let _ = dir;
            dragon_engine::mixpool::MixPool::global().set_enabled(parallel);
            dragon_engine::mixpool::MixPool::global().set_always(parallel);
            let cache = SampleCache::default();
            let mut fx = Pool::with_plugins(Box::new(BoomFactory));
            let mut r = offline_renderer(&p, &cache, 48000.0, &mut fx);
            r.play(PlayMode::Song);
            let (mut l, mut rr) = (vec![0.0f32; 512], vec![0.0f32; 512]);
            let mut caught = 0;
            for _ in 0..30 {
                dragon_engine::fault::set_in_audio(true);
                let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| r.render(&mut l, &mut rr, &[])));
                dragon_engine::fault::set_in_audio(false);
                if let Err(p) = res {
                    dragon_engine::fault::report(dragon_engine::fault::current(), dragon_engine::fault::message(&*p));
                    caught += 1;
                }
            }
            dragon_engine::mixpool::MixPool::global().set_enabled(true);
            dragon_engine::mixpool::MixPool::global().set_always(false);
            let _ = tx.send((caught, dragon_engine::fault::take()));
        });
        let (caught, fault) = rx.recv_timeout(std::time::Duration::from_secs(60)).expect("the render did not hang");
        assert_eq!(caught, 1, "parallel {parallel}: one panic, later blocks render on");
        let (id, msg) = fault.expect("reported");
        assert_eq!(id, Some(bad), "parallel {parallel}: the failing insert is named");
        assert!(msg.contains("boom"), "{msg}");
    }
}

/// Playback under the conditions the app creates, at random: uneven buffer sizes (1 … 2048 frames, as
/// some drivers deliver), recompiles with random parameter values on random inserts (knob moves),
/// seeks, loops on and off, stop / play, live notes, and 44.1 / 48 / 96 kHz. Nothing may panic and
/// the output must stay finite.
#[test]
fn playback_stress_never_panics() {
    let dir = tempfile::tempdir().unwrap();
    let iters: usize = std::env::var("DRAGON_STRESS").ok().and_then(|v| v.parse().ok()).unwrap_or(if cfg!(debug_assertions) { 150 } else { 1500 });
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut failures = Vec::new();
    for rate in [44100.0f32, 48000.0, 96000.0] {
        let mut p = busy_project(dir.path(), 6, 5);
        let synth = p.add_channel("Synth", Instrument::Synth(SynthParams::default()));
        let pat = p.patterns[0].id;
        p.patterns[0].channels.insert(synth, PatternChannel { steps: vec![], notes: (0..16).map(|k| Note { pitch: 48 + (k * 5 % 24) as u8, start: k * PPQ / 2, length: PPQ / 2, ..Note::default() }).collect() });
        let id = p.next_id();
        p.tracks[0].clips.push(Clip { id, name: "p".into(), start: 0.0, length: 32.0, muted: false, color: Rgb::default(), kind: ClipKind::Pattern { pattern: pat } });
        let cache = SampleCache::default();
        let stretch = dragon_engine::render::StretchCache::default();
        let mut pool = Pool::new();
        let mut r = offline_renderer(&p, &cache, rate, &mut pool);
        r.play(PlayMode::Song);
        let mut buf = (vec![0.0f32; 2048], vec![0.0f32; 2048]);
        for it in 0..iters {
            match rnd() % 20 {
                0..=3 => {
                    // A knob move: random value on a random insert's parameter, then a recompile.
                    let chans: Vec<Id> = p.mixer.channels.iter().filter(|c| !c.inserts.is_empty()).map(|c| c.id).collect();
                    let c = chans[(rnd() % chans.len() as u64) as usize];
                    let ch = p.mixer.get_mut(c).unwrap();
                    let k = (rnd() % ch.inserts.len() as u64) as usize;
                    let ins = &mut ch.inserts[k];
                    if let Some(def) = dragon_engine::effects::find(&ins.effect) {
                        let prm = &def.params[(rnd() % def.params.len() as u64) as usize];
                        let n = (rnd() % 1001) as f32 / 1000.0;
                        ins.params.insert(prm.id.to_string(), prm.denormalize(n));
                    }
                    let song = dragon_engine::render::Song::compile(&p, &mut dragon_engine::render::CompileContext { cache: &cache, stretch: &stretch, pool: &mut pool, rate });
                    let _old = r.set_song(std::sync::Arc::new(song));
                }
                4 => r.seek((rnd() % 4000) as f64 / 100.0 - 1.0),
                5 => {
                    p.loop_enabled = !p.loop_enabled;
                    let a = (rnd() % 32) as f64;
                    p.loop_range = Some(dragon_engine::model::Range::new(a, a + 0.25 + (rnd() % 16) as f64));
                    let song = dragon_engine::render::Song::compile(&p, &mut dragon_engine::render::CompileContext { cache: &cache, stretch: &stretch, pool: &mut pool, rate });
                    let _old = r.set_song(std::sync::Arc::new(song));
                }
                6 => {
                    r.stop();
                    r.play(PlayMode::Song);
                }
                7 => r.push_live(dragon_engine::render::LiveEvent::NoteOn { channel: synth, note: 40 + (rnd() % 40) as u8, velocity: 0.8 }),
                8 => r.push_live(dragon_engine::render::LiveEvent::NoteOff { channel: synth, note: 40 + (rnd() % 40) as u8 }),
                _ => {}
            }
            let n = match rnd() % 6 {
                0 => 1 + (rnd() % 16) as usize,
                1 => 2048,
                _ => 1 + (rnd() % 1024) as usize,
            };
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| r.render(&mut buf.0[..n], &mut buf.1[..n], &[])));
            match res {
                Err(e) => {
                    failures.push(format!("{rate} Hz, iteration {it}, block {n}: panic {}", dragon_engine::fault::message(&*e)));
                    break;
                }
                Ok(()) => {
                    if let Some(x) = buf.0[..n].iter().chain(buf.1[..n].iter()).find(|x| !x.is_finite()) {
                        failures.push(format!("{rate} Hz, iteration {it}, block {n}: output {x}"));
                        break;
                    }
                    // An insert that blew up (silenced and reset by the renderer): name it.
                    for (id, times) in dragon_engine::fault::take_nan() {
                        let what = p.mixer.all().flat_map(|c| c.inserts.iter()).find(|x| x.id == id).map(|x| format!("{} {:?}", x.effect, x.params)).unwrap_or_default();
                        failures.push(format!("{rate} Hz, iteration {it}: insert {id} blew up {times}x: {what}"));
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Recompiling while playing (a knob drag recompiles every 40 ms) does not click: a sustained tone through
/// a track with an EQ, compressor and delay, the song swapped every 40 ms with a slightly different EQ
/// gain and fader, shows no discontinuity in the output.
#[test]
fn recompiles_while_playing_do_not_click() {
    let dir = tempfile::tempdir().unwrap();
    let rate = 48000.0f32;
    let src = dir.path().join("tone.wav");
    let s: Vec<f32> = (0..(rate as usize * 4)).map(|i| (i as f32 * 2.0 * std::f32::consts::PI * 220.0 / rate).sin() * 0.4).collect();
    dragon_engine::samples::write_wav(&src, &s, &s, rate as u32, dragon_engine::samples::BitDepth::Float32, false, false).unwrap();
    let mut p = Project::empty();
    p.bpm = 120.0;
    let t = p.add_track("Tone");
    let id = p.next_id();
    p.track_mut(t).unwrap().clips.push(Clip { id, name: "tone".into(), start: 0.0, length: 8.0, muted: false, color: Rgb::default(), kind: ClipKind::Audio(AudioClip { source: src, ..Default::default() }) });
    let m = p.track(t).unwrap().mixer;
    for fx in ["eq", "compressor", "delay"] {
        let iid = p.next_id();
        let def = dragon_engine::effects::find(fx).unwrap();
        p.mixer.get_mut(m).unwrap().inserts.push(Insert {
            id: iid,
            effect: fx.into(),
            enabled: true,
            mix: 1.0,
            params: def.defaults(),
            sidechain: None,
            plugin: None,
            preset: String::new(),
            mods: Vec::new(),
            file: None,
        });
    }
    let cache = SampleCache::default();
    let stretch = dragon_engine::render::StretchCache::default();
    let mut pool = Pool::new();
    let mut r = offline_renderer(&p, &cache, rate, &mut pool);
    r.play(PlayMode::Song);
    let mut out = Vec::new();
    let (mut l, mut rr) = (vec![0.0f32; 480], vec![0.0f32; 480]);
    // 3 s in 10 ms blocks; a recompile every 4th block (40 ms) with a small change.
    for b in 0..300 {
        if b % 4 == 0 && b > 20 {
            let k = (b / 4) as f32;
            let eq = &mut p.mixer.get_mut(m).unwrap().inserts[0];
            eq.params.insert("b3_gain".into(), (k * 0.37).sin() * 3.0);
            p.mixer.get_mut(m).unwrap().volume = 0.7 + (k * 0.21).sin() * 0.05;
            let song = dragon_engine::render::Song::compile(&p, &mut dragon_engine::render::CompileContext { cache: &cache, stretch: &stretch, pool: &mut pool, rate });
            let _old = r.set_song(std::sync::Arc::new(song));
        }
        r.render(&mut l, &mut rr, &[]);
        out.extend_from_slice(&l);
    }
    // Clicks: the second difference of a 220 Hz tone is tiny; a step stands out. Skip the clip's start.
    let body = &out[(rate * 0.5) as usize..];
    let peak = body.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    let worst = body.windows(3).map(|w| (w[2] - 2.0 * w[1] + w[0]).abs()).fold(0.0f32, f32::max);
    // A pure 220 Hz sine of amplitude A has a second difference of at most A·(2π·220/48000)² ≈ 0.0008·A.
    assert!(peak > 0.05, "the tone plays ({peak})");
    assert!(worst < peak * 0.01, "discontinuity {worst} at peak {peak}");
}

/// Edges that commonly click: an audio clip starting and ending in the middle of a waveform, a sampler
/// retriggered on its own still-sounding note (voice stealing), short notes cut by their note-off.
/// The largest step in the output must stay far below the signal (a de-click fade at every edge).
#[test]
fn clip_and_note_edges_do_not_click() {
    let dir = tempfile::tempdir().unwrap();
    let rate = 48000.0f32;
    let tone = |name: &str, hz: f32| {
        let p = dir.path().join(name);
        let s: Vec<f32> = (0..(rate as usize * 2)).map(|i| (i as f32 * 2.0 * std::f32::consts::PI * hz / rate).sin() * 0.5).collect();
        dragon_engine::samples::write_wav(&p, &s, &s, rate as u32, dragon_engine::samples::BitDepth::Float32, false, false).unwrap();
        p
    };
    let worst_step = |p: &Project| {
        let cache = SampleCache::default();
        let mut pool = Pool::new();
        let mut r = offline_renderer(p, &cache, rate, &mut pool);
        r.play(PlayMode::Song);
        let mut out = Vec::new();
        let (mut l, mut rr) = (vec![0.0f32; 256], vec![0.0f32; 256]);
        for _ in 0..(rate as usize * 3 / 256) {
            r.render(&mut l, &mut rr, &[]);
            out.extend_from_slice(&l);
        }
        let peak = out.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        // A 110 Hz sine's sample-to-sample step is at most 2π·110/48000·A ≈ 0.0144·A; a click jumps by A.
        let step = out.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        (step, peak)
    };
    // 1. Audio clip cut in the middle of the wave at both ends (offset start, end mid-cycle).
    let mut p = Project::empty();
    let t = p.add_track("A");
    let id = p.next_id();
    p.track_mut(t).unwrap().clips.push(Clip {
        id,
        name: "a".into(),
        start: 0.5,
        length: 1.37,
        muted: false,
        color: Rgb::default(),
        kind: ClipKind::Audio(AudioClip { source: tone("a.wav", 110.0), offset: 0.0123, ..Default::default() }),
    });
    let (step, peak) = worst_step(&p);
    assert!(peak > 0.2 && step < peak * 0.05, "audio clip edges: step {step} at peak {peak}");
    // 2. Sampler retriggered on its own ringing note, and notes cut short.
    let mut p = Project::empty();
    let ch = p.add_channel("S", Instrument::Sampler(SamplerParams { path: tone("s.wav", 110.0), ..Default::default() }));
    let pat = p.patterns[0].id;
    let notes: Vec<Note> = (0..12).map(|k| Note { pitch: 60, start: k * PPQ / 3, length: PPQ / 5 + (k % 3) * PPQ / 2, ..Note::default() }).collect();
    p.patterns[0].channels.insert(ch, PatternChannel { steps: vec![], notes });
    let t = p.add_track("P");
    let id = p.next_id();
    p.track_mut(t).unwrap().clips.push(Clip { id, name: "p".into(), start: 0.0, length: 4.0, muted: false, color: Rgb::default(), kind: ClipKind::Pattern { pattern: pat } });
    let (step, peak) = worst_step(&p);
    assert!(peak > 0.2 && step < peak * 0.05, "sampler retriggers and note-offs: step {step} at peak {peak}");
    // 3. The synth: legato overlaps and voice stealing at polyphony 1.
    let mut p = Project::empty();
    let ch = p.add_channel("Syn", Instrument::Synth(SynthParams { polyphony: 1, ..Default::default() }));
    let pat = p.patterns[0].id;
    let notes: Vec<Note> = (0..12).map(|k| Note { pitch: 40 + (k as u8 * 5) % 12, start: k * PPQ / 3, length: PPQ / 2, ..Note::default() }).collect();
    p.patterns[0].channels.insert(ch, PatternChannel { steps: vec![], notes });
    let t = p.add_track("P");
    let id = p.next_id();
    p.track_mut(t).unwrap().clips.push(Clip { id, name: "p".into(), start: 0.0, length: 4.0, muted: false, color: Rgb::default(), kind: ClipKind::Pattern { pattern: pat } });
    let (step, peak) = worst_step(&p);
    // The default synth is a saw: its own wave has a full-scale step per cycle, so judge against that.
    assert!(peak > 0.05 && step < peak * 2.2, "synth stealing: step {step} at peak {peak}");
}
