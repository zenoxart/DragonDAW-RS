//! Click / crackle audit of every stock effect during plain playback (no knob moves).
//!
//! Each effect runs the same music twice: once in small fixed blocks and once in irregular blocks like
//! a sound card hands them over (1 … 999 samples). A correct effect gives the same output either way;
//! a jump in the difference right at a block boundary is a click. It also checks for non-finite output,
//! the effect's own sharpest step against the input's, and the processing time per block.
//!
//! `cargo test --release -p dragon-engine --test click_audit -- --nocapture` prints the full report.

use dragon_engine::effects::{create, registry, EffectDef, ParamKind};
use std::collections::BTreeMap;

const RATE: f32 = 48_000.0;
const BPM: f64 = 120.0;

fn music(n: usize) -> (Vec<f32>, Vec<f32>) {
    let mut seed = 0x1234_5678u32;
    let mut noise = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed as f32 / u32::MAX as f32) * 2.0 - 1.0
    };
    let tau = std::f32::consts::TAU;
    let mut l = vec![0.0; n];
    let mut r = vec![0.0; n];
    let beat = (RATE * 0.5) as usize;
    for i in 0..n {
        let t = i as f32 / RATE;
        // A held chord (bass and pad) …
        let pad = (tau * 110.0 * t).sin() * 0.18 + (tau * 164.8 * t).sin() * 0.08 + (tau * 659.3 * t + 0.4).sin() * 0.05;
        // … a kick on every beat, a snare on 2 and 4, hats on the eighths.
        let kb = i % beat;
        let kt = kb as f32 / RATE;
        let kick = (tau * (50.0 + 90.0 * (-kt * 30.0).exp()) * kt).sin() * (-kt * 9.0).exp() * 0.5;
        let sb = (i + beat) % (2 * beat);
        let st = sb as f32 / RATE;
        let n0 = noise();
        let snare = if sb < beat { n0 * (-st * 22.0).exp() * 0.3 } else { 0.0 };
        let hb = i % (beat / 2);
        let hat = noise() * (-(hb as f32 / RATE) * 90.0).exp() * 0.12;
        l[i] = pad + kick + snare + hat;
        r[i] = pad * 0.9 + kick + snare * 0.8 + hat * 1.1 + (tau * 220.0 * t + 1.1).sin() * 0.04;
    }
    (l, r)
}

struct Run {
    l: Vec<f32>,
    r: Vec<f32>,
    bounds: Vec<usize>,
    worst_us: f64,
    mean_us: f64,
}

fn run(def: &EffectDef, params: &BTreeMap<String, f32>, inl: &[f32], inr: &[f32], sizes: &[usize]) -> Run {
    let mut e = create(def.id, RATE, params).unwrap();
    e.set_tempo(BPM);
    let (mut l, mut r) = (inl.to_vec(), inr.to_vec());
    let mut o = 0;
    let mut k = 0;
    let mut bounds = Vec::new();
    let mut worst = 0.0f64;
    let mut total = 0.0f64;
    while o < l.len() {
        let len = sizes[k % sizes.len()].min(l.len() - o);
        k += 1;
        let beat = o as f64 / RATE as f64 * BPM / 60.0;
        e.set_position(beat, true);
        let t0 = std::time::Instant::now();
        e.process(&mut l[o..o + len], &mut r[o..o + len], None);
        let us = t0.elapsed().as_secs_f64() * 1e6;
        total += us;
        // Per 512 samples, so block sizes compare (tiny blocks: the call itself, counted as 64).
        worst = worst.max(us * 512.0 / len.max(64) as f64);
        o += len;
        bounds.push(o);
    }
    let mean = total * 512.0 / l.len() as f64;
    Run { l, r, bounds, worst_us: worst, mean_us: mean }
}

fn peak(v: &[f32]) -> f32 {
    v.iter().fold(0.0f32, |m, x| m.max(x.abs()))
}

