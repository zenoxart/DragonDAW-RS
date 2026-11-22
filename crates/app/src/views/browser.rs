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

pub fn show(ui: &mut Ui, st: &mut State) {
    let filter_id = egui::Id::new("browser.filter");
    let mut filter: String = ui.data_mut(|d| d.get_temp(filter_id).unwrap_or_default());
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut filter).hint_text("🔍 search").desired_width(ui.available_width() - 60.0));
        if ui.button("+ folder").clicked() {
            if let Some(d) = rfd::FileDialog::new().pick_folder() {
                st.settings.browser_folders.push(d);
                st.save_settings();
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));
    ui.separator();
    // FL-style waveform preview of the selected file, docked at the bottom of the browser.
    egui::TopBottomPanel::bottom("browser.preview").resizable(true).min_height(100.0).default_height(110.0).height_range(100.0..=260.0).frame(egui::Frame::NONE).show_inside(ui, |ui| preview_panel(ui, st));
    let mut roots: Vec<(String, PathBuf)> = st.settings.browser_folders.iter().map(|p| (p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or(p.display().to_string()), p.clone())).collect();
    roots.push(("Projects".into(), st.ctx.paths.projects.clone()));
    roots.push(("Recordings".into(), st.recordings_dir()));
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
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
            let r = egui::CollapsingHeader::new(RichText::new(format!("📁 {name}")).strong()).id_salt(path).default_open(i == 0).show(ui, |ui| folder(ui, st, path, &filter.to_lowercase(), 0));
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
}

fn folder(ui: &mut Ui, st: &mut State, dir: &Path, filter: &str, depth: usize) {
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
                egui::CollapsingHeader::new(format!("📁 {name}")).id_salt(&p).show(ui, |ui| folder(ui, st, &p, filter, depth + 1));
            }
            continue;
        }
        let k = kind(&p);
        if k.is_empty() || (!filter.is_empty() && !name.to_lowercase().contains(filter)) {
            continue;
        }
        let id = egui::Id::new(("browse", &p));
        let r = ui.dnd_drag_source(id, p.clone(), |ui| {
            ui.label(format!("{k} {name}"));
        });
        let resp = r.response.interact(egui::Sense::click());
        if st.ui.browser_sel.as_deref() == Some(p.as_path()) {
            ui.painter().rect_stroke(resp.rect.expand(1.0), 3.0, egui::Stroke::new(1.0_f32, theme::ACCENT_2), egui::StrokeKind::Inside);
        }
        if resp.clicked() && k == "🔊" {
            st.preview(&p);
        }
        if resp.double_clicked() {
            match k {
                "🐉" => st.open_path(&p),
                "🎹" => st.import_midi(&p),
                _ => {
                    let beat = st.beat_now();
                    st.add_audio_clip(&p, None, beat);
                }
            }
        }
        resp.context_menu(|ui| {
            if k == "🔊" {
                if ui.button("Add to playlist at playhead").clicked() {
                    let beat = st.beat_now();
                    st.add_audio_clip(&p, None, beat);
                    ui.close_menu();
                }
                if ui.button("New sampler channel").clicked() {
                    st.add_sampler(&p);
                    ui.close_menu();
                }
                if ui.button("Detect tempo & key").clicked() {
                    if let Ok(d) = st.session.cache.get(&p, st.audio.rate()) {
                        let a = dragon_engine::analysis::analyze(&d);
                        st.toast(format!("{name}: {} BPM · {}", a.bpm.map(|b| format!("{b:.1}")).unwrap_or("?".into()), a.key.unwrap_or("?".into())), redhive_contracts::ui::Severity::Info);
                    }
                    ui.close_menu();
                }
            }
        });
    }
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
