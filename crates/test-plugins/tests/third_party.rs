//! Opens the editors of real third-party plugins through Dragon DAW's hosts. Opt-in (it needs installed
//! plugins and, on Linux, an X server):
//!
//! ```text
//! DRAGON_THIRD_PARTY=/usr/lib/vst3:/usr/lib/clap cargo test -p dragon-test-plugins --test third_party -- --ignored --nocapture
//! ```
//!
//! On Windows use `;` between folders, e.g. `C:\Program Files\Common Files\VST3;C:\Program Files\Common Files\CLAP`.
//! Every plugin found is loaded, its own GUI opened and kept running for a moment, and checked to have put a
//! window into the host's editor window. Plugins that have no GUI at all are listed separately (the host
//! shows its generic controls for them). A report lists the result per plugin.

use dragon_engine::effects::Effect;
use dragon_engine::instruments::InstrumentVoice;
use dragon_engine::model::PluginFormat;
use dragon_engine::plugins::{clap::ClapPlugin, scan_file, vst3::Vst3Plugin};
use std::path::PathBuf;

fn plugin_files() -> Vec<(PluginFormat, PathBuf)> {
    let Some(dirs) = std::env::var_os("DRAGON_THIRD_PARTY") else { return Vec::new() };
    let mut out = Vec::new();
    for dir in std::env::split_paths(&dirs) {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            match p.extension().and_then(|e| e.to_str()) {
                Some("vst3") => out.push((PluginFormat::Vst3, p)),
                Some("clap") => out.push((PluginFormat::Clap, p)),
                _ => {}
            }
        }
    }
    out
}

/// What the check needs from a loaded plugin, effect or instrument alike.
trait Editable {
    fn open(&mut self, open: bool) -> bool;
    fn pump(&mut self) -> bool;
    fn error(&self) -> Option<String>;
    fn children(&self) -> usize;
}

/// A plugin used through its effect (`AsEffect`) or instrument (`AsInstrument`) interface.
struct AsEffect<T>(T);
struct AsInstrument<T>(T);

macro_rules! editable {
    ($wrap:ident, $t:ty, $tr:ident) => {
        impl Editable for $wrap<$t> {
            fn open(&mut self, open: bool) -> bool {
                $tr::editor(&mut self.0, open)
            }
            fn pump(&mut self) -> bool {
                $tr::idle(&mut self.0)
            }
            fn error(&self) -> Option<String> {
                $tr::editor_error(&self.0)
            }
            fn children(&self) -> usize {
                self.0.editor_window().map(|w| w.child_count()).unwrap_or(0)
            }
        }
    };
}
editable!(AsEffect, ClapPlugin, Effect);
editable!(AsEffect, Vst3Plugin, Effect);
editable!(AsInstrument, ClapPlugin, InstrumentVoice);
editable!(AsInstrument, Vst3Plugin, InstrumentVoice);

/// Opens, pumps and closes one editor; returns the window count seen inside the host window. The GUI gets
/// up to 3 s to appear (OpenGL GUIs on a software-rendered display can be slow) and must stay open.
fn exercise(p: &mut dyn Editable) -> Result<usize, String> {
    if !p.open(true) {
        return Err(p.error().unwrap_or_else(|| "editor(true) returned false".into()));
    }
    let mut n = 0;
    for i in 0..300 {
        if !p.pump() {
            return Err("the editor closed by itself".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        if i >= 30 {
            n = p.children();
            if n > 0 {
                break;
            }
        }
    }
    p.open(false);
    Ok(n)
}

#[test]
#[ignore]
fn third_party_editors_open() {
    if cfg!(target_os = "linux") && std::env::var_os("DISPLAY").is_none() {
        eprintln!("skipped: no DISPLAY");
        return;
    }
    let files = plugin_files();
    if files.is_empty() {
        eprintln!("skipped: set DRAGON_THIRD_PARTY to plugin folders");
        return;
    }
    let mut ok = 0;
    let mut no_gui = Vec::new();
    let mut failed = Vec::new();
    for (format, path) in files {
        let infos = match scan_file(format, &path) {
            Ok(i) => i,
            Err(e) => {
                failed.push(format!("{}: scan failed: {e}", path.display()));
                continue;
            }
        };
        for info in infos {
            let r = info.to_ref();
            let label = format!("{:?} {} ({})", format, info.name, if info.instrument { "instrument" } else { "effect" });
            let result = match (format, info.instrument) {
                (PluginFormat::Clap, false) => ClapPlugin::load(&r, 48000.0, 512, false).map_err(|e| e.to_string()).and_then(|p| exercise(&mut AsEffect(p))),
                (PluginFormat::Clap, true) => ClapPlugin::load(&r, 48000.0, 512, true).map_err(|e| e.to_string()).and_then(|p| exercise(&mut AsInstrument(p))),
                (_, false) => Vst3Plugin::load(&r, 48000.0, 512, false).map_err(|e| e.to_string()).and_then(|p| exercise(&mut AsEffect(p))),
                (_, true) => Vst3Plugin::load(&r, 48000.0, 512, true).map_err(|e| e.to_string()).and_then(|p| exercise(&mut AsInstrument(p))),
            };
            match result {
                Ok(n) if n > 0 => {
                    ok += 1;
                    println!("OK    {label}: GUI embedded ({n} window(s))");
                }
                Ok(_) => failed.push(format!("{label}: editor opened but put no window into the host window")),
                Err(e) if e.contains("has no editor view") || e.contains("has no GUI") => no_gui.push(format!("{label}: {e}")),
                Err(e) => failed.push(format!("{label}: {e}")),
            }
        }
    }
    for n in &no_gui {
        println!("NOGUI {n}");
    }
    for f in &failed {
        println!("FAIL  {f}");
    }
    println!("{ok} editors opened, {} plugins without a GUI, {} failed", no_gui.len(), failed.len());
    assert!(failed.is_empty(), "{} plugin editors failed", failed.len());
}
