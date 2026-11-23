//! What a project file may make the app load. A `.dragon` project can come from anyone (a shared beat, a
//! template, a collab), and it names files by path – so it must not be able to:
//!
//! - **load code**: a plugin is a library that runs with the user's rights. Only plugin files the user
//!   scanned (or allowed) and those in the platform's standard plugin folders are loaded; a project that
//!   names another path loads the same plugin (by id) from where it is installed here, or nothing until the
//!   user allows it;
//! - **reach out over the network**: on Windows, opening `\\server\share\…` makes the computer sign in to
//!   that server and hands it the user's (hashed) Windows credentials. Network locations named by an opened
//!   project are blocked until the user allows them; files the user picks or drops themselves are not
//!   affected.
//!
//! Whatever was blocked is queued for the UI ([`take_blocked`]), which asks the user.

use crate::model::{PluginFormat, PluginRef};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Something a project wanted that was not loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Blocked {
    /// A plugin file in a place that is not known as a plugin folder (name, path).
    Plugin { name: String, path: PathBuf },
    /// A network location (`\\server\share`) the project reads files from.
    Network { root: String },
}

#[derive(Default)]
struct Trust {
    plugin_paths: HashSet<PathBuf>,
    plugin_ids: HashMap<(bool, String), PathBuf>,
    blocked_roots: HashSet<String>,
    /// Network locations the user allowed (kept in the settings).
    allowed_roots: HashSet<String>,
    queue: Vec<Blocked>,
}

fn state() -> std::sync::MutexGuard<'static, Trust> {
    static T: OnceLock<Mutex<Trust>> = OnceLock::new();
    T.get_or_init(Default::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn key(p: &Path) -> PathBuf {
    // Case-insensitive file systems: compare lower-case on Windows and macOS.
    let s = p.to_string_lossy().replace('/', std::path::MAIN_SEPARATOR_STR);
    PathBuf::from(if cfg!(any(windows, target_os = "macos")) { s.to_lowercase() } else { s })
}

fn queue(b: Blocked) {
    let mut t = state();
    if !t.queue.contains(&b) {
        t.queue.push(b);
    }
}

/// The share a Windows network path is on (`\\server\share`), or `None` for a local path. Device paths
/// (`\\.\…`) count as network too (never something a project should name); `\\?\C:\…` is local.
pub fn network_root(p: &Path) -> Option<String> {
    let s = p.to_string_lossy().replace('/', "\\");
    let rest = if let Some(r) = s.strip_prefix(r"\\?\UNC\") {
        r
    } else if s.starts_with(r"\\?\") {
        return None;
    } else {
        s.strip_prefix(r"\\")?
    };
    let mut parts = rest.split('\\').filter(|x| !x.is_empty());
    let server = parts.next().unwrap_or("");
    let share = parts.next().unwrap_or("");
    Some(format!(r"\\{}\{}", server, share).to_lowercase())
}

// ------------------------------------------------------------------------------------------- plugins

/// Plugin files the user scanned or allowed: loaded wherever they are, and by id for projects made on
/// another computer.
pub fn trust_plugins<'a>(list: impl IntoIterator<Item = (PluginFormat, &'a str, &'a Path)>) {
    let mut t = state();
    for (fmt, id, path) in list {
        t.plugin_paths.insert(key(path));
        t.plugin_ids.entry((fmt == PluginFormat::Clap, id.to_string())).or_insert_with(|| path.to_path_buf());
    }
}

/// Allows one plugin file (the user said so).
pub fn trust_plugin_path(path: &Path) {
    state().plugin_paths.insert(key(path));
}

fn in_standard_folder(path: &Path) -> bool {
    // "…\VST3\..\..\Downloads\x.vst3" is not inside the VST3 folder.
    if path.components().any(|c| matches!(c, std::path::Component::ParentDir | std::path::Component::CurDir)) {
        return false;
    }
    let k = key(path);
    crate::plugins::default_paths().iter().any(|r| k.starts_with(key(r)))
}

