//! A small real-time thread pool for the mixer: `run(count, job)` calls `job(i)` for every `i < count`,
//! spread over the worker threads and the calling (audio) thread, and returns when all are done.
//!
//! Unlike a general pool it never allocates per call (no job boxes, no queues): the job is one borrowed
//! closure published through atomics, workers pull indices from a counter, and the caller works too
//! instead of only waiting. Idle workers spin briefly, then sleep on a condvar.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

struct Shared {
    /// Bumped for every `run`; workers wake when it changes.
    generation: AtomicU64,
    /// The current job (generation, closure, count): the closure as a raw fat pointer, valid only during
    /// `run`. Read together under the lock so a worker never pairs one block's closure with another's.
    job: Mutex<Option<(u64, JobPtr, usize)>>,
    /// Next index to take, tagged with its generation: `generation << 32 | index`. A worker can only take
    /// indices of the generation it read the closure for (compare-and-swap on the whole word).
    next: AtomicU64,
    done: AtomicUsize,
    /// Workers still inside the current generation (the caller waits for 0 before the job's borrow ends).
    active: AtomicUsize,
    sleep: Mutex<()>,
    wake: Condvar,
    quit: AtomicBool,
}

#[derive(Clone, Copy)]
struct JobPtr(*const (dyn Fn(usize) + Sync));
// SAFETY: the pointer is only dereferenced while `run` keeps the closure alive, and the closure is Sync.
unsafe impl Send for JobPtr {}

pub struct MixPool {
    shared: Arc<Shared>,
    workers: usize,
    enabled: AtomicBool,
    /// One `run` at a time owns the workers; a concurrent caller (an export or analysis render next to
    /// the live audio thread) runs its job on its own thread instead.
    busy: AtomicBool,
}

impl MixPool {
    fn new(workers: usize) -> Self {
        let shared = Arc::new(Shared {
            generation: AtomicU64::new(0),
            job: Mutex::new(None),
            next: AtomicU64::new(0),
            done: AtomicUsize::new(0),
            active: AtomicUsize::new(0),
            sleep: Mutex::new(()),
            wake: Condvar::new(),
            quit: AtomicBool::new(false),
        });
        for k in 0..workers {
            let sh = shared.clone();
            let _ = std::thread::Builder::new().name(format!("dragon-mix-{k}")).spawn(move || worker(sh));
        }
        MixPool { shared, workers, enabled: AtomicBool::new(true), busy: AtomicBool::new(false) }
    }

    /// The shared pool: one worker per spare core (the audio thread is the last one), at most 15.
    pub fn global() -> &'static MixPool {
        static POOL: OnceLock<MixPool> = OnceLock::new();
        POOL.get_or_init(|| {
            let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
            let forced = std::env::var("DRAGON_MIX_THREADS").ok().and_then(|v| v.parse::<usize>().ok());
            MixPool::new(forced.unwrap_or(cores).clamp(1, 16) - 1)
        })
    }

    /// Threads that work on a `run`, the caller included (1 while disabled).
    pub fn threads(&self) -> usize {
        if self.enabled.load(Ordering::Relaxed) {
            self.workers + 1
        } else {
            1
        }
    }

    /// Turns parallel mixing on or off (off: everything runs on the calling thread).
    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
    }

    /// Calls `job(i)` for each `i < count` on the pool and the calling thread; returns when all are done.
    pub fn run(&self, count: usize, job: &(dyn Fn(usize) + Sync)) {
        if count == 0 {
            return;
        }
        if self.workers == 0 || count == 1 || self.busy.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            (0..count).for_each(job);
            return;
        }
        let sh = &*self.shared;
        // SAFETY: the 'static lifetime is a lie the caller makes true: `run` does not return while a worker
        // may still call through this pointer (it waits for `done == count` and `active == 0`), and a
        // worker can only call it for indices of this generation.
        let ptr: *const (dyn Fn(usize) + Sync + 'static) = unsafe { std::mem::transmute::<*const (dyn Fn(usize) + Sync + '_), *const (dyn Fn(usize) + Sync + 'static)>(job) };
        let generation = {
            let mut j = sh.job.lock().unwrap_or_else(|e| e.into_inner());
            let g = (sh.generation.load(Ordering::SeqCst) + 1) & 0xffff_ffff;
            sh.done.store(0, Ordering::SeqCst);
            sh.next.store(g << 32, Ordering::SeqCst);
            *j = Some((g, JobPtr(ptr), count));
            sh.generation.store(g, Ordering::SeqCst);
            g
        };
        {
            let _g = sh.sleep.lock().unwrap_or_else(|e| e.into_inner());
            sh.wake.notify_all();
        }
        // The caller works too.
        while let Some(i) = take(sh, generation, count) {
            job(i);
            sh.done.fetch_add(1, Ordering::SeqCst);
        }
        let mut spins = 0u32;
        while sh.done.load(Ordering::SeqCst) < count || sh.active.load(Ordering::SeqCst) > 0 {
            // Spin briefly (the last strips finish within microseconds), then give the core away so a
            // worker that shares it can finish.
            spins += 1;
            if spins < 2000 {
                std::hint::spin_loop();
            } else {
                std::thread::yield_now();
            }
        }
        *sh.job.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.busy.store(false, Ordering::Release);
    }
}