fn settings(def: &EffectDef) -> Vec<(String, BTreeMap<String, f32>)> {
    let mut out = vec![("defaults".to_string(), def.defaults())];
    let mut busy = def.defaults();
    for p in def.params.iter().filter(|p| matches!(p.kind, ParamKind::Linear | ParamKind::Log)) {
        busy.insert(p.id.to_string(), p.denormalize(0.7));
    }
    out.push(("knobs at 70 %".to_string(), busy));
    // With AUDIT_ONLY set: each knob at 70 % on its own, to find the culprit.
    if std::env::var("AUDIT_ONLY").is_ok() {
        for p in def.params.iter().filter(|p| matches!(p.kind, ParamKind::Linear | ParamKind::Log)) {
            let mut m = def.defaults();
            m.insert(p.id.to_string(), p.denormalize(0.7));
            out.push((format!("{} at 70 %", p.id), m));
        }
    }
    for p in def.params.iter().filter(|p| !matches!(p.kind, ParamKind::Linear | ParamKind::Log)) {
        let steps = ((p.max - p.min).round() as usize).min(8);
        for s in 0..=steps {
            let v = p.min + s as f32;
            if (v - p.default).abs() < 0.5 {
                continue;
            }
            let mut m = def.defaults();
            m.insert(p.id.to_string(), v);
            out.push((format!("{}={}", p.id, v), m));
        }
    }
    out
}

#[derive(Debug)]
struct Finding {
    what: String,
    boundary_jump: f32,
    diff: f32,
    nan: bool,
}

fn audit(only: Option<&str>) -> (Vec<Finding>, Vec<String>) {
    dragon_engine::dsp::denormal::flush_to_zero();
    let n = (RATE * 4.0) as usize;
    let (inl, inr) = music(n);
    let irregular = [480, 37, 256, 1, 999, 128, 441, 3, 512, 64, 300];
    let mut found = Vec::new();
    let mut slow = Vec::new();
    // Analysers and effects meant to chop / randomise the signal per block are out of scope.
    let skip = ["scope", "phase_scope", "tuner", "spectrum"];
    for def in registry() {
        if skip.contains(&def.id) || only.is_some_and(|o| o != def.id) {
            continue;
        }
        for (name, params) in settings(def) {
            let a = run(def, &params, &inl, &inr, &[64]);
            let b = run(def, &params, &inl, &inr, &irregular);
            let nan = a.l.iter().chain(&a.r).chain(&b.l).chain(&b.r).any(|x| !x.is_finite());
            let pk = peak(&a.l).max(peak(&a.r)).max(1e-4);
            // Skip the first half second (settling, latency).
            let from = (RATE * 0.5) as usize;
            let mut diff = 0.0f32;
            for i in from..n {
                diff = diff.max((a.l[i] - b.l[i]).abs()).max((a.r[i] - b.r[i]).abs());
            }
            // Jump of the difference across a block boundary of the irregular run.
            let mut jump = 0.0f32;
            for &j in &b.bounds {
                if j < from + 2 || j + 2 >= n {
                    continue;
                }
                for (x, y) in [(&a.l, &b.l), (&a.r, &b.r)] {
                    let e0 = y[j - 1] - x[j - 1];
                    let e1 = y[j] - x[j];
                    jump = jump.max((e1 - e0).abs());
                }
            }
            let budget = 512.0 / RATE as f64 * 1e6;
            if a.mean_us.max(b.mean_us) > budget * 0.08 || a.worst_us.min(b.worst_us) > budget * 0.5 {
                slow.push(format!("{} [{}]: mean {:.0} µs (64-blocks) / {:.0} µs (irregular), worst {:.0} µs per 512 samples – budget {:.0}", def.id, name, a.mean_us, b.mean_us, a.worst_us.min(b.worst_us), budget));
            }
            if nan || jump / pk > 2e-3 || diff / pk > 2e-2 {
                found.push(Finding { what: format!("{} [{}]", def.id, name), boundary_jump: jump / pk, diff: diff / pk, nan });
            }
        }
    }
    (found, slow)
}

