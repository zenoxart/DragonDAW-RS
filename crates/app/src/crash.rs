//! Crash recovery.
//!
//! - A panic hook writes a crash report (message, place, backtrace) to `<local data>/Crash/`.
//! - While a project opens or exports, a marker file names it; the plugin host leaves a breadcrumb naming
//!   the plugin being loaded. A native plugin crash cannot be caught, but the markers survive it.
//! - At the next start, left-over markers mean the last run died there: the app offers to open the
//!   project in safe mode – third-party plugins off, or loaded without their saved state – instead of
//!   crashing again.

use std::path::{Path, PathBuf};

/// How a project is opened after a crash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafeMode {
    /// Third-party plugins are not loaded at all (their slots keep their settings and come back when the
    /// project is opened normally).
    NoPlugins,
    /// Plugins load at their default settings; their saved state is ignored (it is replaced when the
    /// project is saved).
    NoPluginStates,
}

impl SafeMode {
    pub fn label(self) -> &'static str {
        match self {
            SafeMode::NoPlugins => "third-party plugins off",
            SafeMode::NoPluginStates => "plugins without their saved settings",
        }
    }
}

/// What the last run was doing when it ended unexpectedly.
#[derive(Debug, Clone, Default)]
pub struct CrashInfo {
    /// "opening" or "exporting".
    pub during: String,
    pub project: Option<PathBuf>,
    /// The plugin that was loading (name, kind, path).
    pub plugin: Option<String>,
    /// The crash report written by the panic hook, if the crash was a panic.
    pub report: Option<PathBuf>,
    pub message: Option<String>,
}

fn dir(local: &Path) -> PathBuf {
    local.join("Crash")
}

/// Installs the panic hook and the plugin breadcrumb. Call once, first thing at startup.
pub fn install(local: &Path) {
    let d = dir(local);
    let _ = std::fs::create_dir_all(&d);
    dragon_engine::plugins::set_breadcrumb_file(d.join("plugin-loading.txt"));
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_else(|| "panic".into());
        let at = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
        let bt = std::backtrace::Backtrace::force_capture();
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|x| x.as_secs()).unwrap_or(0);
        let text = format!("Dragon DAW {} crashed\nthread: {thread}\nmessage: {msg}\nat: {at}\n\n{bt}\n", env!("CARGO_PKG_VERSION"));
        let _ = std::fs::write(d.join(format!("crash-{secs}.txt")), &text);
        let _ = std::fs::write(d.join("last-crash.txt"), &text);
        prev(info);
    }));
}

/// Marks that a project is being opened / exported ("opening" / "exporting").
pub fn mark(local: &Path, kind: &str, path: &Path) {
    let d = dir(local);
    let _ = std::fs::create_dir_all(&d);
    let _ = std::fs::write(d.join(format!("{kind}.txt")), path.to_string_lossy().as_bytes());
}

/// The project opened / exported fine.
pub fn clear(local: &Path, kind: &str) {
    let _ = std::fs::remove_file(dir(local).join(format!("{kind}.txt")));
}

/// Reads (and clears) what the last run left behind. `None` when it ended normally.
pub fn take(local: &Path) -> Option<CrashInfo> {
    let d = dir(local);
    let read = |name: &str| -> Option<(String, std::time::SystemTime)> {
        let p = d.join(name);
        let t = std::fs::metadata(&p).and_then(|m| m.modified()).ok()?;
        let s = std::fs::read_to_string(&p).ok()?;
        let _ = std::fs::remove_file(&p);
        Some((s, t))
    };
    let opening = read("opening.txt");
    let exporting = read("exporting.txt");
    let plugin = dragon_engine::plugins::take_breadcrumb();
    let (during, marker) = match (&opening, &exporting) {
        (Some(o), _) => ("opening".to_string(), Some(o.clone())),
        (None, Some(e)) => ("exporting".to_string(), Some(e.clone())),
        _ if plugin.is_some() => ("loading a plugin".to_string(), None),
        _ => return None,
    };
    // A panic report from the same run (not older than the marker).
    let since = marker.as_ref().map(|m| m.1);
    let report = d.join("last-crash.txt");
    let (report, message) = match std::fs::metadata(&report).and_then(|m| m.modified()) {
        Ok(t) if since.is_none_or(|s| t >= s) => {
            let text = std::fs::read_to_string(&report).unwrap_or_default();
            let msg = text.lines().find_map(|l| l.strip_prefix("message: ")).map(str::to_string);
            (Some(report), msg)
        }
        _ => (None, None),
    };
    let project = match during.as_str() {
        "opening" => marker.map(|m| PathBuf::from(m.0.trim())),
        _ => None,
    };
    Some(CrashInfo { during, project, plugin, report, message })
}