/// Takes the next index of generation `g`, if any is left.
fn take(sh: &Shared, g: u64, count: usize) -> Option<usize> {
    let mut v = sh.next.load(Ordering::SeqCst);
    loop {
        let (vg, i) = (v >> 32, (v & 0xffff_ffff) as usize);
        if vg != g || i >= count {
            return None;
        }
        match sh.next.compare_exchange_weak(v, v + 1, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return Some(i),
            Err(now) => v = now,
        }
    }
}

impl Drop for MixPool {
    fn drop(&mut self) {
        self.shared.quit.store(true, Ordering::SeqCst);
        let _g = self.shared.sleep.lock();
        self.shared.wake.notify_all();
    }
}

fn worker(sh: Arc<Shared>) {
    let mut seen = sh.generation.load(Ordering::SeqCst);
    loop {
        // Wait for a new generation: spin a little (blocks come every few ms while playing), then sleep.
        let t0 = Instant::now();
        loop {
            if sh.quit.load(Ordering::SeqCst) {
                return;
            }
            let g = sh.generation.load(Ordering::SeqCst);
            if g != seen {
                seen = g;
                break;
            }
            if t0.elapsed() < Duration::from_micros(200) {
                std::hint::spin_loop();
            } else {
                let guard = sh.sleep.lock().unwrap_or_else(|e| e.into_inner());
                if sh.generation.load(Ordering::SeqCst) == seen && !sh.quit.load(Ordering::SeqCst) {
                    let _ = sh.wake.wait_timeout(guard, Duration::from_millis(50));
                }
            }
        }
        sh.active.fetch_add(1, Ordering::SeqCst);
        let job = *sh.job.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((g, JobPtr(f), count)) = job {
            while let Some(i) = take(&sh, g, count) {
                // SAFETY: `run` of generation `g` keeps the closure alive until every taken index is done
                // and no worker is active.
                unsafe { (*f)(i) };
                sh.done.fetch_add(1, Ordering::SeqCst);
            }
        }
        sh.active.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_callers_are_safe() {
        let pool = Arc::new(MixPool::new(3));
        let hs: Vec<_> = (0..4)
            .map(|_| {
                let pool = pool.clone();
                std::thread::spawn(move || {
                    for round in 0..300 {
                        let n = 2 + round % 29;
                        let hits: Vec<AtomicUsize> = (0..n).map(|_| AtomicUsize::new(0)).collect();
                        pool.run(n, &|i| {
                            hits[i].fetch_add(1, Ordering::SeqCst);
                        });
                        assert!(hits.iter().all(|h| h.load(Ordering::SeqCst) == 1));
                    }
                })
            })
            .collect();
        for h in hs {
            h.join().unwrap();
        }
    }

    #[test]
    fn every_index_once_and_reusable() {
        let pool = MixPool::new(3);
        for round in 0..200 {
            let n = 1 + round % 37;
            let hits: Vec<AtomicUsize> = (0..n).map(|_| AtomicUsize::new(0)).collect();
            pool.run(n, &|i| {
                hits[i].fetch_add(1, Ordering::SeqCst);
            });
            assert!(hits.iter().all(|h| h.load(Ordering::SeqCst) == 1), "round {round}");
        }
    }
}
