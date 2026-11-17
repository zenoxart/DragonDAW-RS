//! Mixer effect chains: a track's effect slots with all their settings (and third-party plugins' state),
//! saved as `.dragonchain` files in a folder and applied to any track; and copying a single effect with
//! its settings from one track to another (drag and drop between mixer tracks).

use dragon_engine::model::{Id, Insert, Project};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Most effect slots a mixer track has.
pub const MAX_SLOTS: usize = 10;
pub const EXT: &str = "dragonchain";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Chain {
    pub name: String,
    pub inserts: Vec<Insert>,
}

/// Shows a folder in the system's file manager.
pub fn reveal(dir: &Path) {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(program).arg(dir).spawn();
}

/// The chains folder (created on demand).
pub fn dir(projects: &Path) -> PathBuf {
    projects.join("Mixer chains")
}

/// Saves `chain` as `<dir>/<name>.dragonchain` (a name that is not a valid file name is cleaned).
pub fn save(dir: &Path, chain: &Chain) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let clean: String = chain.name.chars().map(|c| if c.is_alphanumeric() || " -_().".contains(c) { c } else { '_' }).collect();
    let clean = clean.trim().trim_matches('.');
    let path = dir.join(format!("{}.{EXT}", if clean.is_empty() { "Chain" } else { clean }));
    let text = serde_json::to_string_pretty(chain).map_err(std::io::Error::other)?;
    std::fs::write(&path, text)?;
    Ok(path)
}

pub fn load(path: &Path) -> std::io::Result<Chain> {
    let text = std::fs::read_to_string(path)?;
    serde_json::from_str(&text).map_err(std::io::Error::other)
}

/// The chains in `dir` (and its sub-folders, one level), sorted by name: (label, path).
pub fn list(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let mut scan = |d: &Path, prefix: &str| {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == EXT) {
                let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                out.push((format!("{prefix}{stem}"), p));
            }
        }
    };
    scan(dir, "");
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if e.path().is_dir() {
            let name = e.file_name().to_string_lossy().to_string();
            scan(&e.path(), &format!("{name} / "));
        }
    }
    out.sort_by_key(|a| a.0.to_lowercase());
    out
}

/// A copy of an insert for another track: new id; a sidechain source is kept when it still makes sense
/// there (an existing channel that is not the track itself).
fn copy_for(p: &mut Project, ins: &Insert, track: Id) -> Insert {
    let mut c = ins.clone();
    c.id = p.next_id();
    if let Some(sc) = c.sidechain {
        if sc == track || p.mixer.get(sc).is_none() {
            c.sidechain = None;
        }
    }
    c
}

/// Applies a chain to mixer track `track`: replaces its effects, or adds after them (up to the slot
/// limit). Returns how many effects were placed.
pub fn apply(p: &mut Project, track: Id, chain: &Chain, replace: bool) -> usize {
    let copies: Vec<Insert> = chain.inserts.iter().map(|i| copy_for(p, i, track)).collect();
    let Some(t) = p.mixer.get_mut(track) else { return 0 };
    if replace {
        t.inserts.clear();
    }
    let room = MAX_SLOTS.saturating_sub(t.inserts.len());
    let n = copies.len().min(room);
    t.inserts.extend(copies.into_iter().take(n));
    n
}

/// Copies (or with `move_it` moves) the effect in slot `index` of `from` to the track `to`, at slot `at`
/// (the end when `None`). Returns the new insert's id.
pub fn copy_insert(p: &mut Project, from: Id, index: usize, to: Id, at: Option<usize>, move_it: bool) -> Option<Id> {
    let ins = p.mixer.get(from)?.inserts.get(index)?.clone();
    if p.mixer.get(to)?.inserts.len() >= MAX_SLOTS && !(move_it && from == to) {
        return None;
    }
    let c = copy_for(p, &ins, to);
    let id = c.id;
    if move_it {
        if let Some(f) = p.mixer.get_mut(from) {
            f.inserts.remove(index);
        }
    }
    let t = p.mixer.get_mut(to)?;
    let at = at.unwrap_or(t.inserts.len()).min(t.inserts.len());
    t.inserts.insert(at, c);
    Some(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_effects(names: &[&str]) -> (Project, Id) {
        let mut p = Project::empty();
        let t = p.add_track("Vox");
        let m = p.track(t).unwrap().mixer;
        for n in names {
            let id = p.next_id();
            let mut params = dragon_engine::effects::find(n).unwrap().defaults();
            params.insert("output".into(), -3.5);
            p.mixer.get_mut(m).unwrap().inserts.push(Insert { id, effect: n.to_string(), enabled: true, mix: 0.7, params, sidechain: None, plugin: None, preset: String::new(), mods: Vec::new() });
        }
        (p, m)
    }

    #[test]
    fn save_list_load_apply() {
        let dir = tempfile::tempdir().unwrap();
        let (mut p, m) = with_effects(&["eq", "compressor", "deesser"]);
        let chain = Chain { name: "Vocal: clean/bright".into(), inserts: p.mixer.get(m).unwrap().inserts.clone() };
        let path = save(dir.path(), &chain).unwrap();
        assert_eq!(path.file_name().unwrap().to_string_lossy(), "Vocal_ clean_bright.dragonchain");
        std::fs::create_dir_all(dir.path().join("Drums")).unwrap();
        save(&dir.path().join("Drums"), &Chain { name: "Kick".into(), inserts: vec![] }).unwrap();
        let l = list(dir.path());
        assert_eq!(l.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), vec!["Drums / Kick", "Vocal_ clean_bright"]);
        let back = load(&path).unwrap();
        assert_eq!(back, chain);
        // Apply to another track: same effects and settings, new ids.
        let other = p.add_mixer_channel("Backing");
        assert_eq!(apply(&mut p, other, &back, true), 3);
        let a = &p.mixer.get(m).unwrap().inserts;
        let b = &p.mixer.get(other).unwrap().inserts;
        for (x, y) in a.iter().zip(b) {
            assert_eq!((x.effect.as_str(), &x.params, x.mix), (y.effect.as_str(), &y.params, y.mix));
            assert_ne!(x.id, y.id);
        }
        // Adding stops at the slot limit.
        for _ in 0..5 {
            apply(&mut p, other, &back, false);
        }
        assert_eq!(p.mixer.get(other).unwrap().inserts.len(), MAX_SLOTS);
    }

    #[test]
    fn copy_and_move_one_effect() {
        let (mut p, m) = with_effects(&["eq", "compressor"]);
        let other = p.add_mixer_channel("Other");
        let id = copy_insert(&mut p, m, 1, other, None, false).unwrap();
        assert_eq!(p.mixer.get(m).unwrap().inserts.len(), 2, "copy keeps the original");
        let c = &p.mixer.get(other).unwrap().inserts[0];
        assert_eq!((c.id, c.effect.as_str(), c.params.get("output").copied()), (id, "compressor", Some(-3.5)));
        copy_insert(&mut p, m, 0, other, Some(0), true).unwrap();
        assert_eq!(p.mixer.get(m).unwrap().inserts.len(), 1, "moved away");
        assert_eq!(p.mixer.get(other).unwrap().inserts[0].effect, "eq");
    }
}