/// The file to load for a project's plugin: its own path when that is allowed, else the same plugin
/// installed here (same id), else `None` (queued for the user).
pub fn resolve_plugin(p: &PluginRef) -> Option<PathBuf> {
    let local = network_root(&p.path).is_none();
    let allowed = local && (state().plugin_paths.contains(&key(&p.path)) || in_standard_folder(&p.path));
    if allowed {
        return Some(p.path.clone());
    }
    if let Some(here) = state().plugin_ids.get(&(p.format == PluginFormat::Clap, p.id.clone())).cloned() {
        return Some(here);
    }
    tracing::warn!("plugin {} not loaded: {} is not a known plugin location", p.name, p.path.display());
    queue(Blocked::Plugin { name: p.name.clone(), path: p.path.clone() });
    None
}

// ------------------------------------------------------------------------------------------- network

/// Blocks the network locations a just-opened project reads from (except `allow`, the share the project
/// itself is on); returns the blocked roots, which are also queued for the user.
pub fn block_network_files<'a>(files: impl IntoIterator<Item = &'a Path>, allow: Option<&Path>) -> Vec<String> {
    let own = allow.and_then(network_root);
    let allowed = state().allowed_roots.clone();
    let mut roots: Vec<String> = files.into_iter().filter_map(network_root).filter(|r| Some(r) != own.as_ref() && !allowed.contains(r)).collect();
    roots.sort();
    roots.dedup();
    {
        let mut t = state();
        for r in &roots {
            t.blocked_roots.insert(r.clone());
        }
    }
    for r in &roots {
        queue(Blocked::Network { root: r.clone() });
    }
    roots
}

/// Lifts the block on a network location (the user allowed it).
pub fn allow_network(root: &str) {
    let r = root.to_lowercase();
    let mut t = state();
    t.blocked_roots.remove(&r);
    t.allowed_roots.insert(r);
}

/// Whether `path` may be read (it is not on a blocked network location).
pub fn may_read(path: &Path) -> bool {
    match network_root(path) {
        Some(r) => !state().blocked_roots.contains(&r),
        None => true,
    }
}

/// An error for reading a blocked file.
pub fn check_read(path: &Path) -> crate::Result<()> {
    if may_read(path) {
        Ok(())
    } else {
        Err(crate::EngineError::Io(format!("{}: on a network location this project is not allowed to read (yet)", path.display())))
    }
}

/// The SHA-256 of a file (lower-case hex): downloads are checked against their published checksums.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// The checksum for `file_name` in a `sha256sum`-style list ("<hex>  <name>" per line).
pub fn checksum_for(list: &str, file_name: &str) -> Option<String> {
    list.lines().find_map(|l| {
        let (hex, name) = l.trim().split_once(char::is_whitespace)?;
        (name.trim().trim_start_matches('*') == file_name && hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit())).then(|| hex.to_ascii_lowercase())
    })
}

/// A program that comes with the system: on Windows taken from System32 (never a same-named program that
/// happens to be found first), elsewhere from PATH.
pub fn system_tool(name: &str) -> PathBuf {
    #[cfg(windows)]
    {
        let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let sub = if name.eq_ignore_ascii_case("powershell.exe") { root.join(r"System32\WindowsPowerShell\v1.0") } else { root.join("System32") };
        let p = sub.join(name);
        if p.is_file() {
            return p;
        }
    }
    PathBuf::from(name)
}

