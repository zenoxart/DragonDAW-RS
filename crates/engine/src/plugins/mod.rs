//! Third-party plugin hosting: CLAP and VST3 instruments and effects.
//!
//! * **Scanning** looks through the standard plugin folders (plus user folders) and reads each library's
//!   descriptors. [`scan_isolated`] runs the scan of one file in a child process (`dragon scan-plugin`), so
//!   a plugin that crashes while loading cannot take the DAW down – the same idea as FL Studio's
//!   verification scan.
//! * **Hosting** wraps a plugin instance in the engine's [`Effect`] / [`InstrumentVoice`] traits: audio in
//!   place, notes as events, parameters as sample-accurate parameter events, latency reported to the plugin
//!   delay compensation, state saved into the project.

pub mod clap;
pub mod vst3;
pub mod window;

use crate::effects::Effect;
use crate::instruments::InstrumentVoice;
use crate::model::{PluginFormat, PluginRef};
use crate::render::PluginFactory;
use crate::{EngineError, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PluginInfo {
    pub format: PluginFormat,
    pub path: PathBuf,
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub instrument: bool,
    pub category: String,
}

impl PluginInfo {
    pub fn to_ref(&self) -> PluginRef {
        PluginRef { format: self.format, path: self.path.clone(), id: self.id.clone(), name: self.name.clone(), state: Vec::new(), params: Default::default() }
    }
}

/// Standard plugin folders of the platform.
pub fn default_paths() -> Vec<PathBuf> {
    let mut v = Vec::new();
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    #[cfg(target_os = "linux")]
    {
        if let Some(h) = &home {
            v.push(h.join(".clap"));
            v.push(h.join(".vst3"));
        }
        v.extend(["/usr/lib/clap", "/usr/local/lib/clap", "/usr/lib/vst3", "/usr/local/lib/vst3"].iter().map(PathBuf::from));
    }
    #[cfg(target_os = "windows")]
    {
        let common = std::env::var_os("COMMONPROGRAMFILES").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Program Files\Common Files"));
        v.push(common.join("CLAP"));
        v.push(common.join("VST3"));
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            let common = PathBuf::from(local).join("Programs").join("Common");
            v.push(common.join("CLAP"));
            v.push(common.join("VST3"));
        }
        let _ = &home;
    }
    #[cfg(target_os = "macos")]
    {
        v.push(PathBuf::from("/Library/Audio/Plug-Ins/CLAP"));
        v.push(PathBuf::from("/Library/Audio/Plug-Ins/VST3"));
        if let Some(h) = &home {
            v.push(h.join("Library/Audio/Plug-Ins/CLAP"));
            v.push(h.join("Library/Audio/Plug-Ins/VST3"));
        }
    }
    v
}

/// Plugin files below the given folders (`.clap` files/bundles and `.vst3` bundles/files).
pub fn find_plugin_files(roots: &[PathBuf]) -> Vec<(PluginFormat, PathBuf)> {
    let mut out = Vec::new();
    fn walk(dir: &Path, depth: usize, out: &mut Vec<(PluginFormat, PathBuf)>) {
        if depth > 6 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let ext = p.extension().and_then(|x| x.to_str()).map(|x| x.to_ascii_lowercase());
            match ext.as_deref() {
                Some("clap") => out.push((PluginFormat::Clap, p)),
                Some("vst3") => out.push((PluginFormat::Vst3, p)),
                _ if p.is_dir() => walk(&p, depth + 1, out),
                _ => {}
            }
        }
    }
    for r in roots {
        walk(r, 0, &mut out);
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out.dedup();
    out
}

/// Reads the descriptors of one plugin file (loads it into this process).
pub fn scan_file(format: PluginFormat, path: &Path) -> Result<Vec<PluginInfo>> {
    match format {
        PluginFormat::Clap => clap::scan(path),
        PluginFormat::Vst3 => vst3::scan(path),
    }
}