/// Every stock effect gives the same result whatever the sound card's block size – no clicks at block
/// boundaries, nothing non-finite.
#[test]
fn stock_effects_are_click_free_in_playback() {
    let only = std::env::var("AUDIT_ONLY").ok();
    let (found, slow) = audit(only.as_deref());
    for s in &slow {
        println!("slow: {s}");
    }
    for f in &found {
        println!("{:<50} boundary jump {:.4}  max diff {:.4}  nan {}", f.what, f.boundary_jump, f.diff, f.nan);
    }
    assert!(found.is_empty(), "{} settings click or depend on the block size", found.len());
}

/// A smooth chord swelling from quiet to loud and back (through every threshold), no drums.
fn swell(n: usize) -> (Vec<f32>, Vec<f32>) {
    let tau = std::f32::consts::TAU;
    let l: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 / RATE;
            let env = 0.02 + 0.5 * (0.5 - 0.5 * (tau * t / (n as f32 / RATE)).cos());
            env * ((tau * 110.0 * t).sin() * 0.6 + (tau * 277.2 * t).sin() * 0.3 + (tau * 1318.5 * t).sin() * 0.1)
        })
        .collect();
    let r: Vec<f32> = l.iter().enumerate().map(|(i, x)| x * 0.9 + (tau * 330.0 * i as f32 / RATE).sin() * 0.02).collect();
    (l, r)
}

/// The sharpest isolated step: |second difference| against its neighbourhood's RMS (a click stands out
/// from what's around it; a smooth or steadily distorted tone doesn't).
fn spike(v: &[f32], from: usize) -> (f32, usize) {
    let d2: Vec<f32> = v.windows(3).map(|w| (w[0] - 2.0 * w[1] + w[2]).abs()).collect();
    let pk = v.iter().fold(0.0f32, |m, x| m.max(x.abs())).max(1e-4);
    let w = 1200;
    let mut best = (0.0f32, 0);
    // Running sum of squares over [i-w, i+w].
    let sq: Vec<f64> = d2.iter().map(|x| (*x as f64) * (*x as f64)).collect();
    let mut acc = vec![0.0f64; sq.len() + 1];
    for i in 0..sq.len() {
        acc[i + 1] = acc[i] + sq[i];
    }
    for i in from.max(w)..d2.len().saturating_sub(w) {
        let around = (acc[i + w + 1] - acc[i - w] - sq[i]) / (2 * w) as f64;
        // Floor: steps below 1e-4 of the peak are inaudible whatever the neighbourhood.
        let rms = (around.sqrt() as f32).max(pk * 1e-4);
        let sc = d2[i] / rms;
        if sc > best.0 {
            best = (sc, i + 1);
        }
    }
    best
}

/// No stock effect puts an isolated click into a smooth swelling chord.
#[test]
fn stock_effects_do_not_click_on_a_smooth_swell() {
    dragon_engine::dsp::denormal::flush_to_zero();
    let only = std::env::var("AUDIT_ONLY").ok();
    let n = (RATE * 3.0) as usize;
    let (inl, inr) = swell(n);
    let base = spike(&inl, 0).0.max(spike(&inr, 0).0);
    // Effects that chop, crush, randomise or add noise on purpose; analysers.
    let skip = ["scope", "phase_scope", "tuner", "spectrum", "bitcrusher", "glitch", "stutter", "tape_stop", "rhythm_gate"];
    let mut bad = Vec::new();
    for def in registry() {
        if skip.contains(&def.id) || only.as_deref().is_some_and(|o| o != def.id) {
            continue;
        }
        for (name, params) in settings(def) {
            // With every knob at 70 % these two are driven into square-wave fuzz (25 dB of drive and more,
            // plus Dragonforge's transient Grit): their edges are the sound, not clicks. Their modes and
            // defaults are still checked.
            if name == "knobs at 70 %" && matches!(def.id, "saturator" | "drum_bus") {
                continue;
            }
            let o = run(def, &params, &inl, &inr, &[256]);
            let from = (RATE * 0.3) as usize;
            let (a, ia) = spike(&o.l, from);
            let (b, ib) = spike(&o.r, from);
            let (sc, at) = if a >= b { (a, ia) } else { (b, ib) };
            let ch = if a >= b { &o.l } else { &o.r };
            let step = if at > 0 { (ch[at - 1] - 2.0 * ch[at] + ch[at + 1]).abs() / peak(ch).max(1e-4) } else { 0.0 };
            if sc > (base * 4.0).max(12.0) {
                bad.push(format!("{} [{}]: spike {:.1} (input {:.1}) at {:.3} s, step {:.1} dB below the peak", def.id, name, sc, base, at as f32 / RATE, -20.0 * step.max(1e-9).log10()));
            }
        }
    }
    for b in &bad {
        println!("{b}");
    }
    assert!(bad.is_empty(), "{} settings click on a smooth swell", bad.len());
}

