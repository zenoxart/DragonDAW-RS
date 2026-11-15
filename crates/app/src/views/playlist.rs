//! Playlist: tracks with audio and pattern clips, takes / comping lanes, markers, loop and punch ranges,
//! snap, zoom, tools (draw, paint, select, slice, erase, mute) and drag & drop from the browser.

use super::nav::{self, AreaZoom, ViewTarget};
use super::widgets::{self, snap, snap_floor};
use crate::state::{State, Tool, WAVE_BUCKET};
use crate::theme;
use dragon_engine::model::*;
use dragon_engine::time::TempoMap;
use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use std::path::PathBuf;

const HEADER: f32 = 196.0;
const RULER: f32 = 34.0;
const TAKE_H: f32 = 22.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DragKind {
    Move {
        grab: f64,
        track_offset: i32,
    },
    ResizeLeft,
    ResizeRight,
    Paint,
    Comp {
        take: Id,
        from: f64,
    },
    Loop {
        from: f64,
    },
    Punch {
        from: f64,
    },
    /// Box selection (Ctrl + drag, or a drag on empty space with the Select tool), from a beat and a y.
    Select {
        from: f64,
        y: f32,
    },
    /// FL's slice tool: drag a line across clips (any direction); every clip it crosses is cut where the
    /// line passes its track (snapped; Alt = no snap).
    Knife {
        from: f64,
        y: f32,
    },
    /// FL's zoom tool: a box from a screen position.
    Zoom {
        from: Pos2,
    },
    /// FL's slip edit: moves the audio inside the clip (the clip's audio offset, seconds, at the start).
    Slip {
        from: f64,
        offset: f64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drag {
    pub clip: Id,
    pub track: Id,
    pub kind: DragKind,
}

struct Row {
    track: Id,
    y: f32,
    h: f32,
    takes: Vec<(Id, String, f32)>,
}

pub fn show(ui: &mut Ui, st: &mut State) {
    toolbar(ui, st);
    if st.ui.pl_picker {
        egui::SidePanel::left("pl.picker.panel").resizable(true).default_width(super::picker::WIDTH).width_range(130.0..=320.0).frame(egui::Frame::NONE).show_inside(ui, |ui| super::picker::show(ui, st));
    }
    let avail = ui.available_rect_before_wrap();
    let (rect, resp) = ui.allocate_exact_size(avail.size(), Sense::click_and_drag());
    if resp.hovered() || resp.is_pointer_button_down_on() {
        st.ui.editor = crate::state::Editor::Playlist;
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, theme::BG);

    let timeline = Rect::from_min_max(Pos2::new(rect.left() + HEADER, rect.top() + RULER), rect.max);
    ui.data_mut(|d| d.insert_temp(egui::Id::new("pl.timeline"), timeline));
    // ----- animated zoom (area zoom / zoom out ease in over a few frames)
    if let Some(t) = st.ui.pl_anim {
        let cur = ViewTarget { zoom: st.ui.playlist_zoom as f64, x: st.ui.pl_x, vscale: st.ui.pl_vzoom, y: st.ui.pl_y };
        let (n, running) = nav::step(cur, t);
        st.ui.playlist_zoom = n.zoom as f32;
        st.ui.pl_x = n.x.max(0.0);
        st.ui.pl_vzoom = n.vscale;
        st.ui.pl_y = n.y.max(0.0);
        if running {
            ui.ctx().request_repaint();
        } else {
            st.ui.pl_anim = None;
        }
    }
    let zoom = st.ui.playlist_zoom as f64;
    let x_of = |beat: f64, sx: f64| timeline.left() + ((beat - sx) * zoom) as f32;
    let beat_of = |x: f32, sx: f64| sx + ((x - timeline.left()) as f64 / zoom);

    // ----- Ctrl + right-drag area zoom, Ctrl + right-click zoom out, middle-drag pan
    match nav::area_zoom(ui, timeline, "playlist") {
        Some(AreaZoom::To(r)) => {
            let (b0, b1) = (beat_of(r.left(), st.ui.pl_x), beat_of(r.right(), st.ui.pl_x));
            let nz = (timeline.width() as f64 / (b1 - b0).max(1e-3)).clamp(3.0, 400.0);
            let vz = st.ui.pl_vzoom;
            let vs = if r.height() > 24.0 { (vz * timeline.height() / r.height()).clamp(0.4, 4.0) } else { vz };
            let content_y = r.top() - timeline.top() + st.ui.pl_y;
            st.ui.pl_anim = Some(ViewTarget { zoom: nz, x: b0.max(0.0), vscale: vs, y: (content_y * vs / vz).max(0.0) });
        }
        Some(AreaZoom::Out(at)) => {
            let b = beat_of(at.x, st.ui.pl_x);
            let nz = (zoom / 2.0).clamp(3.0, 400.0);
            let vz = st.ui.pl_vzoom;
            let vs = if vz > 1.0 { (vz / 2.0).max(1.0) } else { vz };
            st.ui.pl_anim = Some(ViewTarget { zoom: nz, x: (b - (at.x - timeline.left()) as f64 / nz).max(0.0), vscale: vs, y: (st.ui.pl_y * vs / vz).max(0.0) });
        }
        None => {}
    }
    let pan = nav::middle_drag(ui, rect);
    if pan != Vec2::ZERO {
        st.ui.pl_anim = None;
        st.ui.pl_x = (st.ui.pl_x - pan.x as f64 / zoom).max(0.0);
        st.ui.pl_y = (st.ui.pl_y - pan.y).max(0.0);
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    // ----- scrolling and zoom
    if resp.hovered() {
        // egui reports Ctrl+wheel (and pinch) as a zoom factor, not as scrolling.
        let (scroll, zoom_d, shift, alt) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers.shift, i.modifiers.alt));
        if zoom_d != 1.0 || scroll != Vec2::ZERO {
            st.ui.pl_anim = None;
        }
        let raw = ui.input(|i| i.raw_scroll_delta);
        let notch = raw.y + raw.x;
        if shift && alt && notch != 0.0 {
            // FL Shift+Alt+wheel: nudge the selected clips by a quarter of the snap.
            let step = st.ui.snap.max(0.0625) / 4.0 * notch.signum() as f64;
            let ids = st.ui.clips.clone();
            st.session.edit("Nudge clips", |p| {
                for c in p.tracks.iter_mut().flat_map(|t| t.clips.iter_mut()).filter(|c| ids.contains(&c.id)) {
                    c.start = (c.start + step).max(0.0);
                }
            });
        } else if shift && alt {
        } else if alt && scroll.y != 0.0 {
            // Alt + wheel: track height (vertical zoom) around the mouse.
            let vz = st.ui.pl_vzoom;
            let nv = (vz * (1.0 + scroll.y / 300.0)).clamp(0.4, 4.0);
            let my = ui.input(|i| i.pointer.hover_pos()).map(|p| p.y - timeline.top()).unwrap_or(0.0);
            st.ui.pl_y = ((st.ui.pl_y + my) * nv / vz - my).max(0.0);
            st.ui.pl_vzoom = nv;
        } else if zoom_d != 1.0 {
            let mouse_beat = ui.input(|i| i.pointer.hover_pos()).map(|p| beat_of(p.x, st.ui.pl_x)).unwrap_or(st.ui.pl_x);
            st.ui.playlist_zoom = (st.ui.playlist_zoom * zoom_d).clamp(3.0, 400.0);
            let nz = st.ui.playlist_zoom as f64;
            if let Some(p) = ui.input(|i| i.pointer.hover_pos()) {
                st.ui.pl_x = (mouse_beat - (p.x - timeline.left()) as f64 / nz).max(0.0);
            }
        } else if shift || scroll.x != 0.0 {
            let d = if shift { scroll.y } else { scroll.x };
            st.ui.pl_x = (st.ui.pl_x - d as f64 / zoom).max(0.0);
            if !shift {
                st.ui.pl_y = (st.ui.pl_y - scroll.y).max(0.0);
            }
        } else {
            st.ui.pl_y = (st.ui.pl_y - scroll.y).max(0.0);
        }
    }
    let sx = st.ui.pl_x;
    let beat_now = st.beat_now();
    let playing = st.transport().playing;
    if playing && st.settings.follow_playhead && st.ui.pl_drag.is_none() {
        let visible = timeline.width() as f64 / zoom;
        if beat_now > sx + visible * 0.9 || beat_now < sx {
            st.ui.pl_x = (beat_now - visible * 0.1).max(0.0);
        }
    }
    let sx = st.ui.pl_x;

    // ----- rows layout
    let p = &st.session.project;
    let mut rows = Vec::new();
    let mut y = timeline.top() - st.ui.pl_y;
    for t in &p.tracks {
        let mut takes = Vec::new();
        let h = (t.height.clamp(28.0, 200.0) * st.ui.pl_vzoom).clamp(18.0, 600.0);
        let mut yy = y + h;
        if t.show_takes {
            for k in &t.takes {
                takes.push((k.id, k.name.clone(), yy));
                yy += TAKE_H;
            }
        }
        rows.push(Row { track: t.id, y, h, takes });
        y = yy;
    }
    let content_bottom = y;

    // ----- grid
    let end_beat = beat_of(timeline.right(), sx);
    let bpb = p.beats_per_bar(0.0);
    let step = if zoom < 6.0 {
        bpb * 4.0
    } else if zoom < 14.0 {
        bpb
    } else if zoom < 60.0 {
        1.0
    } else {
        0.25
    };
    let mut b = snap_floor(sx, step);
    let tgrid = painter.with_clip_rect(timeline);
    while b <= end_beat {
        let x = x_of(b, sx);
        let bar = (b / bpb).fract().abs() < 1e-6;
        tgrid.line_segment([Pos2::new(x, timeline.top()), Pos2::new(x, timeline.bottom())], Stroke::new(1.0_f32, if bar { theme::GRID_BAR } else { theme::GRID }));
        b += step;
    }
    // Loop & punch ranges.
    if let Some(r) = p.loop_range {
        let col = if p.loop_enabled { theme::PLAYHEAD.gamma_multiply(0.07) } else { Color32::from_white_alpha(4) };
        tgrid.rect_filled(Rect::from_x_y_ranges(x_of(r.start, sx)..=x_of(r.end, sx), timeline.y_range()), 0.0, col);
    }
    if let (Some(r), true) = (p.punch, p.punch_enabled) {
        tgrid.rect_filled(Rect::from_x_y_ranges(x_of(r.start, sx)..=x_of(r.end, sx), timeline.y_range()), 0.0, theme::REC.gamma_multiply(0.06));
    }

    // ----- clips
    let tm = TempoMap::from_project(p);
    let mut clip_rects: Vec<(Id, Id, Rect)> = Vec::new();
    let mut clip_menus: Vec<(Id, Id, Rect)> = Vec::new();
    let pattern_names: Vec<(Id, String, Rgb)> = p.patterns.iter().map(|x| (x.id, x.name.clone(), x.color)).collect();
    let tracks: Vec<Track> = p.tracks.clone();
    let patterns = p.patterns.clone();
    let rate = st.audio.rate() as f64;
    for (row, t) in rows.iter().zip(&tracks) {
        let lane = Rect::from_x_y_ranges(timeline.x_range(), row.y..=row.y + row.h);
        if lane.bottom() < timeline.top() || lane.top() > timeline.bottom() {
            continue;
        }
        tgrid.line_segment([Pos2::new(timeline.left(), lane.bottom()), Pos2::new(timeline.right(), lane.bottom())], Stroke::new(1.0_f32, theme::GRID));
        for c in &t.clips {
            let r = Rect::from_x_y_ranges(x_of(c.start, sx)..=x_of(c.end(), sx), lane.top() + 1.0..=lane.bottom() - 1.0);
            if r.right() < timeline.left() || r.left() > timeline.right() {
                clip_rects.push((t.id, c.id, r));
                continue;
            }
            let selected = st.ui.clips.contains(&c.id);
            draw_clip(st, &tgrid, r, c, &tm, rate, &pattern_names, &patterns, selected, t.muted || c.muted);
            clip_rects.push((t.id, c.id, r));
            // Pattern clips get FL's menu triangle in the title bar (make unique, split, source pattern …).
            if let ClipKind::Pattern { pattern } = c.kind {
                if r.width() > 18.0 {
                    let icon = Rect::from_min_size(Pos2::new(r.left().max(timeline.left()) + 1.0, r.top() + 1.0), Vec2::new(12.0, 11.0));
                    tgrid.add(egui::Shape::convex_polygon(
                        vec![Pos2::new(icon.left() + 2.0, icon.top() + 3.0), Pos2::new(icon.right() - 2.0, icon.top() + 3.0), Pos2::new(icon.center().x, icon.bottom() - 2.0)],
                        Color32::BLACK,
                        Stroke::NONE,
                    ));
                    clip_menus.push((c.id, pattern, icon));
                }
            }
        }
        for (kid, name, ky) in &row.takes {
            let kl = Rect::from_x_y_ranges(timeline.x_range(), *ky..=*ky + TAKE_H);
            tgrid.rect_filled(kl, 0.0, theme::PANEL.gamma_multiply(0.6));
            if let Some(k) = t.takes.iter().find(|k| k.id == *kid) {
                for c in &k.clips {
                    let r = Rect::from_x_y_ranges(x_of(c.start, sx)..=x_of(c.end(), sx), kl.top() + 2.0..=kl.bottom() - 2.0);
                    tgrid.rect_filled(r, 2.0, theme::rgb(c.color).gamma_multiply(0.35));
                }
            }
            tgrid.text(Pos2::new(timeline.left() + 4.0, kl.center().y), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(10.0), theme::TEXT_DIM);
        }
    }

    for (cid, pattern, icon) in clip_menus {
        if !timeline.contains(icon.center()) {
            continue;
        }
        let r = ui.interact(icon, egui::Id::new(("pl.clipmenu", cid)), Sense::click()).on_hover_text("Pattern clip menu");
        let popup = egui::Id::new(("pl.clipmenu.popup", cid));
        if r.clicked() {
            ui.memory_mut(|m| m.toggle_popup(popup));
        }
        egui::popup_below_widget(ui, popup, &r, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
            ui.set_min_width(170.0);
            super::rack::pattern_menu(ui, st, pattern, Some(cid));
        });
    }

    let p = &st.session.project;
    // ----- ruler with markers
    let ruler = Rect::from_min_max(Pos2::new(timeline.left(), rect.top()), Pos2::new(rect.right(), timeline.top()));
    let rp = painter.with_clip_rect(ruler);
    rp.rect_filled(ruler, 0.0, theme::PANEL);
    let mut b = snap_floor(sx, bpb.max(step));
    while b <= end_beat {
        let x = x_of(b, sx);
        let bar = (b / bpb).round() as i64 + 1;
        rp.line_segment([Pos2::new(x, ruler.bottom() - 8.0), Pos2::new(x, ruler.bottom())], Stroke::new(1.0_f32, theme::GRID_BAR));
        rp.text(Pos2::new(x + 3.0, ruler.bottom() - 2.0), egui::Align2::LEFT_BOTTOM, bar.to_string(), egui::FontId::monospace(10.0), theme::TEXT_DIM);
        b += bpb.max(step);
    }
    if let Some(r) = p.loop_range {
        let lr = Rect::from_x_y_ranges(x_of(r.start, sx)..=x_of(r.end, sx), ruler.top()..=ruler.top() + 7.0);
        rp.rect_filled(lr, 2.0, if p.loop_enabled { theme::PLAYHEAD } else { theme::GRID_BAR });
    }
    if let Some(r) = p.punch {
        let lr = Rect::from_x_y_ranges(x_of(r.start, sx)..=x_of(r.end, sx), ruler.top() + 8.0..=ruler.top() + 12.0);
        rp.rect_filled(lr, 1.0, if p.punch_enabled { theme::REC } else { theme::GRID_BAR });
    }
    for m in &p.markers {
        let x = x_of(m.beat, sx);
        rp.add(egui::Shape::convex_polygon(vec![Pos2::new(x, ruler.top() + 13.0), Pos2::new(x + 6.0, ruler.top() + 13.0), Pos2::new(x, ruler.top() + 20.0)], theme::ACCENT_2, Stroke::NONE));
        rp.text(Pos2::new(x + 8.0, ruler.top() + 12.0), egui::Align2::LEFT_TOP, &m.name, egui::FontId::proportional(10.0), theme::ACCENT_2);
    }

    // ----- playhead
    let px = x_of(beat_now.max(0.0), sx);
    if px >= timeline.left() && px <= timeline.right() {
        painter.line_segment([Pos2::new(px, ruler.top()), Pos2::new(px, rect.bottom())], Stroke::new(1.5_f32, theme::PLAYHEAD));
    }

    // ----- track headers
    let header = Rect::from_min_max(Pos2::new(rect.left(), timeline.top()), Pos2::new(timeline.left(), rect.bottom()));
    let mut header_ui = ui.new_child(egui::UiBuilder::new().max_rect(header));
    header_ui.set_clip_rect(header);
    for (row, t) in rows.iter().zip(&tracks) {
        let r = Rect::from_min_size(Pos2::new(header.left(), row.y), Vec2::new(HEADER, row.h));
        if r.bottom() < header.top() || r.top() > header.bottom() {
            continue;
        }
        track_header(&mut header_ui, st, r, t);
    }
    // Corner: add track.
    let corner = Rect::from_min_max(rect.min, Pos2::new(timeline.left(), timeline.top()));
    let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(corner.shrink(4.0)));
    cui.horizontal(|ui| {
        if ui.button("+ Track").clicked() {
            st.ctx.run("dragon.add.track", serde_json::Value::Null);
        }
        ui.label(RichText::new(format!("{} tracks", tracks.len())).small().color(theme::TEXT_DIM));
    });

    // ----- interaction
    let hover = resp.hover_pos();
    let row_at = |y: f32| rows.iter().position(|r| y >= r.y && y < r.y + r.h);
    let take_at = |y: f32| rows.iter().find_map(|r| r.takes.iter().find(|(_, _, ky)| y >= *ky && y < *ky + TAKE_H).map(|(k, _, _)| (r.track, *k)));
    let clip_at = |pos: Pos2| clip_rects.iter().rev().find(|(_, _, r)| r.contains(pos)).copied();
    let tool = st.ui.tool;
    let grid = st.ui.snap;

    // Ruler: click to seek, drag to set loop (Shift: punch).
    if let Some(pos) = resp.interact_pointer_pos() {
        if ruler.contains(pos) && resp.drag_started_by(egui::PointerButton::Primary) {
            let b0 = snap(beat_of(pos.x, sx), grid.max(0.25));
            let shift = ui.input(|i| i.modifiers.shift);
            st.ui.pl_drag = Some(Drag { clip: 0, track: 0, kind: if shift { DragKind::Punch { from: b0 } } else { DragKind::Loop { from: b0 } } });
        }
    }
    if resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            if ruler.contains(pos) {
                st.seek(snap(beat_of(pos.x, sx), grid));
            }
        }
    }

    // Drop from the picker: patterns, audio files and automation clips.
    let dnd_pos = ui.ctx().pointer_latest_pos();
    if let Some(items) = resp.dnd_hover_payload::<Vec<super::picker::PickItem>>() {
        if let Some(pos) = dnd_pos.filter(|p| timeline.contains(*p)) {
            let beat = snap(beat_of(pos.x, sx).max(0.0), grid);
            let p = ui.painter_at(timeline);
            let first = row_at(pos.y);
            for (k, item) in items.iter().enumerate() {
                let len = super::picker::length(st, item);
                let (y, h) = first.and_then(|i| rows.get(i + k)).map(|r| (r.y, r.h)).unwrap_or((pos.y - 12.0 + k as f32 * 26.0, 24.0));
                let ghost = Rect::from_min_size(Pos2::new(x_of(beat, sx), y + 1.0), Vec2::new(((len * zoom) as f32).max(8.0), h - 2.0));
                p.rect_filled(ghost, 3.0, theme::ACCENT.gamma_multiply(0.25));
                p.rect_stroke(ghost, 3.0, Stroke::new(1.5_f32, theme::ACCENT), egui::StrokeKind::Inside);
            }
            if items.len() > 1 {
                p.text(pos + Vec2::new(12.0, -14.0), egui::Align2::LEFT_BOTTOM, format!("{} items", items.len()), egui::FontId::proportional(11.0), theme::TEXT);
            }
            ui.ctx().request_repaint();
        }
    }
    if let Some(items) = resp.dnd_release_payload::<Vec<super::picker::PickItem>>() {
        if let Some(pos) = dnd_pos.filter(|p| timeline.contains(*p)) {
            let beat = snap(beat_of(pos.x, sx).max(0.0), grid);
            let track = row_at(pos.y).map(|i| rows[i].track);
            super::picker::place_many(st, &items, track, beat);
        }
    }
    // Drop from the browser.
    if let Some(path) = resp.dnd_release_payload::<PathBuf>() {
        if let Some(pos) = hover {
            let beat = snap(beat_of(pos.x, sx).max(0.0), grid);
            let track = row_at(pos.y).map(|i| rows[i].track);
            st.add_audio_clip(&path, track, beat);
        }
    }

    if resp.drag_started_by(egui::PointerButton::Primary) && st.ui.pl_drag.is_none() {
        // Where the button went down (the drag only starts after a few pixels of movement).
        if let Some(pos) = ui.input(|i| i.pointer.press_origin()).or_else(|| resp.interact_pointer_pos()).filter(|p| timeline.contains(*p)) {
            if let Some((track, take)) = take_at(pos.y) {
                st.ui.pl_drag = Some(Drag { clip: 0, track, kind: DragKind::Comp { take, from: snap(beat_of(pos.x, sx), grid) } });
            } else if tool == Tool::Zoom {
                st.ui.pl_drag = Some(Drag { clip: 0, track: 0, kind: DragKind::Zoom { from: pos } });
            } else if tool == Tool::Playback {
                // Handled below (press and hold).
            } else if let (Tool::Slip, Some((tid, cid, _))) = (tool, clip_at(pos)) {
                let off = st.session.project.track(tid).and_then(|t| t.clips.iter().find(|c| c.id == cid)).and_then(|c| match &c.kind {
                    ClipKind::Audio(a) => Some(a.offset),
                    _ => None,
                });
                if let Some(offset) = off {
                    st.ui.pl_drag = Some(Drag { clip: cid, track: tid, kind: DragKind::Slip { from: beat_of(pos.x, sx), offset } });
                }
            } else if tool == Tool::Slice {
                st.ui.pl_drag = Some(Drag { clip: 0, track: 0, kind: DragKind::Knife { from: beat_of(pos.x, sx), y: pos.y + st.ui.pl_y } });
            } else if let Some((tid, cid, r)) = clip_at(pos) {
                let edge = 6.0;
                let kind = if pos.x > r.right() - edge {
                    DragKind::ResizeRight
                } else if pos.x < r.left() + edge && st.ui.resize_left {
                    DragKind::ResizeLeft
                } else {
                    let start = st.session.project.track(tid).and_then(|t| t.clips.iter().find(|c| c.id == cid)).map(|c| c.start).unwrap_or(0.0);
                    DragKind::Move { grab: beat_of(pos.x, sx) - start, track_offset: 0 }
                };
                let (ctrl, shift) = ui.input(|i| (i.modifiers.ctrl, i.modifiers.shift));
                if ctrl && !shift {
                    // FL: Ctrl + drag always selects, even when it starts on a clip.
                    st.ui.pl_drag = Some(Drag { clip: 0, track: 0, kind: DragKind::Select { from: beat_of(pos.x, sx), y: pos.y + st.ui.pl_y } });
                } else {
                    if !st.ui.clips.contains(&cid) {
                        st.ui.clips = vec![cid];
                        let groups = st.session.project.clip_groups.clone();
                        for g in groups.iter().filter(|g| g.contains(&cid)) {
                            st.ui.clips.extend(g.iter().filter(|c| **c != cid));
                        }
                    }
                    let (mut cid, mut tid) = (cid, tid);
                    if shift && matches!(kind, DragKind::Move { .. }) {
                        // FL: Shift + drag a clip drags copies of the selection; the originals stay.
                        let ids = st.ui.clips.clone();
                        let copies = st.session.edit("Clone clips", |p| dragon_engine::edit::copy_clips_in_place(p, &ids));
                        if let Some(k) = ids.iter().position(|x| *x == cid) {
                            cid = copies[k];
                            tid = st.session.project.tracks.iter().find(|t| t.clips.iter().any(|c| c.id == cid)).map(|t| t.id).unwrap_or(tid);
                        }
                        st.ui.clips = copies;
                    }
                    st.ui.pl_drag = Some(Drag { clip: cid, track: tid, kind });
                }
            } else if ui.input(|i| i.modifiers.ctrl) || tool == Tool::Select {
                st.ui.pl_drag = Some(Drag { clip: 0, track: 0, kind: DragKind::Select { from: beat_of(pos.x, sx), y: pos.y + st.ui.pl_y } });
            } else if tool == Tool::Paint {
                st.ui.pl_drag = Some(Drag { clip: 0, track: 0, kind: DragKind::Paint });
            }
        }
    }

    if let (Some(d), Some(pos)) = (st.ui.pl_drag, ui.input(|i| i.pointer.interact_pos())) {
        let beat = beat_of(pos.x, sx);
        match d.kind {
            DragKind::Move { grab, .. } => {
                let target_row = row_at(pos.y).map(|i| rows[i].track);
                let ids = st.ui.clips.clone();
                let new_start = snap((beat - grab).max(0.0), grid);
                st.session.gesture("Move clips", |p| {
                    let Some(anchor) = p.track(d.track).and_then(|t| t.clips.iter().find(|c| c.id == d.clip)).map(|c| c.start) else { return };
                    let delta = new_start - anchor;
                    let min_start = p.tracks.iter().flat_map(|t| t.clips.iter()).filter(|c| ids.contains(&c.id)).map(|c| c.start).fold(f64::MAX, f64::min);
                    let delta = delta.max(-min_start);
                    for t in p.tracks.iter_mut() {
                        for c in t.clips.iter_mut().filter(|c| ids.contains(&c.id)) {
                            c.start += delta;
                        }
                    }
                    // Moving to other tracks: every selected clip moves by the same number of tracks (when
                    // all of them still land on a track).
                    if let Some(to) = target_row {
                        let idx = |p: &Project, tid: Id| p.tracks.iter().position(|t| t.id == tid);
                        let (Some(from_i), Some(to_i)) = (idx(p, d.track), idx(p, to)) else { return };
                        let delta = to_i as i64 - from_i as i64;
                        if delta != 0 {
                            let moves: Vec<(usize, Id)> = p.tracks.iter().enumerate().flat_map(|(ti, t)| t.clips.iter().filter(|c| ids.contains(&c.id)).map(move |c| (ti, c.id))).collect();
                            let n = p.tracks.len() as i64;
                            if moves.iter().all(|(ti, _)| (0..n).contains(&(*ti as i64 + delta))) {
                                for (ti, cid) in moves {
                                    let k = p.tracks[ti].clips.iter().position(|c| c.id == cid).unwrap();
                                    let c = p.tracks[ti].clips.remove(k);
                                    p.tracks[(ti as i64 + delta) as usize].clips.push(c);
                                }
                            }
                        }
                    }
                });
                let anchor_track = st.session.project.tracks.iter().find(|t| t.clips.iter().any(|c| c.id == d.clip)).map(|t| t.id);
                if let (Some(dr), Some(t)) = (&mut st.ui.pl_drag, anchor_track) {
                    dr.track = t;
                }
            }
            DragKind::ResizeRight => {
                let b = snap(beat, grid);
                st.session.gesture("Resize clip", |p| {
                    if let Some(c) = p.track_mut(d.track).and_then(|t| t.clips.iter_mut().find(|c| c.id == d.clip)) {
                        c.length = (b - c.start).max(grid.max(1.0 / 16.0));
                    }
                });
            }
            DragKind::ResizeLeft => {
                let b = snap(beat, grid).max(0.0);
                let tm = TempoMap::from_project(&st.session.project);
                st.session.gesture("Trim clip", |p| {
                    if let Some(c) = p.track_mut(d.track).and_then(|t| t.clips.iter_mut().find(|c| c.id == d.clip)) {
                        let end = c.end();
                        let nb = b.min(end - 1.0 / 16.0);
                        let delta = nb - c.start;
                        if let ClipKind::Audio(a) = &mut c.kind {
                            let ds = tm.beat_to_seconds(nb) - tm.beat_to_seconds(c.start);
                            a.offset = (a.offset + ds * a.speed).max(0.0);
                        }
                        c.start = nb;
                        c.length = (c.length - delta).max(1.0 / 16.0);
                    }
                });
            }
            DragKind::Paint => {
                if let Some(i) = row_at(pos.y) {
                    let track = rows[i].track;
                    let b = snap_floor(beat.max(0.0), grid.max(1.0));
                    let pat = st.ui.pattern;
                    let len = st.session.project.pattern(pat).map(|x| x.length_beats()).unwrap_or(4.0);
                    let free = st.session.project.track(track).map(|t| !t.clips.iter().any(|c| c.start < b + len - 1e-9 && c.end() > b + 1e-9)).unwrap_or(false);
                    if free {
                        st.session.gesture("Paint patterns", |p| place_pattern(p, track, pat, b, len));
                    }
                }
            }
            DragKind::Loop { from } | DragKind::Punch { from } => {
                let b = snap(beat, grid.max(0.25)).max(0.0);
                let r = Range::new(from.min(b), from.max(b));
                let punch = matches!(d.kind, DragKind::Punch { .. });
                if r.len() > 0.0 {
                    st.session.gesture("Set range", |p| {
                        if punch {
                            p.punch = Some(r);
                            p.punch_enabled = true;
                        } else {
                            p.loop_range = Some(r);
                            p.loop_enabled = true;
                        }
                    });
                }
            }
            DragKind::Select { from, y } => {
                let y0 = y - st.ui.pl_y;
                let sel = Rect::from_two_pos(Pos2::new(x_of(from, sx), y0), pos);
                painter.rect_filled(sel, 2.0, theme::ACCENT.gamma_multiply(0.12));
                painter.rect_stroke(sel, 2.0, Stroke::new(1.0_f32, theme::ACCENT), egui::StrokeKind::Inside);
                let add = ui.input(|i| i.modifiers.shift);
                let hits: Vec<Id> = clip_rects.iter().filter(|(_, _, r)| r.intersects(sel)).map(|(_, c, _)| *c).collect();
                let base: Vec<Id> = if add { ui.data(|dd| dd.get_temp(egui::Id::new("pl.sel.base"))).unwrap_or_else(|| st.ui.clips.clone()) } else { Vec::new() };
                if add {
                    ui.data_mut(|dd| dd.insert_temp(egui::Id::new("pl.sel.base"), base.clone()));
                }
                let mut all = base;
                for h in hits {
                    if !all.contains(&h) {
                        all.push(h);
                    }
                }
                st.ui.clips = all;
            }
            DragKind::Knife { from, y } => {
                let alt = ui.input(|i| i.modifiers.alt);
                let sb = |b: f64| if alt { b } else { snap(b, grid) };
                let (a, b) = ((sb(from), y - st.ui.pl_y), (sb(beat), pos.y));
                let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
                let mut cuts: Vec<(Id, Id, f64)> = Vec::new();
                for row in rows.iter().filter(|r| r.y + r.h > y0 && r.y < y1.max(y0 + 1.0)) {
                    let cy = (row.y + row.h / 2.0).clamp(y0, y1);
                    let t = if (b.1 - a.1).abs() < 1e-3 { 0.0 } else { ((cy - a.1) / (b.1 - a.1)) as f64 };
                    let at = sb(a.0 + (b.0 - a.0) * t.clamp(0.0, 1.0));
                    if let Some(tr) = st.session.project.track(row.track) {
                        for c in tr.clips.iter().filter(|c| c.start < at - 1e-9 && c.end() > at + 1e-9) {
                            cuts.push((row.track, c.id, at));
                        }
                    }
                    let x = x_of(at, sx);
                    painter.line_segment([Pos2::new(x, row.y), Pos2::new(x, row.y + row.h)], Stroke::new(1.0_f32, Color32::WHITE.gamma_multiply(0.5)));
                }
                painter.line_segment([Pos2::new(x_of(a.0, sx), a.1), Pos2::new(x_of(b.0, sx), b.1)], Stroke::new(2.0_f32, theme::REC));
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                if !ui.input(|i| i.pointer.any_down()) && !cuts.is_empty() {
                    st.session.edit("Slice clips", |p| {
                        for (t, c, at) in &cuts {
                            dragon_engine::edit::split_clip(p, *t, *c, *at);
                        }
                    });
                }
            }
            DragKind::Zoom { from } => {
                let r = Rect::from_two_pos(from, pos);
                painter.rect_filled(r, 2.0, theme::ACCENT.gamma_multiply(0.10));
                painter.rect_stroke(r, 2.0, Stroke::new(1.0_f32, theme::ACCENT), egui::StrokeKind::Inside);
                if !ui.input(|i| i.pointer.any_down()) && r.width() > 6.0 {
                    let (b0, b1) = (beat_of(r.left(), sx), beat_of(r.right(), sx));
                    let nz = (timeline.width() as f64 / (b1 - b0).max(1e-3)).clamp(3.0, 400.0);
                    let vz = st.ui.pl_vzoom;
                    let vs = if r.height() > 24.0 { (vz * timeline.height() / r.height()).clamp(0.4, 4.0) } else { vz };
                    let content_y = r.top() - timeline.top() + st.ui.pl_y;
                    st.ui.pl_anim = Some(ViewTarget { zoom: nz, x: b0.max(0.0), vscale: vs, y: (content_y * vs / vz).max(0.0) });
                }
            }
            DragKind::Slip { from, offset } => {
                let secs = (beat - from) * 60.0 / st.session.project.bpm.max(1.0);
                let new = (offset - secs).max(0.0);
                st.session.gesture("Slip edit", |p| {
                    if let Some(ClipKind::Audio(a)) = p.track_mut(d.track).and_then(|t| t.clips.iter_mut().find(|c| c.id == d.clip)).map(|c| &mut c.kind) {
                        a.offset = new;
                    }
                });
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            DragKind::Comp { take, from } => {
                let b = snap(beat, grid);
                let x0 = x_of(from.min(b), sx);
                let x1 = x_of(from.max(b), sx);
                if let Some(r) = rows.iter().find(|r| r.track == d.track).and_then(|r| r.takes.iter().find(|k| k.0 == take)) {
                    painter.rect_filled(Rect::from_x_y_ranges(x0..=x1, r.2..=r.2 + TAKE_H), 2.0, theme::ACCENT_2.gamma_multiply(0.4));
                }
            }
        }
        if !ui.input(|i| i.pointer.any_down()) {
            if let DragKind::Comp { take, from } = d.kind {
                let b = snap(beat, grid);
                let r = Range::new(from.min(b), from.max(b));
                if r.len() > 0.0 {
                    st.session.edit("Comp take", |p| dragon_engine::edit::comp_promote(p, d.track, take, r));
                }
            }
            st.ui.pl_drag = None;
            ui.data_mut(|dd| dd.remove::<Vec<Id>>(egui::Id::new("pl.sel.base")));
            st.session.end_gesture();
        }
    }

    // FL's playback tool: hold the mouse to play from there, drag to scrub, release to stop.
    let scrub_id = egui::Id::new("pl.scrub");
    let scrubbing: bool = ui.data(|d| d.get_temp(scrub_id)).unwrap_or(false);
    if tool == Tool::Playback && resp.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_down()) {
        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()).filter(|p| timeline.contains(*p)) {
            let b = beat_of(pos.x, sx).max(0.0);
            if !scrubbing {
                st.seek(b);
                if !st.transport().playing {
                    st.toggle_play();
                }
                ui.data_mut(|d| d.insert_temp(scrub_id, true));
                ui.data_mut(|d| d.insert_temp(scrub_id.with("x"), pos.x));
            } else if ui.data(|d| d.get_temp::<f32>(scrub_id.with("x"))).is_some_and(|x| (x - pos.x).abs() > 1.0) {
                st.seek(b);
                ui.data_mut(|d| d.insert_temp(scrub_id.with("x"), pos.x));
            }
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    } else if scrubbing && !ui.input(|i| i.pointer.primary_down()) {
        ui.data_mut(|d| d.insert_temp(scrub_id, false));
        st.stop();
    }
    if tool == Tool::Zoom && resp.secondary_clicked() {
        if let Some(at) = resp.interact_pointer_pos() {
            let b = beat_of(at.x, sx);
            let nz = (zoom / 2.0).clamp(3.0, 400.0);
            st.ui.pl_anim = Some(ViewTarget { zoom: nz, x: (b - (at.x - timeline.left()) as f64 / nz).max(0.0), vscale: st.ui.pl_vzoom, y: st.ui.pl_y });
        }
    }
    // Clicks with tools.
    if (resp.clicked() || resp.secondary_clicked()) && !matches!(tool, Tool::Playback | Tool::Zoom) {
        if let Some(pos) = resp.interact_pointer_pos().filter(|p| timeline.contains(*p)) {
            let beat = beat_of(pos.x, sx);
            let hit = clip_at(pos);
            if resp.secondary_clicked() {
                if ui.input(|i| i.modifiers.ctrl) {
                    // Ctrl + right-click is zoom out.
                } else if let Some((_, cid, _)) = hit {
                    st.session.edit("Delete clip", |p| dragon_engine::edit::delete_clips(p, &[cid]));
                    st.ui.clips.retain(|c| *c != cid);
                }
            } else {
                match (tool, hit) {
                    (Tool::Erase, Some((_, cid, _))) => st.session.edit("Delete clip", |p| dragon_engine::edit::delete_clips(p, &[cid])),
                    (Tool::Mute, Some((tid, cid, _))) => st.session.edit("Mute clip", |p| {
                        if let Some(c) = p.track_mut(tid).and_then(|t| t.clips.iter_mut().find(|c| c.id == cid)) {
                            c.muted = !c.muted;
                        }
                    }),
                    (Tool::Slice, Some((tid, cid, _))) => {
                        let b = snap(beat, grid);
                        st.session.edit("Slice clip", |p| dragon_engine::edit::split_clip(p, tid, cid, b));
                    }
                    (_, Some((_, cid, _))) => {
                        if ui.input(|i| i.modifiers.shift || i.modifiers.ctrl) {
                            if let Some(i) = st.ui.clips.iter().position(|c| *c == cid) {
                                st.ui.clips.remove(i);
                            } else {
                                st.ui.clips.push(cid);
                            }
                        } else {
                            st.ui.clips = vec![cid];
                        }
                        // Grouped clips (FL Shift+G) are selected together.
                        let groups = st.session.project.clip_groups.clone();
                        for g in groups.iter().filter(|g| g.contains(&cid)) {
                            for c in g {
                                if !st.ui.clips.contains(c) {
                                    st.ui.clips.push(*c);
                                }
                            }
                        }
                    }
                    (Tool::Draw | Tool::Paint, None) => {
                        if let Some(i) = row_at(pos.y) {
                            let track = rows[i].track;
                            let b = snap_floor(beat.max(0.0), grid.max(0.25));
                            let pat = st.ui.pattern;
                            let len = st.session.project.pattern(pat).map(|x| x.length_beats()).unwrap_or(4.0);
                            let id = st.session.edit("Place pattern", |p| place_pattern(p, track, pat, b, len));
                            st.ui.clips = vec![id];
                        }
                    }
                    (_, None) => {
                        st.ui.clips.clear();
                        st.seek(snap(beat, grid));
                    }
                }
            }
        }
    }
    if resp.double_clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            if let Some((tid, cid, _)) = clip_at(pos) {
                let kind = st.session.project.track(tid).and_then(|t| t.clips.iter().find(|c| c.id == cid)).map(|c| c.kind.clone());
                match kind {
                    Some(ClipKind::Pattern { pattern }) => {
                        st.ui.pattern = pattern;
                        st.ctx.open("dragon.piano", None);
                    }
                    Some(ClipKind::Audio(_)) => st.ui.clip_window = Some(cid),
                    Some(ClipKind::Automation { .. }) => st.ctx.open("dragon.automation", None),
                    None => {}
                }
            } else if ruler.contains(pos) {
                st.ctx.run("dragon.add.marker", serde_json::Value::Null);
            }
        }
    }
    // Cursor hints.
    if let Some(pos) = hover {
        if let Some((_, _, r)) = clip_at(pos) {
            if pos.x > r.right() - 6.0 || pos.x < r.left() + 6.0 {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
        }
    }
    let _ = content_bottom;
}