/// The window shown after a crash: what happened, and how to open the project again.
pub fn window(ctx: &egui::Context, st: &mut crate::state::State) {
    let Some(info) = st.crash.clone() else { return };
    let mut close = false;
    egui::Window::new("Dragon DAW closed unexpectedly").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
        ui.set_max_width(520.0);
        let what = match (&info.project, info.during.as_str()) {
            (Some(p), "opening") => format!("It closed while opening {}.", p.display()),
            (_, "exporting") => "It closed while exporting.".to_string(),
            _ => format!("It closed while {}.", info.during),
        };
        ui.label(what);
        if let Some(pl) = &info.plugin {
            let first = pl.lines().next().unwrap_or(pl);
            ui.label(egui::RichText::new(format!("The plugin loading at that moment: {first}")).strong().color(crate::theme::WARN));
            ui.label("Third-party plugins run inside Dragon DAW; when one crashes, it takes the app with it.");
        }
        if let Some(m) = &info.message {
            ui.label(egui::RichText::new(format!("Error: {m}")).monospace().small());
        }
        if let Some(r) = &info.report {
            ui.label(egui::RichText::new(format!("Crash report: {}", r.display())).small().color(crate::theme::TEXT_DIM));
        }
        ui.add_space(8.0);
        if let Some(p) = info.project.clone() {
            ui.label("Open the project again:");
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button("Safe mode – plugins off")
                    .on_hover_text("Third-party plugins are not loaded; their slots keep their settings. Save, remove or replace the plugin that crashed, then open the project normally.")
                    .clicked()
                {
                    st.open_path_mode(&p, Some(SafeMode::NoPlugins));
                    close = true;
                }
                if ui.button("Plugins without saved settings").on_hover_text("Plugins load at their defaults – their saved state (which may be what crashes) is ignored and replaced when you save.").clicked() {
                    st.open_path_mode(&p, Some(SafeMode::NoPluginStates));
                    close = true;
                }
                if ui.button("Open normally").clicked() {
                    st.open_path_mode(&p, None);
                    close = true;
                }
                if ui.button("Not now").clicked() {
                    close = true;
                }
            });
        } else if ui.button("OK").clicked() {
            close = true;
        }
    });
    if close {
        st.crash = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_survive_a_crash_and_are_read_once() {
        let d = std::env::temp_dir().join(format!("dragon-crash-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        assert!(take(&d).is_none(), "clean start");
        mark(&d, "opening", Path::new("/music/song.dragon"));
        let i = take(&d).expect("the open did not finish");
        assert_eq!(i.during, "opening");
        assert_eq!(i.project.as_deref(), Some(Path::new("/music/song.dragon")));
        assert!(take(&d).is_none(), "read once");
        mark(&d, "exporting", Path::new("/x/mix.wav"));
        clear(&d, "exporting");
        assert!(take(&d).is_none(), "a finished export leaves nothing");
        mark(&d, "exporting", Path::new("/x/mix.wav"));
        let i = take(&d).unwrap();
        assert_eq!(i.during, "exporting");
        assert!(i.project.is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}
