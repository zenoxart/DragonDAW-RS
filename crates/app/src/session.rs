//! The open project: model, undo history, sample cache, plugin pool and compilation into a `Song`.

use dragon_engine::history::History;
use dragon_engine::model::Project;
use dragon_engine::render::{CompileContext, Pool, Song, StretchCache};
use dragon_engine::samples::SampleCache;
use std::path::PathBuf;
use std::sync::Arc;

pub struct Session {
    pub project: Project,
    pub path: Option<PathBuf>,
    pub history: History,
    pub cache: SampleCache,
    pub stretch: StretchCache,
    pub pool: Pool,
    /// Snapshot taken at the start of a continuous gesture (drag, knob turn).
    gesture: Option<(String, Project)>,
    compiled_revision: Option<u64>,
    pub warnings: Vec<String>,
}

impl Session {
    pub fn new(project: Project, path: Option<PathBuf>) -> Self {
        Self::with_pool(project, path, Pool::with_plugins(Box::new(dragon_engine::plugins::Host)))
    }

    /// A session with its own plugin pool (safe mode: no plugins, or plugins without saved state).
    pub fn with_pool(project: Project, path: Option<PathBuf>, pool: Pool) -> Self {
        let cache = SampleCache::default();
        cache.set_base(path.as_ref().and_then(|p| p.parent().map(|d| d.to_path_buf())));
        Session { project, path, history: History::default(), cache, stretch: StretchCache::default(), pool, gesture: None, compiled_revision: None, warnings: Vec::new() }
    }

    pub fn title(&self) -> String {
        let name = self.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Untitled".into());
        format!("{name}{}", if self.history.is_dirty() { " *" } else { "" })
    }

    /// One undoable edit.
    pub fn edit<R>(&mut self, label: &str, f: impl FnOnce(&mut Project) -> R) -> R {
        let before = self.project.clone();
        let r = f(&mut self.project);
        self.history.record(label, &before);
        r
    }

    /// Continuous edits (drags): the first call snapshots, later calls only bump the revision;
    /// [`Session::end_gesture`] commits one undo step.
    pub fn gesture<R>(&mut self, label: &str, f: impl FnOnce(&mut Project) -> R) -> R {
        if self.gesture.is_none() {
            self.gesture = Some((label.to_string(), self.project.clone()));
        }
        let r = f(&mut self.project);
        self.history.touch();
        r
    }

    pub fn end_gesture(&mut self) {
        if let Some((label, before)) = self.gesture.take() {
            self.history.record(&label, &before);
        }
    }

    pub fn in_gesture(&self) -> bool {
        self.gesture.is_some()
    }

    pub fn undo(&mut self) -> Option<String> {
        self.end_gesture();
        self.history.undo(&mut self.project)
    }

    pub fn redo(&mut self) -> Option<String> {
        self.end_gesture();
        self.history.redo(&mut self.project)
    }

    /// Recompiles when the project changed since the last compile.
    pub fn compile_if_changed(&mut self, rate: f32) -> Option<Arc<Song>> {
        if self.compiled_revision == Some(self.history.revision) {
            return None;
        }
        self.compiled_revision = Some(self.history.revision);
        Some(self.compile(rate))
    }

    pub fn compile(&mut self, rate: f32) -> Arc<Song> {
        let song = Song::compile(&self.project, &mut CompileContext { cache: &self.cache, stretch: &self.stretch, pool: &mut self.pool, rate });
        self.warnings = song.warnings.clone();
        Arc::new(song)
    }

    pub fn force_recompile(&mut self) {
        self.compiled_revision = None;
    }

    pub fn set_path(&mut self, p: PathBuf) {
        self.cache.set_base(p.parent().map(|d| d.to_path_buf()));
        self.path = Some(p);
    }
}