pub fn place_pattern(p: &mut Project, track: Id, pattern: Id, start: f64, len: f64) -> Id {
    let id = p.next_id();
    let (name, color) = p.pattern(pattern).map(|x| (x.name.clone(), x.color)).unwrap_or_default();
    if let Some(t) = p.track_mut(track) {
        t.clips.push(Clip { id, name, start, length: len, muted: false, color, kind: ClipKind::Pattern { pattern } });
    }
    id
}

#[allow(clippy::too_many_arguments)]
fn draw_clip(st: &mut State, p: &egui::Painter, r: Rect, c: &Clip, tm: &TempoMap, rate: f64, names: &[(Id, String, Rgb)], patterns: &[Pattern], selected: bool, muted: bool) {
    let base = theme::rgb(c.color);
    let fill = if muted { base.gamma_multiply(0.25) } else { base.gamma_multiply(0.55) };
    p.rect_filled(r, 3.0, fill);
    p.rect_stroke(r, 3.0, Stroke::new(if selected { 2.0_f32 } else { 1.0 }, if selected { Color32::WHITE } else { base }), egui::StrokeKind::Inside);
    let title = Rect::from_min_size(r.min, Vec2::new(r.width(), 13.0));
    p.rect_filled(title, 3.0, base.gamma_multiply(if muted { 0.4 } else { 0.9 }));
    let label = match &c.kind {
        ClipKind::Pattern { pattern } => names.iter().find(|n| n.0 == *pattern).map(|n| n.1.clone()).unwrap_or_else(|| c.name.clone()),
        ClipKind::Audio(_) => c.name.clone(),
        ClipKind::Automation { lane } => st.session.project.automation.iter().find(|l| l.id == *lane).map(|l| crate::state::target_name(&st.session.project, &l.target)).unwrap_or_else(|| c.name.clone()),
    };
    let indent = if matches!(c.kind, ClipKind::Pattern { .. }) && r.width() > 18.0 { 15.0 } else { 3.0 };
    p.with_clip_rect(r.intersect(p.clip_rect())).text(title.left_center() + Vec2::new(indent, 0.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(10.0), Color32::BLACK);
    let body = Rect::from_min_max(Pos2::new(r.left(), r.top() + 13.0), r.max);
    let vis = body.intersect(p.clip_rect());
    if vis.width() <= 0.0 {
        return;
    }
    match &c.kind {
        ClipKind::Automation { lane } => {
            let Some(l) = st.session.project.automation.iter().find(|l| l.id == *lane) else {
                p.text(body.center(), egui::Align2::CENTER_CENTER, "lane deleted", egui::FontId::proportional(10.0), theme::REC);
                return;
            };
            let (min, max) = dragon_engine::midi::target_range(&l.target);
            let span = (max - min).max(1e-6);
            let col = if muted { theme::TEXT_DIM } else { Color32::from_gray(240) };
            let y_of = |v: f32| body.bottom() - 2.0 - ((v - min) / span).clamp(0.0, 1.0) * (body.height() - 4.0);
            let mut pts = Vec::new();
            let mut x = vis.left().floor();
            while x <= vis.right() {
                let b = ((x - r.left()) as f64) / (r.width() as f64) * c.length;
                if let Some(v) = l.value_at(b) {
                    pts.push(Pos2::new(x, y_of(v)));
                }
                x += 2.0;
            }
            if pts.len() > 1 {
                for w in pts.windows(2) {
                    p.add(egui::Shape::convex_polygon(vec![w[0], w[1], Pos2::new(w[1].x, body.bottom()), Pos2::new(w[0].x, body.bottom())], col.gamma_multiply(0.12), Stroke::NONE));
                }
                p.add(egui::Shape::line(pts, Stroke::new(1.5_f32, col)));
            }
            for pt in &l.points {
                if pt.beat > c.length {
                    break;
                }
                let px = r.left() + (pt.beat / c.length) as f32 * r.width();
                if px >= vis.left() && px <= vis.right() {
                    p.circle_filled(Pos2::new(px, y_of(pt.value)), 2.5, col);
                }
            }
        }
        ClipKind::Audio(a) => {
            let Some(w) = st.waveform(&a.source) else {
                p.text(body.center(), egui::Align2::CENTER_CENTER, "missing file", egui::FontId::proportional(10.0), theme::REC);
                return;
            };
            let gain = dragon_engine::dsp::db_to_gain(a.gain_db);
            let s0 = tm.beat_to_seconds(c.start);
            let mid = body.center().y;
            let half = body.height() * 0.48;
            let col = if muted { theme::TEXT_DIM } else { Color32::from_gray(230) };
            let mut x = vis.left().floor();
            while x < vis.right() {
                let b0 = c.start + ((x - r.left()) as f64) / (r.width() as f64) * c.length;
                let b1 = c.start + ((x + 1.0 - r.left()) as f64) / (r.width() as f64) * c.length;
                let f0 = ((a.offset + (tm.beat_to_seconds(b0) - s0) * a.speed) * rate) as usize / WAVE_BUCKET;
                let f1 = (((a.offset + (tm.beat_to_seconds(b1) - s0) * a.speed) * rate) as usize / WAVE_BUCKET).max(f0 + 1);
                if f0 < w.len() {
                    let (lo, hi) = w[f0..f1.min(w.len())].iter().fold((0.0f32, 0.0f32), |(l, h), (a, b)| (l.min(*a), h.max(*b)));
                    p.line_segment([Pos2::new(x, mid - (hi * gain).min(1.0) * half), Pos2::new(x, mid - (lo * gain).max(-1.0) * half)], Stroke::new(1.0_f32, col));
                }
                x += 1.0;
            }
            // Fades.
            let fx = |secs: f64| (tm.seconds_to_beat(s0 + secs) - c.start) / c.length * r.width() as f64;
            if a.fade_in > 0.0 {
                let w = fx(a.fade_in) as f32;
                p.line_segment([Pos2::new(r.left(), body.bottom()), Pos2::new(r.left() + w, body.top())], Stroke::new(1.0_f32, theme::ACCENT_2));
            }
            if a.fade_out > 0.0 {
                let e = tm.beat_to_seconds(c.end()) - s0;
                let w = (fx(e) - fx(e - a.fade_out)) as f32;
                p.line_segment([Pos2::new(r.right() - w, body.top()), Pos2::new(r.right(), body.bottom())], Stroke::new(1.0_f32, theme::ACCENT_2));
            }
        }
        ClipKind::Pattern { pattern } => {
            let Some(pt) = patterns.iter().find(|x| x.id == *pattern) else { return };
            let notes: Vec<&Note> = pt.channels.values().flat_map(|ch| ch.notes.iter()).collect();
            let steps: Vec<(usize, &Step, usize)> = pt.channels.values().enumerate().flat_map(|(ci, ch)| ch.steps.iter().enumerate().filter(|(_, s)| s.on).map(move |(i, s)| (i, s, ci))).collect();
            let len = pt.length_beats().max(1e-6);
            let reps = (c.length / len).ceil() as usize;
            let (lo, hi) = notes.iter().fold((127u8, 0u8), |(l, h), n| (l.min(n.pitch), h.max(n.pitch)));
            let span = (hi.saturating_sub(lo)).max(12) as f32;
            for rep in 0..reps {
                let off = rep as f64 * len;
                for n in &notes {
                    let b = off + n.start as f64 / PPQ as f64;
                    if b >= c.length {
                        continue;
                    }
                    let x0 = r.left() + (b / c.length) as f32 * r.width();
                    let x1 = r.left() + (((off + n.end() as f64 / PPQ as f64).min(c.length)) / c.length) as f32 * r.width();
                    let y = body.bottom() - 3.0 - (n.pitch.saturating_sub(lo)) as f32 / span * (body.height() - 6.0);
                    p.line_segment([Pos2::new(x0, y), Pos2::new(x1.max(x0 + 1.0), y)], Stroke::new(2.0_f32, Color32::from_gray(235)));
                }
                let rows = pt.channels.len().max(1) as f32;
                for (i, _, ci) in &steps {
                    let b = off + *i as f64 * 0.25;
                    if b >= c.length {
                        continue;
                    }
                    let x = r.left() + (b / c.length) as f32 * r.width();
                    let y = body.top() + 2.0 + *ci as f32 / rows * (body.height() - 4.0);
                    p.rect_filled(Rect::from_min_size(Pos2::new(x, y), Vec2::new(2.5, ((body.height() - 4.0) / rows).max(2.0) - 1.0)), 0.0, Color32::from_gray(235));
                }
            }
        }
    }
}

fn track_header(ui: &mut Ui, st: &mut State, r: Rect, t: &Track) {
    let p = ui.painter();
    p.rect_filled(r.shrink2(Vec2::new(0.0, 0.5)), 0.0, theme::PANEL);
    p.rect_filled(Rect::from_min_size(r.min, Vec2::new(4.0, r.height())), 0.0, theme::rgb(t.color));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r.shrink2(Vec2::new(8.0, 3.0))));
    let tid = t.id;
    child.horizontal(|ui| {
        let mut name = t.name.clone();
        let resp = ui.add(egui::TextEdit::singleline(&mut name).desired_width(78.0).frame(false).font(egui::TextStyle::Small));
        if resp.changed() {
            st.session.gesture("Rename track", |p| {
                // The track's own mixer channel follows the name while they still match.
                let Some((old, mixer)) = p.track(tid).map(|t| (t.name.clone(), t.mixer)) else { return };
                if let Some(m) = p.mixer.get_mut(mixer).filter(|m| m.name == old) {
                    m.name = name.clone();
                }
                if let Some(t) = p.track_mut(tid) {
                    t.name = name;
                }
            });
        }
        if resp.lost_focus() {
            st.session.end_gesture();
        }
        if widgets::toggle(ui, t.muted, "M", theme::WARN).on_hover_text("Mute").clicked() {
            st.session.edit("Mute track", |p| p.track_mut(tid).iter_mut().for_each(|t| t.muted = !t.muted));
        }
        if widgets::toggle(ui, t.solo, "S", theme::OK).on_hover_text("Solo").clicked() {
            st.session.edit("Solo track", |p| p.track_mut(tid).iter_mut().for_each(|t| t.solo = !t.solo));
        }
        if widgets::toggle(ui, t.armed, "⚫", theme::REC).on_hover_text("Arm for recording").clicked() {
            st.session.edit("Arm track", |p| p.track_mut(tid).iter_mut().for_each(|t| t.armed = !t.armed));
        }
    });
    if r.height() >= 44.0 {
        child.horizontal(|ui| {
            let mixer = st.session.project.mixer.get(t.mixer).map(|m| m.name.clone()).unwrap_or_default();
            if ui.small_button(format!("➡ {mixer}")).on_hover_text("Select the track's mixer channel").clicked() {
                st.ui.mixer = t.mixer;
                st.ctx.open("dragon.mixer", None);
            }
            if !t.takes.is_empty()
                && widgets::toggle(ui, t.show_takes, &format!("{} takes", t.takes.len()), theme::ACCENT_2).on_hover_text("Show take lanes – drag a range in a take to comp it onto the main lane").clicked()
            {
                st.session.edit("Show takes", |p| p.track_mut(tid).iter_mut().for_each(|t| t.show_takes = !t.show_takes));
            }
            ui.menu_button("…", |ui| {
                let mut h = t.height;
                if ui.add(egui::Slider::new(&mut h, 28.0..=200.0).text("height")).changed() {
                    st.session.gesture("Track height", |p| p.track_mut(tid).iter_mut().for_each(|t| t.height = h));
                }
                ui.label("Input");
                let mut input = t.input.unwrap_or(InputSource { left: 0, right: Some(1) });
                let mut stereo = input.right.is_some();
                let mut changed = ui.add(egui::DragValue::new(&mut input.left).prefix("in ").range(0..=63)).changed();
                changed |= ui.checkbox(&mut stereo, "stereo").changed();
                if changed {
                    input.right = stereo.then_some(input.left + 1);
                    st.session.edit("Track input", |p| p.track_mut(tid).iter_mut().for_each(|t| t.input = Some(input)));
                }
                let mut mon = t.monitor;
                ui.horizontal(|ui| {
                    ui.label("Monitor");
                    for (m, n) in [(MonitorMode::Auto, "auto"), (MonitorMode::Input, "input"), (MonitorMode::Off, "off")] {
                        ui.selectable_value(&mut mon, m, n);
                    }
                });
                if mon != t.monitor {
                    st.session.edit("Monitoring", |p| p.track_mut(tid).iter_mut().for_each(|t| t.monitor = mon));
                }
                if ui.button("New take lane from current clips").clicked() {
                    st.session.edit("New take", |p| dragon_engine::edit::push_take(p, tid, "Take"));
                    ui.close_menu();
                }
                if ui.button("Delete track").clicked() {
                    st.session.edit("Delete track", |p| p.tracks.retain(|t| t.id != tid));
                    ui.close_menu();
                }
            });
        });
    }
    if st.session.in_gesture() && !ui.ctx().input(|i| i.pointer.any_down()) && !ui.ctx().wants_keyboard_input() {
        st.session.end_gesture();
    }
}

