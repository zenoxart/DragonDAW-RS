//! Presets of third-party plugins.
//!
//! * **VST3**: `.vstpreset` files in the standard preset folders (`Documents\VST3 Presets` …), matched to
//!   the plugin by the class id in the file's header; saving writes one there too, so other hosts see it.
//! * **CLAP** (and anything without a standard file): Dragon presets, `.dpreset` files in
//!   `<presets dir>/<plugin name>/` holding the plugin's saved state.
//!
//! The state a preset loads is the same blob the project stores for the plugin (for VST3: component and
//! controller state packed as in [`pack`]).

use crate::model::{PluginFormat, PluginRef};
use std::path::{Path, PathBuf};

/// One preset found for a plugin.
#[derive(Debug, Clone, PartialEq)]
pub struct Preset {
    pub name: String,
    pub path: PathBuf,
    /// Folder it was found in, relative to its root (a category such as "Bass" or the vendor's).
    pub group: String,
}

const DRAGON_MAGIC: &[u8; 4] = b"DRGP";

/// Packs VST3 component and controller state (the project's format for VST3 state).
pub fn pack(component: &[u8], controller: &[u8]) -> Vec<u8> {
    let mut v = (component.len() as u32).to_le_bytes().to_vec();
    v.extend_from_slice(component);
    v.extend_from_slice(controller);
    v
}

fn unpack(state: &[u8]) -> (Vec<u8>, Vec<u8>) {
    if state.len() < 4 {
        return (Vec::new(), Vec::new());
    }
    let n = (u32::from_le_bytes([state[0], state[1], state[2], state[3]]) as usize).min(state.len() - 4);
    (state[4..4 + n].to_vec(), state[4 + n..].to_vec())
}

/// The standard VST3 preset folders of this system.
pub fn vst3_roots() -> Vec<PathBuf> {
    let mut v = Vec::new();
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    if cfg!(windows) {
        if let Some(h) = env("USERPROFILE") {
            v.push(h.join("Documents").join("VST3 Presets"));
        }
        if let Some(a) = env("APPDATA") {
            v.push(a.join("VST3 Presets"));
        }
        if let Some(pd) = env("PROGRAMDATA") {
            v.push(pd.join("VST3 Presets"));
        }
    } else if cfg!(target_os = "macos") {
        if let Some(h) = env("HOME") {
            v.push(h.join("Library/Audio/Presets"));
        }
        v.push(PathBuf::from("/Library/Audio/Presets"));
    } else {
        if let Some(h) = env("HOME") {
            v.push(h.join(".vst3/presets"));
        }
        v.push(PathBuf::from("/usr/share/vst3/presets"));
        v.push(PathBuf::from("/usr/local/share/vst3/presets"));
    }
    v
}

/// Where a new VST3 preset of `plugin` is saved.
pub fn vst3_save_dir(plugin: &PluginRef) -> PathBuf {
    vst3_roots().into_iter().next().unwrap_or_else(std::env::temp_dir).join(safe(&plugin.name))
}

fn safe(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_alphanumeric() || " -_.()".contains(c) { c } else { '_' }).collect();
    let t = t.trim().to_string();
    if t.is_empty() {
        "Preset".into()
    } else {
        t
    }
}

/// Normalises a VST3 class id for comparing ("{…}" and dashes dropped, upper case).
fn norm_id(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_hexdigit()).map(|c| c.to_ascii_uppercase()).collect()
}

/// Reads a `.vstpreset`: (class id, component state, controller state).
pub fn read_vstpreset(b: &[u8]) -> Option<(String, Vec<u8>, Vec<u8>)> {
    if b.len() < 48 || &b[0..4] != b"VST3" {
        return None;
    }
    let class = String::from_utf8_lossy(&b[8..40]).to_string();
    let list = i64::from_le_bytes(b[40..48].try_into().ok()?) as usize;
    if list + 8 > b.len() || &b[list..list + 4] != b"List" {
        return None;
    }
    let n = i32::from_le_bytes(b[list + 4..list + 8].try_into().ok()?).max(0) as usize;
    let (mut comp, mut cont) = (Vec::new(), Vec::new());
    for k in 0..n.min(64) {
        let e = list + 8 + k * 20;
        if e + 20 > b.len() {
            break;
        }
        let id = &b[e..e + 4];
        let off = i64::from_le_bytes(b[e + 4..e + 12].try_into().ok()?) as usize;
        let size = i64::from_le_bytes(b[e + 12..e + 20].try_into().ok()?) as usize;
        let Some(data) = b.get(off..off.checked_add(size)?) else { continue };
        match id {
            b"Comp" => comp = data.to_vec(),
            b"Cont" => cont = data.to_vec(),
            _ => {}
        }
    }
    Some((class, comp, cont))
}

