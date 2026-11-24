//! Undo / redo for every edit: the editor takes a snapshot of the project before each change.
//! Snapshots share nothing mutable and audio lives in files, so a snapshot is a cheap structural copy.
//!
//! The history is also kept on disk next to the project (and next to every autosave), as a chain of
//! differences from the saved project, so it survives closing the project or a crash: reopened, every step
//! can still be undone (up to the undo limit).

use crate::model::Project;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Undo steps kept (the oldest drop off beyond this).
pub const UNDO_STEPS: usize = 500;

pub struct History {
    undo: Vec<(String, Project)>,
    redo: Vec<(String, Project)>,
    /// Maximum steps kept (0 = unlimited).
    pub limit: usize,
    /// Revision counter: bumps on every change, undo and redo (used for "unsaved" and recompiles).
    pub revision: u64,
    saved_revision: u64,
}

impl Default for History {
    fn default() -> Self {
        History { undo: Vec::new(), redo: Vec::new(), limit: UNDO_STEPS, revision: 0, saved_revision: 0 }
    }
}

impl History {
    /// Records the state *before* an edit named `label`.
    pub fn record(&mut self, label: &str, before: &Project) {
        self.undo.push((label.to_string(), before.clone()));
        if self.limit > 0 && self.undo.len() > self.limit {
            let excess = self.undo.len() - self.limit;
            self.undo.drain(..excess);
        }
        self.redo.clear();
        self.revision += 1;
    }

    /// Marks a change that is merged into the previous undo step (continuous drags).
    pub fn touch(&mut self) {
        self.revision += 1;
    }

    /// How many steps can be undone.
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }
    pub fn undo(&mut self, current: &mut Project) -> Option<String> {
        let (label, prev) = self.undo.pop()?;
        let now = std::mem::replace(current, prev);
        self.redo.push((label.clone(), now));
        self.revision += 1;
        Some(label)
    }

    pub fn redo(&mut self, current: &mut Project) -> Option<String> {
        let (label, next) = self.redo.pop()?;
        let now = std::mem::replace(current, next);
        self.undo.push((label.clone(), now));
        self.revision += 1;
        Some(label)
    }

    pub fn can_undo(&self) -> Option<&str> {
        self.undo.last().map(|u| u.0.as_str())
    }
    pub fn can_redo(&self) -> Option<&str> {
        self.redo.last().map(|u| u.0.as_str())
    }

    /// The steps that can be redone, next first (for the history panel).
    pub fn redo_labels(&self) -> Vec<String> {
        self.redo.iter().rev().map(|u| u.0.clone()).collect()
    }

    /// Labels, oldest first (for the history panel).
    pub fn labels(&self) -> Vec<String> {
        self.undo.iter().map(|u| u.0.clone()).collect()
    }

    pub fn mark_saved(&mut self) {
        self.saved_revision = self.revision;
    }

    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }

    /// Every step in order: (label, done). The done ones are what is applied now (undoable); the others can
    /// be redone. Position `p` in this list is "after step p" (`goto(p + 1)`).
    pub fn timeline(&self) -> Vec<(String, bool)> {
        self.undo.iter().map(|u| (u.0.clone(), true)).chain(self.redo.iter().rev().map(|u| (u.0.clone(), false))).collect()
    }

    /// How many steps are applied now (0 = the oldest state kept).
    pub fn position(&self) -> usize {
        self.undo.len()
    }

    /// Brings the project to the state after `pos` steps (0 = before the first kept step), undoing or
    /// redoing as many steps as needed in one go; the steps stay in the history either way.
    pub fn goto(&mut self, current: &mut Project, pos: usize) -> usize {
        let pos = pos.min(self.undo.len() + self.redo.len());
        let mut moved = 0;
        while self.undo.len() > pos && self.undo(current).is_some() {
            moved += 1;
        }
        while self.undo.len() < pos && self.redo(current).is_some() {
            moved += 1;
        }
        moved
    }

    /// The history as differences from `current` (for saving).
    pub fn to_saved(&self, current: &Project) -> Option<SavedHistory> {
        let cur = serde_json::to_value(current).ok()?;
        let base = fingerprint(&cur);
        // Undo: from the current state back, step by step (newest first).
        let mut undo = Vec::new();
        let mut prev = cur.clone();
        for (label, snap) in self.undo.iter().rev() {
            let v = serde_json::to_value(snap).ok()?;
            undo.push((label.clone(), diff(&prev, &v).unwrap_or(Delta::Same)));
            prev = v;
        }
        let mut redo = Vec::new();
        let mut prev = cur;
        for (label, snap) in self.redo.iter().rev() {
            let v = serde_json::to_value(snap).ok()?;
            redo.push((label.clone(), diff(&prev, &v).unwrap_or(Delta::Same)));
            prev = v;
        }
        Some(SavedHistory { version: 1, base, undo, redo })
    }

    /// Restores a saved history onto `current` (the project it was saved with). Returns false (and keeps
    /// the history empty) when the project has changed since – the differences would not fit.
    pub fn restore(&mut self, current: &Project, saved: &SavedHistory) -> bool {
        let Ok(cur) = serde_json::to_value(current) else { return false };
        if saved.version != 1 || fingerprint(&cur) != saved.base {
            return false;
        }
        let build = |list: &[(String, Delta)]| -> Option<Vec<(String, Project)>> {
            let mut v = cur.clone();
            let mut out = Vec::new();
            for (label, d) in list {
                d.apply(&mut v);
                let mut p: Project = serde_json::from_value(v.clone()).ok()?;
                p.repair();
                out.push((label.clone(), p));
            }
            Some(out)
        };
        let (Some(mut undo), Some(mut redo)) = (build(&saved.undo), build(&saved.redo)) else { return false };
        undo.reverse();
        redo.reverse();
        if self.limit > 0 && undo.len() > self.limit {
            let excess = undo.len() - self.limit;
            undo.drain(..excess);
        }
        self.undo = undo;
        self.redo = redo;
        true
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.revision += 1;
        self.saved_revision = self.revision;
    }
}

