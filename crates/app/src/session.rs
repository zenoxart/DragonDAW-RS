//! The open project: model, undo history, sample cache, plugin pool and compilation into a `Song`.

use dragon_engine::history::History;
use dragon_engine::model::{Id, Project};
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
    /// The built-in effect parameters last pushed to the running effects (see `push_live_params`).
    live_params: std::collections::HashMap<Id, std::collections::BTreeMap<String, f32>>,
}

impl Session {
    pub fn new(project: Project, path: Option<PathBuf>) -> Self {
        Self::with_pool(project, path, Pool::with_plugins(Box::new(dragon_engine::plugins::Host)))
    }

    /// A session with its own plugin pool (safe mode: no plugins, or plugins without saved state).
    pub fn with_pool(project: Project, path: Option<PathBuf>, pool: Pool) -> Self {
        let cache = SampleCache::default();
        cache.set_base(path.as_ref().and_then(|p| p.parent().map(|d| d.to_path_buf())));
        Session { project, path, history: History::default(), cache, stretch: StretchCache::default(), pool, gesture: None, compiled_revision: None, warnings: Vec::new(), live_params: Default::default() }
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
            // A gesture that changed nothing (a click on a handle) leaves no undo step.
            if before != self.project {
                self.history.record(&label, &before);
            }
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

    /// Jumps to the state after `pos` steps of the history (undoing or redoing as many as needed).
    pub fn goto(&mut self, pos: usize) -> usize {
        self.end_gesture();
        self.history.goto(&mut self.project, pos)
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

    /// Pushes changed built-in effect parameters straight to the running effects, without a recompile.
    /// Called every frame while a knob is dragged, so the change is heard at frame rate (and smoothed per
    /// 32 samples by the effect wrapper) instead of in 40 ms steps – which sounded grainy, "downsampled".
    pub fn push_live_params(&mut self) {
        for m in self.project.mixer.all() {
            for ins in m.inserts.iter().filter(|i| i.plugin.is_none()) {
                if self.live_params.get(&ins.id) == Some(&ins.params) {
                    continue;
                }
                // Through the effect's inbox: the audio thread applies it before the next block (the UI
                // never locks a running effect).
                if let Some(def) = dragon_engine::effects::find(&ins.effect) {
                    self.pool.push_builtin(ins.id, def, &ins.params);
                }
                self.live_params.insert(ins.id, ins.params.clone());
            }
        }
    }

    /// Whether a running effect's latency changed (delay compensation needs a recompile).
    pub fn latency_moved(&self) -> bool {
        self.pool.latency_moved()
    }

    pub fn force_recompile(&mut self) {
        self.compiled_revision = None;
    }

    pub fn set_path(&mut self, p: PathBuf) {
        self.cache.set_base(p.parent().map(|d| d.to_path_buf()));
        self.path = Some(p);
    }
}