/// Writes a `.vstpreset` with the component and (optional) controller state.
pub fn write_vstpreset(class_id: &str, comp: &[u8], cont: &[u8]) -> Vec<u8> {
    let mut id = norm_id(class_id);
    id.truncate(32);
    while id.len() < 32 {
        id.push('0');
    }
    let mut b = Vec::with_capacity(64 + comp.len() + cont.len());
    b.extend_from_slice(b"VST3");
    b.extend_from_slice(&1i32.to_le_bytes());
    b.extend_from_slice(id.as_bytes());
    b.extend_from_slice(&0i64.to_le_bytes()); // list offset, patched below
    let comp_off = b.len();
    b.extend_from_slice(comp);
    let cont_off = b.len();
    b.extend_from_slice(cont);
    let list = b.len();
    b.extend_from_slice(b"List");
    let entries: Vec<(&[u8; 4], usize, usize)> = if cont.is_empty() { vec![(b"Comp", comp_off, comp.len())] } else { vec![(b"Comp", comp_off, comp.len()), (b"Cont", cont_off, cont.len())] };
    b.extend_from_slice(&(entries.len() as i32).to_le_bytes());
    for (id, off, size) in entries {
        b.extend_from_slice(id);
        b.extend_from_slice(&(off as i64).to_le_bytes());
        b.extend_from_slice(&(size as i64).to_le_bytes());
    }
    b[40..48].copy_from_slice(&(list as i64).to_le_bytes());
    b
}

fn walk(dir: &Path, root: &Path, depth: usize, ext: &str, out: &mut Vec<(PathBuf, String)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if depth > 0 {
                walk(&p, root, depth - 1, ext, out);
            }
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case(ext)) {
            let group = p.parent().and_then(|d| d.strip_prefix(root).ok()).map(|g| g.display().to_string()).unwrap_or_default();
            out.push((p, group));
        }
        if out.len() > 20_000 {
            return;
        }
    }
}

