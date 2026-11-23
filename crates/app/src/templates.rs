//! Project templates: a project to start from. Two come with Dragon DAW – a mixing set-up and a beat set-up,
//! both with four group buses – and any project can be saved as a template (File › Save as template…,
//! kept in `<Projects>/Templates`). One template can be the default: File › New then starts from it.
//!
//! The group buses (each a mixer bus with its effect, fed by a track of the same name):
//! Vocals → Purify, Melodies → Wingspan (stereo widening), Drums → Dragonforge, FX → Prism EQ.
//! Master: Philosopher's Stone; the beat template puts the Obsidian soft clipper before it.

use crate::state::State;
use crate::theme;
use dragon_engine::model::{Id, Insert, Instrument, Project};
use egui::RichText;
use redhive_contracts::ui::Severity;
use std::path::{Path, PathBuf};

/// The templates that come with Dragon DAW: (key, title, what it sets up).
pub const BUILTIN: [(&str, &str, &str); 2] = [
    ("builtin:mixing", "Mixing – four group buses", "Vocals (Purify), Melodies (Wingspan stereo widening), Drums (Dragonforge) and FX (Prism EQ) buses, each fed by its own track; Philosopher's Stone on the master."),
    (
        "builtin:beats",
        "Beat making – four group buses",
        "The starter drum kit routed into the Drums bus (Dragonforge), plus Vocals, Melodies and FX buses; Obsidian soft clipper before Philosopher's Stone on the master.",
    ),
];

/// A group bus: (name, icon, effect, parameter changes).
type Group = (&'static str, &'static str, &'static str, &'static [(&'static str, f32)]);

/// The group buses.
const GROUPS: [Group; 4] = [("Vocals", "vocal", "clean_voice", &[]), ("Melodies", "synth", "imager", &[("width", 1.3)]), ("Drums", "kick", "drum_bus", &[]), ("FX", "sfx", "eq", &[])];

/// Where saved templates live.
pub fn dir(st: &State) -> PathBuf {
    st.ctx.paths.projects.join("Templates")
}

/// Every template: (key, title, description, file for the user's own).
pub fn list(st: &State) -> Vec<(String, String, String, Option<PathBuf>)> {
    let mut out: Vec<(String, String, String, Option<PathBuf>)> = BUILTIN.iter().map(|(k, t, d)| (k.to_string(), t.to_string(), d.to_string(), None)).collect();
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir(st)).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "dragon")).collect();
    files.sort();
    for f in files {
        let name = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        out.push((f.to_string_lossy().to_string(), name, "Your template".into(), Some(f)));
    }
    out
}

fn insert(p: &mut Project, ch: Id, effect: &str, set: &[(&str, f32)]) {
    let iid = p.next_id();
    let mut params = dragon_engine::effects::find(effect).map(|d| d.defaults()).unwrap_or_default();
    for (k, v) in set {
        params.insert((*k).into(), *v);
    }
    let ins = Insert { id: iid, effect: effect.into(), enabled: true, mix: 1.0, params, sidechain: None, plugin: None, preset: String::new(), mods: Vec::new() };
    if let Some(c) = p.mixer.get_mut(ch) {
        c.inserts.push(ins);
    }
}

/// Adds the four group buses (with their effects); returns their ids by group name.
fn group_buses(p: &mut Project) -> Vec<(&'static str, Id)> {
    let mut out = Vec::new();
    for (name, icon, effect, set) in GROUPS {
        let b = crate::modes::bus(p, &format!("{name} bus"), icon);
        if let Some(c) = p.mixer.get_mut(b) {
            c.dock = dragon_engine::model::Dock::Left;
            c.volume = 1.0;
        }
        insert(p, b, effect, set);
        out.push((name, b));
    }
    out
}

