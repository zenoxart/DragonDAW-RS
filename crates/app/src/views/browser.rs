//! Browser: user folders, the projects folder and recordings; click to preview audio, drag files to the
//! playlist, channel rack, drum pads or sampler; double-click to open projects / import MIDI.

use crate::state::State;
use crate::theme;
use dragon_engine::device::Command as Cmd;
use dragon_engine::render::LiveEvent;
use egui::{RichText, Ui};
use std::path::{Path, PathBuf};

const AUDIO: &[&str] = &["wav", "flac", "mp3", "ogg", "aif", "aiff", "m4a"];

fn kind(p: &Path) -> &'static str {
    let ext = p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
    if AUDIO.contains(&ext.as_str()) {
        "🔊"
    } else if ext == "mid" || ext == "midi" {
        "🎹"
    } else if ext == "dragon" {
        "🐉"
    } else {
        ""
    }
}

/// One navigable row of the browser (in display order): a file, or a folder with its open state's id.
#[derive(Clone)]
struct Row {
    path: PathBuf,
    folder: Option<egui::Id>,
    rect: egui::Rect,
}

/// The files (audio, MIDI, projects) under each root, listed in the background (a big sample library
/// would stall the UI) and refreshed every 30 s; searched by name.
mod index {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::Instant;

    struct Entry {
        at: Instant,
        files: Arc<Vec<PathBuf>>,
        building: bool,
    }

    fn table() -> &'static Mutex<HashMap<PathBuf, Entry>> {
        static T: OnceLock<Mutex<HashMap<PathBuf, Entry>>> = OnceLock::new();
        T.get_or_init(Default::default)
    }

    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            if out.len() >= 200_000 {
                return;
            }
            let p = e.path();
            if p.file_name().is_some_and(|f| f.to_string_lossy().starts_with('.')) {
                continue;
            }
            if p.is_dir() {
                if depth < 12 {
                    walk(&p, depth + 1, out);
                }
            } else if !super::kind(&p).is_empty() {
                out.push(p);
            }
        }
    }

    /// The files under `root` as far as listed (empty while the first listing runs).
    pub fn files(root: &Path, ctx: &egui::Context) -> (Arc<Vec<PathBuf>>, bool) {
        let mut t = table().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let e = t.entry(root.to_path_buf()).or_insert(Entry { at: Instant::now() - std::time::Duration::from_secs(3600), files: Arc::new(Vec::new()), building: false });
        if !e.building && e.at.elapsed().as_secs() > 30 {
            e.building = true;
            let (root, ctx) = (root.to_path_buf(), ctx.clone());
            std::thread::spawn(move || {
                let mut out = Vec::new();
                walk(&root, 0, &mut out);
                out.sort_by_key(|p| p.file_name().map(|f| f.to_string_lossy().to_lowercase()));
                let mut t = table().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                t.insert(root, Entry { at: Instant::now(), files: Arc::new(out), building: false });
                ctx.request_repaint();
            });
        }
        (e.files.clone(), e.building)
    }
}