/// The presets of `plugin`: matching `.vstpreset` files (VST3) and its Dragon presets, by group and name.
pub fn find(plugin: &PluginRef, dragon_dir: &Path) -> Vec<Preset> {
    let mut out = Vec::new();
    if plugin.format == PluginFormat::Vst3 {
        let want = norm_id(&plugin.id);
        for root in vst3_roots() {
            let mut files = Vec::new();
            walk(&root, &root, 4, "vstpreset", &mut files);
            for (f, group) in files {
                let mut head = [0u8; 40];
                let ok = std::fs::File::open(&f).and_then(|mut h| std::io::Read::read_exact(&mut h, &mut head)).is_ok();
                if ok && &head[0..4] == b"VST3" && norm_id(&String::from_utf8_lossy(&head[8..40])) == want {
                    out.push(Preset { name: stem(&f), path: f, group });
                }
            }
        }
    }
    let own = dragon_dir.join(safe(&plugin.name));
    let mut files = Vec::new();
    walk(&own, &own, 2, "dpreset", &mut files);
    out.extend(files.into_iter().map(|(f, group)| Preset { name: stem(&f), path: f, group: if group.is_empty() { "Dragon presets".into() } else { group } }));
    out.sort_by(|a, b| a.group.to_lowercase().cmp(&b.group.to_lowercase()).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    out.dedup_by(|a, b| a.path == b.path);
    out
}

fn stem(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

/// The state to load for preset file `path` (as stored in the project for this plugin format).
pub fn load(plugin: &PluginRef, path: &Path) -> Result<Vec<u8>, String> {
    let b = std::fs::read(path).map_err(|e| e.to_string())?;
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("vstpreset")) {
        let (class, comp, cont) = read_vstpreset(&b).ok_or("not a VST3 preset")?;
        if plugin.format == PluginFormat::Vst3 && norm_id(&class) != norm_id(&plugin.id) {
            return Err("the preset belongs to another plugin".into());
        }
        if comp.is_empty() {
            return Err("the preset has no plugin state".into());
        }
        return Ok(pack(&comp, &cont));
    }
    if b.len() < 8 || &b[0..4] != DRAGON_MAGIC {
        return Err("not a Dragon preset".into());
    }
    let n = u32::from_le_bytes([b[4], b[5], b[6], b[7]]) as usize;
    let id = b.get(8..8 + n).map(|x| String::from_utf8_lossy(x).to_string()).unwrap_or_default();
    if id != plugin.id {
        return Err("the preset belongs to another plugin".into());
    }
    Ok(b[8 + n..].to_vec())
}

/// Saves `state` of `plugin` as preset `name`: a `.vstpreset` for VST3 (in the standard folder, so other
/// hosts find it), else a Dragon preset in `dragon_dir`. Returns the file.
pub fn save(plugin: &PluginRef, name: &str, state: &[u8], dragon_dir: &Path) -> Result<PathBuf, String> {
    let (dir, file, bytes) = if plugin.format == PluginFormat::Vst3 {
        let (comp, cont) = unpack(state);
        let dir = vst3_save_dir(plugin);
        (dir.clone(), dir.join(format!("{}.vstpreset", safe(name))), write_vstpreset(&plugin.id, &comp, &cont))
    } else {
        let dir = dragon_dir.join(safe(&plugin.name));
        let mut b = DRAGON_MAGIC.to_vec();
        b.extend_from_slice(&(plugin.id.len() as u32).to_le_bytes());
        b.extend_from_slice(plugin.id.as_bytes());
        b.extend_from_slice(state);
        (dir.clone(), dir.join(format!("{}.dpreset", safe(name))), b)
    };
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(&file, bytes).map_err(|e| e.to_string())?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(format: PluginFormat) -> PluginRef {
        PluginRef { format, path: PathBuf::from("x"), id: "ABCDEF0123456789ABCDEF0123456789".into(), name: "Test Synth".into(), state: Vec::new(), params: Default::default() }
    }

    /// A .vstpreset written here reads back (header, chunk list, both states) and loads as packed state.
    #[test]
    fn vstpreset_round_trip() {
        let b = write_vstpreset("{ABCDEF01-2345-6789-ABCD-EF0123456789}", b"component", b"controller");
        let (class, comp, cont) = read_vstpreset(&b).unwrap();
        assert_eq!(norm_id(&class), "ABCDEF0123456789ABCDEF0123456789");
        assert_eq!((comp.as_slice(), cont.as_slice()), (&b"component"[..], &b"controller"[..]));
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("p.vstpreset");
        std::fs::write(&f, &b).unwrap();
        let st = load(&plugin(PluginFormat::Vst3), &f).unwrap();
        assert_eq!(unpack(&st), (b"component".to_vec(), b"controller".to_vec()));
        let mut other = plugin(PluginFormat::Vst3);
        other.id = "00000000000000000000000000000000".into();
        assert!(load(&other, &f).is_err(), "another plugin's preset is refused");
    }

    /// Dragon presets (CLAP): save, find, load; another plugin cannot load them.
    #[test]
    fn dragon_presets_save_find_load() {
        let dir = tempfile::tempdir().unwrap();
        let p = plugin(PluginFormat::Clap);
        let f = save(&p, "Fat / Bass", b"state-bytes", dir.path()).unwrap();
        assert!(f.ends_with("Test Synth/Fat _ Bass.dpreset"), "{}", f.display());
        let found = find(&p, dir.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Fat _ Bass");
        assert_eq!(load(&p, &found[0].path).unwrap(), b"state-bytes");
        let mut other = p.clone();
        other.id = "other".into();
        assert!(load(&other, &f).is_err());
    }
}
