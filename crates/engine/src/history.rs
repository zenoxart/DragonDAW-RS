//! Undo / redo for every edit: the editor takes a snapshot of the project before each change.
//! Snapshots share nothing mutable and audio lives in files, so a snapshot is a cheap structural copy.

use crate::model::Project;

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

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.revision += 1;
        self.saved_revision = self.revision;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