/// Debug aid: `AUDIT_DUMP=<effect id> AUDIT_SET=<setting name>` prints the samples around its sharpest step.
#[test]
fn dump_spike() {
    let (Ok(id), Ok(set)) = (std::env::var("AUDIT_DUMP"), std::env::var("AUDIT_SET")) else {
        return;
    };
    let def = registry().iter().find(|d| d.id == id).unwrap();
    let (_, params) = settings(def).into_iter().find(|(n, _)| *n == set).unwrap();
    let n = (RATE * 3.0) as usize;
    let (inl, inr) = swell(n);
    let o = run(def, &params, &inl, &inr, &[256]);
    let (sc, at) = spike(&o.l, (RATE * 0.3) as usize);
    println!("spike {sc:.1} at {at}");
    for (i, (x, y)) in inl.iter().zip(&o.l).enumerate().take(at + 12).skip(at - 12) {
        println!("{i:>7} in {x:+.5}  out {y:+.5}");
    }
    // The same spot one, two and three cycles of the 110 Hz chord earlier and later.
    for c in [-3i64, -2, -1, 1, 2, 3] {
        let base = (at as i64 + c * 436) as usize;
        let lo = o.l[base - 20..base + 20].iter().fold(0.0f32, |m, x| m.min(*x));
        let hi = o.l[base - 20..base + 20].iter().fold(0.0f32, |m, x| m.max(*x));
        println!("cycle {c:+}: min {lo:+.3} max {hi:+.3}");
    }
}

/// CPU of every stock effect at its defaults on music, as % of one core in real time
/// (`cargo test --release -p dragon-engine --test click_audit cpu_table -- --ignored --nocapture`).
#[test]
#[ignore]
fn cpu_table() {
    dragon_engine::dsp::denormal::flush_to_zero();
    let n = (RATE * 6.0) as usize;
    let (inl, inr) = music(n);
    let mut rows = Vec::new();
    for def in registry() {
        let mut e = create(def.id, RATE, &def.defaults()).unwrap();
        e.set_tempo(BPM);
        let (mut l, mut r) = (inl.clone(), inr.clone());
        // Warm-up second, then measure.
        let warm = RATE as usize;
        for (a, b) in l[..warm].chunks_mut(256).zip(r[..warm].chunks_mut(256)) {
            e.process(a, b, None);
        }
        let t0 = std::time::Instant::now();
        for (a, b) in l[warm..].chunks_mut(256).zip(r[warm..].chunks_mut(256)) {
            e.process(a, b, None);
        }
        let pct = t0.elapsed().as_secs_f64() / ((n - warm) as f64 / RATE as f64) * 100.0;
        rows.push((pct, def.id));
    }
    rows.sort_by(|a, b| b.0.total_cmp(&a.0));
    let total: f64 = rows.iter().map(|r| r.0).sum();
    for (pct, id) in &rows {
        println!("{id:<16} {pct:6.3} %");
    }
    println!("mean per effect: {:.3} % of a core", total / rows.len() as f64);
}

/// One effect for a profiler: `AUDIT_ONLY=<id>` (ignored).
#[test]
#[ignore]
fn profile_one() {
    let Ok(id) = std::env::var("AUDIT_ONLY") else { return };
    dragon_engine::dsp::denormal::flush_to_zero();
    let def = registry().iter().find(|d| d.id == id).unwrap();
    let n = (RATE * 2.0) as usize;
    let (inl, inr) = music(n);
    let mut e = create(def.id, RATE, &def.defaults()).unwrap();
    let (mut l, mut r) = (inl, inr);
    for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
        e.process(a, b, None);
    }
}
