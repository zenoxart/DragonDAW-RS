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
        }
        System.dealloc(p, l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        if WATCH.with(|w| w.get()) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
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
            p.mixer.get_mut(m).unwrap().inserts.push(Insert { id: iid, effect: def.id.into(), enabled: true, mix: 1.0, params: def.defaults(), sidechain: None, plugin: None, preset: String::new(), mods: Vec::new() });
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
    TRACE_ON.store(std::env::var_os("DRAGON_ALLOC_TRACE").is_some(), Ordering::Relaxed);
    ALLOCS.store(0, Ordering::Relaxed);
    WATCH.with(|w| w.set(true));
    for k in 0..200 {
        let n = [64, 256, 1024, 4096][k % 4];
        r.render(&mut l[..n], &mut rr[..n], &[]);
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

#[test]
fn two_hundred_stock_effects_run_in_real_time() {
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
        let cache = SampleCache::default();
        let mut fx = Pool::new();
        let mut r = offline_renderer(&p, &cache, 48000.0, &mut fx);
        r.play(PlayMode::Song);
        let (mut l, mut rr) = (vec![0.0f32; 48000], vec![0.0f32; 48000]);
        for (a, b) in l.chunks_mut(512).zip(rr.chunks_mut(512)) {
            r.render(a, b, &[]);
        }
        dragon_engine::mixpool::MixPool::global().set_enabled(true);
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
