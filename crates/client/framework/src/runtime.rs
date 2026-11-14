//! Background work for an immediate-mode UI.
//!
//! The UI thread never blocks: work runs on a shared Tokio runtime and its result is picked up by polling a
//! [`Promise`] on the next frame (the task asks egui for a repaint when it finishes).

use std::future::Future;
use std::sync::{Arc, Mutex, OnceLock};

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
static REPAINT: OnceLock<egui::Context> = OnceLock::new();

/// The shared runtime (created on first use).
pub fn runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| tokio::runtime::Builder::new_multi_thread().worker_threads(4).enable_all().thread_name("redhive-worker").build().expect("tokio runtime"))
}

/// Registers the egui context so background tasks can request a repaint.
pub fn set_repaint_context(ctx: &egui::Context) {
    let _ = REPAINT.set(ctx.clone());
}

pub fn request_repaint() {
    if let Some(c) = REPAINT.get() {
        c.request_repaint();
    }
}

/// Fire-and-forget.
pub fn spawn<F>(f: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    runtime().spawn(async move {
        f.await;
        request_repaint();
    });
}

/// The result of a background task, polled from the UI.
pub struct Promise<T> {
    slot: Arc<Mutex<Option<T>>>,
}

impl<T: Send + 'static> Promise<T> {
    pub fn spawn<F>(f: F) -> Self
    where
        F: Future<Output = T> + Send + 'static,
    {
        let slot = Arc::new(Mutex::new(None));
        let s = slot.clone();
        runtime().spawn(async move {
            let v = f.await;
            if let Ok(mut g) = s.lock() {
                *g = Some(v);
            }
            request_repaint();
        });
        Promise { slot }
    }

    /// An already finished promise.
    pub fn ready(v: T) -> Self {
        Promise { slot: Arc::new(Mutex::new(Some(v))) }
    }

    /// Takes the result once it is there.
    pub fn take(&self) -> Option<T> {
        self.slot.lock().ok().and_then(|mut g| g.take())
    }

    pub fn is_ready(&self) -> bool {
        self.slot.lock().map(|g| g.is_some()).unwrap_or(false)
    }
}

/// Polls an optional promise: returns the result and clears the slot.
pub fn poll<T: Send + 'static>(p: &mut Option<Promise<T>>) -> Option<T> {
    let r = p.as_ref().and_then(|p| p.take());
    if r.is_some() {
        *p = None;
    }
    r
}
