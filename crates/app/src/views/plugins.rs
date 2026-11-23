//! Plugin manager: plugin folders, crash-safe scanning (each file in a child process), the plugin list
//! with search, and adding plugins as instruments or effects.

use crate::state::State;
use crate::theme;
use dragon_engine::model::{Insert, Instrument, PluginFormat};
use dragon_engine::plugins::{self, PluginInfo};
use egui::{RichText, Ui};
use redhive_client_framework::runtime::Promise;
use redhive_contracts::ui::Severity;
use std::time::Duration;

/// The executable used for isolated scans: the `dragon` CLI next to this binary, or this binary itself
/// (`dragon-daw scan-plugin` is not a mode of the GUI, so the CLI is preferred).
fn scanner() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let name = if cfg!(windows) { "dragon.exe" } else { "dragon" };
    let p = dir.join(name);
    p.exists().then_some(p)
}

pub fn start_scan(st: &mut State) {
    if st.ui.plugin_scan.is_some() {
        return;
    }
    let mut roots = plugins::default_paths();
    roots.extend(st.settings.plugin_folders.iter().cloned());
    let scanner = scanner();
    st.ui.status = "Scanning plugins…".into();
    st.ui.plugin_scan = Some(Promise::spawn(async move {
        tokio::task::spawn_blocking(move || {
            let mut out: Vec<PluginInfo> = Vec::new();
            let mut errors = Vec::new();
            for (fmt, path) in plugins::find_plugin_files(&roots) {
                let r = match &scanner {
                    Some(exe) => plugins::scan_isolated(exe, fmt, &path, Duration::from_secs(20)),
                    None => plugins::scan_file(fmt, &path),
                };
                match r {
                    Ok(v) => out.extend(v),
                    Err(e) => errors.push(e.to_string()),
                }
            }
            (out, errors)
        })
        .await
        .unwrap_or_default()
    }));
}

pub fn poll_scan(st: &mut State) {
    if let Some((list, errors)) = redhive_client_framework::runtime::poll(&mut st.ui.plugin_scan) {
        let n = list.len();
        st.settings.plugins = list;
        crate::state::apply_trust(&st.settings);
        st.save_settings();
        for e in &errors {
            st.ctx.log.warn("plugins", e.clone());
        }
        st.toast(format!("Found {n} plugin(s){}", if errors.is_empty() { String::new() } else { format!(", {} file(s) skipped", errors.len()) }), if errors.is_empty() { Severity::Success } else { Severity::Warning });
        st.ui.status.clear();
    }
}

pub fn show(ui: &mut Ui, st: &mut State) {
    ui.heading("Plugins");
    ui.label(
        RichText::new("CLAP and VST3 instruments and effects. Each file is scanned in its own process, so a plugin that crashes while loading is skipped instead of taking Dragon DAW down.")
            .small()
            .color(theme::TEXT_DIM),
    );
    ui.horizontal(|ui| {
        let scanning = st.ui.plugin_scan.is_some();
        if ui.add_enabled(!scanning, egui::Button::new(if scanning { "Scanning…" } else { "Scan now" })).clicked() {
            start_scan(st);
        }
        if scanning {
            ui.spinner();
        }
        if ui.button("+ plugin folder").clicked() {
            if let Some(d) = rfd::FileDialog::new().pick_folder() {
                st.settings.plugin_folders.push(d);
                st.save_settings();
            }
        }
    });
    egui::CollapsingHeader::new("Folders").show(ui, |ui| {
        for p in plugins::default_paths() {
            ui.label(RichText::new(format!("{} (standard)", p.display())).small());
        }
        let mut remove = None;
        for (i, p) in st.settings.plugin_folders.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(p.display().to_string()).small());
                if ui.small_button("🗙").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            st.settings.plugin_folders.remove(i);
            st.save_settings();
        }
    });
    ui.separator();
    let fid = egui::Id::new("plugins.filter");
    let mut filter: String = ui.data_mut(|d| d.get_temp(fid).unwrap_or_default());
    ui.add(egui::TextEdit::singleline(&mut filter).hint_text("🔍 filter by name, vendor or category"));
    ui.data_mut(|d| d.insert_temp(fid, filter.clone()));
    let f = filter.to_lowercase();
    let list: Vec<PluginInfo> = st.settings.plugins.iter().filter(|p| f.is_empty() || format!("{} {} {}", p.name, p.vendor, p.category).to_lowercase().contains(&f)).cloned().collect();
    if st.settings.plugins.is_empty() {
        ui.label("No plugins scanned yet.");
    }
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        egui::Grid::new("plugins.grid").striped(true).num_columns(5).show(ui, |ui| {
            ui.strong("Name");
            ui.strong("Vendor");
            ui.strong("Type");
            ui.strong("Format");
            ui.strong("");
            ui.end_row();
            for p in list {
                ui.label(&p.name).on_hover_text(p.path.display().to_string());
                ui.label(&p.vendor);
                ui.label(if p.instrument { "Instrument" } else { "Effect" });
                ui.label(match p.format {
                    PluginFormat::Clap => "CLAP",
                    PluginFormat::Vst3 => "VST3",
                });
                if p.instrument {
                    if ui.button("Add to rack").clicked() {
                        let r = p.to_ref();
                        let name = p.name.clone();
                        let id = st.session.edit("Add plugin instrument", |x| x.add_channel(&name, Instrument::Plugin(r)));
                        st.ui.channel = Some(id);
                        st.ui.instrument_windows.push(id);
                    }
                } else if ui.button("Insert on selected mixer channel").clicked() {
                    let r = p.to_ref();
                    let m = st.ui.mixer;
                    let iid = st.session.edit("Add plugin", |x| {
                        let iid = x.next_id();
                        if let Some(c) = x.mixer.get_mut(m) {
                            c.inserts.push(Insert { id: iid, effect: "plugin".into(), enabled: true, mix: 1.0, params: Default::default(), sidechain: None, plugin: Some(r), preset: String::new(), mods: Vec::new() });
                        }
                        iid
                    });
                    st.ui.effect_windows.push((m, iid));
                }
                ui.end_row();
            }
        });
    });
}