pub fn show(ui: &mut Ui, st: &mut State) {
    if ui.rect_contains_pointer(ui.max_rect()) {
        st.ui.editor = crate::state::Editor::Browser;
    }
    let filter_id = egui::Id::new("browser.filter");
    let mut filter: String = ui.data_mut(|d| d.get_temp(filter_id).unwrap_or_default());
    ui.horizontal(|ui| {
        if let Some(img) = crate::brand::icon_image(ui.ctx(), crate::brand::icon::SEARCH, 16.0) {
            ui.add(img);
        }
        ui.add(egui::TextEdit::singleline(&mut filter).hint_text("search files").desired_width(ui.available_width() - 60.0));
        if ui.button("+ folder").clicked() {
            if let Some(d) = rfd::FileDialog::new().pick_folder() {
                st.settings.browser_folders.push(d);
                st.save_settings();
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));
    ui.separator();
    // Double-click plays a sound twice: the second time when the first has finished.
    if let Some((p, t0, secs)) = st.ui.preview_again.clone() {
        if t0.elapsed().as_secs_f64() >= secs + 0.12 {
            st.ui.preview_again = None;
            st.preview(&p);
        } else {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
        }
    }
    // Waveform preview of the selected file, docked at the bottom of the browser.
    egui::TopBottomPanel::bottom("browser.preview").resizable(true).min_height(100.0).default_height(110.0).height_range(100.0..=260.0).frame(egui::Frame::NONE).show_inside(ui, |ui| preview_panel(ui, st));
    let mut roots: Vec<(String, PathBuf)> = st.settings.browser_folders.iter().map(|p| (p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or(p.display().to_string()), p.clone())).collect();
    roots.push(("Projects".into(), st.ctx.paths.projects.clone()));
    roots.push(("Recordings".into(), st.recordings_dir()));
    // Keyboard (while the browser has the focus and no text field does): Up / Down move the selection
    // (sounds preview), Left / Right close / open folders, Enter puts the selected sound into the rack.
    let keys = if st.ui.editor == crate::state::Editor::Browser && !ui.ctx().wants_keyboard_input() {
        ui.input(|i| (i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::ArrowDown), i.key_pressed(egui::Key::ArrowLeft), i.key_pressed(egui::Key::ArrowRight), i.key_pressed(egui::Key::Enter)))
    } else {
        (false, false, false, false, false)
    };
    let mut rows: Vec<Row> = Vec::new();
    let scroll_to: Option<PathBuf> = ui.data_mut(|d| d.remove_temp(egui::Id::new("browser.scroll_to")));
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let terms: Vec<String> = filter.to_lowercase().split_whitespace().map(str::to_string).collect();
        if !terms.is_empty() {
            // Search: every matching file under the folders, flat, with the folder it sits in.
            let mut shown = 0;
            let mut listing = false;
            for (rname, root) in &roots {
                let (files, building) = index::files(root, ui.ctx());
                listing |= building;
                for p in files.iter() {
                    let name = p.file_name().map(|f| f.to_string_lossy().to_lowercase()).unwrap_or_default();
                    if !terms.iter().all(|t| name.contains(t.as_str())) {
                        continue;
                    }
                    if shown >= 500 {
                        break;
                    }
                    shown += 1;
                    let rel = p.parent().and_then(|d| d.strip_prefix(root).ok()).map(|d| d.display().to_string()).unwrap_or_default();
                    let sub = if rel.is_empty() { rname.clone() } else { format!("{rname} / {rel}") };
                    let r = file_row(ui, st, p, Some(&sub));
                    if scroll_to.as_deref() == Some(p.as_path()) {
                        ui.scroll_to_rect(r, None);
                    }
                    rows.push(Row { path: p.clone(), folder: None, rect: r });
                }
            }
            if shown == 0 {
                ui.label(RichText::new(if listing { "Listing your folders…" } else { "No file matches" }).small().color(theme::TEXT_DIM));
            } else if shown >= 500 {
                ui.label(RichText::new("First 500 matches – type more to narrow it down").small().color(theme::TEXT_DIM));
            }
            return;
        }
        current_project(ui, st);
        if !st.settings.recent.is_empty() {
            egui::CollapsingHeader::new(RichText::new("Recent projects").strong()).default_open(true).show(ui, |ui| {
                for p in st.settings.recent.clone() {
                    let name = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    if ui.selectable_label(false, format!("🐉 {name}")).on_hover_text(p.display().to_string()).double_clicked() {
                        st.open_path(&p);
                    }
                }
            });
        }
        let mut remove = None;
        for (i, (name, path)) in roots.iter().enumerate() {
            let cid = ui.make_persistent_id(path);
            let r = egui::CollapsingHeader::new(RichText::new(name.to_string()).strong()).icon(folder_icon).id_salt(path).default_open(i == 0).show(ui, |ui| folder(ui, st, path, 0, &mut rows, scroll_to.as_deref()));
            rows_insert_folder(&mut rows, path, cid, r.header_response.rect);
            mark_selected(ui, st, path, r.header_response.rect);
            if r.header_response.clicked() {
                st.ui.browser_sel = Some(path.clone());
            }
            if i < st.settings.browser_folders.len() {
                r.header_response.context_menu(|ui| {
                    if ui.button("Remove from browser").clicked() {
                        remove = Some(i);
                        ui.close_menu();
                    }
                });
            }
        }
        if let Some(i) = remove {
            st.settings.browser_folders.remove(i);
            st.save_settings();
        }
        if st.settings.browser_folders.is_empty() {
            ui.label(RichText::new("Add your sample folders with “+ folder”. Drag files onto the playlist, rack or pads.").small().color(theme::TEXT_DIM));
        }
    });
    // Rows in screen order (folder headers are pushed after their contents).
    rows.sort_by(|a, b| a.rect.top().total_cmp(&b.rect.top()));
    navigate(ui, st, &rows, keys);
}

