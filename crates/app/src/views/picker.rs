//! The picker panel on the playlist's left (like FL Studio's picker): every pattern (split ones
//! included), the audio files the project uses and its automation clips. Drag an entry onto a playlist
//! track to place it; click a pattern to make it current, double-click to open it in the piano roll.

use crate::state::{target_name, State};
use crate::theme;
use dragon_engine::model::*;
use egui::{Color32, CornerRadius, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use std::path::PathBuf;

/// What is being dragged from the picker.
#[derive(Debug, Clone, PartialEq)]
pub enum PickItem {
    Pattern(Id),
    Audio(PathBuf),
    Automation(Id),
}

pub const WIDTH: f32 = 176.0;

/// Audio files the project refers to: clip sources and sampler sounds, without duplicates.
pub fn audio_files(p: &Project) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut push = |x: &PathBuf| {
        if !x.as_os_str().is_empty() && !out.contains(x) {
            out.push(x.clone());
        }
    };
    for t in &p.tracks {
        for c in &t.clips {
            if let ClipKind::Audio(a) = &c.kind {
                push(&a.source);
            }
        }
    }
    for ch in &p.channels {
        if let Instrument::Sampler(s) = &ch.instrument {
            push(&s.path);
        }
    }
    out
}

/// Automation lanes that can live in the playlist as clips (tempo stays a global curve).
pub fn automation_lanes(p: &Project) -> Vec<(Id, String)> {
    p.automation.iter().filter(|l| l.target != Target::Tempo).map(|l| (l.id, target_name(p, &l.target))).collect()
}

/// How many clips in the playlist use the item (shown as a small counter).
fn uses(p: &Project, item: &PickItem) -> usize {
    p.tracks
        .iter()
        .flat_map(|t| t.clips.iter())
        .filter(|c| match (&c.kind, item) {
            (ClipKind::Pattern { pattern }, PickItem::Pattern(id)) => pattern == id,
            (ClipKind::Audio(a), PickItem::Audio(path)) => &a.source == path,
            (ClipKind::Automation { lane }, PickItem::Automation(id)) => lane == id,
            _ => false,
        })
        .count()
}

pub fn show(ui: &mut Ui, st: &mut State) {
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, theme::PANEL);
    ui.data_mut(|d| d.insert_temp(egui::Id::new("pl.picker"), rect));
    let mut rows: Vec<(String, Rect)> = Vec::new();
    egui::ScrollArea::vertical().id_salt("pl.picker.scroll").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        ui.add_space(4.0);
        ui.label(RichText::new("Drag onto a track").small().color(theme::TEXT_DIM));
        // ----- patterns
        section(ui, "Patterns", st.session.project.patterns.len());
        let pats: Vec<(Id, String, Rgb, usize)> =
            st.session.project.patterns.iter().map(|x| (x.id, x.name.clone(), x.color, x.channels.values().map(|c| c.notes.len() + c.steps.iter().filter(|s| s.on).count()).sum())).collect();
        for (id, name, color, notes) in pats {
            let item = PickItem::Pattern(id);
            let n = uses(&st.session.project, &item);
            let (resp, r) = entry(ui, &item, &name, theme::rgb(color), "♫", n, st.ui.pattern == id, if notes == 0 { "empty" } else { "" });
            rows.push((format!("pattern:{id}"), r));
            if resp.clicked() {
                st.ui.pattern = id;
            }
            if resp.double_clicked() {
                st.ui.pattern = id;
                st.ctx.open("dragon.piano", None);
            }
            resp.on_hover_text(format!("{name}\nClick: make current · double-click: piano roll · drag: place in the playlist"));
        }
        if ui.add(egui::Button::new(RichText::new("+ New pattern").small()).fill(theme::PANEL_2).min_size(Vec2::new(ui.available_width(), 20.0))).clicked() {
            st.ctx.run("dragon.add.pattern", serde_json::Value::Null);
        }
        // ----- audio
        let files = audio_files(&st.session.project);
        section(ui, "Audio clips", files.len());
        if files.is_empty() {
            ui.label(RichText::new("Drop samples from the browser or record a take").small().color(theme::TEXT_DIM));
        }
        for path in files {
            let name = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let item = PickItem::Audio(path.clone());
            let n = uses(&st.session.project, &item);
            let missing = if path.exists() { "" } else { "missing" };
            let (resp, r) = entry(ui, &item, &name, theme::ACCENT_2, "🔊", n, false, missing);
            rows.push((format!("audio:{name}"), r));
            resp.on_hover_text(path.display().to_string());
        }
        // ----- automation
        let lanes = automation_lanes(&st.session.project);
        section(ui, "Automation clips", lanes.len());
        if lanes.is_empty() {
            ui.label(RichText::new("Right-click a knob or fader and choose \"Create automation lane\"").small().color(theme::TEXT_DIM));
        }
        for (id, name) in lanes {
            let item = PickItem::Automation(id);
            let n = uses(&st.session.project, &item);
            let (resp, r) = entry(ui, &item, &name, theme::WARN, "↗", n, false, "");
            rows.push((format!("automation:{id}"), r));
            if resp.double_clicked() {
                st.ctx.open("dragon.automation", None);
            }
            resp.on_hover_text(format!("{name}\nDrag into the playlist: the curve plays where the clip sits"));
        }
        ui.add_space(8.0);
    });
    ui.data_mut(|d| d.insert_temp(egui::Id::new("pl.picker.rows"), rows));
}