/// What was blocked since the last call (for the UI to ask about).
pub fn take_blocked() -> Vec<Blocked> {
    std::mem::take(&mut state().queue)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_lists() {
        let list = "aaaa  x\n0123456789abcdef0123456789abcdef0123456789abcdef0123456789ABCDEF  ffmpeg.zip\n";
        assert_eq!(checksum_for(list, "ffmpeg.zip").as_deref(), Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"));
        assert_eq!(checksum_for(list, "x"), None, "not a sha256");
        assert_eq!(checksum_for(list, "other.zip"), None);
    }

    #[test]
    fn network_roots() {
        assert_eq!(network_root(Path::new(r"\\Evil\Share\x\kick.wav")).as_deref(), Some(r"\\evil\share"));
        assert_eq!(network_root(Path::new("//evil@SSL/dav/k.wav")).as_deref(), Some(r"\\evil@ssl\dav"));
        assert_eq!(network_root(Path::new(r"\\?\UNC\nas\music\a.wav")).as_deref(), Some(r"\\nas\music"));
        assert_eq!(network_root(Path::new(r"\\.\pipe\x")).as_deref(), Some(r"\\.\pipe"));
        assert_eq!(network_root(Path::new(r"\\?\C:\Music\a.wav")), None);
        assert_eq!(network_root(Path::new(r"C:\Music\a.wav")), None);
        assert_eq!(network_root(Path::new("/home/me/a.wav")), None);
    }

    #[test]
    fn network_files_of_a_project_wait_for_the_user() {
        let files = [PathBuf::from(r"\\attacker\s\a.wav"), PathBuf::from(r"\\mynas\music\b.wav"), PathBuf::from("/local/c.wav")];
        let roots = block_network_files(files.iter().map(|p| p.as_path()), Some(Path::new(r"\\MyNas\Music\song.dragon")));
        assert_eq!(roots, vec![r"\\attacker\s".to_string()], "the share the project is on stays allowed");
        assert!(!may_read(&files[0]) && may_read(&files[1]) && may_read(&files[2]));
        assert!(take_blocked().contains(&Blocked::Network { root: r"\\attacker\s".into() }));
        allow_network(r"\\attacker\s");
        assert!(may_read(&files[0]));
    }

    #[test]
    fn plugins_load_only_from_known_places() {
        let evil = PluginRef { format: PluginFormat::Vst3, path: PathBuf::from("/tmp/project/evil.vst3"), id: "evil-id".into(), name: "Evil".into(), state: Vec::new(), params: Default::default() };
        assert_eq!(resolve_plugin(&evil), None);
        assert!(take_blocked().iter().any(|b| matches!(b, Blocked::Plugin { name, .. } if name == "Evil")));
        // The same plugin, scanned here at another path: that one loads.
        let here = PathBuf::from("/opt/plugins/Good.vst3");
        trust_plugins([(PluginFormat::Vst3, "good-id", here.as_path())]);
        let moved = PluginRef { format: PluginFormat::Vst3, path: PathBuf::from(r"D:\Their PC\Good.vst3"), id: "good-id".into(), name: "Good".into(), state: Vec::new(), params: Default::default() };
        assert_eq!(resolve_plugin(&moved), Some(here.clone()));
        // A network path is never loaded as is, even when it was scanned.
        let unc = PathBuf::from(r"\\host\share\x.vst3");
        trust_plugin_path(&unc);
        let r = PluginRef { format: PluginFormat::Clap, path: unc, id: "nope".into(), name: "Net".into(), state: Vec::new(), params: Default::default() };
        assert_eq!(resolve_plugin(&r), None);
        let _ = take_blocked();
        // "..": not inside a standard folder.
        if let Some(std_dir) = crate::plugins::default_paths().first() {
            let sneaky = PluginRef { format: PluginFormat::Vst3, path: std_dir.join("..").join("..").join("evil2.vst3"), id: "e2".into(), name: "E2".into(), state: Vec::new(), params: Default::default() };
            assert_eq!(resolve_plugin(&sneaky), None);
            assert_eq!(resolve_plugin(&PluginRef { path: std_dir.join("Real.vst3"), ..sneaky.clone() }), Some(std_dir.join("Real.vst3")));
        }
        let _ = take_blocked();
        // Allowed by the user.
        trust_plugin_path(&evil.path);
        assert_eq!(resolve_plugin(&evil), Some(evil.path.clone()));
    }
}