/// The open project: its undo history as a folder (Current project / History). Every step is listed, the
/// current one marked; double-click a step to bring the whole project back to the state after it (or to
/// the start) – steps after it stay as redo steps, so you can jump forward again.
fn current_project(ui: &mut Ui, st: &mut State) {
    let name = st.session.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Untitled".into());
    egui::CollapsingHeader::new(RichText::new(format!("Current project – {name}")).strong()).id_salt("browser.current").icon(folder_icon).default_open(true).show(ui, |ui| {
        let steps = st.session.history.timeline();
        let pos = st.session.history.position();
        egui::CollapsingHeader::new(format!("History ({} step{})", steps.len(), if steps.len() == 1 { "" } else { "s" })).id_salt("browser.history").icon(folder_icon).default_open(true).show(ui, |ui| {
            if steps.is_empty() {
                ui.label(RichText::new("No changes yet. Every edit appears here; double-click one to go back (or forward) to it.").small().color(theme::TEXT_DIM));
                return;
            }
            let mut go = None;
            egui::ScrollArea::vertical().id_salt("browser.history.scroll").max_height(260.0).stick_to_bottom(true).show(ui, |ui| {
                // Position 0: before the oldest step kept.
                let row = |ui: &mut Ui, k: usize, text: String, done: bool| {
                    let cur = k == pos;
                    let col = if cur {
                        egui::Color32::WHITE
                    } else if done {
                        theme::TEXT
                    } else {
                        theme::TEXT_DIM
                    };
                    let label = RichText::new(format!("{} {k:>3}  {text}", if cur { "▶" } else { " " })).monospace().size(11.5).color(col);
                    let label = if done || cur { label } else { label.italics() };
                    ui.selectable_label(cur, label).on_hover_text(if cur {
                        "The project is here now".to_string()
                    } else if k < pos {
                        format!("Double-click: go back to the state after this step (undoes {} step(s))", pos - k)
                    } else {
                        format!("Double-click: go forward to the state after this step (redoes {} step(s))", k - pos)
                    })
                };
                if row(ui, 0, "Start of the history".into(), true).double_clicked() {
                    go = Some(0);
                }
                for (i, (label, done)) in steps.iter().enumerate() {
                    if row(ui, i + 1, label.clone(), *done).double_clicked() {
                        go = Some(i + 1);
                    }
                }
            });
            if let Some(k) = go {
                if k != pos {
                    let n = st.session.goto(k);
                    st.ui.status = format!("History: {} – {n} step(s) {}", if k == 0 { "start".to_string() } else { steps[k - 1].0.clone() }, if k < pos { "undone" } else { "redone" });
                }
            }
            ui.label(RichText::new("Kept with the project (History folder) – up to the undo limit in Options.").small().color(theme::TEXT_DIM));
        });
    });
}