// ------------------------------------------------------------------------------------- on disk

/// The saved history: differences from the project it was saved with (`base` is that project's
/// fingerprint), the undo steps newest first, the redo steps next first.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SavedHistory {
    pub version: u32,
    pub base: u64,
    pub undo: Vec<(String, Delta)>,
    pub redo: Vec<(String, Delta)>,
}

/// A difference between two JSON values.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Delta {
    Same,
    Set(Value),
    /// Changed fields of an object (`None` = removed).
    Fields(BTreeMap<String, Option<Delta>>),
    /// Changed items of an array of the same length.
    Items(BTreeMap<usize, Delta>),
}

/// What turns `a` into `b` (`None` when they are equal).
pub fn diff(a: &Value, b: &Value) -> Option<Delta> {
    if a == b {
        return None;
    }
    Some(match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let mut f = BTreeMap::new();
            for (k, vb) in y {
                match x.get(k) {
                    Some(va) => {
                        if let Some(d) = diff(va, vb) {
                            f.insert(k.clone(), Some(d));
                        }
                    }
                    None => {
                        f.insert(k.clone(), Some(Delta::Set(vb.clone())));
                    }
                }
            }
            for k in x.keys().filter(|k| !y.contains_key(*k)) {
                f.insert(k.clone(), None);
            }
            Delta::Fields(f)
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => Delta::Items(x.iter().zip(y).enumerate().filter_map(|(i, (va, vb))| diff(va, vb).map(|d| (i, d))).collect()),
        _ => Delta::Set(b.clone()),
    })
}

impl Delta {
    pub fn apply(&self, v: &mut Value) {
        match self {
            Delta::Same => {}
            Delta::Set(x) => *v = x.clone(),
            Delta::Fields(f) => {
                if !v.is_object() {
                    *v = Value::Object(Default::default());
                }
                let o = v.as_object_mut().expect("object");
                for (k, d) in f {
                    match d {
                        None => {
                            o.remove(k);
                        }
                        Some(d) => d.apply(o.entry(k.clone()).or_insert(Value::Null)),
                    }
                }
            }
            Delta::Items(items) => {
                if let Value::Array(a) = v {
                    for (i, d) in items {
                        if let Some(x) = a.get_mut(*i) {
                            d.apply(x);
                        }
                    }
                }
            }
        }
    }
}

