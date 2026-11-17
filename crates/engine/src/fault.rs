//! What the audio engine was doing when something went wrong. Before running an insert (or a channel's
//! instrument) the renderer notes its id in a thread-local, so a panic hook running on that thread can
//! name the effect that failed; [`in_audio`] tells the hook that the panic happened inside a render
//! (it is caught there and the audio recovers, so it is not an app crash).

use crate::model::Id;
use std::cell::Cell;

thread_local! {
    static CURRENT: Cell<Option<Id>> = const { Cell::new(None) };
    static IN_AUDIO: Cell<bool> = const { Cell::new(false) };
}

/// The insert / channel being processed on this thread (`None` between them).
#[inline]
pub fn set_current(id: Option<Id>) {
    CURRENT.with(|c| c.set(id));
}

pub fn current() -> Option<Id> {
    CURRENT.with(|c| c.get())
}

/// Marks this thread as inside a render (the panic is caught and the audio recovers).
#[inline]
pub fn set_in_audio(on: bool) {
    IN_AUDIO.with(|c| c.set(on));
}

pub fn in_audio() -> bool {
    IN_AUDIO.with(|c| c.get())
}

/// The last fault caught in the audio engine: (insert or channel id, message). Taken by the app.
static LAST: std::sync::Mutex<Option<(Option<Id>, String)>> = std::sync::Mutex::new(None);

/// Records a fault; the first one wins until the app takes it (a worker's report names the insert that
/// failed, the callback's later one only knows its own thread).
pub fn report(id: Option<Id>, msg: String) {
    if let Ok(mut l) = LAST.try_lock() {
        if l.is_none() {
            *l = Some((id, msg));
        }
    }
}

/// Whether a fault is waiting for the app.
pub fn pending() -> bool {
    LAST.lock().map(|l| l.is_some()).unwrap_or(false)
}

pub fn take() -> Option<(Option<Id>, String)> {
    LAST.lock().ok().and_then(|mut l| l.take())
}

/// The message of a caught panic payload.
pub fn message(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "panic".into())
}

/// Inserts whose output blew up (NaN / infinity) and was reset: (insert id, times). Taken by the app.
static NAN: std::sync::Mutex<Vec<(Id, u32)>> = std::sync::Mutex::new(Vec::new());

pub fn report_nan(id: Id) {
    if let Ok(mut v) = NAN.try_lock() {
        if let Some(x) = v.iter_mut().find(|x| x.0 == id) {
            x.1 += 1;
        } else if v.len() < 64 {
            v.push((id, 1));
        }
    }
}

pub fn take_nan() -> Vec<(Id, u32)> {
    NAN.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default()
}
