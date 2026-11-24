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
    /// the slice tool: drag a line across clips (any direction); every clip it crosses is cut where the
    /// line passes its track (snapped; Alt = no snap).
    Knife {
        from: f64,
        y: f32,
        /// Right-drag: the smaller piece of each cut clip is deleted.
        delete_small: bool,
    },
    /// the zoom tool: a box from a screen position.
    Zoom {
        from: Pos2,
    },
    /// the slip edit: moves the audio inside the clip (the clip's audio offset, seconds, at the start).
    Slip {
        from: f64,
        offset: f64,
    },
    /// Clip gain: dragging the dB handle on an audio clip up / down (the selected audio clips move together;
    /// their start gains are in `UiState::gain_drag`).
    Gain {
        y: f32,
    },
    /// Dragging an audio clip's fade-in (`out` = false) or fade-out handle sideways.
    Fade {
        out: bool,
    },
}

/// The fade handles of an audio clip drawn in `r` (fade-in, fade-out), when the clip is large enough:
/// small squares on the title bar's lower edge, at the end of each fade.
pub fn fade_handles(r: Rect, length: f64, fade_in: f64, fade_out: f64) -> Option<(Rect, Rect)> {
    if r.width() < 28.0 || r.height() < 24.0 {
        return None;
    }
    let w = |b: f64| (b / length.max(1e-9)) as f32 * r.width();
    let y = r.top() + 13.0;
    let xi = (r.left() + w(fade_in)).clamp(r.left() + 5.0, r.right() - 5.0);
    let xo = (r.right() - w(fade_out)).clamp(r.left() + 5.0, r.right() - 5.0);
    Some((Rect::from_center_size(Pos2::new(xi, y), Vec2::splat(10.0)), Rect::from_center_size(Pos2::new(xo, y), Vec2::splat(10.0))))
}

/// The gain the clip's fades give at `b` beats into a clip of `len` beats.
fn fade_env(a: &AudioClip, len: f64, b: f64) -> f32 {
    let mut g = 1.0;
    if a.fade_in > 0.0 && b < a.fade_in {
        g *= dragon_engine::render::fade_gain(a.fade_curve, b / a.fade_in);
    }
    if a.fade_out > 0.0 && b > len - a.fade_out {
        g *= dragon_engine::render::fade_gain(a.fade_curve, (len - b) / a.fade_out);
    }
    g
}

/// "250 ms", "1.20 s".
fn ms_label(secs: f64) -> String {
    if secs < 1.0 {
        format!("{:.0} ms", secs * 1000.0)
    } else {
        format!("{secs:.2} s")
    }
}

/// The playlist's geometry for file drops: timeline rect, track rows (id, top, bottom), view and grid.
#[derive(Clone)]
pub struct DropInfo {
    pub pass: u64,
    pub timeline: Rect,
    pub tracks: Vec<(Id, f32, f32)>,
    pub x: f64,
    pub zoom: f64,
    pub grid: f64,
}

impl DropInfo {
    /// The playlist drop geometry if the playlist was drawn in the last couple of frames.
    pub fn get(ctx: &egui::Context) -> Option<DropInfo> {
        let now = ctx.cumulative_pass_nr();
        ctx.data(|d| d.get_temp::<DropInfo>(egui::Id::new("pl.drop"))).filter(|i| now.saturating_sub(i.pass) <= 2)
    }

    /// Where a file dropped at `pos` lands: (track index or `None` = a new track below, beat on the grid,
    /// never before 0). `None` when `pos` is not over the playlist's timeline.
    pub fn target(&self, pos: Pos2) -> Option<(Option<usize>, f64)> {
        if !self.timeline.contains(pos) {
            return None;
        }
        let beat = (self.x + (pos.x - self.timeline.left()) as f64 / self.zoom).max(0.0);
        let beat = if self.grid > 0.0 { (beat / self.grid).floor() * self.grid } else { beat };
        let track = self.tracks.iter().position(|(_, a, b)| pos.y >= *a && pos.y < *b);
        Some((track, beat))
    }
}

/// The clip-gain handle at the top centre of an audio clip, when the clip is large enough to show it.
pub fn gain_handle(r: Rect) -> Option<Rect> {
    (r.width() >= 64.0 && r.height() >= 26.0).then(|| Rect::from_center_size(Pos2::new(r.center().x, r.top() + 8.0), Vec2::new(48.0, 13.0)))
}