/// Puts a folder's header row in the list (its contents were drawn first).
fn rows_insert_folder(rows: &mut Vec<Row>, path: &Path, id: egui::Id, rect: egui::Rect) {
    rows.push(Row { path: path.to_path_buf(), folder: Some(id), rect });
}

fn mark_selected(ui: &Ui, st: &State, path: &Path, rect: egui::Rect) {
    if st.ui.browser_sel.as_deref() == Some(path) {
        ui.painter().rect_stroke(rect.expand(1.0), 3.0, egui::Stroke::new(1.0_f32, theme::ACCENT_2), egui::StrokeKind::Inside);
    }
}

/// Applies the arrow keys and Enter to the rows.
fn navigate(ui: &Ui, st: &mut State, rows: &[Row], (up, down, left, right, enter): (bool, bool, bool, bool, bool)) {
    if rows.is_empty() || !(up || down || left || right || enter) {
        return;
    }
    let cur = st.ui.browser_sel.as_ref().and_then(|s| rows.iter().position(|r| &r.path == s));
    let go = |st: &mut State, k: usize| {
        let r = &rows[k];
        if r.folder.is_none() && kind(&r.path) == "🔊" {
            st.preview(&r.path);
        } else {
            st.ui.browser_sel = Some(r.path.clone());
        }
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("browser.scroll_to"), r.path.clone()));
    };
    if up || down {
        let k = match cur {
            None => 0,
            Some(c) if up => c.saturating_sub(1),
            Some(c) => (c + 1).min(rows.len() - 1),
        };
        go(st, k);
        return;
    }
    let Some(c) = cur else { return };
    let row = &rows[c];
    match row.folder {
        Some(id) if left || right => {
            let mut cs = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
            cs.set_open(right);
            cs.store(ui.ctx());
        }
        None if left => {
            // Left on a file: up to its folder.
            if let Some(k) = rows[..c].iter().rposition(|r| r.folder.is_some() && row.path.starts_with(&r.path)) {
                go(st, k);
            }
        }
        None if enter => match kind(&row.path) {
            "🔊" => {
                let p = row.path.clone();
                st.add_sampler(&p);
                st.toast(format!("{} added to the channel rack", p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()), redhive_contracts::ui::Severity::Success);
            }
            "🐉" => {
                let p = row.path.clone();
                st.open_path(&p);
            }
            "🎹" => {
                let p = row.path.clone();
                st.import_midi(&p);
            }
            _ => {}
        },
        _ => {}
    }
}

fn folder(ui: &mut Ui, st: &mut State, dir: &Path, depth: usize, rows: &mut Vec<Row>, scroll_to: Option<&Path>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        ui.label(RichText::new("(empty)").small().color(theme::TEXT_DIM));
        return;
    };
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| !p.file_name().map(|f| f.to_string_lossy().starts_with('.')).unwrap_or(true)).collect();
    entries.sort_by_key(|p| (!p.is_dir(), p.file_name().map(|f| f.to_string_lossy().to_lowercase())));
    for p in entries.into_iter().take(800) {
        let name = p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
        if p.is_dir() {
            if depth < 6 {
                let cid = ui.make_persistent_id(&p);
                let r = egui::CollapsingHeader::new(name.to_string()).icon(folder_icon).id_salt(&p).show(ui, |ui| folder(ui, st, &p, depth + 1, rows, scroll_to));
                rows_insert_folder(rows, &p, cid, r.header_response.rect);
                mark_selected(ui, st, &p, r.header_response.rect);
                if r.header_response.clicked() {
                    st.ui.browser_sel = Some(p.clone());
                }
                if scroll_to == Some(p.as_path()) {
                    ui.scroll_to_rect(r.header_response.rect, None);
                }
            }
            continue;
        }
        if kind(&p).is_empty() {
            continue;
        }
        let r = file_row(ui, st, &p, None);
        if scroll_to == Some(p.as_path()) {
            ui.scroll_to_rect(r, None);
        }
        rows.push(Row { path: p, folder: None, rect: r });
    }
}