fn toolbar(ui: &mut Ui, st: &mut State) {
    ui.horizontal(|ui| {
        for (t, icon, name) in Tool::ALL {
            if ui.selectable_label(st.ui.tool == t, icon).on_hover_text(name).clicked() {
                st.ui.tool = t;
            }
        }
        ui.separator();
        ui.label("Snap");
        let cur = widgets::SNAPS.iter().find(|s| (s.1 - st.ui.snap).abs() < 1e-9).map(|s| s.0).unwrap_or("custom");
        egui::ComboBox::from_id_salt("pl.snap").selected_text(cur).width(80.0).show_ui(ui, |ui| {
            for (n, v) in widgets::SNAPS {
                ui.selectable_value(&mut st.ui.snap, *v, *n);
            }
        });
        ui.separator();
        if ui.small_button("−").clicked() {
            st.ui.playlist_zoom = (st.ui.playlist_zoom / 1.25).max(3.0);
        }
        if ui.small_button("+").clicked() {
            st.ui.playlist_zoom = (st.ui.playlist_zoom * 1.25).min(400.0);
        }
        if ui.small_button("Fit").on_hover_text("Zoom to the whole song").clicked() {
            let len = st.session.project.song_length().max(16.0);
            st.ui.playlist_zoom = ((ui.available_width() + 400.0) / len as f32).clamp(3.0, 400.0);
            st.ui.pl_x = 0.0;
        }
        ui.checkbox(&mut st.settings.follow_playhead, "Follow");
        ui.toggle_value(&mut st.ui.pl_picker, "Picker").on_hover_text("Show the patterns, audio and automation clips to drag into the playlist");
        ui.separator();
        ui.label(
            RichText::new("Ctrl+drag select · Shift+drag clip = clone · Ctrl+B duplicate · Ctrl+right-drag zoom to area · Ctrl+wheel zoom · middle-drag pan · drag ruler = loop · right-click deletes")
                .small()
                .color(theme::TEXT_DIM),
        );
    });
}