/// "+1.5 dB", "0 dB", "-6.0 dB".
pub fn gain_label(db: f32) -> String {
    if db.abs() < 0.05 {
        "0 dB".into()
    } else {
        format!("{:+.1} dB", db)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drag {
    pub clip: Id,
    pub track: Id,
    pub kind: DragKind,
}

/// A track's height in compact mode.
pub const COMPACT_H: f32 = 22.0;

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
            let nz = (timeline.width() as f64 / (b1 - b0).max(1e-3)).clamp(MIN_ZOOM as f64, 400.0);
            let vz = st.ui.pl_vzoom;
            let vs = if r.height() > 24.0 { (vz * timeline.height() / r.height()).clamp(0.4, 4.0) } else { vz };
            let content_y = r.top() - timeline.top() + st.ui.pl_y;
            st.ui.pl_anim = Some(ViewTarget { zoom: nz, x: b0.max(0.0), vscale: vs, y: (content_y * vs / vz).max(0.0) });
        }
        Some(AreaZoom::Out(at)) => {
            let b = beat_of(at.x, st.ui.pl_x);
            let nz = (zoom / 2.0).clamp(MIN_ZOOM as f64, 400.0);
            let vz = st.ui.pl_vzoom;
            let vs = if vz > 1.0 { (vz / 2.0).max(1.0) } else { vz };
            st.ui.pl_anim = Some(ViewTarget { zoom: nz, x: (b - (at.x - timeline.left()) as f64 / nz).max(0.0), vscale: vs, y: (st.ui.pl_y * vs / vz).max(0.0) });
        }
        None => {}
    }
    // the side-specific modifiers: Right-Shift slices, Right-Alt mutes / quantizes, Left-Shift + right-drag pans.
    use super::keys::{side, SideKey};
    let shift_held = ui.input(|i| i.modifiers.shift);
    let alt_held = ui.input(|i| i.modifiers.alt);
    let rsh = shift_held && side(st, SideKey::RightShift);
    let lsh = shift_held && !rsh && side(st, SideKey::LeftShift);
    let ralt = alt_held && side(st, SideKey::RightAlt);
    let pan = nav::pan_drag(ui, rect, lsh);
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
            // Shift+Alt+wheel: nudge the selected clips by a quarter of the snap.
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
            st.ui.playlist_zoom = (st.ui.playlist_zoom * zoom_d).clamp(MIN_ZOOM, 400.0);
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
        // Compact: one thin row per track.
        let h = if st.settings.compact_tracks { COMPACT_H } else { (t.height.clamp(28.0, 200.0) * st.ui.pl_vzoom).clamp(18.0, 600.0) };
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
    // Where files dropped from the file manager land (read by the app's drop handling).
    {
        let pass = ui.ctx().cumulative_pass_nr();
        let tracks: Vec<(Id, f32, f32)> = rows.iter().map(|r| (r.track, r.y, r.y + r.h)).collect();
        let info = DropInfo { pass, timeline, tracks, x: st.ui.pl_x, zoom: st.ui.playlist_zoom as f64, grid: st.ui.snap };
        ui.data_mut(|d| d.insert_temp(egui::Id::new("pl.drop"), info.clone()));
        // Files dragged over from the file manager: show where they will land.
        let hovered: Vec<String> = ui.input(|i| i.raw.hovered_files.iter().filter_map(|f| f.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string())).collect());
        if let (false, Some(pos)) = (hovered.is_empty(), ui.input(|i| i.pointer.latest_pos())) {
            if let Some((track, beat)) = info.target(pos) {
                let x = timeline.left() + ((beat - st.ui.pl_x) * zoom) as f32;
                for (k, name) in hovered.iter().enumerate() {
                    let (y0, y1) = match track.and_then(|t| info.tracks.get(t + k)) {
                        Some(&(_, a, b)) => (a, b),
                        None => {
                            let base = info.tracks.last().map(|t| t.2).unwrap_or(timeline.top()) + 40.0 * (k as f32 - track.map(|t| info.tracks.len().saturating_sub(t)).unwrap_or(0) as f32).max(0.0);
                            (base, base + 40.0)
                        }
                    };
                    let r = Rect::from_min_max(Pos2::new(x, y0 + 2.0), Pos2::new(x + 160.0, y1 - 2.0));
                    painter.rect_filled(r, 3.0, theme::ACCENT.gamma_multiply(0.25));
                    painter.rect_stroke(r, 3.0, Stroke::new(1.5_f32, theme::ACCENT), egui::StrokeKind::Inside);
                    painter.text(r.left_top() + Vec2::new(4.0, 2.0), egui::Align2::LEFT_TOP, name, egui::FontId::proportional(10.0), Color32::WHITE);
                }
                ui.ctx().request_repaint();
            }
        }
    }

    // ----- grid
    let end_beat = beat_of(timeline.right(), sx);
    let bpb = p.beats_per_bar(0.0);
    // Grid lines at least ~7 px apart (quarter beats, beats, then bars, doubling) – readable at any zoom.
    let step = grid_step(zoom, bpb, 7.0);
    // Bar numbers at least ~34 px apart.
    let label_step = grid_step(zoom, bpb, 34.0).max(bpb);
    st.ui.pl_timeline_w = timeline.width();
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
            // Pattern clips get the menu triangle in the title bar (make unique, split, source pattern …).
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
    let mut b = snap_floor(sx, label_step);
    while b <= end_beat {
        let x = x_of(b, sx);
        let bar = (b / bpb).round() as i64 + 1;
        rp.line_segment([Pos2::new(x, ruler.bottom() - 8.0), Pos2::new(x, ruler.bottom())], Stroke::new(1.0_f32, theme::GRID_BAR));
        rp.text(Pos2::new(x + 3.0, ruler.bottom() - 2.0), egui::Align2::LEFT_BOTTOM, bar.to_string(), egui::FontId::monospace(10.0), theme::TEXT_DIM);
        b += label_step;
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
    ui.data_mut(|d| d.insert_temp(egui::Id::new("pl.header"), header));
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
        ui.menu_button("+ Track ⏷", |ui| {
            if ui.button("Track").on_hover_text("An empty track for patterns, audio and automation clips").clicked() {
                st.ctx.run("dragon.add.track", serde_json::Value::Null);
                ui.close_menu();
            }
            if ui.button(RichText::new("🎤 Audio track (records)").color(theme::REC)).on_hover_text("A track with an input and its own mixer channel, armed to record your mic or instrument").clicked() {
                st.ctx.run("dragon.add.recording", serde_json::Value::Null);
                ui.close_menu();
            }
        });
        ui.label(RichText::new(format!("{} tracks", tracks.len())).small().color(theme::TEXT_DIM));
    });

    // The audio take being recorded: a red audio clip growing on each armed track (it becomes the real
    // audio clip when recording stops).
    if let (Some(b0), true) = (st.ui.record_beat, st.audio.is_recording()) {
        let now = beat_now.max(b0);
        let p = ui.painter_at(timeline);
        for (row, t) in rows.iter().zip(&tracks) {
            if !t.armed || now <= b0 {
                continue;
            }
            let r = Rect::from_min_max(Pos2::new(x_of(b0, sx), row.y + 2.0), Pos2::new(x_of(now, sx), row.y + row.h - 2.0));
            p.rect_filled(r, 4.0, theme::REC.gamma_multiply(0.35));
            p.rect_stroke(r, 4.0, Stroke::new(1.5_f32, theme::REC), egui::StrokeKind::Inside);
            p.text(r.left_top() + Vec2::new(6.0, 4.0), egui::Align2::LEFT_TOP, "⏺ Recording audio", egui::FontId::proportional(11.0), Color32::WHITE);
        }
        ui.ctx().request_repaint();
    }
    if st.ui.pl_drag.is_none() {
        ui.data_mut(|dd| dd.remove::<bool>(egui::Id::new("pl.spawned")));
    }
    // While clips are dragged: below the last track, a new grouped track awaits them.
    if let (Some(d), Some(pos)) = (st.ui.pl_drag, ui.input(|i| i.pointer.interact_pos())) {
        if matches!(d.kind, DragKind::Move { .. }) && pos.y > content_bottom + 4.0 && timeline.contains(pos) && !ui.data(|dd| dd.get_temp::<bool>(egui::Id::new("pl.spawned")).unwrap_or(false)) {
            let zone = Rect::from_min_max(Pos2::new(timeline.left() + 2.0, content_bottom + 2.0), Pos2::new(timeline.right() - 2.0, (content_bottom + 56.0).min(timeline.bottom())));
            let p = ui.painter();
            p.rect_filled(zone, 6.0, theme::ACCENT.gamma_multiply(0.12));
            p.extend(egui::Shape::dashed_line(&[zone.left_top(), zone.right_top(), zone.right_bottom(), zone.left_bottom(), zone.left_top()], Stroke::new(1.5_f32, theme::ACCENT), 6.0, 4.0));
            let mixer = st.session.project.track(d.track).and_then(|t| st.session.project.mixer.get(t.mixer)).map(|m| m.name.clone()).unwrap_or_default();
            p.text(zone.center(), egui::Align2::CENTER_CENTER, format!("+  New track, grouped – still routed to {mixer}"), egui::FontId::proportional(13.0), theme::TEXT);
        }
    }
    ui.ctx().data_mut(|dd| dd.insert_temp(egui::Id::new("pl.content_bottom"), content_bottom));

    // ----- interaction
    let hover = resp.hover_pos();
    let row_at = |y: f32| rows.iter().position(|r| y >= r.y && y < r.y + r.h);
    let take_at = |y: f32| rows.iter().find_map(|r| r.takes.iter().find(|(_, _, ky)| y >= *ky && y < *ky + TAKE_H).map(|(k, _, _)| (r.track, *k)));
    let clip_at = |pos: Pos2| clip_rects.iter().rev().find(|(_, _, r)| r.contains(pos)).copied();
    // A fade handle under `pos`: (track, clip, fade-out?).
    let fade_rects: Vec<(Id, Id, Rect, Rect)> = clip_rects
        .iter()
        .filter_map(|(tid, cid, r)| {
            let c = st.session.project.track(*tid)?.clips.iter().find(|c| c.id == *cid)?;
            let ClipKind::Audio(a) = &c.kind else { return None };
            let (hi, ho) = fade_handles(*r, c.length, a.fade_in, a.fade_out)?;
            Some((*tid, *cid, hi, ho))
        })
        .collect();
    let fade_hit = |pos: Pos2| -> Option<(Id, Id, bool)> {
        let (tid, cid, _) = clip_rects.iter().rev().find(|(_, _, r)| r.expand(4.0).contains(pos)).copied()?;
        let &(_, _, hi, ho) = fade_rects.iter().find(|f| f.0 == tid && f.1 == cid)?;
        let (on_in, on_out) = (hi.expand(3.0).contains(pos), ho.expand(3.0).contains(pos));
        match (on_in, on_out) {
            // Where the two meet: the left half grabs the fade in, the right half the fade out.
            (true, true) => Some((tid, cid, pos.x >= (hi.center().x + ho.center().x) / 2.0)),
            (true, false) => Some((tid, cid, false)),
            (false, true) => Some((tid, cid, true)),
            _ => None,
        }
    };
    let tool = st.ui.tool;
    let grid = st.ui.snap;

    // Ruler: click to seek, drag to set loop (Shift: punch).
    if let Some(pos) = resp.interact_pointer_pos() {
        if ruler.contains(pos) && resp.drag_started_by(egui::PointerButton::Primary) {
            // Loop and punch ranges always snap to bars (Alt: to beats).
            let bar = if ui.input(|i| i.modifiers.alt) { 1.0 } else { st.session.project.beats_per_bar(0.0) };
            let b0 = snap(beat_of(pos.x, sx), bar).max(0.0);
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
    if let Some(items) = super::widgets::dnd_release::<Vec<super::picker::PickItem>>(&resp) {
        if let Some(pos) = dnd_pos.filter(|p| timeline.contains(*p)) {
            let beat = snap(beat_of(pos.x, sx).max(0.0), grid);
            let track = row_at(pos.y).map(|i| rows[i].track);
            super::picker::place_many(st, &items, track, beat);
        }
    }
    // Drop from the browser. (While something is dragged, egui reports no hover for the playlist, so
    // the pointer position is used – this is why dropped samples used to vanish.)
    if let Some(path) = resp.dnd_hover_payload::<PathBuf>() {
        if let Some(pos) = dnd_pos.filter(|p| timeline.contains(*p)) {
            let beat = snap(beat_of(pos.x, sx).max(0.0), grid);
            let p = ui.painter_at(timeline);
            let (y, h) = row_at(pos.y).and_then(|i| rows.get(i)).map(|r| (r.y, r.h)).unwrap_or((pos.y - 12.0, 24.0));
            // The sample's real length when it is cached, else a bar.
            let secs = st.session.cache.peek(&path, st.audio.rate()).map(|d| d.duration());
            let len = secs.map(|s| s * st.session.project.bpm / 60.0).unwrap_or(4.0);
            let ghost = Rect::from_min_size(Pos2::new(x_of(beat, sx), y + 1.0), Vec2::new(((len * zoom) as f32).max(10.0), h - 2.0));
            p.rect_filled(ghost, 3.0, theme::ACCENT.gamma_multiply(0.25));
            p.rect_stroke(ghost, 3.0, Stroke::new(1.5_f32, theme::ACCENT), egui::StrokeKind::Inside);
            let name = path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
            p.text(ghost.left_top() + Vec2::new(3.0, -2.0), egui::Align2::LEFT_BOTTOM, name, egui::FontId::proportional(11.0), theme::TEXT);
            ui.ctx().request_repaint();
        }
    }
    if let Some(path) = super::widgets::dnd_release::<PathBuf>(&resp) {
        if let Some(pos) = dnd_pos.filter(|p| timeline.contains(*p)) {
            let beat = snap(beat_of(pos.x, sx).max(0.0), grid);
            let track = row_at(pos.y).map(|i| rows[i].track);
            st.add_audio_clip_at(&path, track, beat);
        }
    }

    // Right-click and hold on empty space opens the slice tool ("click and hold" function).
    let now_t = ui.input(|i| i.time);
    if ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Secondary) && !i.pointer.middle_down()) && !ui.input(|i| i.modifiers.ctrl) && !lsh {
        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()).filter(|p| timeline.contains(*p) && clip_at(*p).is_none()) {
            st.ui.hold_slice = Some((now_t, pos));
        }
    }
    if let Some((t0, origin)) = st.ui.hold_slice {
        let moved = ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| p.distance(origin) > 6.0);
        if !ui.input(|i| i.pointer.secondary_down()) {
            st.ui.hold_slice = None;
        } else if now_t - t0 > 0.4 && st.ui.pl_drag.is_none() && !moved {
            st.ui.hold_slice = None;
            st.ui.pl_drag = Some(Drag { clip: 0, track: 0, kind: DragKind::Knife { from: beat_of(origin.x, sx), y: origin.y + st.ui.pl_y, delete_small: false } });
        } else {
            ui.ctx().request_repaint();
        }
    }
    // Slice tool (or Right-Shift), right-drag: slice and delete the smaller piece.
    if (tool == Tool::Slice || rsh) && resp.drag_started_by(egui::PointerButton::Secondary) && st.ui.pl_drag.is_none() && !ui.input(|i| i.modifiers.ctrl) {
        if let Some(pos) = ui.input(|i| i.pointer.press_origin()).filter(|p| timeline.contains(*p)) {
            st.ui.pl_drag = Some(Drag { clip: 0, track: 0, kind: DragKind::Knife { from: beat_of(pos.x, sx), y: pos.y + st.ui.pl_y, delete_small: true } });
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
            } else if tool == Tool::Slice || rsh {
                st.ui.pl_drag = Some(Drag { clip: 0, track: 0, kind: DragKind::Knife { from: beat_of(pos.x, sx), y: pos.y + st.ui.pl_y, delete_small: false } });
            } else if let Some((tid, cid, out)) = fade_hit(pos) {
                // A fade handle: drag it sideways to lengthen / shorten the fade.
                st.ui.pl_drag = Some(Drag { clip: cid, track: tid, kind: DragKind::Fade { out } });
            } else if let Some((tid, cid, r)) = clip_at(pos).filter(|(tid, cid, r)| {
                gain_handle(*r).is_some_and(|h| h.contains(pos)) && st.session.project.track(*tid).and_then(|t| t.clips.iter().find(|c| c.id == *cid)).is_some_and(|c| matches!(c.kind, ClipKind::Audio(_)))
            }) {
                // Clip gain handle: this clip, or every selected audio clip when it is selected.
                let _ = r;
                let ids: Vec<Id> = if st.ui.clips.contains(&cid) { st.ui.clips.clone() } else { vec![cid] };
                st.ui.gain_drag = st
                    .session
                    .project
                    .tracks
                    .iter()
                    .flat_map(|t| t.clips.iter().map(move |c| (t.id, c)))
                    .filter(|(_, c)| ids.contains(&c.id))
                    .filter_map(|(t, c)| match &c.kind {
                        ClipKind::Audio(a) => Some((t, c.id, a.gain_db)),
                        _ => None,
                    })
                    .collect();
                st.ui.pl_drag = Some(Drag { clip: cid, track: tid, kind: DragKind::Gain { y: pos.y } });
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
                    // Ctrl + drag always selects, even when it starts on a clip.
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
                        // Shift + drag a clip drags copies of the selection; the originals stay.
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
                                    let Some(k) = p.tracks[ti].clips.iter().position(|c| c.id == cid) else { continue };
                                    let c = p.tracks[ti].clips.remove(k);
                                    p.tracks[(ti as i64 + delta) as usize].clips.push(c);
                                }
                            }
                        }
                    }
                });
                // Dragged below the last track: the clips go onto a new track grouped with theirs, routed to
                // the same mixer channel.
                // Once per drag (afterwards the clips move between tracks as usual).
                let spawned = egui::Id::new("pl.spawned");
                if target_row.is_none() && pos.y > content_bottom + 4.0 && pos.y < timeline.bottom() && !ui.data(|dd| dd.get_temp::<bool>(spawned).unwrap_or(false)) {
                    st.session.gesture("Move clips", |p| {
                        dragon_engine::edit::clips_to_grouped_track(p, d.track, &ids);
                    });
                    ui.data_mut(|dd| dd.insert_temp(spawned, true));
                }
                let anchor_track = st.session.project.tracks.iter().find(|t| t.clips.iter().any(|c| c.id == d.clip)).map(|t| t.id);
                if let (Some(dr), Some(t)) = (&mut st.ui.pl_drag, anchor_track) {
                    dr.track = t;
                }
            }
            DragKind::ResizeRight => {
                let b = snap(beat, grid);
                // Left-Shift while resizing: ripple – the clips after it on the track move with its end.
                let ripple = lsh;
                st.session.gesture("Resize clip", |p| {
                    let Some(t) = p.track_mut(d.track) else { return };
                    let Some((start, old_end)) = t.clips.iter().find(|c| c.id == d.clip).map(|c| (c.start, c.end())) else { return };
                    let new_len = (b - start).max(grid.max(1.0 / 16.0));
                    let delta = start + new_len - old_end;
                    for c in t.clips.iter_mut() {
                        if c.id == d.clip {
                            c.length = new_len;
                        } else if ripple && c.start >= old_end - 1e-9 {
                            c.start = (c.start + delta).max(0.0);
                        }
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
                let bar = if ui.input(|i| i.modifiers.alt) { 1.0 } else { st.session.project.beats_per_bar(0.0) };
                let b = snap(beat, bar).max(0.0);
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
            DragKind::Knife { from, y, delete_small } => {
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
                            let right = dragon_engine::edit::split_clip(p, *t, *c, *at);
                            if let (true, Some(right)) = (delete_small, right) {
                                // Keep the longer piece.
                                let len = |p: &Project, id: Id| p.track(*t).and_then(|tr| tr.clips.iter().find(|x| x.id == id)).map(|x| x.length).unwrap_or(0.0);
                                let small = if len(p, right) < len(p, *c) { right } else { *c };
                                dragon_engine::edit::delete_clips(p, &[small]);
                            }
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
                    let nz = (timeline.width() as f64 / (b1 - b0).max(1e-3)).clamp(MIN_ZOOM as f64, 400.0);
                    let vz = st.ui.pl_vzoom;
                    let vs = if r.height() > 24.0 { (vz * timeline.height() / r.height()).clamp(0.4, 4.0) } else { vz };
                    let content_y = r.top() - timeline.top() + st.ui.pl_y;
                    st.ui.pl_anim = Some(ViewTarget { zoom: nz, x: b0.max(0.0), vscale: vs, y: (content_y * vs / vz).max(0.0) });
                }
            }
            DragKind::Gain { y } => {
                // 0.1 dB per pixel up (Shift: 0.02 dB), like a fader.
                let now_y = ui.input(|i| i.pointer.interact_pos()).map(|p| p.y).unwrap_or(y);
                let per_px = if ui.input(|i| i.modifiers.shift) { 0.02 } else { 0.1 };
                let delta = (y - now_y) * per_px;
                let starts = st.ui.gain_drag.clone();
                st.session.gesture("Clip gain", |p| {
                    for &(tid, cid, g0) in &starts {
                        if let Some(ClipKind::Audio(a)) = p.track_mut(tid).and_then(|t| t.clips.iter_mut().find(|c| c.id == cid)).map(|c| &mut c.kind) {
                            a.gain_db = ((g0 + delta) * 10.0).round() / 10.0;
                            a.gain_db = a.gain_db.clamp(-48.0, 24.0);
                        }
                    }
                });
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            }
            DragKind::Fade { out } => {
                let b = if ui.input(|i| i.modifiers.shift) { snap(beat, grid) } else { beat };
                st.session.gesture(if out { "Fade out" } else { "Fade in" }, |p| {
                    if let Some(c) = p.track_mut(d.track).and_then(|t| t.clips.iter_mut().find(|c| c.id == d.clip)) {
                        let (start, len) = (c.start, c.length);
                        if let ClipKind::Audio(a) = &mut c.kind {
                            if out {
                                a.fade_out = (start + len - b).clamp(0.0, (len - a.fade_in).max(0.0));
                            } else {
                                a.fade_in = (b - start).clamp(0.0, (len - a.fade_out).max(0.0));
                            }
                        }
                    }
                });
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
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

    // the playback tool: hold the mouse to play from there, drag to scrub, release to stop.
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
            let nz = (zoom / 2.0).clamp(MIN_ZOOM as f64, 400.0);
            st.ui.pl_anim = Some(ViewTarget { zoom: nz, x: (b - (at.x - timeline.left()) as f64 / nz).max(0.0), vscale: st.ui.pl_vzoom, y: st.ui.pl_y });
        }
    }
    // double right-click activates the mute tool.
    if resp.secondary_clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            let now = ui.input(|i| i.time);
            let id = egui::Id::new("pl.last_rclick");
            let last: Option<(f64, Pos2)> = ui.ctx().data(|d| d.get_temp(id));
            if last.is_some_and(|(t, q)| now - t < 0.35 && q.distance(p) < 6.0) {
                st.ui.tool = Tool::Mute;
                ui.ctx().data_mut(|d| d.remove::<(f64, Pos2)>(id));
            } else {
                ui.ctx().data_mut(|d| d.insert_temp(id, (now, p)));
            }
        }
    }
    // Right-Shift + click slices the clip under the pointer (Right-Shift + right-click also deletes the
    // smaller piece); Right-Alt + click mutes, Right-Alt + right-click quantizes the selection.
    let side_click = (rsh || ralt) && (resp.clicked() || resp.secondary_clicked());
    if side_click {
        if let Some(pos) = resp.interact_pointer_pos().filter(|p| timeline.contains(*p)) {
            let hit = clip_at(pos);
            let at = if alt_held && rsh { beat_of(pos.x, sx) } else { snap(beat_of(pos.x, sx), grid) };
            match (rsh, resp.secondary_clicked(), hit) {
                (true, del, Some((tid, cid, _))) => st.session.edit("Slice clip", |p| {
                    if let (Some(right), true) = (dragon_engine::edit::split_clip(p, tid, cid, at), del) {
                        let len = |p: &Project, id: Id| p.track(tid).and_then(|t| t.clips.iter().find(|x| x.id == id)).map(|x| x.length).unwrap_or(0.0);
                        let small = if len(p, right) < len(p, cid) { right } else { cid };
                        dragon_engine::edit::delete_clips(p, &[small]);
                    }
                }),
                (false, false, _) => {
                    let mut ids = st.ui.clips.clone();
                    if let Some((_, cid, _)) = hit {
                        if !ids.contains(&cid) {
                            ids = vec![cid];
                        }
                    }
                    st.session.edit("Mute clips", |p| p.tracks.iter_mut().flat_map(|t| t.clips.iter_mut()).filter(|c| ids.contains(&c.id)).for_each(|c| c.muted = !c.muted));
                }
                (false, true, _) => {
                    let ids = st.ui.clips.clone();
                    let g = if grid > 0.0 { grid } else { 1.0 };
                    st.session.edit("Quantize clips", |p| p.tracks.iter_mut().flat_map(|t| t.clips.iter_mut()).filter(|c| ids.contains(&c.id)).for_each(|c| c.start = (c.start / g).round() * g));
                }
                _ => {}
            }
        }
    }
    super::keys::tool_menu(ui, st, &resp);
    // Clicks with tools.
    if (resp.clicked() || resp.secondary_clicked()) && !side_click && st.ui.tool_menu.is_none() && !(lsh && resp.secondary_clicked()) && !matches!(tool, Tool::Playback | Tool::Zoom) {
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
                        // Grouped clips (Shift+G) are selected together.
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
        if let Some((tid, cid, out)) = resp.interact_pointer_pos().and_then(fade_hit) {
            // Double-click a fade handle: no fade.
            st.session.edit(if out { "Remove fade out" } else { "Remove fade in" }, |p| {
                if let Some(ClipKind::Audio(a)) = p.track_mut(tid).and_then(|t| t.clips.iter_mut().find(|c| c.id == cid)).map(|c| &mut c.kind) {
                    if out {
                        a.fade_out = 0.0;
                    } else {
                        a.fade_in = 0.0;
                    }
                }
            });
        } else if let Some(pos) = resp.interact_pointer_pos() {
            let on_gain = clip_at(pos).filter(|(_, _, r)| gain_handle(*r).is_some_and(|h| h.contains(pos)));
            if let Some((tid, cid, _)) = on_gain.filter(|(tid, cid, _)| st.session.project.track(*tid).and_then(|t| t.clips.iter().find(|c| c.id == *cid)).is_some_and(|c| matches!(c.kind, ClipKind::Audio(_)))) {
                // Double-click the gain handle: back to 0 dB.
                st.session.edit("Reset clip gain", |p| {
                    if let Some(ClipKind::Audio(a)) = p.track_mut(tid).and_then(|t| t.clips.iter_mut().find(|c| c.id == cid)).map(|c| &mut c.kind) {
                        a.gain_db = 0.0;
                    }
                });
            } else if let Some((tid, cid, _)) = clip_at(pos) {
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
            }
        }
    }
    // Cursor hints.
    if let Some(pos) = hover {
        if fade_hit(pos).is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        } else if let Some((_, _, r)) = clip_at(pos) {
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
            if let Some(h) = gain_handle(r) {
                let changed = a.gain_db.abs() >= 0.05;
                p.rect_filled(h, 4.0, Color32::from_black_alpha(if changed { 170 } else { 110 }));
                p.text(h.center(), egui::Align2::CENTER_CENTER, gain_label(a.gain_db), egui::FontId::monospace(9.0), if changed { theme::WARN } else { theme::TEXT_DIM });
            }
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
                    // The fades shape the waveform as they will sound.
                    let g = gain * fade_env(a, c.length, b0 - c.start);
                    p.line_segment([Pos2::new(x, mid - wave_disp(hi * g) * half), Pos2::new(x, mid - wave_disp(lo * g) * half)], Stroke::new(1.0_f32, col));
                }
                x += 1.0;
            }
            // Fades: the curve, the faded-out part shaded, and a handle at each fade's end.
            let bx = |b: f64| r.left() + (b / c.length.max(1e-9)) as f32 * r.width();
            let curve = |from: f64, to: f64, out: bool| -> Vec<Pos2> {
                (0..=24)
                    .map(|k| {
                        let b = from + (to - from) * k as f64 / 24.0;
                        let x = if out { (to - b) / (to - from).max(1e-9) } else { (b - from) / (to - from).max(1e-9) };
                        Pos2::new(bx(b), body.bottom() - dragon_engine::render::fade_gain(a.fade_curve, x) * body.height())
                    })
                    .collect()
            };
            for (on, from, to, out) in [(a.fade_in > 0.0, 0.0, a.fade_in, false), (a.fade_out > 0.0, c.length - a.fade_out, c.length, true)] {
                if !on {
                    continue;
                }
                let line = curve(from, to, out);
                // Shade above the curve (what the fade takes away).
                for w in line.windows(2) {
                    p.add(egui::Shape::convex_polygon(vec![Pos2::new(w[0].x, body.top()), Pos2::new(w[1].x, body.top()), w[1], w[0]], Color32::from_black_alpha(95), Stroke::NONE));
                }
                p.add(egui::Shape::line(line, Stroke::new(1.5_f32, theme::ACCENT_2)));
            }
            if let Some((hi, ho)) = fade_handles(r, c.length, a.fade_in, a.fade_out) {
                let hover = p.ctx().pointer_hover_pos();
                let active = match st.ui.pl_drag {
                    Some(Drag { clip, kind: DragKind::Fade { out }, .. }) if clip == c.id => Some(out),
                    _ => None,
                };
                let over_clip = hover.is_some_and(|h| r.expand(4.0).contains(h));
                let secs = |b0: f64, b1: f64| tm.beat_to_seconds(b1) - tm.beat_to_seconds(b0);
                for (h, out, len) in [(hi, false, a.fade_in), (ho, true, a.fade_out)] {
                    let hot = active == Some(out) || hover.is_some_and(|q| h.expand(3.0).contains(q));
                    // Handles show on a hovered or selected clip, and wherever a fade is set.
                    if !(over_clip || selected || len > 0.0 || hot) {
                        continue;
                    }
                    let fill = if hot {
                        Color32::WHITE
                    } else if len > 0.0 {
                        theme::ACCENT_2
                    } else {
                        Color32::from_white_alpha(150)
                    };
                    p.rect_filled(h.shrink(1.0), 2.0, fill);
                    p.rect_stroke(h.shrink(1.0), 2.0, Stroke::new(1.0_f32, Color32::BLACK), egui::StrokeKind::Inside);
                    if hot {
                        let t = if out { secs(c.end() - len, c.end()) } else { secs(c.start, c.start + len) };
                        let text = format!("{} {}", if out { "Fade out" } else { "Fade in" }, if len > 0.0 { ms_label(t) } else { "– drag".into() });
                        let at = Pos2::new(h.center().x, h.bottom() + 3.0);
                        let galley = p.layout_no_wrap(text, egui::FontId::proportional(10.5), Color32::WHITE);
                        let pos = Pos2::new(if out { at.x - galley.size().x - 4.0 } else { at.x + 4.0 }, at.y);
                        let bg = Rect::from_min_size(pos, galley.size()).expand(3.0);
                        p.with_clip_rect(p.clip_rect().expand(40.0)).rect_filled(bg, 3.0, Color32::from_black_alpha(200));
                        p.with_clip_rect(p.clip_rect().expand(40.0)).galley(pos, galley, Color32::WHITE);
                    }
                }
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
    let tid = t.id;
    if ui.input(|i| i.pointer.primary_pressed() && i.pointer.interact_pos().is_some_and(|p| r.contains(p))) {
        st.ui.track = Some(tid);
        st.ui.editor = crate::state::Editor::Playlist;
    }
    // Grip (the colour strip): drag up / down to move the track, live, one undo step.
    let grip = Rect::from_min_size(r.min, Vec2::new(8.0, r.height()));
    let gr = ui.interact(grip, egui::Id::new(("pl.grip", tid)), Sense::click_and_drag()).on_hover_text("Drag to move the track (Alt+Up / Down)");
    if gr.hovered() || gr.dragged() {
        ui.ctx().set_cursor_icon(if gr.dragged() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
    }
    let acc_key = egui::Id::new(("pl.grip.acc", tid));
    if gr.drag_started() {
        ui.data_mut(|d| d.insert_temp(acc_key, 0.0f32));
    }
    if gr.dragged() {
        let mut acc: f32 = ui.data(|d| d.get_temp(acc_key)).unwrap_or(0.0) + gr.drag_delta().y;
        let step = r.height().max(16.0);
        while acc.abs() >= step * 0.6 {
            let dir = acc.signum() as i32;
            st.session.gesture("Move track", |p| p.move_tracks(&[tid], dir));
            acc -= step * dir as f32;
        }
        ui.data_mut(|d| d.insert_temp(acc_key, acc));
    }
    if gr.drag_stopped() {
        st.session.end_gesture();
    }
    let p = ui.painter();
    p.rect_filled(r.shrink2(Vec2::new(0.0, 0.5)), 0.0, theme::PANEL);
    p.rect_filled(Rect::from_min_size(r.min, Vec2::new(4.0, r.height())), 0.0, theme::rgb(t.color));
    if gr.hovered() || gr.dragged() {
        for k in 0..3 {
            p.circle_filled(Pos2::new(r.left() + 5.5, r.center().y - 5.0 + k as f32 * 5.0), 1.2, theme::TEXT);
        }
    }
    let compact = r.height() < 26.0;
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r.shrink2(Vec2::new(8.0, if compact { 2.0 } else { 3.0 }))));
    child.horizontal(|ui| {
        // The track's kind: an audio track (it has an input, or audio clips) shows a microphone.
        if is_audio_track(t) {
            let (ir, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
            if !crate::brand::paint_icon(ui.painter(), ir, 11, Color32::WHITE) {
                ui.painter().text(ir.center(), egui::Align2::CENTER_CENTER, "🎤", egui::FontId::proportional(11.0), theme::ACCENT_2);
            }
            ui.interact(ir, egui::Id::new(("pl.kind", tid)), Sense::hover()).on_hover_text("Audio track – records from its input into audio clips");
        }
        let mut name = t.name.clone();
        let resp = ui.add(egui::TextEdit::singleline(&mut name).desired_width(if is_audio_track(t) { 44.0 } else { 60.0 }).frame(false).font(egui::TextStyle::Small));
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
        // Grouped tracks mute and solo together.
        let group = t.group;
        let in_group = move |x: &Track| x.id == tid || (group.is_some() && x.group == group);
        if widgets::toggle(ui, t.muted, "M", theme::WARN).on_hover_text(if group.is_some() { "Mute (the whole group)" } else { "Mute" }).clicked() {
            let on = !t.muted;
            st.session.edit("Mute track", |p| p.tracks.iter_mut().filter(|x| in_group(x)).for_each(|x| x.muted = on));
        }
        if widgets::toggle(ui, t.solo, "S", theme::OK).on_hover_text(if group.is_some() { "Solo (the whole group)" } else { "Solo" }).clicked() {
            let on = !t.solo;
            st.session.edit("Solo track", |p| p.tracks.iter_mut().filter(|x| in_group(x)).for_each(|x| x.solo = on));
        }
        if let Some(g) = group {
            let others: Vec<String> = st.session.project.tracks.iter().filter(|x| x.group == Some(g) && x.id != tid).map(|x| x.name.clone()).collect();
            let mixer = st.session.project.mixer.get(t.mixer).map(|m| m.name.clone()).unwrap_or_default();
            let chip = ui.add(egui::Label::new(RichText::new("🔗").small().color(theme::ACCENT_2)).sense(Sense::click()));
            let chip = chip.on_hover_text(format!("Grouped with {} – routed to {mixer}; mute / solo together. Click to ungroup.", others.join(", ")));
            if chip.clicked() {
                st.session.edit("Ungroup track", |p| {
                    if let Some(x) = p.track_mut(tid) {
                        x.group = None;
                    }
                    // A group of one is no group.
                    let left: Vec<Id> = p.tracks.iter().filter(|x| x.group == Some(g)).map(|x| x.id).collect();
                    if left.len() == 1 {
                        if let Some(x) = p.track_mut(left[0]) {
                            x.group = None;
                        }
                    }
                });
            }
        }
        let frozen = t.frozen.is_some();
        let tip = if frozen {
            "Frozen – rendered to audio, its instruments and effects rest. Click to unfreeze (edit again)."
        } else {
            "Freeze: render this track's mixer channel to audio so its instruments and effects use no CPU"
        };
        if widgets::toggle(ui, frozen, "❄", Color32::from_rgb(0x8a, 0xd4, 0xff)).on_hover_text(tip).clicked() {
            crate::commands::freeze_toggle(st, tid);
        }
        if widgets::toggle(ui, t.armed, "⚫", theme::REC).on_hover_text("Arm for recording").clicked() {
            st.session.edit("Arm track", |p| p.track_mut(tid).iter_mut().for_each(|t| t.armed = !t.armed));
        }
    });
    if r.height() >= 44.0 && t.input.is_some() && (t.audio || t.armed) {
        // Audio tracks: input and monitoring right in the header.
        child.horizontal(|ui| {
            if let Some(src) = t.input {
                super::recording::input_combo(ui, st, tid, src, 70.0, true);
            }
            let (label, next, tip) = match t.monitor {
                MonitorMode::Auto => ("🎧 auto", MonitorMode::Input, "Monitoring: hear the input while armed (click: always)"),
                MonitorMode::Input => ("🎧 on", MonitorMode::Off, "Monitoring: always hear the input (click: off)"),
                MonitorMode::Off => ("🎧 off", MonitorMode::Auto, "Monitoring off (click: while armed)"),
            };
            if widgets::toggle(ui, t.monitor != MonitorMode::Off, label, theme::ACCENT_2).on_hover_text(tip).clicked() {
                st.session.edit("Monitoring", |p| p.track_mut(tid).iter_mut().for_each(|t| t.monitor = next));
            }
        });
    }
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
    // One row, every control BAR_H tall, icons on the buttons; view options in a menu.
    ui.horizontal(|ui| {
        ui.spacing_mut().interact_size.y = widgets::BAR_H;
        widgets::tool_buttons(ui, st);
        ui.separator();
        let cur = widgets::SNAPS.iter().find(|s| (s.1 - st.ui.snap).abs() < 1e-9).map(|s| s.0).unwrap_or("custom");
        egui::ComboBox::from_id_salt("pl.snap").selected_text(format!("Snap {cur}")).width(100.0).show_ui(ui, |ui| {
            for (n, v) in widgets::SNAPS {
                ui.selectable_value(&mut st.ui.snap, *v, *n);
            }
        });
        ui.separator();
        let sq = egui::Vec2::splat(widgets::BAR_H);
        let zoom_icon = |ui: &mut Ui, tip: &str| match crate::brand::icon_image(ui.ctx(), crate::brand::icon::DRAW + 8, widgets::BAR_H - 8.0) {
            Some(img) => ui.add(egui::Button::image(img).min_size(sq)).on_hover_text(tip),
            None => ui.add(egui::Button::new("Z").min_size(sq)).on_hover_text(tip),
        };
        if ui.add(egui::Button::new(RichText::new("−").strong()).min_size(sq)).on_hover_text("Zoom out (Ctrl+wheel)").clicked() {
            st.ui.playlist_zoom = (st.ui.playlist_zoom / 1.25).max(MIN_ZOOM);
        }
        if ui.add(egui::Button::new(RichText::new("+").strong()).min_size(sq)).on_hover_text("Zoom in (Ctrl+wheel)").clicked() {
            st.ui.playlist_zoom = (st.ui.playlist_zoom * 1.25).min(400.0);
        }
        if zoom_icon(ui, "Fit: zoom to the whole song").clicked() {
            fit_song(st);
        }
        let marker = match crate::brand::icon_image(ui.ctx(), crate::brand::icon::MARKER, widgets::BAR_H - 10.0) {
            Some(img) => ui.add(egui::Button::image_and_text(img, RichText::new("Marker").color(theme::ACCENT_2))),
            None => ui.button(RichText::new("Marker").color(theme::ACCENT_2)),
        };
        if marker.on_hover_text("Add a time marker at the playhead (Alt+T)").clicked() {
            st.ctx.run("dragon.add.marker", serde_json::Value::Null);
        }
        ui.separator();
        let follow = st.settings.follow_playhead;
        if widgets::icon_toggle(ui, follow, crate::brand::icon::PLAY, "Follow", theme::PLAYHEAD).on_hover_text("Follow the playhead while playing").clicked() {
            st.settings.follow_playhead = !follow;
        }
        let compact = st.settings.compact_tracks;
        if widgets::icon_toggle(ui, compact, crate::brand::icon::DRAW + 2, "Compact", theme::ACCENT_2).on_hover_text("Compact tracks: one thin row each, so many more fit on screen").clicked() {
            st.ctx.run("dragon.view.compact_tracks", serde_json::Value::Null);
        }
        let picker = st.ui.pl_picker;
        if widgets::icon_toggle(ui, picker, crate::brand::icon::PATTERN, "Picker", theme::ACCENT).on_hover_text("Show the patterns, audio and automation clips to drag into the playlist").clicked() {
            st.ui.pl_picker = !picker;
        }
        ui.separator();
        // The mouse and key reference, on hover instead of a line of small text.
        ui.add(egui::Button::new(RichText::new("?").strong()).min_size(sq)).on_hover_text(
            "Ctrl+drag: select\nShift+drag a clip: clone\nCtrl+B: duplicate\nCtrl+right-drag: zoom to an area · Ctrl+right-click: zoom out\nCtrl+wheel: zoom · middle-drag: pan\nDrag the ruler: loop range (snaps to bars)\nRight-click: delete\nLeft / Right: move the playhead (faster while held)",
        );
    });
}