/// Scans one file in a child process: `exe scan-plugin <format> <path>` must print the JSON list.
pub fn scan_isolated(exe: &Path, format: PluginFormat, path: &Path, timeout: std::time::Duration) -> Result<Vec<PluginInfo>> {
    let fmt = match format {
        PluginFormat::Clap => "clap",
        PluginFormat::Vst3 => "vst3",
    };
    let mut child =
        std::process::Command::new(exe).arg("scan-plugin").arg(fmt).arg(path).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn().map_err(|e| EngineError::Plugin(e.to_string()))?;
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut out = String::new();
                if let Some(mut s) = child.stdout.take() {
                    use std::io::Read;
                    let _ = s.read_to_string(&mut out);
                }
                if !status.success() {
                    return Err(EngineError::Plugin(format!("{} crashed or failed while loading ({status})", path.display())));
                }
                return serde_json::from_str(&out).map_err(|e| EngineError::Plugin(format!("bad scan output: {e}")));
            }
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                return Err(EngineError::Plugin(format!("{} did not answer within {:?}", path.display(), timeout)));
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
            Err(e) => return Err(EngineError::Plugin(e.to_string())),
        }
    }
}

/// Loaded libraries stay loaded for the life of the process (unloading code that may still have threads or
/// callbacks running is a common source of crashes).
pub(crate) fn library(path: &Path) -> Result<Arc<libloading::Library>> {
    static LIBS: OnceLock<Mutex<HashMap<PathBuf, Arc<libloading::Library>>>> = OnceLock::new();
    let libs = LIBS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut g = libs.lock().unwrap();
    if let Some(l) = g.get(path) {
        return Ok(l.clone());
    }
    // SAFETY: loading a plugin library runs its initialisers; this is inherent to plugin hosting.
    let lib = unsafe { libloading::Library::new(path) }.map_err(|e| EngineError::Plugin(format!("{}: {e}", path.display())))?;
    let lib = Arc::new(lib);
    g.insert(path.to_path_buf(), lib.clone());
    Ok(lib)
}

/// Creates plugin instances for the renderer.
#[derive(Default)]
pub struct Host;

impl PluginFactory for Host {
    fn create_effect(&mut self, p: &PluginRef, rate: f32, max_block: usize) -> Option<Box<dyn Effect>> {
        let r: Result<Box<dyn Effect>> = match p.format {
            PluginFormat::Clap => clap::ClapPlugin::load(p, rate, max_block, false).map(|x| Box::new(x) as Box<dyn Effect>),
            PluginFormat::Vst3 => vst3::Vst3Plugin::load(p, rate, max_block, false).map(|x| Box::new(x) as Box<dyn Effect>),
        };
        r.map_err(|e| tracing::error!("plugin {}: {e}", p.name)).ok()
    }

    fn create_instrument(&mut self, p: &PluginRef, rate: f32, max_block: usize) -> Option<Box<dyn InstrumentVoice>> {
        let r: Result<Box<dyn InstrumentVoice>> = match p.format {
            PluginFormat::Clap => clap::ClapPlugin::load(p, rate, max_block, true).map(|x| Box::new(x) as Box<dyn InstrumentVoice>),
            PluginFormat::Vst3 => vst3::Vst3Plugin::load(p, rate, max_block, true).map(|x| Box::new(x) as Box<dyn InstrumentVoice>),
        };
        r.map_err(|e| tracing::error!("plugin {}: {e}", p.name)).ok()
    }
}

/// Pending note / parameter events for the next process call.
#[derive(Debug, Clone, Copy)]
pub(crate) enum HostEvent {
    NoteOn { key: u8, velocity: f32 },
    NoteOff { key: u8 },
    Param { id: u32, value: f64 },
}

/// Planar scratch buffers handed to plugins.
pub(crate) struct Scratch {
    pub in_l: Vec<f32>,
    pub in_r: Vec<f32>,
    pub sc_l: Vec<f32>,
    pub sc_r: Vec<f32>,
    pub out_l: Vec<f32>,
    pub out_r: Vec<f32>,
}

impl Scratch {
    pub fn new(n: usize) -> Self {
        Scratch { in_l: vec![0.0; n], in_r: vec![0.0; n], sc_l: vec![0.0; n], sc_r: vec![0.0; n], out_l: vec![0.0; n], out_r: vec![0.0; n] }
    }
    /// Copies the sidechain signal (or silence) into the sidechain buffers.
    pub fn set_sidechain(&mut self, n: usize, sc: Option<(&[f32], &[f32])>) {
        match sc {
            Some((a, b)) => {
                self.sc_l[..n].copy_from_slice(&a[..n]);
                self.sc_r[..n].copy_from_slice(&b[..n]);
            }
            None => {
                self.sc_l[..n].fill(0.0);
                self.sc_r[..n].fill(0.0);
            }
        }
    }
    pub fn ensure(&mut self, n: usize) {
        if self.in_l.len() < n {
            *self = Scratch::new(n);
        }
    }
}