/// A file row: icon and name (and, in search results, the folder it sits in under it). Click previews
/// a sound, double-click plays it twice (and adds it nowhere) or opens a project / imports a MIDI file;
/// drag it anywhere; right-click for more.
fn file_row(ui: &mut Ui, st: &mut State, p: &Path, sub: Option<&str>) -> egui::Rect {
    let k = kind(p);
    let name = p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
    let id = egui::Id::new(("browse", p));
    let r = ui.dnd_drag_source(id, p.to_path_buf(), |ui| {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let tile = match k {
                    "🐉" => crate::brand::icon::PROJECT_FILE,
                    "🎹" => crate::brand::icon::MIDI_FILE,
                    _ => crate::brand::icon::AUDIO_FILE,
                };
                match crate::brand::icon_image(ui.ctx(), tile, 15.0) {
                    Some(img) => {
                        ui.add(img);
                        ui.label(&name);
                    }
                    None => {
                        ui.label(format!("{k} {name}"));
                    }
                }
            });
            if let Some(sub) = sub {
                ui.label(RichText::new(sub).small().color(theme::TEXT_DIM));
            }
        });
    });
    let resp = r.response.interact(egui::Sense::click());
    mark_selected(ui, st, p, resp.rect);
    if resp.double_clicked() {
        match k {
            "🐉" => st.open_path(p),
            "🎹" => st.import_midi(p),
            _ => {
                // Twice: now (the second click restarted it) and once more when it ends.
                st.preview(p);
                let secs = st.waveform(p).map(|w| (w.len() * crate::state::WAVE_BUCKET) as f64 / st.audio.rate() as f64).unwrap_or(0.0);
                st.ui.preview_again = Some((p.to_path_buf(), std::time::Instant::now(), secs));
            }
        }
    } else if resp.clicked() {
        if k == "🔊" {
            st.preview(p);
        } else {
            st.ui.browser_sel = Some(p.to_path_buf());
        }
    }
    let hover = match k {
        "🔊" => "Click: listen · double-click: listen twice · Enter: into the channel rack · drag onto the playlist, rack or pads",
        "🐉" => "Double-click or Enter: open the project",
        _ => "Double-click or Enter: import the MIDI file",
    };
    let resp = resp.on_hover_text(hover);
    let path = p.to_path_buf();
    resp.context_menu(|ui| {
        if k == "🔊" {
            if ui.button("Add to playlist at playhead").clicked() {
                let beat = st.beat_now();
                st.add_audio_clip(&path, None, beat);
                ui.close_menu();
            }
            if ui.button("New sampler channel (Enter)").clicked() {
                st.add_sampler(&path);
                ui.close_menu();
            }
            if ui.button("Detect tempo & key").clicked() {
                if let Ok(d) = st.session.cache.get(&path, st.audio.rate()) {
                    let a = dragon_engine::analysis::analyze(&d);
                    st.toast(format!("{name}: {} BPM · {}", a.bpm.map(|b| format!("{b:.1}")).unwrap_or("?".into()), a.key.unwrap_or("?".into())), redhive_contracts::ui::Severity::Info);
                }
                ui.close_menu();
            }
        }
    });
    resp.rect
}