/// Waveform display scale: level in dB over a 54 dB range instead of linear amplitude, so a recording at
/// a healthy level (peaks around −12 … −18 dBFS) fills most of the clip instead of a thin line, as in
/// the waveform views of pro DAWs. Full scale stays at the clip edge.
pub fn wave_disp(a: f32) -> f32 {
    const RANGE: f32 = 54.0;
    let m = a.abs();
    if m < 1e-6 {
        return 0.0;
    }
    let v = (1.0 + 20.0 * m.log10() / RANGE).clamp(0.0, 1.0);
    v.copysign(a)
}

/// An audio track: made to record (or armed), or holding only audio clips.
pub fn is_audio_track(t: &Track) -> bool {
    t.audio || t.armed || (!t.clips.is_empty() && t.clips.iter().all(|c| matches!(c.kind, ClipKind::Audio(_))))
}

/// The furthest the playlist zooms out (pixels per beat): far enough for any song to fit.
pub const MIN_ZOOM: f32 = 0.05;

/// The smallest grid step (quarter beat, beat, then bars doubling) whose lines are at least `min_px` apart.
fn grid_step(zoom: f64, bpb: f64, min_px: f64) -> f64 {
    for st in [0.25, 1.0] {
        if st * zoom >= min_px {
            return st;
        }
    }
    let mut st = bpb;
    while st * zoom < min_px && st < 1e6 {
        st *= 2.0;
    }
    st
}

/// Zooms the playlist so the whole song (and a little room after it) fits the timeline.
pub fn fit_song(st: &mut State) {
    let len = st.session.project.song_length().max(16.0) * 1.05;
    let w = if st.ui.pl_timeline_w > 50.0 { st.ui.pl_timeline_w } else { 1000.0 };
    st.ui.playlist_zoom = (w / len as f32).clamp(MIN_ZOOM, 400.0);
    st.ui.pl_x = 0.0;
}
