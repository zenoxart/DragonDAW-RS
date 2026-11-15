//! Piano roll: notes of one channel in the selected pattern. Draw / paint / select / slice / erase / mute
//! tools, snap, scale highlighting and snapping, chord stamps, arpeggiator, strum, flam, quantize, legato,
//! humanize, chord progressions, velocity lane and ghost notes of the other channels.

use super::keyboard::is_black;
use super::widgets::{self, snap_floor};
use crate::state::{State, Tool};
use crate::theme;
use dragon_engine::device::Command as Cmd;
use dragon_engine::edit::{self, ArpMode, CHORDS, SCALES};
use dragon_engine::instruments::note_name;
use dragon_engine::model::*;
use dragon_engine::render::LiveEvent;
use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};

const KEYS_W: f32 = 54.0;
const VEL_H: f32 = 70.0;
const RULER: f32 = 18.0;

#[derive(Debug, Clone)]
pub enum Drag {
    Move { anchor_tick: i64, anchor_pitch: i32, orig: Vec<(usize, Note)> },
    Resize { anchor_tick: i64, orig: Vec<(usize, Note)> },
    Select { from: Pos2 },
    Velocity,
    Paint { last_tick: u32 },
}

pub fn show(ui: &mut Ui, st: &mut State) {
    let pat = st.ui.pattern;
    if st.ui.piano_channel.is_none() {
        st.ui.piano_channel = st.ui.channel.or(st.session.project.channels.first().map(|c| c.id));
    }
    let Some(ch) = st.ui.piano_channel else {
        ui.label("Add a channel to the rack to write notes.");
        return;
    };
    toolbar(ui, st, pat, ch);
    let notes: Vec<Note> = st.session.project.pattern(pat).and_then(|p| p.channels.get(&ch)).map(|c| c.notes.clone()).unwrap_or_default();
    let ghosts: Vec<(Rgb, Note)> = st
        .session
        .project
        .pattern(pat)
        .map(|p| {
            p.channels
                .iter()
                .filter(|(id, _)| **id != ch)
                .flat_map(|(id, c)| {
                    let col = st.session.project.channel(*id).map(|x| x.color).unwrap_or_default();
                    c.notes.iter().map(move |n| (col, *n))
                })
                .collect()
        })
        .unwrap_or_default();
    let ch_color = st.session.project.channel(ch).map(|c| theme::rgb(c.color)).unwrap_or(theme::ACCENT);
    let pat_len = st.session.project.pattern(pat).map(|p| p.length_beats()).unwrap_or(4.0);

    let avail = ui.available_rect_before_wrap();
    // Switching channel or pattern: centre the view on its notes.
    let focus_id = egui::Id::new("pr.focus");
    if ui.data(|d| d.get_temp::<(Id, Id)>(focus_id)) != Some((pat, ch)) {
        ui.data_mut(|d| d.insert_temp(focus_id, (pat, ch)));
        if !notes.is_empty() {
            let mean = notes.iter().map(|n| n.pitch as f32).sum::<f32>() / notes.len() as f32;
            let h = avail.height() - RULER - VEL_H;
            st.ui.pr_y = ((127.0 - mean) * st.ui.key_height - h / 2.0).clamp(0.0, (128.0 * st.ui.key_height - h).max(0.0));
            st.ui.pr_x = 0.0;
        }
    }
    let (rect, resp) = ui.allocate_exact_size(avail.size(), Sense::click_and_drag());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, theme::BG);
    let grid = Rect::from_min_max(Pos2::new(rect.left() + KEYS_W, rect.top() + RULER), Pos2::new(rect.right(), rect.bottom() - VEL_H));
    let vel = Rect::from_min_max(Pos2::new(grid.left(), grid.bottom() + 4.0), rect.max);
    let kh = st.ui.key_height;
    let zoom = st.ui.piano_zoom as f64;

    // scroll / zoom
    if resp.hovered() {
        let (scroll, zoom_d, shift) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers.shift));
        if zoom_d != 1.0 {
            st.ui.piano_zoom = (st.ui.piano_zoom * zoom_d).clamp(12.0, 800.0);
        } else if shift || scroll.x != 0.0 {
            st.ui.pr_x = (st.ui.pr_x - (if shift { scroll.y } else { scroll.x }) as f64 / zoom).max(0.0);
        } else {
            st.ui.pr_y = (st.ui.pr_y - scroll.y).clamp(0.0, 128.0 * kh - grid.height());
        }
        if ui.input(|i| i.modifiers.alt) && scroll.y != 0.0 {
            st.ui.key_height = (kh + scroll.y / 60.0).clamp(6.0, 30.0);
        }
    }
    let (sx, sy) = (st.ui.pr_x, st.ui.pr_y);
    let x_of = |tick: f64| grid.left() + ((tick / PPQ as f64 - sx) * zoom) as f32;
    let tick_of = |x: f32| ((sx + (x - grid.left()) as f64 / zoom) * PPQ as f64).max(0.0);
    let y_of = |pitch: u8| grid.top() + (127 - pitch as i32) as f32 * kh - sy;
    let pitch_of = |y: f32| (127 - ((y - grid.top() + sy) / kh).floor() as i32).clamp(0, 127) as u8;
    let scale: Option<&[u8]> = st.ui.scale.map(|i| SCALES[i].1);
    let root = st.ui.scale_root;

    // key rows
    let gp = p.with_clip_rect(grid);
    let top_pitch = pitch_of(grid.top());
    let bottom_pitch = pitch_of(grid.bottom());
    for pitch in bottom_pitch..=top_pitch {
        let y = y_of(pitch);
        let row = Rect::from_x_y_ranges(grid.x_range(), y..=y + kh);
        let in_scale = scale.map(|s| edit::in_scale(pitch, root, s)).unwrap_or(true);
        let col = if is_black(pitch) { Color32::from_gray(26) } else { Color32::from_gray(34) };
        gp.rect_filled(row, 0.0, if in_scale { col } else { col.gamma_multiply(0.6) });
        if pitch % 12 == 0 {
            gp.line_segment([row.left_bottom(), row.right_bottom()], Stroke::new(1.0_f32, theme::GRID_BAR));
        }
    }
    // beat grid
    let end_beat = sx + grid.width() as f64 / zoom;
    let step = if zoom < 40.0 {
        1.0
    } else if zoom < 160.0 {
        0.25
    } else {
        0.125
    };
    let mut b = snap_floor(sx, step);
    while b <= end_beat {
        let x = grid.left() + ((b - sx) * zoom) as f32;
        let strong = (b % 4.0).abs() < 1e-9;
        let beat = (b.fract()).abs() < 1e-9;
        gp.line_segment(
            [Pos2::new(x, grid.top()), Pos2::new(x, grid.bottom())],
            Stroke::new(
                1.0_f32,
                if strong {
                    theme::GRID_BAR
                } else if beat {
                    theme::GRID
                } else {
                    Color32::from_gray(38)
                },
            ),
        );
        b += step;
    }
    // pattern end
    let ex = grid.left() + ((pat_len - sx) * zoom) as f32;
    gp.rect_filled(Rect::from_x_y_ranges(ex..=grid.right().max(ex), grid.y_range()), 0.0, Color32::from_black_alpha(70));

    // ghost notes
    for (c, n) in &ghosts {
        let r = Rect::from_min_max(Pos2::new(x_of(n.start as f64), y_of(n.pitch) + 1.0), Pos2::new(x_of(n.end() as f64), y_of(n.pitch) + kh - 1.0));
        gp.rect_filled(r, 2.0, theme::rgb(*c).gamma_multiply(0.18));
    }
    // notes
    let mut note_rects = Vec::with_capacity(notes.len());
    for (i, n) in notes.iter().enumerate() {
        let r = Rect::from_min_max(Pos2::new(x_of(n.start as f64), y_of(n.pitch) + 1.0), Pos2::new(x_of(n.end() as f64).max(x_of(n.start as f64) + 3.0), y_of(n.pitch) + kh - 1.0));
        note_rects.push(r);
        let sel = st.ui.notes.contains(&i);
        let base = if n.muted { Color32::from_gray(90) } else { ch_color.lerp_to_gamma(Color32::WHITE, 0.15) };
        gp.rect_filled(r, 2.0, base.gamma_multiply(0.45 + 0.55 * n.velocity));
        gp.rect_stroke(r, 2.0, Stroke::new(if sel { 2.0_f32 } else { 1.0 }, if sel { Color32::WHITE } else { Color32::BLACK.gamma_multiply(0.6) }), egui::StrokeKind::Inside);
        if n.slide {
            gp.text(r.left_center() + Vec2::new(2.0, 0.0), egui::Align2::LEFT_CENTER, "⤴", egui::FontId::proportional(9.0), Color32::BLACK);
        }
        if r.width() > 26.0 && kh >= 11.0 {
            gp.with_clip_rect(r).text(r.left_center() + Vec2::new(3.0, 0.0), egui::Align2::LEFT_CENTER, note_name(n.pitch), egui::FontId::proportional(9.0), Color32::BLACK);
        }
    }
    // playhead (pattern mode only maps cleanly)
    let t = st.audio.transport().state;
    if t.playing {
        let beat = if st.ui.pattern_mode { st.beat_now().rem_euclid(pat_len.max(1e-6)) } else { st.beat_now() };
        let x = grid.left() + ((beat - sx) * zoom) as f32;
        if x >= grid.left() {
            p.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, grid.bottom())], Stroke::new(1.5_f32, theme::PLAYHEAD));
        }
    }
    // ruler
    let ruler = Rect::from_min_max(Pos2::new(grid.left(), rect.top()), Pos2::new(rect.right(), grid.top()));
    let rp = p.with_clip_rect(ruler);
    rp.rect_filled(ruler, 0.0, theme::PANEL);
    // Label every beat when there is room, otherwise every bar (or every few bars).
    let label_step = [1.0, 4.0, 8.0, 16.0, 32.0].into_iter().find(|s| s * zoom >= 38.0).unwrap_or(64.0);
    let mut b = snap_floor(sx, label_step);
    while b <= end_beat {
        let x = grid.left() + ((b - sx) * zoom) as f32;
        let text = if label_step >= 4.0 { format!("{}", (b / 4.0).floor() as i64 + 1) } else { format!("{}.{}", (b / 4.0).floor() as i64 + 1, (b % 4.0) as i64 + 1) };
        rp.text(Pos2::new(x + 2.0, ruler.center().y), egui::Align2::LEFT_CENTER, text, egui::FontId::monospace(9.0), theme::TEXT_DIM);
        b += label_step;
    }
    // keyboard
    let keys = Rect::from_min_max(Pos2::new(rect.left(), grid.top()), Pos2::new(grid.left(), grid.bottom()));
    let kp = p.with_clip_rect(keys);
    for pitch in bottom_pitch..=top_pitch {
        let y = y_of(pitch);
        let r = Rect::from_x_y_ranges(keys.x_range(), y..=y + kh);
        let black = is_black(pitch);
        let held = st.ui.held_keys.iter().any(|(_, n)| *n == pitch);
        kp.rect_filled(
            r.shrink2(Vec2::new(0.0, 0.5)),
            1.0,
            if held {
                theme::ACCENT_2
            } else if black {
                Color32::from_gray(20)
            } else {
                Color32::from_gray(225)
            },
        );
        if pitch % 12 == 0 || kh >= 13.0 && !black {
            kp.text(r.right_center() - Vec2::new(3.0, 0.0), egui::Align2::RIGHT_CENTER, note_name(pitch), egui::FontId::proportional(9.0), if black { Color32::WHITE } else { Color32::BLACK });
        }
    }
    // velocity lane
    p.rect_filled(vel, 0.0, theme::PANEL);
    let vp = p.with_clip_rect(vel);
    for (i, n) in notes.iter().enumerate() {
        let x = x_of(n.start as f64);
        let h = n.velocity * (vel.height() - 6.0);
        let sel = st.ui.notes.contains(&i);
        vp.line_segment([Pos2::new(x, vel.bottom() - 2.0), Pos2::new(x, vel.bottom() - 2.0 - h)], Stroke::new(3.0_f32, if sel { Color32::WHITE } else { ch_color }));
        vp.circle_filled(Pos2::new(x, vel.bottom() - 2.0 - h), 3.0, if sel { Color32::WHITE } else { ch_color });
    }
    p.text(Pos2::new(rect.left() + 4.0, vel.center().y), egui::Align2::LEFT_CENTER, "velocity", egui::FontId::proportional(10.0), theme::TEXT_DIM);

    // ---------------- interaction
    let grid_ticks = (st.ui.snap * PPQ as f64).round().max(0.0);
    let snap_tick = |t: f64| if grid_ticks > 0.0 { (t / grid_ticks).round() * grid_ticks } else { t };
    let snap_tick_floor = |t: f64| if grid_ticks > 0.0 { (t / grid_ticks).floor() * grid_ticks } else { t };
    let note_at = |pos: Pos2| note_rects.iter().enumerate().rev().find(|(_, r)| r.contains(pos)).map(|(i, r)| (i, *r));
    let tool = st.ui.tool;
    let preview = |st: &mut State, pitch: u8, vel: f32| {
        st.audio.send(Cmd::Live(LiveEvent::NoteOn { channel: ch, note: pitch, velocity: vel }));
        st.audio.send(Cmd::Live(LiveEvent::NoteOff { channel: ch, note: pitch }));
    };

    // keyboard click previews
    if resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos().filter(|p| keys.contains(*p)) {
            preview(st, pitch_of(pos.y), 0.8);
        }
    }

    if resp.drag_started() && st.ui.pr_drag.is_none() {
        if let Some(pos) = resp.interact_pointer_pos() {
            if vel.contains(pos) {
                st.ui.pr_drag = Some(Drag::Velocity);
            } else if grid.contains(pos) {
                match note_at(pos) {
                    Some((i, r)) if tool != Tool::Erase && tool != Tool::Slice => {
                        if !st.ui.notes.contains(&i) {
                            st.ui.notes = vec![i];
                        }
                        let orig: Vec<(usize, Note)> = st.ui.notes.iter().filter_map(|k| notes.get(*k).map(|n| (*k, *n))).collect();
                        let anchor_tick = tick_of(pos.x) as i64;
                        st.ui.pr_drag = Some(if pos.x > r.right() - 5.0 { Drag::Resize { anchor_tick, orig } } else { Drag::Move { anchor_tick, anchor_pitch: pitch_of(pos.y) as i32, orig } });
                        preview(st, notes[i].pitch, notes[i].velocity);
                    }
                    None if tool == Tool::Select || ui.input(|i| i.modifiers.ctrl) => st.ui.pr_drag = Some(Drag::Select { from: pos }),
                    None if tool == Tool::Paint => st.ui.pr_drag = Some(Drag::Paint { last_tick: u32::MAX }),
                    _ => {}
                }
            }
        }
    }
    if let (Some(d), Some(pos)) = (st.ui.pr_drag.clone(), ui.input(|i| i.pointer.interact_pos())) {
        match d {
            Drag::Move { anchor_tick, anchor_pitch, orig } => {
                let dt = snap_tick(tick_of(pos.x) - anchor_tick as f64) as i64;
                let dp = pitch_of(pos.y) as i32 - anchor_pitch;
                let min_start = orig.iter().map(|(_, n)| n.start as i64).min().unwrap_or(0);
                let dt = dt.max(-min_start);
                st.session.gesture("Move notes", |p| {
                    if let Some(v) = p.pattern_mut(pat).and_then(|pt| pt.channels.get_mut(&ch)) {
                        for (i, n) in &orig {
                            if let Some(x) = v.notes.get_mut(*i) {
                                x.start = (n.start as i64 + dt) as u32;
                                x.pitch = (n.pitch as i32 + dp).clamp(0, 127) as u8;
                            }
                        }
                    }
                });
            }
            Drag::Resize { anchor_tick, orig } => {
                let dl = (tick_of(pos.x) - anchor_tick as f64).round() as i64;
                st.session.gesture("Resize notes", |p| {
                    if let Some(v) = p.pattern_mut(pat).and_then(|pt| pt.channels.get_mut(&ch)) {
                        for (i, n) in &orig {
                            if let Some(x) = v.notes.get_mut(*i) {
                                let end = snap_tick((n.end() as i64 + dl) as f64).max(n.start as f64 + 1.0);
                                x.length = (end as u32).saturating_sub(n.start).max(1);
                            }
                        }
                    }
                });
                if let Some((_, n)) = orig.first() {
                    st.ui.last_note_len = st.session.project.pattern(pat).and_then(|pt| pt.channels.get(&ch)).and_then(|c| c.notes.get(orig[0].0)).map(|x| x.length).unwrap_or(n.length);
                }
            }
            Drag::Select { from } => {
                let r = Rect::from_two_pos(from, pos);
                p.rect(r, 0.0, theme::ACCENT_2.gamma_multiply(0.12), Stroke::new(1.0_f32, theme::ACCENT_2), egui::StrokeKind::Inside);
                st.ui.notes = note_rects.iter().enumerate().filter(|(_, nr)| nr.intersects(r)).map(|(i, _)| i).collect();
            }
            Drag::Velocity => {
                if vel.x_range().contains(pos.x) {
                    let v = ((vel.bottom() - 2.0 - pos.y) / (vel.height() - 6.0)).clamp(0.02, 1.0);
                    let hit: Vec<usize> = notes.iter().enumerate().filter(|(i, n)| (x_of(n.start as f64) - pos.x).abs() < 5.0 && (st.ui.notes.is_empty() || st.ui.notes.contains(i))).map(|(i, _)| i).collect();
                    if !hit.is_empty() {
                        st.session.gesture("Velocity", |p| {
                            if let Some(c) = p.pattern_mut(pat).and_then(|pt| pt.channels.get_mut(&ch)) {
                                for i in hit {
                                    c.notes[i].velocity = v;
                                }
                            }
                        });
                    }
                }
            }
            Drag::Paint { last_tick } => {
                if grid.contains(pos) {
                    let t = snap_tick_floor(tick_of(pos.x)) as u32;
                    if t != last_tick {
                        let pitch = snap_pitch(st, pitch_of(pos.y));
                        let len = st.ui.last_note_len;
                        if !notes.iter().any(|n| n.pitch == pitch && n.start <= t && n.end() > t) {
                            st.session.gesture("Paint notes", |p| {
                                if let Some(pt) = p.pattern_mut(pat) {
                                    pt.channels.entry(ch).or_default().notes.push(Note { pitch, start: t, length: len, velocity: 0.78, ..Default::default() });
                                }
                            });
                        }
                        st.ui.pr_drag = Some(Drag::Paint { last_tick: t });
                    }
                }
            }
        }
        if !ui.input(|i| i.pointer.any_down()) {
            st.ui.pr_drag = None;
            st.session.end_gesture();
        }
    }

    if (resp.clicked() || resp.secondary_clicked()) && st.ui.pr_drag.is_none() {
        if let Some(pos) = resp.interact_pointer_pos().filter(|p| grid.contains(*p)) {
            let hit = note_at(pos);
            let tick = tick_of(pos.x);
            if resp.secondary_clicked() || (tool == Tool::Erase && hit.is_some()) {
                if let Some((i, _)) = hit {
                    st.session.edit("Delete note", |p| {
                        if let Some(c) = p.pattern_mut(pat).and_then(|pt| pt.channels.get_mut(&ch)) {
                            c.notes.remove(i);
                        }
                    });
                    st.ui.notes.clear();
                }
            } else if let Some((i, _)) = hit {
                match tool {
                    Tool::Mute => st.session.edit("Mute note", |p| {
                        if let Some(c) = p.pattern_mut(pat).and_then(|pt| pt.channels.get_mut(&ch)) {
                            c.notes[i].muted = !c.notes[i].muted;
                        }
                    }),
                    Tool::Slice => {
                        let at = snap_tick(tick) as u32;
                        st.session.edit("Slice note", |p| {
                            if let Some(c) = p.pattern_mut(pat).and_then(|pt| pt.channels.get_mut(&ch)) {
                                if let Some(rest) = edit::split_note(&mut c.notes[i], at) {
                                    c.notes.push(rest);
                                }
                            }
                        });
                    }
                    _ => {
                        if ui.input(|i| i.modifiers.shift) {
                            if let Some(k) = st.ui.notes.iter().position(|x| *x == i) {
                                st.ui.notes.remove(k);
                            } else {
                                st.ui.notes.push(i);
                            }
                        } else {
                            st.ui.notes = vec![i];
                        }
                        st.ui.last_note_len = notes[i].length;
                    }
                }
            } else if matches!(tool, Tool::Draw | Tool::Paint) {
                let start = snap_tick_floor(tick) as u32;
                let pitch = snap_pitch(st, pitch_of(pos.y));
                let len = st.ui.last_note_len;
                let chord = st.ui.chord;
                let new: Vec<Note> = if chord == 0 { vec![Note { pitch, start, length: len, velocity: 0.78, ..Default::default() }] } else { edit::chord(pitch, CHORDS[chord - 1].1, start, len, 0.78, false) };
                preview(st, pitch, 0.78);
                let first = notes.len();
                let n = new.len();
                st.session.edit("Add note", |p| {
                    if let Some(pt) = p.pattern_mut(pat) {
                        pt.channels.entry(ch).or_default().notes.extend(new);
                    }
                });
                st.ui.notes = (first..first + n).collect();
            } else {
                st.ui.notes.clear();
            }
        }
    }
    if let Some(pos) = resp.hover_pos() {
        if let Some((_, r)) = note_at(pos) {
            if pos.x > r.right() - 5.0 {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
        }
    }
}