/// The waveform of the file selected in the browser: name, length and rate, a playhead while the preview
/// plays, click to play it again, drag it into the playlist / rack like the file itself.
fn preview_panel(ui: &mut Ui, st: &mut State) {
    ui.add_space(4.0);
    let Some(path) = st.ui.browser_sel.clone() else {
        ui.label(RichText::new("Click an audio file to preview it here").small().color(theme::TEXT_DIM));
        return;
    };
    let name = path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
    let rate = st.audio.rate() as f64;
    let w = st.waveform(&path);
    let secs = w.as_ref().map(|w| (w.len() * crate::state::WAVE_BUCKET) as f64 / rate).unwrap_or(0.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(&name).small().strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("⏹").on_hover_text("Stop the preview").clicked() {
                st.audio.send(Cmd::Live(LiveEvent::StopPreview));
                st.ui.preview_started = None;
            }
            ui.label(RichText::new(format!("{secs:.2} s")).small().monospace().color(theme::TEXT_DIM));
        });
    });
    let size = egui::vec2(ui.available_width(), ui.available_height().max(40.0) - 4.0);
    let id = egui::Id::new(("browser.preview.drag", &path));
    let r = ui.dnd_drag_source(id, path.clone(), |ui| {
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 4.0, theme::BG);
        match &w {
            Some(w) if !w.is_empty() => {
                let n = w.len();
                let mid = rect.center().y;
                let half = rect.height() * 0.46;
                let mut x = rect.left();
                while x < rect.right() {
                    let b0 = (((x - rect.left()) / rect.width()) * n as f32) as usize;
                    let b1 = ((((x + 1.0 - rect.left()) / rect.width()) * n as f32) as usize).max(b0 + 1).min(n);
                    let (lo, hi) = w[b0.min(n - 1)..b1].iter().fold((0.0f32, 0.0f32), |(l, h), (a, b)| (l.min(*a), h.max(*b)));
                    p.line_segment([egui::pos2(x, mid - super::playlist::wave_disp(hi) * half), egui::pos2(x, mid - super::playlist::wave_disp(lo) * half)], egui::Stroke::new(1.0_f32, theme::ACCENT_2));
                    x += 1.0;
                }
                p.line_segment([egui::pos2(rect.left(), mid), egui::pos2(rect.right(), mid)], egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(20)));
                // Playhead while the preview plays.
                if let Some(t0) = st.ui.preview_started {
                    let t = t0.elapsed().as_secs_f64();
                    if t < secs && !st.transport().playing {
                        let x = rect.left() + (t / secs.max(1e-6)) as f32 * rect.width();
                        p.line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())], egui::Stroke::new(1.5_f32, theme::PLAYHEAD));
                        ui.ctx().request_repaint();
                    } else {
                        st.ui.preview_started = None;
                    }
                }
            }
            _ => {
                p.text(rect.center(), egui::Align2::CENTER_CENTER, "cannot read this file", egui::FontId::proportional(11.0), theme::REC);
            }
        }
    });
    let resp = r.response.interact(egui::Sense::click()).on_hover_text("Click to play · drag into the playlist or onto a channel");
    if resp.clicked() {
        st.preview(&path);
    }
    ui.data_mut(|d| d.insert_temp(egui::Id::new("browser.preview.rect"), resp.rect));
}

/// The folder rows' icon: the Higgsfield folder, open or closed with the row (a plain arrow as fallback).
fn folder_icon(ui: &mut Ui, openness: f32, resp: &egui::Response) {
    let r = egui::Rect::from_center_size(resp.rect.center(), egui::Vec2::splat(16.0));
    let k = if openness > 0.5 { crate::brand::icon::FOLDER_OPEN } else { crate::brand::icon::FOLDER };
    if !crate::brand::paint_icon(ui.painter(), r, k, egui::Color32::WHITE) {
        let c = r.center();
        let pts =
            if openness > 0.5 { vec![c + egui::vec2(-4.0, -2.0), c + egui::vec2(4.0, -2.0), c + egui::vec2(0.0, 3.0)] } else { vec![c + egui::vec2(-2.0, -4.0), c + egui::vec2(3.0, 0.0), c + egui::vec2(-2.0, 4.0)] };
        ui.painter().add(egui::Shape::convex_polygon(pts, crate::theme::TEXT_DIM, egui::Stroke::NONE));
    }
}