/// A track named `name` whose mixer channel plays into `bus`.
fn track_into(p: &mut Project, name: &str, icon: &str, bus: Id) {
    let t = p.add_track(name);
    let m = p.track(t).map(|x| x.mixer).unwrap_or(0);
    if let Some(c) = p.mixer.get_mut(m) {
        c.icon = icon.into();
        c.output = Some(bus);
    }
}

/// The mixing template.
pub fn mixing(p: &mut Project) {
    p.meta.name = "New mix".into();
    p.mode = "mix".into();
    let buses = group_buses(p);
    for ((name, icon, ..), (_, bus)) in GROUPS.iter().zip(buses) {
        track_into(p, name, icon, bus);
    }
    let m = p.mixer.master.id;
    insert(p, m, "dragon_particle", &[]);
}

/// The beat template, on a project that already has the starter kit (empty patterns).
pub fn beats(p: &mut Project) {
    p.meta.name = "New beat".into();
    p.mode = "beat".into();
    let buses = group_buses(p);
    let bus = |n: &str| buses.iter().find(|b| b.0 == n).map(|b| b.1);
    // The kit's channels play into the Drums bus, other instruments into Melodies.
    let routes: Vec<(Id, bool)> = p.channels.iter().map(|c| (c.mixer, matches!(c.instrument, Instrument::DrumPads(_)) || is_drum_name(&c.name))).collect();
    let master = p.mixer.master.id;
    for (m, drum) in routes {
        let to = if drum { bus("Drums") } else { bus("Melodies") };
        if let (Some(c), Some(to)) = (p.mixer.get_mut(m), to) {
            if c.id != master && c.output.is_none() {
                c.output = Some(to);
            }
        }
    }
    for (name, icon) in [("Vocals", "vocal"), ("FX", "sfx")] {
        if let Some(b) = bus(name) {
            track_into(p, name, icon, b);
        }
    }
    let m = p.mixer.master.id;
    p.mixer.master.inserts.retain(|i| i.effect != "limiter");
    insert(p, m, "clipper", &[("threshold", -3.0), ("ceiling", -0.3)]);
    insert(p, m, "dragon_particle", &[]);
}

fn is_drum_name(n: &str) -> bool {
    let n = n.to_lowercase();
    ["kick", "snare", "clap", "hat", "perc", "drum", "tom", "crash", "ride", "rim", "808", "shaker"].iter().any(|k| n.contains(k))
}

/// Starts a new project from template `key` (a built-in key or a template file).
pub fn new_from(st: &mut State, key: &str) {
    match key {
        "builtin:mixing" => {
            st.new_project();
            mixing(&mut st.session.project);
        }
        "builtin:beats" => {
            st.new_demo_project();
            let p = &mut st.session.project;
            for t in &mut p.tracks {
                t.clips.clear();
            }
            for pat in &mut p.patterns {
                for c in pat.channels.values_mut() {
                    c.notes.clear();
                    for s in &mut c.steps {
                        s.on = false;
                    }
                }
            }
            beats(p);
        }
        file => match dragon_engine::project_io::load(Path::new(file)) {
            Ok(mut p) => {
                // A copy: saving asks where the new project goes, the template stays as it is.
                p.meta.name = "Untitled".into();
                st.replace_session(crate::session::Session::new(p, None));
            }
            Err(e) => {
                st.toast(format!("The template could not be opened: {e}"), Severity::Error);
                return;
            }
        },
    }
    st.session.history.clear();
    st.session.force_recompile();
    for v in ["dragon.playlist", "dragon.mixer"] {
        st.ctx.open(v, None);
    }
}

/// File › New: the default template, or an empty project when there is none (or it is gone).
pub fn new_default(st: &mut State) {
    let key = st.settings.default_template.clone();
    let known = !key.is_empty() && (key.starts_with("builtin:") || Path::new(&key).is_file());
    if known {
        new_from(st, &key);
    } else {
        st.new_project();
    }
}