fn snap_pitch(st: &State, pitch: u8) -> u8 {
    match st.ui.scale {
        Some(i) => edit::snap_to_scale(pitch, st.ui.scale_root, SCALES[i].1),
        None => pitch,
    }
}

fn toolbar(ui: &mut Ui, st: &mut State, pat: Id, ch: Id) {
    ui.horizontal_wrapped(|ui| {
        let name = st.session.project.channel(ch).map(|c| c.name.clone()).unwrap_or_default();
        egui::ComboBox::from_id_salt("pr.channel").selected_text(name).width(110.0).show_ui(ui, |ui| {
            for c in &st.session.project.channels {
                if ui.selectable_label(c.id == ch, &c.name).clicked() {
                    st.ui.piano_channel = Some(c.id);
                    st.ui.channel = Some(c.id);
                    st.ui.notes.clear();
                }
            }
        });
        for (t, icon, name) in Tool::ALL {
            if ui.selectable_label(st.ui.tool == t, icon).on_hover_text(name).clicked() {
                st.ui.tool = t;
            }
        }
        ui.separator();
        let cur = widgets::SNAPS.iter().find(|s| (s.1 - st.ui.snap).abs() < 1e-9).map(|s| s.0).unwrap_or("custom");
        egui::ComboBox::from_id_salt("pr.snap").selected_text(format!("Snap {cur}")).width(100.0).show_ui(ui, |ui| {
            for (n, v) in widgets::SNAPS {
                ui.selectable_value(&mut st.ui.snap, *v, *n);
            }
        });
        let scale_name = st.ui.scale.map(|i| format!("{} {}", note_name(st.ui.scale_root).trim_end_matches(char::is_numeric), SCALES[i].0)).unwrap_or("No scale".into());
        egui::ComboBox::from_id_salt("pr.scale").selected_text(scale_name).width(130.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut st.ui.scale, None, "No scale");
            for (i, (n, _)) in SCALES.iter().enumerate() {
                ui.selectable_value(&mut st.ui.scale, Some(i), *n);
            }
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                for r in 0..12u8 {
                    ui.selectable_value(&mut st.ui.scale_root, r, note_name(r + 60).trim_end_matches(char::is_numeric).to_string());
                }
            });
        });
        let chord_name = if st.ui.chord == 0 { "Single notes".to_string() } else { CHORDS[st.ui.chord - 1].0.to_string() };
        egui::ComboBox::from_id_salt("pr.chord").selected_text(format!("Stamp: {chord_name}")).width(130.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut st.ui.chord, 0, "Single notes");
            for (i, (n, _)) in CHORDS.iter().enumerate() {
                ui.selectable_value(&mut st.ui.chord, i + 1, *n);
            }
        });
        ui.separator();
        for (id, label) in [("edit.quantize", "Quantize"), ("edit.legato", "Legato"), ("edit.strum", "Strum"), ("edit.humanize", "Humanize")] {
            if ui.small_button(label).clicked() {
                st.ctx.run(&format!("dragon.{id}"), serde_json::Value::Null);
            }
        }
        ui.menu_button("Arp", |ui| {
            for (mode, n) in [(ArpMode::Up, "Up"), (ArpMode::Down, "Down"), (ArpMode::UpDown, "Up-down"), (ArpMode::Random, "Random")] {
                if ui.button(n).clicked() {
                    let sel = st.ui.notes.clone();
                    let grid = ((st.ui.snap.max(0.125)) * PPQ as f64) as u32;
                    st.session.edit("Arpeggiate", |p| {
                        if let Some(c) = p.pattern_mut(pat).and_then(|pt| pt.channels.get_mut(&ch)) {
                            let src: Vec<Note> = if sel.is_empty() { c.notes.clone() } else { sel.iter().filter_map(|i| c.notes.get(*i).copied()).collect() };
                            let arp = edit::arpeggiate(&src, mode, grid, 0.9, 1);
                            let mut keep: Vec<Note> = c.notes.iter().enumerate().filter(|(i, _)| !sel.is_empty() && !sel.contains(i)).map(|(_, n)| *n).collect();
                            keep.extend(arp);
                            c.notes = keep;
                        }
                    });
                    st.ui.notes.clear();
                    ui.close_menu();
                }
            }
        });
        if ui.small_button("Flam").clicked() {
            let sel = st.ui.notes.clone();
            st.session.edit("Flam", |p| {
                if let Some(c) = p.pattern_mut(pat).and_then(|pt| pt.channels.get_mut(&ch)) {
                    let src: Vec<Note> = if sel.is_empty() { c.notes.clone() } else { sel.iter().filter_map(|i| c.notes.get(*i).copied()).collect() };
                    let extra = edit::flam(&src, 6);
                    c.notes.extend(extra);
                }
            });
        }
        ui.menu_button("Progression", |ui| {
            let scale = st.ui.scale.unwrap_or(0);
            ui.label(RichText::new(format!("in {} {}", note_name(st.ui.scale_root + 48), SCALES[scale].0)).small());
            for (name, degrees) in [("I–V–vi–IV", &[0usize, 4, 5, 3][..]), ("vi–IV–I–V", &[5, 3, 0, 4]), ("ii–V–I", &[1, 4, 0, 0]), ("i–VI–III–VII", &[0, 5, 2, 6])] {
                if ui.button(name).clicked() {
                    let root = 48 + st.ui.scale_root;
                    let notes = edit::progression(root, SCALES[scale].1, degrees, PPQ * 4, true);
                    st.session.edit("Chord progression", |p| {
                        if let Some(pt) = p.pattern_mut(pat) {
                            pt.channels.entry(ch).or_default().notes.extend(notes);
                        }
                    });
                    ui.ctx().data_mut(|d| d.remove::<(Id, Id)>(egui::Id::new("pr.focus")));
                    ui.close_menu();
                }
            }
        });
        let sel = st.ui.notes.clone();
        if let Some(&i) = sel.first() {
            ui.separator();
            if let Some(n) = st.session.project.pattern(pat).and_then(|pt| pt.channels.get(&ch)).and_then(|c| c.notes.get(i)).copied() {
                let mut slide = n.slide;
                if ui.checkbox(&mut slide, "slide").changed() {
                    st.session.edit("Slide", |p| {
                        if let Some(c) = p.pattern_mut(pat).and_then(|pt| pt.channels.get_mut(&ch)) {
                            for k in &sel {
                                if let Some(x) = c.notes.get_mut(*k) {
                                    x.slide = slide;
                                }
                            }
                        }
                    });
                }
                let mut pan = n.pan;
                let r = ui.add(egui::Slider::new(&mut pan, -1.0..=1.0).text("pan").show_value(false));
                widgets::apply(st, &r, "Note pan", |p| {
                    if let Some(c) = p.pattern_mut(pat).and_then(|pt| pt.channels.get_mut(&ch)) {
                        for k in &sel {
                            if let Some(x) = c.notes.get_mut(*k) {
                                x.pan = pan;
                            }
                        }
                    }
                });
            }
        }
    });
}