/// A fingerprint of a project's JSON (object keys are sorted, so it is stable).
fn fingerprint(v: &Value) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.to_string().hash(&mut h);
    h.finish()
}

/// Where the history of the project file `project` is kept: `History/<name>.history.json` in its folder.
pub fn history_path(project: &Path) -> PathBuf {
    let dir = project.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = project.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Untitled".into());
    dir.join("History").join(format!("{stem}.history.json"))
}

/// Writes the history of `current` for the project file `project`.
pub fn save_history(h: &History, current: &Project, project: &Path) -> std::io::Result<()> {
    let Some(saved) = h.to_saved(current) else { return Ok(()) };
    let path = history_path(project);
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec(&saved).map_err(std::io::Error::other)?)?;
    std::fs::rename(tmp, path)
}

/// Reads the history saved for the project file `project`, if any.
pub fn load_history(project: &Path) -> Option<SavedHistory> {
    let bytes = std::fs::read(history_path(project)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_survives_saving_and_jumps_to_any_step() {
        let mut h = History::default();
        let mut p = Project::default();
        let steps = [("tempo 100", 100.0), ("tempo 110", 110.0), ("tempo 120", 120.0), ("tempo 130", 130.0)];
        for (label, bpm) in steps {
            h.record(label, &p);
            p.bpm = bpm;
        }
        p.meta.name = "x".into();
        h.record("rename", &p.clone());
        p.meta.name = "renamed".into();
        // Two undone: they stay as redo steps.
        h.undo(&mut p);
        h.undo(&mut p);
        assert_eq!(p.bpm, 120.0);
        let dir = std::env::temp_dir().join(format!("dragon_hist_{}", std::process::id()));
        let file = dir.join("Song.dragon");
        save_history(&h, &p, &file).unwrap();
        assert!(history_path(&file).exists());
        // "Reopen": a fresh history from the file.
        let mut h2 = History::default();
        assert!(h2.restore(&p, &load_history(&file).unwrap()));
        assert_eq!(h2.timeline(), h.timeline());
        assert_eq!(h2.position(), 3);
        let mut q = p.clone();
        h2.goto(&mut q, 0);
        assert_eq!(q.bpm, 140.0, "back to the very start");
        h2.goto(&mut q, 2);
        assert_eq!(q.bpm, 110.0);
        h2.goto(&mut q, 5);
        assert_eq!((q.bpm, q.meta.name.as_str()), (130.0, "renamed"), "forward through the redo steps");
        // A project changed elsewhere does not take a history that no longer fits.
        let mut other = p.clone();
        other.bpm = 99.0;
        assert!(!History::default().restore(&other, &load_history(&file).unwrap()));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn deltas_round_trip() {
        let a = serde_json::json!({"a": 1, "b": [1, 2, 3], "c": {"d": "x", "e": null}, "gone": true});
        let b = serde_json::json!({"a": 2, "b": [1, 5, 3], "c": {"d": "y", "e": null}, "new": [1]});
        let d = diff(&a, &b).unwrap();
        let mut x = a.clone();
        d.apply(&mut x);
        assert_eq!(x, b);
        let c = serde_json::json!({"b": [1, 2]});
        let mut y = a.clone();
        diff(&a, &c).unwrap().apply(&mut y);
        assert_eq!(y, c);
    }

    #[test]
    fn undo_redo() {
        let mut h = History::default();
        let mut p = Project::default();
        h.record("tempo", &p);
        p.bpm = 90.0;
        assert!(h.is_dirty());
        assert_eq!(h.undo(&mut p).as_deref(), Some("tempo"));
        assert_eq!(p.bpm, 140.0);
        h.redo(&mut p);
        assert_eq!(p.bpm, 90.0);
        h.mark_saved();
        assert!(!h.is_dirty());
        h.limit = 2;
        for _ in 0..5 {
            h.record("x", &p);
        }
        assert_eq!(h.labels().len(), 2);
    }
}