/// Saves the current project as template `name` (plugin states included).
pub fn save_current(st: &mut State, name: &str) -> Result<PathBuf, String> {
    let clean: String = name.trim().chars().map(|c| if c.is_alphanumeric() || " -_()".contains(c) { c } else { '_' }).collect();
    let clean = clean.trim();
    if clean.is_empty() {
        return Err("give the template a name".into());
    }
    let d = dir(st);
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let path = d.join(format!("{clean}.dragon"));
    st.capture_plugin_states();
    let mut p = st.session.project.clone();
    p.meta.name = clean.to_string();
    dragon_engine::project_io::save(&p, &path, 0).map_err(|e| e.to_string())?;
    Ok(path)
}

/// The templates window (File › New from template…, File › Save as template…).
pub fn window(ctx: &egui::Context, st: &mut State) {
    if !st.templates_open {
        return;
    }
    let mut open = true;
    let mut action: Option<(String, u8)> = None; // (key, 0 = new, 1 = make default, 2 = delete)
    let mut save_name: Option<String> = None;
    egui::Window::new("Templates").id(egui::Id::new("templates")).open(&mut open).collapsible(false).default_width(620.0).show(ctx, |ui| {
        ui.label(RichText::new("Start a project from a template. The default one is what File › New gives you.").color(theme::TEXT_DIM));
        ui.add_space(6.0);
        let def = st.settings.default_template.clone();
        egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
            for (key, title, desc, file) in list(st) {
                egui::Frame::new().fill(theme::PANEL).corner_radius(8.0).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&title).strong().size(15.0));
                                if def == key {
                                    ui.label(RichText::new("default").small().color(theme::ACCENT_2));
                                }
                            });
                            ui.add(egui::Label::new(RichText::new(&desc).small().color(theme::TEXT_DIM)).wrap());
                        });
                    });
                    ui.horizontal(|ui| {
                        if ui.add(egui::Button::new(RichText::new("New project").strong().color(egui::Color32::WHITE)).fill(theme::ACCENT)).clicked() {
                            action = Some((key.clone(), 0));
                        }
                        let is_def = def == key;
                        if ui.selectable_label(is_def, if is_def { "Default for File › New" } else { "Make default" }).on_hover_text("File › New starts from this template").clicked() && !is_def {
                            action = Some((key.clone(), 1));
                        }
                        if file.is_some() && ui.button("Delete").clicked() {
                            action = Some((key.clone(), 2));
                        }
                    });
                });
                ui.add_space(4.0);
            }
        });
        ui.horizontal(|ui| {
            let none = st.settings.default_template.is_empty();
            if ui.selectable_label(none, "File › New: empty project").on_hover_text("No default template").clicked() && !none {
                action = Some((String::new(), 1));
            }
        });
        ui.separator();
        ui.label(RichText::new("Save the current project as a template").strong());
        ui.label(RichText::new("Everything is kept – tracks, routing, effects, instruments, plugin settings, tempo. Clips too, so clear what you don't want first.").small().color(theme::TEXT_DIM));
        ui.horizontal(|ui| {
            let id = egui::Id::new("templates.name");
            let mut name: String = ui.data(|d| d.get_temp(id)).unwrap_or_default();
            ui.add(egui::TextEdit::singleline(&mut name).hint_text("Template name").desired_width(260.0));
            if ui.add_enabled(!name.trim().is_empty(), egui::Button::new("Save as template")).clicked() {
                save_name = Some(name.clone());
                name.clear();
            }
            ui.data_mut(|d| d.insert_temp(id, name));
        });
    });
    if let Some(name) = save_name {
        match save_current(st, &name) {
            Ok(p) => st.toast(format!("Template saved: {}", p.display()), Severity::Success),
            Err(e) => st.toast(format!("Could not save the template: {e}"), Severity::Error),
        }
    }
    match action {
        Some((key, 0)) => {
            st.templates_open = false;
            st.ui.start_screen = false;
            new_from(st, &key);
            return;
        }
        Some((key, 1)) => {
            st.settings.default_template = key;
            st.save_settings();
        }
        Some((key, 2)) => {
            if st.settings.default_template == key {
                st.settings.default_template.clear();
                st.save_settings();
            }
            let _ = std::fs::remove_file(&key);
        }
        _ => {}
    }
    if !open {
        st.templates_open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain(p: &Project, id: Id) -> Vec<String> {
        p.mixer.get(id).unwrap().inserts.iter().map(|i| i.effect.clone()).collect()
    }

    #[test]
    fn mixing_template_has_four_group_buses_and_the_stone() {
        let mut p = Project::empty();
        mixing(&mut p);
        for (name, effect) in [("Vocals bus", "clean_voice"), ("Melodies bus", "imager"), ("Drums bus", "drum_bus"), ("FX bus", "eq")] {
            let bus = p.mixer.channels.iter().find(|c| c.name == name).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(chain(&p, bus.id), [effect], "{name}");
            let feeding = p.mixer.channels.iter().filter(|c| c.output == Some(bus.id)).count();
            assert_eq!(feeding, 1, "a track plays into {name}");
        }
        assert_eq!(chain(&p, p.mixer.master.id), ["dragon_particle"]);
    }

    #[test]
    fn beat_template_routes_the_kit_and_clips_before_the_stone() {
        let mut st = State::new(redhive_client_framework::AppContext::new("en"), crate::state::Settings { show_splash: false, ..Default::default() });
        new_from(&mut st, "builtin:beats");
        let p = &st.session.project;
        assert_eq!(chain(p, p.mixer.master.id), ["clipper", "dragon_particle"], "soft clipper before the Stone");
        let drums = p.mixer.channels.iter().find(|c| c.name == "Drums bus").unwrap().id;
        let kick = p.channels.iter().find(|c| c.name.to_lowercase().contains("kick")).expect("the kit has a kick");
        assert_eq!(p.mixer.get(kick.mixer).unwrap().output, Some(drums), "the kick plays into the Drums bus");
        assert!(p.patterns.iter().all(|pt| pt.channels.values().all(|c| c.notes.is_empty() && c.steps.iter().all(|s| !s.on))), "nothing written yet");
        assert_eq!(st.session.history.undo_len(), 0, "starts clean");
    }

    #[test]
    fn default_template_and_saved_templates() {
        let mut st = State::new(redhive_client_framework::AppContext::new("en"), crate::state::Settings { show_splash: false, ..Default::default() });
        let tmp = tempfile::tempdir().unwrap();
        st.ctx.paths.projects = tmp.path().to_path_buf();
        // No default: File › New is empty.
        new_default(&mut st);
        assert!(st.session.project.mixer.channels.iter().all(|c| !c.name.ends_with(" bus")));
        // Mixing as default.
        st.settings.default_template = "builtin:mixing".into();
        new_default(&mut st);
        assert!(st.session.project.mixer.channels.iter().any(|c| c.name == "Vocals bus"));
        // Save it as my own template, change the project, start from mine.
        let f = save_current(&mut st, "My mix/../x").unwrap();
        assert_eq!(f.parent().unwrap(), dir(&st), "the name can't leave the folder");
        assert!(list(&st).iter().any(|t| t.3.as_deref() == Some(f.as_path())));
        st.new_project();
        new_from(&mut st, &f.to_string_lossy());
        assert!(st.session.project.mixer.channels.iter().any(|c| c.name == "Vocals bus"));
        assert!(st.session.path.is_none(), "a new project, not the template file");
        // A default that was deleted falls back to an empty project.
        st.settings.default_template = tmp.path().join("gone.dragon").to_string_lossy().into();
        new_default(&mut st);
        assert!(st.session.project.mixer.channels.iter().all(|c| !c.name.ends_with(" bus")));
    }
}
