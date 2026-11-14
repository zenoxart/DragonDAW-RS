//! Project files (`.dragon`, JSON), collect-and-save into a `Sounds` folder, rotating backups, autosave and
//! crash recovery.

use crate::model::*;
use crate::{EngineError, Result};
use std::path::{Path, PathBuf};

pub const EXTENSION: &str = "dragon";
const LOCK: &str = "session.lock";

/// Saves the project. Paths inside the project folder are stored relative to it. Keeps `backups` older
/// versions as `name.dragon.bak1`, `.bak2`, ….
pub fn save(p: &Project, path: &Path, backups: usize) -> Result<()> {
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    std::fs::create_dir_all(&dir).map_err(|e| EngineError::Io(e.to_string()))?;
    if path.exists() && backups > 0 {
        for i in (1..backups).rev() {
            let from = backup_path(path, i);
            if from.exists() {
                let _ = std::fs::rename(&from, backup_path(path, i + 1));
            }
        }
        let _ = std::fs::copy(path, backup_path(path, 1));
    }
    let mut q = p.clone();
    rewrite_paths(&mut q, |f| relative_to(f, &dir));
    let text = serde_json::to_string_pretty(&q).map_err(|e| EngineError::Project(e.to_string()))?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text).map_err(|e| EngineError::Io(e.to_string()))?;
    std::fs::rename(&tmp, path).map_err(|e| EngineError::Io(e.to_string()))
}

fn backup_path(path: &Path, i: usize) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(format!(".bak{i}"));
    PathBuf::from(s)
}

/// Loads a project; relative paths are resolved against the project folder.
pub fn load(path: &Path) -> Result<Project> {
    let text = std::fs::read_to_string(path).map_err(|e| EngineError::Io(format!("{}: {e}", path.display())))?;
    let mut p: Project = serde_json::from_str(&text).map_err(|e| EngineError::Project(e.to_string()))?;
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    rewrite_paths(&mut p, |f| if f.is_relative() && !f.as_os_str().is_empty() { dir.join(f) } else { f.to_path_buf() });
    p.repair();
    Ok(p)
}

/// Files the project references that do not exist.
pub fn missing_files(p: &Project) -> Vec<PathBuf> {
    p.referenced_files().into_iter().filter(|f| !f.exists()).collect()
}

/// Replaces every file path in the project (clips, takes, samplers, drum pads).
pub fn rewrite_paths(p: &mut Project, mut f: impl FnMut(&Path) -> PathBuf) {
    for t in &mut p.tracks {
        for c in t.clips.iter_mut().chain(t.takes.iter_mut().flat_map(|k| k.clips.iter_mut())) {
            if let ClipKind::Audio(a) = &mut c.kind {
                a.source = f(&a.source);
            }
        }
    }
    for c in &mut p.channels {
        match &mut c.instrument {
            Instrument::Sampler(s) if !s.path.as_os_str().is_empty() => s.path = f(&s.path),
            Instrument::DrumPads(pads) => {
                for l in pads.iter_mut().flat_map(|p| p.layers.iter_mut()) {
                    l.path = f(&l.path);
                }
            }
            _ => {}
        }
    }
}

fn relative_to(f: &Path, dir: &Path) -> PathBuf {
    f.strip_prefix(dir).map(Path::to_path_buf).unwrap_or_else(|_| f.to_path_buf())
}

/// Copies every referenced file into `<project dir>/Sounds` (renaming on clashes) and points the project
/// at the copies – the project folder then contains everything it needs.
pub fn collect_files(p: &mut Project, project_path: &Path) -> Result<usize> {
    let dir = project_path.parent().map(Path::to_path_buf).unwrap_or_default();
    let sounds = dir.join("Sounds");
    std::fs::create_dir_all(&sounds).map_err(|e| EngineError::Io(e.to_string()))?;
    let mut map = std::collections::HashMap::new();
    let mut copied = 0;
    for f in p.referenced_files() {
        if f.starts_with(&dir) || !f.exists() {
            continue;
        }
        let name = f.file_name().map(|s| s.to_os_string()).unwrap_or_default();
        let mut dest = sounds.join(&name);
        let mut k = 1;
        while dest.exists() && !same_file(&dest, &f) {
            let stem = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let ext = f.extension().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            dest = sounds.join(format!("{stem}_{k}.{ext}"));
            k += 1;
        }
        if !dest.exists() {
            std::fs::copy(&f, &dest).map_err(|e| EngineError::Io(e.to_string()))?;
            copied += 1;
        }
        map.insert(f.clone(), dest);
    }
    rewrite_paths(p, |f| map.get(f).cloned().unwrap_or_else(|| f.to_path_buf()));
    Ok(copied)
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(x), Ok(y)) => x.len() == y.len(),
        _ => false,
    }
}