fn section(ui: &mut Ui, title: &str, n: usize) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(title.to_uppercase()).small().strong().color(theme::ACCENT));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(10.0);
            ui.label(RichText::new(n.to_string()).small().color(theme::TEXT_DIM));
        });
    });
    let r = ui.available_rect_before_wrap();
    ui.painter().line_segment([Pos2::new(r.left(), r.top()), Pos2::new(r.right(), r.top())], Stroke::new(1.0_f32, theme::GRID_BAR));
    ui.add_space(2.0);
}

/// One draggable row: colour chip, icon, name, use counter.
#[allow(clippy::too_many_arguments)]
fn entry(ui: &mut Ui, item: &PickItem, name: &str, color: Color32, icon: &str, uses: usize, current: bool, note: &str) -> (egui::Response, Rect) {
    let id = egui::Id::new(("pl.picker.item", format!("{item:?}")));
    let w = ui.available_width() - 8.0;
    let resp = ui
        .dnd_drag_source(id, item.clone(), |ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::new(w, 22.0), Sense::hover());
            paint_entry(ui, r, name, color, icon, uses, current, note);
        })
        .response;
    let r = resp.rect;
    // `dnd_drag_source` senses drags only; clicks come from an overlay on the same rect.
    let click = ui.interact(r, id.with("click"), Sense::click());
    if click.hovered() {
        ui.painter().rect_stroke(r, 4.0, Stroke::new(1.0_f32, color), egui::StrokeKind::Inside);
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    (click, r)
}

#[allow(clippy::too_many_arguments)]
fn paint_entry(ui: &Ui, r: Rect, name: &str, color: Color32, icon: &str, uses: usize, current: bool, note: &str) {
    let p = ui.painter();
    p.rect_filled(r, 4.0, if current { color.gamma_multiply(0.35) } else { theme::PANEL_2 });
    p.rect_filled(Rect::from_min_size(r.min, Vec2::new(5.0, r.height())), CornerRadius { nw: 4, sw: 4, ne: 0, se: 0 }, color);
    p.text(Pos2::new(r.left() + 15.0, r.center().y), egui::Align2::CENTER_CENTER, icon, egui::FontId::proportional(12.0), color);
    let right = if uses > 0 {
        let pill = Rect::from_center_size(Pos2::new(r.right() - 14.0, r.center().y), Vec2::new(20.0, 14.0));
        p.rect_filled(pill, 7.0, color.gamma_multiply(0.5));
        p.text(pill.center(), egui::Align2::CENTER_CENTER, format!("{uses}"), egui::FontId::proportional(9.5), Color32::WHITE);
        pill.left() - 4.0
    } else {
        r.right() - 4.0
    };
    let text_rect = Rect::from_min_max(Pos2::new(r.left() + 25.0, r.top()), Pos2::new(right, r.bottom()));
    let label = if note.is_empty() { name.to_string() } else { format!("{name}  ({note})") };
    p.with_clip_rect(text_rect).text(Pos2::new(text_rect.left(), r.center().y), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(11.5), if note == "missing" { theme::REC } else { theme::TEXT });
}

/// Places a picked item on `track` (or a fitting / new track) at `beat`. Returns the new clip.
pub fn place(st: &mut State, item: &PickItem, track: Option<Id>, beat: f64) -> Option<Id> {
    match item {
        PickItem::Audio(path) => {
            st.add_audio_clip(path, track, beat);
            st.ui.clips.first().copied()
        }
        PickItem::Pattern(id) => {
            let id = *id;
            let len = st.session.project.pattern(id)?.length_beats();
            let cid = st.session.edit("Place pattern", |p| {
                let name = p.pattern(id).map(|x| x.name.clone()).unwrap_or_default();
                let tid = track.or_else(|| p.tracks.iter().find(|t| t.clips.is_empty()).map(|t| t.id)).unwrap_or_else(|| p.add_track(&name));
                super::playlist::place_pattern(p, tid, id, beat, len)
            });
            st.ui.pattern = id;
            st.ui.clips = vec![cid];
            Some(cid)
        }
        PickItem::Automation(lane) => {
            let lane = *lane;
            let l = st.session.project.automation.iter().find(|l| l.id == lane)?;
            let len = l.length_beats();
            let name = target_name(&st.session.project, &l.target);
            let cid = st.session.edit("Place automation clip", |p| {
                let tid = track.or_else(|| p.tracks.iter().find(|t| t.clips.is_empty()).map(|t| t.id)).unwrap_or_else(|| p.add_track(&name));
                let id = p.next_id();
                if let Some(t) = p.track_mut(tid) {
                    t.clips.push(Clip { id, name, start: beat, length: len, muted: false, color: Rgb(0xe8, 0xb3, 0x4a), kind: ClipKind::Automation { lane } });
                }
                id
            });
            st.ui.clips = vec![cid];
            Some(cid)
        }
    }
}

/// Length in beats the item will take in the playlist (for the drop preview).
pub fn length(st: &mut State, item: &PickItem) -> f64 {
    match item {
        PickItem::Pattern(id) => st.session.project.pattern(*id).map(|p| p.length_beats()).unwrap_or(4.0),
        PickItem::Automation(id) => st.session.project.automation.iter().find(|l| l.id == *id).map(|l| l.length_beats()).unwrap_or(4.0),
        PickItem::Audio(path) => match st.session.cache.get(path, st.audio.rate()) {
            Ok(d) => {
                let tm = dragon_engine::time::TempoMap::from_project(&st.session.project);
                tm.seconds_to_beat(d.duration())
            }
            Err(_) => 4.0,
        },
    }
}
