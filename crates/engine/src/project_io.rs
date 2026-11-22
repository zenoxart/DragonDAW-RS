//! Project files (`.dragon`, JSON), classic project folders (`Samples`, `Recordings`, `Renders`, `Backup`), rotating backups, autosave and
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
    // Projects in a project folder keep their backups in its `Backup` subfolder.
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let base = if dir.join(BACKUP_DIR).is_dir() { dir.join(BACKUP_DIR).join(path.file_name().unwrap_or_default()) } else { path.to_path_buf() };
    let mut s = base.into_os_string();
    s.push(format!(".bak{i}"));
    PathBuf::from(s)
}

/// Subfolders of a project folder: collected samples, recordings, exports and backups.
pub const SAMPLES_DIR: &str = "Samples";
pub const RECORDINGS_DIR: &str = "Recordings";
pub const RENDERS_DIR: &str = "Renders";
pub const BACKUP_DIR: &str = "Backup";

/// Where a project chosen as `.../Name.dragon` lives as a project folder: `.../Name/Name.dragon`, unless the
/// file already sits in a folder of that name.
pub fn project_folder_path(chosen: &Path) -> PathBuf {
    let stem = chosen.file_stem().map(|s| s.to_os_string()).unwrap_or_else(|| "Untitled".into());
    let parent = chosen.parent().map(Path::to_path_buf).unwrap_or_default();
    let file = PathBuf::from(&stem).with_extension(EXTENSION);
    if parent.file_name() == Some(stem.as_os_str()) {
        parent.join(file)
    } else {
        parent.join(&stem).join(file)
    }
}

/// Saves into a project folder: creates `Samples`, `Recordings`, `Renders` and `Backup` next to the project
/// file, copies every file the project uses from outside the folder into `Samples` (pointing the project at
/// the copies), then writes the project. Returns the number of files copied.
pub fn save_in_folder(p: &mut Project, path: &Path, backups: usize) -> Result<usize> {
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    for sub in [SAMPLES_DIR, RECORDINGS_DIR, RENDERS_DIR, BACKUP_DIR] {
        std::fs::create_dir_all(dir.join(sub)).map_err(|e| EngineError::Io(e.to_string()))?;
    }
    let copied = collect_files(p, path)?;
    save(p, path, backups)?;
    Ok(copied)
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
    if let Some(v) = &mut p.video {
        v.path = f(&v.path);
    }
    if let Some(r) = &mut p.reference {
        r.path = f(&r.path);
    }
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

/// Copies every referenced file into `<project dir>/Samples` (renaming on clashes) and points the project
/// at the copies – the project folder then contains everything it needs. The reference track and the
/// video are included.
pub fn collect_files(p: &mut Project, project_path: &Path) -> Result<usize> {
    let dir = project_path.parent().map(Path::to_path_buf).unwrap_or_default();
    let sounds = dir.join(SAMPLES_DIR);
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
        v.sort_by_key(|b| std::cmp::Reverse(b.1));
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
mod folder_tests {
    use super::*;

    #[test]
    fn saving_creates_an_fl_style_project_folder() {
        let tmp = tempfile::tempdir().unwrap();
        // A sample somewhere outside the project.
        let lib = tmp.path().join("library");
        std::fs::create_dir_all(&lib).unwrap();
        let kick = lib.join("kick.wav");
        crate::samples::write_wav(&kick, &[0.1; 100], &[0.1; 100], 44100, crate::samples::BitDepth::Float32, false, false).unwrap();
        let mut p = Project::new_default();
        p.add_channel("Kick", Instrument::Sampler(SamplerParams { path: kick.clone(), ..Default::default() }));
        // The user picks ".../Songs/My Song.dragon".
        let chosen = tmp.path().join("Songs").join("My Song.dragon");
        let path = project_folder_path(&chosen);
        assert_eq!(path, tmp.path().join("Songs").join("My Song").join("My Song.dragon"));
        // Choosing the file inside its own folder again keeps it there.
        assert_eq!(project_folder_path(&path), path);
        assert_eq!(save_in_folder(&mut p, &path, 3).unwrap(), 1);
        let dir = path.parent().unwrap();
        for sub in [SAMPLES_DIR, RECORDINGS_DIR, RENDERS_DIR, BACKUP_DIR] {
            assert!(dir.join(sub).is_dir(), "{sub}");
        }
        assert!(dir.join(SAMPLES_DIR).join("kick.wav").is_file());
        // The project now points at the copy, stored relative to the folder.
        let text = std::fs::read_to_string(&path).unwrap();
        // (JSON escapes Windows' backslash separator.)
        assert!(text.contains("Samples/kick.wav") || text.contains("Samples\\\\kick.wav"), "{text}");
        assert!(!text.contains(&lib.display().to_string()));
        // Saving again: backups go to Backup/, nothing new to copy.
        assert_eq!(save_in_folder(&mut p, &path, 3).unwrap(), 0);
        assert!(dir.join(BACKUP_DIR).join("My Song.dragon.bak1").is_file());
        // The folder is self-contained: move it and it still loads with its sample.
        let moved = tmp.path().join("Elsewhere");
        std::fs::rename(dir, &moved).unwrap();
        let q = load(&moved.join("My Song.dragon")).unwrap();
        assert!(missing_files(&q).is_empty(), "{:?}", missing_files(&q));
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
        assert!(text.contains("Samples"));
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