/// Autosave and crash recovery in a folder.
pub struct Autosave {
    dir: PathBuf,
    /// Autosaves kept.
    pub keep: usize,
}

impl Autosave {
    pub fn new(dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&dir);
        Autosave { dir, keep: 10 }
    }

    /// Marks the session as running; returns `true` when the previous session did not end cleanly.
    pub fn begin_session(&self) -> bool {
        let lock = self.dir.join(LOCK);
        let crashed = lock.exists();
        let _ = std::fs::write(&lock, std::process::id().to_string());
        crashed
    }

    pub fn end_session(&self) {
        let _ = std::fs::remove_file(self.dir.join(LOCK));
    }

    /// Writes an autosave; `name` is the project name.
    pub fn save(&self, p: &Project, name: &str) -> Result<PathBuf> {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let safe: String = name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
        let path = self.dir.join(format!("{safe}_{stamp}.{EXTENSION}"));
        // Absolute paths: an autosave lives outside the project folder.
        let text = serde_json::to_string(p).map_err(|e| EngineError::Project(e.to_string()))?;
        std::fs::write(&path, text).map_err(|e| EngineError::Io(e.to_string()))?;
        let mut all = self.list();
        while all.len() > self.keep {
            let (old, _) = all.remove(all.len() - 1);
            let _ = std::fs::remove_file(old);
        }
        Ok(path)
    }

    /// Autosaves, newest first, with their modification times.
    pub fn list(&self) -> Vec<(PathBuf, std::time::SystemTime)> {
        let mut v: Vec<(PathBuf, std::time::SystemTime)> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == EXTENSION))
            .filter_map(|e| e.metadata().ok().and_then(|m| m.modified().ok()).map(|t| (e.path(), t)))
            .collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        v
    }

    pub fn load(&self, path: &Path) -> Result<Project> {
        let text = std::fs::read_to_string(path).map_err(|e| EngineError::Io(e.to_string()))?;
        let mut p: Project = serde_json::from_str(&text).map_err(|e| EngineError::Project(e.to_string()))?;
        p.repair();
        Ok(p)
    }
}

/// Recently opened projects (most recent first).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct RecentProjects {
    pub items: Vec<PathBuf>,
}

impl RecentProjects {
    pub fn add(&mut self, p: &Path) {
        self.items.retain(|x| x != p);
        self.items.insert(0, p.to_path_buf());
        self.items.truncate(10);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_load_collect_and_backups() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let wav = outside.path().join("kick.wav");
        crate::samples::write_wav(&wav, &[0.1; 100], &[0.1; 100], 44100, crate::samples::BitDepth::Int16, false, false).unwrap();
        let mut p = Project::new_default();
        p.add_channel("Kick", Instrument::Sampler(SamplerParams { path: wav.clone(), ..Default::default() }));
        let path = dir.path().join("song").join("song.dragon");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        assert_eq!(collect_files(&mut p, &path).unwrap(), 1);
        save(&p, &path, 3).unwrap();
        save(&p, &path, 3).unwrap();
        assert!(backup_path(&path, 1).exists());
        // Stored relative.
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("Sounds"));
        assert!(!text.contains(&dir.path().display().to_string()));
        let back = load(&path).unwrap();
        assert!(missing_files(&back).is_empty());
        assert_eq!(back.channels.len(), 1);
    }

    #[test]
    fn autosave_and_crash_detection() {
        let dir = tempfile::tempdir().unwrap();
        let a = Autosave::new(dir.path().to_path_buf());
        assert!(!a.begin_session());
        assert!(a.begin_session(), "a second start without end_session counts as a crash");
        let p = Project::new_default();
        let f = a.save(&p, "My song").unwrap();
        assert_eq!(a.list().len(), 1);
        assert_eq!(a.load(&f).unwrap().tracks.len(), 4);
        a.end_session();
        assert!(!a.begin_session());
    }
}
