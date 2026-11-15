//! Mixer, laid out like FL Studio's: a row of insert strips (master first, then the numbered inserts) with
//! peak meters, faders, pan and stereo-separation knobs, mute / solo, and a route arrow at the foot of every
//! strip. Selecting an insert lights the arrows of the inserts it routes to; clicking an arrow toggles a route
//! from the selected insert (right-click: "route to this track only"), and a routed arrow shows its level knob.
//! The panel on the right shows the selected insert's effect slots: as many as are used plus one empty slot
//! (up to ten), with bypass LEDs, mix knobs, drag-to-reorder, replace / sidechain / remove menus, followed by
//! its routing list and channel options.

use super::widgets::{self, knob};
use crate::state::State;
use crate::theme;
use dragon_engine::model::*;
use egui::{Color32, Pos2, RichText, Sense, Stroke, Ui, Vec2};

const PANEL_W: f32 = 330.0;
/// Height of an effect slot row.
const SLOT_H: f32 = 30.0;

/// How the selected insert reaches another one.
#[derive(Clone, Copy, PartialEq)]
enum Route {
    None,
    /// The main output (unity, after the fader).
    Main,
    /// A send with its level.
    Send(f32),
}

fn main_target(c: &MixerChannel, master: Id) -> Option<Id> {
    if c.id == master || !c.main_out || c.kind == ChannelKind::Vca {
        None
    } else {
        Some(c.output.unwrap_or(master))
    }
}

fn route_of(src: &MixerChannel, target: Id, master: Id) -> Route {
    if main_target(src, master) == Some(target) {
        Route::Main
    } else if let Some(s) = src.sends.iter().find(|s| s.target == target) {
        Route::Send(s.level)
    } else {
        Route::None
    }
}

/// Toggles a route from `src` to `target`, FL style: a new route is the main output when the insert has none,
/// otherwise a send at unity; an existing route is removed (removing the main route leaves only the sends).
fn toggle_route(st: &mut State, src: Id, target: Id) {
    let master = st.session.project.mixer.master.id;
    st.session.edit("Route", |p| {
        let Some(c) = p.mixer.get_mut(src) else { return };
        match route_of(c, target, master) {
            Route::Main => {
                c.main_out = false;
                c.output = None;
            }
            Route::Send(_) => c.sends.retain(|s| s.target != target),
            Route::None => {
                if main_target(c, master).is_none() {
                    c.main_out = true;
                    c.output = if target == master { None } else { Some(target) };
                } else {
                    c.sends.push(MixerSend { target, level: 1.0, pre_fader: false });
                }
            }
        }
    });
}

/// FL's "Route to this track only": the main output goes to `target`, every other route is removed.
fn route_only(st: &mut State, src: Id, target: Id) {
    let master = st.session.project.mixer.master.id;
    st.session.edit("Route to this track only", |p| {
        if let Some(c) = p.mixer.get_mut(src) {
            c.sends.clear();
            c.main_out = true;
            c.output = if target == master { None } else { Some(target) };
        }
    });
}

pub fn show(ui: &mut Ui, st: &mut State) {
    let meters = st.audio.meters().snapshot();
    let all: Vec<MixerChannel> = std::iter::once(st.session.project.mixer.master.clone()).chain(st.session.project.mixer.channels.iter().cloned()).collect();
    let master = st.session.project.mixer.master.id;
    if !all.iter().any(|c| c.id == st.ui.mixer) {
        st.ui.mixer = master;
    }
    let selected = all.iter().find(|c| c.id == st.ui.mixer).cloned().unwrap_or_else(|| all[0].clone());
    let avail = ui.available_size();
    let strips_w = (avail.x - PANEL_W - 10.0).max(160.0);
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(Vec2::new(strips_w, avail.y), egui::Layout::top_down(egui::Align::Min), |ui| {
            egui::ScrollArea::horizontal().id_salt("mixer.strips").auto_shrink([false, false]).show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let mut rects = Vec::with_capacity(all.len());
                    for (k, m) in all.iter().enumerate() {
                        rects.push((m.id, strip(ui, st, m, k, &selected, &all, &meters, avail.y - 22.0)));
                        if k == 0 {
                            ui.add_space(6.0);
                        }
                    }
                    cables(ui, st, &all, &rects, &meters);
                    ui.vertical(|ui| {
                        if ui.add(egui::Button::new(RichText::new("+\ninsert").small()).min_size(Vec2::new(46.0, 40.0))).on_hover_text("Add a mixer insert").clicked() {
                            st.ctx.run("dragon.add.mixer", serde_json::Value::Null);
                        }
                        if ui.add(egui::Button::new(RichText::new("+\nVCA").small()).min_size(Vec2::new(46.0, 40.0))).clicked() {
                            st.ctx.run("dragon.add.vca", serde_json::Value::Null);
                        }
                    });
                });
            });
        });
        ui.separator();
        ui.allocate_ui_with_layout(Vec2::new(PANEL_W, avail.y), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_width(PANEL_W);
            egui::ScrollArea::vertical().id_salt("mixer.panel").auto_shrink([false, false]).show(ui, |ui| {
                insert_panel(ui, st, &selected, &all, &meters);
            });
        });
    });
    if st.session.in_gesture() && !ui.ctx().input(|i| i.pointer.any_down()) && !ui.ctx().wants_keyboard_input() {
        st.session.end_gesture();
    }
}

#[allow(clippy::too_many_arguments)]
fn strip(ui: &mut Ui, st: &mut State, m: &MixerChannel, index: usize, sel: &MixerChannel, all: &[MixerChannel], meters: &dragon_engine::render::MeterValues, height: f32) -> egui::Rect {
    let id = m.id;
    let master = all[0].id;
    let is_master = id == master;
    let is_sel = sel.id == id;
    let vca = m.kind == ChannelKind::Vca;
    let width = if is_master { 96.0 } else { 88.0 };
    let fill = if is_sel { theme::PANEL_2 } else { theme::PANEL };
    let frame = egui::Frame::NONE.fill(fill).stroke(Stroke::new(1.0_f32, if is_sel { theme::ACCENT } else { theme::GRID })).corner_radius(3.0).inner_margin(egui::Margin::symmetric(3, 4));
    let resp = frame.show(ui, |ui| {
        ui.set_width(width);
        ui.set_min_height(height);
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            // Number, colour bar and name.
            let label = if is_master {
                "M".to_string()
            } else if vca {
                "VCA".into()
            } else {
                index.to_string()
            };
            let feeds_sel = !is_sel && route_of(m, sel.id, master) != Route::None;
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).small().monospace().color(if is_sel { theme::ACCENT_2 } else { theme::TEXT_DIM }));
                if feeds_sel {
                    ui.label(RichText::new("▶").small().color(theme::OK)).on_hover_text(format!("Routes into {}", sel.name));
                }
            });
            let (bar, _) = ui.allocate_exact_size(Vec2::new(width, 3.0), Sense::hover());
            ui.painter().rect_filled(bar, 1.0, theme::rgb(m.color));
            let name = ui.add(egui::Label::new(RichText::new(&m.name).small().strong()).truncate().sense(Sense::click()));
            if name.clicked() {
                st.ui.mixer = id;
            }
            if !vca {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let mut pan = m.pan;
                    let r = knob(ui, &mut pan, -1.0, 1.0, 0.0, "pan", widgets::pan_fmt);
                    widgets::apply(st, &r, "Pan", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.pan = pan));
                    r.context_menu(|ui| learn_menu(ui, st, Target::MixerPan(id)));
                    let mut w = m.width;
                    let r = knob(ui, &mut w, 0.0, 2.0, 1.0, "sep", widgets::pct);
                    widgets::apply(st, &r, "Stereo separation", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.width = w));
                });
            }
            // Meter + fader fill the strip.
            let fader_h = (height - if vca { 110.0 } else { 222.0 }).max(50.0);
            ui.horizontal(|ui| {
                ui.add_space(if is_master { 12.0 } else { 6.0 });
                let (l, rr) = meters.strips.get(&id).copied().unwrap_or((0.0, 0.0));
                widgets::meter(ui, l, rr, fader_h);
                let mut v = m.volume;
                let r = widgets::fader(ui, &mut v, 2.0, fader_h);
                widgets::apply(st, &r, "Volume", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.volume = v));
                r.context_menu(|ui| learn_menu(ui, st, Target::MixerVolume(id)));
            });
            let (l, rr) = meters.strips.get(&id).copied().unwrap_or((0.0, 0.0));
            let pk = dragon_engine::dsp::gain_to_db(l.max(rr).max(1e-6));
            ui.label(RichText::new(format!("{:.1} {}", dragon_engine::dsp::gain_to_db(m.volume), if pk < -99.0 { "-∞".into() } else { format!("{pk:.0}") })).size(9.0).monospace().color(theme::meter_color(l.max(rr))));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                ui.add_space(if is_master { 18.0 } else { 10.0 });
                let (lr, lresp) = ui.allocate_exact_size(Vec2::new(16.0, 16.0), Sense::click());
                ui.painter().circle_filled(lr.center(), 5.5, if m.muted { theme::GRID_BAR } else { theme::OK });
                ui.painter().circle_stroke(lr.center(), 6.5, Stroke::new(1.0_f32, theme::BG));
                if lresp.on_hover_text(if m.muted { "Muted – click to enable" } else { "Enabled – click to mute" }).clicked() {
                    st.session.edit("Mute", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.muted = !c.muted));
                }
                if !is_master && widgets::toggle(ui, m.solo, "S", theme::ACCENT_2).on_hover_text("Solo").clicked() {
                    st.session.edit("Solo", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.solo = !c.solo));
                }
            });
            // Route arrow (from the selected insert into this one).
            if !vca {
                route_arrow(ui, st, sel, m, master)
            } else {
                false
            }
        })
        .inner
    });
    if !resp.inner && resp.response.contains_pointer() && ui.input(|i| i.pointer.primary_clicked()) && !ui.ctx().is_being_dragged(egui::Id::new(("mixer.jack", id))) {
        st.ui.mixer = id;
    }
    resp.response.rect
}

/// Where a strip's cable plugs in: its output jack (right) and input jack (left), above the route arrow.
fn jacks(r: egui::Rect) -> (Pos2, Pos2) {
    let y = r.bottom() - 58.0;
    (Pos2::new(r.center().x + 14.0, y), Pos2::new(r.center().x - 14.0, y))
}

/// A cable sagging between two jacks, FL style (cubic bezier with a soft shadow).
fn cable(p: &egui::Painter, a: Pos2, b: Pos2, col: Color32, width: f32, floor: f32) -> [Pos2; 4] {
    let sag = ((b.x - a.x).abs() * 0.35 + 24.0).min(floor - a.y.max(b.y)).max(8.0);
    let pts = [a, a + Vec2::new((b.x - a.x) * 0.15, sag), b + Vec2::new((a.x - b.x) * 0.15, sag), b];
    let shadow = pts.map(|q| q + Vec2::new(1.5, 3.0));
    p.add(egui::epaint::CubicBezierShape::from_points_stroke(shadow, false, Color32::TRANSPARENT, Stroke::new(width + 1.5, Color32::from_black_alpha(110))));
    p.add(egui::epaint::CubicBezierShape::from_points_stroke(pts, false, Color32::TRANSPARENT, Stroke::new(width, col)));
    p.add(egui::epaint::CubicBezierShape::from_points_stroke(pts.map(|q| q + Vec2::new(0.0, -width * 0.3)), false, Color32::TRANSPARENT, Stroke::new((width * 0.3).max(0.6), Color32::from_white_alpha(45))));
    pts
}

fn bezier(pts: &[Pos2; 4], t: f32) -> Pos2 {
    let u = 1.0 - t;
    let v = pts[0].to_vec2() * (u * u * u) + pts[1].to_vec2() * (3.0 * u * u * t) + pts[2].to_vec2() * (3.0 * u * t * t) + pts[3].to_vec2() * (t * t * t);
    v.to_pos2()
}

/// Draws the routing as hanging cables (every route; the selected insert's in its own colour, the rest
/// dimmed), animates signal flowing through them while audio passes, and lets a cable be pulled from a
/// strip's output jack onto another strip to create a route.
fn cables(ui: &mut Ui, st: &mut State, all: &[MixerChannel], rects: &[(Id, egui::Rect)], meters: &dragon_engine::render::MeterValues) {
    let master = all[0].id;
    let sel = st.ui.mixer;
    let rect_of = |id: Id| rects.iter().find(|r| r.0 == id).map(|r| r.1);
    let floor = rects.iter().map(|r| r.1.bottom()).fold(0.0f32, f32::max) - 4.0;
    ui.data_mut(|d| d.insert_temp(egui::Id::new("mixer.strips"), rects.to_vec()));
    let p = ui.painter().clone();
    let time = ui.input(|i| i.time) as f32;
    let reduced = st.settings.reduced_motion;
    let mut animate = false;
    // Dim cables first, the selected insert's on top.
    let mut list: Vec<(Id, Id, bool, f32)> = Vec::new();
    for c in all.iter().filter(|c| c.kind == ChannelKind::Audio && c.id != master) {
        if let Some(t) = main_target(c, master) {
            list.push((c.id, t, true, 1.0));
        }
        for s in &c.sends {
            list.push((c.id, s.target, false, s.level));
        }
    }
    list.sort_by_key(|(a, b, _, _)| *a == sel || *b == sel);
    for (src, dst, main, level) in list {
        let (Some(ra), Some(rb)) = (rect_of(src), rect_of(dst)) else { continue };
        let a = jacks(ra).0;
        let b = jacks(rb).1;
        let hot = src == sel || dst == sel;
        let color = all.iter().find(|c| c.id == src).map(|c| theme::rgb(c.color)).unwrap_or(theme::ACCENT);
        let col = if hot { color } else { color.gamma_multiply(0.35) };
        let w = if main { 2.6 } else { 1.6 + 1.6 * level };
        let pts = cable(&p, a, b, col, if hot { w } else { w * 0.8 }, floor);
        // Signal dots ride along the cable, faster and brighter the louder the source.
        let (l, r) = meters.strips.get(&src).copied().unwrap_or((0.0, 0.0));
        let lvl = l.max(r);
        if lvl > 0.01 && !reduced {
            animate = true;
            for k in 0..3 {
                let t = ((time * (0.6 + lvl) + k as f32 / 3.0) % 1.0).clamp(0.0, 1.0);
                p.circle_filled(bezier(&pts, t), (1.5 + 2.0 * lvl.min(1.0)) * if hot { 1.0 } else { 0.7 }, Color32::WHITE.gamma_multiply(0.5 + 0.5 * lvl.min(1.0)));
            }
        }
        for (q, out) in [(a, true), (b, false)] {
            p.circle_filled(q, 4.0, theme::BG);
            p.circle_stroke(q, 4.0, Stroke::new(1.5_f32, if out { col } else { col.gamma_multiply(0.8) }));
        }
    }
    // Jacks: pull a new cable from any strip's output.
    for &(id, r) in rects {
        let c = all.iter().find(|c| c.id == id);
        if id == master || c.map(|c| c.kind != ChannelKind::Audio).unwrap_or(true) {
            continue;
        }
        let a = jacks(r).0;
        let jid = egui::Id::new(("mixer.jack", id));
        let resp = ui.interact(egui::Rect::from_center_size(a, Vec2::splat(14.0)), jid, Sense::drag());
        let hov = resp.hovered() || resp.dragged();
        p.circle_filled(a, if hov { 5.5 } else { 3.0 }, if hov { theme::ACCENT_2 } else { theme::TEXT_DIM });
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        let resp = resp.on_hover_text("Drag a cable onto another insert to route this one into it");
        if resp.dragged() {
            if let Some(ptr) = ui.ctx().pointer_latest_pos() {
                let target = rects.iter().find(|(t, rr)| *t != id && rr.contains(ptr)).map(|x| x.0);
                let col = if target.is_some() { theme::OK } else { theme::ACCENT_2 };
                cable(&p, a, ptr, col, 2.5, floor.max(ptr.y + 30.0));
                if let Some(rr) = target.and_then(rect_of) {
                    p.rect_stroke(rr, 3.0, Stroke::new(2.0_f32, theme::OK), egui::StrokeKind::Inside);
                }
                animate = true;
            }
        }
        if resp.drag_stopped() {
            if let Some(ptr) = ui.ctx().pointer_latest_pos() {
                if let Some((t, _)) = rects.iter().find(|(t, rr)| *t != id && rr.contains(ptr)) {
                    let target = all.iter().find(|c| c.id == *t);
                    if target.map(|c| c.kind == ChannelKind::Audio).unwrap_or(false) && c.map(|c| route_of(c, *t, master) == Route::None).unwrap_or(false) {
                        toggle_route(st, id, *t);
                        st.ui.mixer = id;
                    }
                }
            }
        }
    }
    if animate {
        ui.ctx().request_repaint();
    }
}

/// Returns `true` when the pointer is on the arrow (so the click does not select the strip).
fn route_arrow(ui: &mut Ui, st: &mut State, sel: &MixerChannel, target: &MixerChannel, master: Id) -> bool {
    let tid = target.id;
    let possible = sel.id != tid && sel.kind == ChannelKind::Audio && tid != sel.id && !(sel.id == master);
    let (rect, r) = ui.allocate_exact_size(Vec2::new(30.0, 16.0), if possible { Sense::click() } else { Sense::hover() });
    let route = route_of(sel, tid, master);
    let col = match route {
        Route::None => theme::GRID_BAR,
        Route::Main => theme::ACCENT_2,
        Route::Send(_) => theme::PLAYHEAD,
    };
    if possible {
        let c = rect.center();
        let tri = vec![Pos2::new(c.x - 7.0, c.y - 5.0), Pos2::new(c.x + 7.0, c.y - 5.0), Pos2::new(c.x, c.y + 6.0)];
        let fill = if route == Route::None {
            if r.hovered() {
                theme::TEXT_DIM
            } else {
                Color32::TRANSPARENT
            }
        } else {
            col
        };
        ui.painter().add(egui::Shape::convex_polygon(tri, fill, Stroke::new(1.0_f32, col)));
    }
    if !possible {
        return false;
    }
    let on_arrow = r.hovered();
    let r = r.on_hover_text(match route {
        Route::None => format!("Route {} ➡ {}", sel.name, target.name),
        Route::Main => format!("{} ➡ {} (main output) – click to remove", sel.name, target.name),
        Route::Send(l) => format!("{} ➡ {} at {:.1} dB – click to remove", sel.name, target.name, dragon_engine::dsp::gain_to_db(l)),
    });
    if r.clicked() {
        toggle_route(st, sel.id, tid);
    }
    r.context_menu(|ui| {
        if ui.button("Route to this track only").clicked() {
            route_only(st, sel.id, tid);
            ui.close_menu();
        }
        if let Route::Send(_) = route {
            let mut pre = sel.sends.iter().find(|s| s.target == tid).map(|s| s.pre_fader).unwrap_or(false);
            if ui.checkbox(&mut pre, "Pre-fader").changed() {
                let sid = sel.id;
                st.session.edit("Send pre/post", |p| {
                    if let Some(x) = p.mixer.get_mut(sid).and_then(|c| c.sends.iter_mut().find(|x| x.target == tid)) {
                        x.pre_fader = pre;
                    }
                });
            }
        }
    });
    // Level knob for sends.
    if let Route::Send(level) = route {
        let mut lvl = level;
        let sid = sel.id;
        let r = knob(ui, &mut lvl, 0.0, 1.0, 1.0, "send", widgets::db_fmt);
        let knob_hovered = r.hovered();
        widgets::apply(st, &r, "Send level", |p| {
            if let Some(x) = p.mixer.get_mut(sid).and_then(|c| c.sends.iter_mut().find(|x| x.target == tid)) {
                x.level = lvl;
            }
        });
        return on_arrow || knob_hovered;
    }
    on_arrow
}

/// Colour of an effect category (slot stripes, editor accents).
pub fn category_color(cat: &str) -> Color32 {
    match cat {
        "Dynamics" => Color32::from_rgb(0xf0, 0x8a, 0x4b),
        "EQ" | "Filter" => Color32::from_rgb(0x4b, 0xc0, 0xf0),
        "Delay" | "Space" => Color32::from_rgb(0x9b, 0x7b, 0xf5),
        "Modulation" => Color32::from_rgb(0x5d, 0xe0, 0xa8),
        "Distortion" => Color32::from_rgb(0xf0, 0x52, 0x6a),
        "Mastering" => Color32::from_rgb(0xf5, 0xc8, 0x42),
        "Pitch" => Color32::from_rgb(0xe0, 0x6e, 0xd8),
        "Utility" => Color32::from_rgb(0x9a, 0xa5, 0xb5),
        "Vocal" => Color32::from_rgb(0xff, 0x7a, 0xa8),
        "Channel strip" => Color32::from_rgb(0xd9, 0x9a, 0x4e),
        "Drums" => Color32::from_rgb(0xff, 0x5a, 0x36),
        _ => theme::ACCENT,
    }
}

fn effect_name(ins: &Insert) -> String {
    ins.plugin.as_ref().map(|p| p.name.clone()).or_else(|| dragon_engine::effects::find(&ins.effect).map(|d| d.name.to_string())).unwrap_or_else(|| ins.effect.clone())
}

/// The selected insert: effect slots, routing and options.
fn insert_panel(ui: &mut Ui, st: &mut State, m: &MixerChannel, all: &[MixerChannel], meters: &dragon_engine::render::MeterValues) {
    let id = m.id;
    let master = all[0].id;
    let is_master = id == master;
    let index = all.iter().position(|c| c.id == id).unwrap_or(0);
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(Vec2::new(5.0, 18.0), Sense::hover());
        ui.painter().rect_filled(r, 1.0, theme::rgb(m.color));
        ui.label(RichText::new(if is_master { "Master".to_string() } else { format!("Insert {index}") }).strong());
        if !is_master {
            let mut name = m.name.clone();
            if ui.add(egui::TextEdit::singleline(&mut name).desired_width(120.0)).changed() {
                st.session.gesture("Rename insert", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.name = name.clone()));
            }
            let mut col = [m.color.0, m.color.1, m.color.2];
            if ui.color_edit_button_srgb(&mut col).changed() {
                st.session.gesture("Colour", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.color = Rgb(col[0], col[1], col[2])));
            }
        }
    });
    if m.kind == ChannelKind::Vca {
        ui.label(RichText::new("A VCA carries no audio: assign inserts to it under Options and its fader scales theirs.").small().color(theme::TEXT_DIM));
        options(ui, st, m, all);
        return;
    }
    ui.add_space(4.0);
    ui.label(RichText::new("EFFECT SLOTS").small().color(theme::TEXT_DIM));
    let mut drop: Option<(usize, usize)> = None;
    for (k, ins) in m.inserts.iter().enumerate() {
        let iid = ins.id;
        let cat = dragon_engine::effects::find(&ins.effect).map(|d| d.category).unwrap_or("Plugin");
        let ccol = category_color(cat);
        let row = egui::Frame::NONE
            .fill(if ins.enabled { theme::BG } else { theme::BG.gamma_multiply(0.6) })
            .stroke(Stroke::new(1.0_f32, ccol.gamma_multiply(0.35)))
            .corner_radius(5.0)
            .inner_margin(egui::Margin { left: 8, right: 4, top: 2, bottom: 2 })
            .show(ui, |ui| {
                let full = ui.max_rect();
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(full.min - Vec2::new(8.0, 2.0), Vec2::new(4.0, SLOT_H + 4.0)),
                    egui::CornerRadius { nw: 5, sw: 5, ne: 0, se: 0 },
                    if ins.enabled { ccol } else { theme::GRID_BAR },
                );
                ui.allocate_ui_with_layout(Vec2::new(ui.available_width(), SLOT_H), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 5.0;
                    let (hr, handle) = ui.allocate_exact_size(Vec2::new(10.0, SLOT_H), Sense::drag());
                    for dy in [-5.0, 0.0, 5.0] {
                        ui.painter().line_segment(
                            [Pos2::new(hr.left() + 1.0, hr.center().y + dy), Pos2::new(hr.right() - 1.0, hr.center().y + dy)],
                            Stroke::new(1.5_f32, if handle.hovered() { theme::ACCENT_2 } else { theme::TEXT_DIM }),
                        );
                    }
                    let handle = handle.on_hover_text("Drag to reorder");
                    handle.dnd_set_drag_payload(SlotDrag { mixer: id, index: k });
                    ui.label(RichText::new(format!("{}", k + 1)).monospace().color(theme::TEXT_DIM));
                    let led = if ins.enabled { theme::OK } else { theme::GRID_BAR };
                    let (lr, lresp) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
                    if ins.enabled {
                        ui.painter().circle_filled(lr.center(), 8.0, led.gamma_multiply(0.25));
                    }
                    ui.painter().circle_filled(lr.center(), 5.5, led);
                    ui.painter().circle_stroke(lr.center(), 6.5, Stroke::new(1.0_f32, theme::BG));
                    if lresp.on_hover_text(if ins.enabled { "On – click to bypass" } else { "Bypassed – click to enable" }).clicked() {
                        st.session.edit("Bypass effect", |p| {
                            if let Some(i) = p.mixer.get_mut(id).and_then(|c| c.inserts.iter_mut().find(|i| i.id == iid)) {
                                i.enabled = !i.enabled;
                            }
                        });
                    }
                    let gr = meters.gain_reduction.get(&iid).copied().unwrap_or(0.0);
                    let label = effect_name(ins);
                    let name_w = (ui.available_width() - 40.0).max(80.0);
                    let (br, b) = ui.allocate_exact_size(Vec2::new(name_w, SLOT_H - 4.0), Sense::click());
                    let pnt = ui.painter();
                    pnt.rect_filled(br, 4.0, if b.hovered() { theme::PANEL_2 } else { theme::PANEL });
                    pnt.with_clip_rect(br.shrink(2.0)).text(
                        br.left_center() + Vec2::new(7.0, -4.0),
                        egui::Align2::LEFT_CENTER,
                        &label,
                        egui::FontId::proportional(13.0),
                        if ins.enabled { theme::TEXT } else { theme::TEXT_DIM },
                    );
                    pnt.text(br.left_center() + Vec2::new(7.0, 8.0), egui::Align2::LEFT_CENTER, cat, egui::FontId::proportional(9.0), ccol.gamma_multiply(0.9));
                    if gr > 0.1 {
                        // Gain-reduction bar along the slot's right edge.
                        let gw = (gr / 24.0).min(1.0) * 50.0;
                        let gb = egui::Rect::from_min_max(Pos2::new(br.right() - 4.0 - gw, br.bottom() - 6.0), Pos2::new(br.right() - 4.0, br.bottom() - 3.0));
                        pnt.rect_filled(gb, 1.0, theme::WARN);
                        pnt.text(Pos2::new(br.right() - 6.0, br.top() + 8.0), egui::Align2::RIGHT_CENTER, format!("−{gr:.1}"), egui::FontId::monospace(9.0), theme::WARN);
                    }
                    let b = b.on_hover_text("Click: open the editor · right-click: replace, sidechain, remove");
                    if b.clicked() && !st.ui.effect_windows.contains(&(id, iid)) {
                        st.ui.effect_windows.push((id, iid));
                    }
                    b.context_menu(|ui| slot_menu(ui, st, id, iid, k, m, all));
                    let mut mix = ins.mix;
                    let r = widgets::mini_knob(ui, &mut mix, 0.0, 1.0, 1.0, ccol).on_hover_text(format!("Mix (dry/wet) {:.0}% – double-click resets", mix * 100.0));
                    widgets::apply(st, &r, "Effect mix", |p| {
                        if let Some(i) = p.mixer.get_mut(id).and_then(|c| c.inserts.iter_mut().find(|i| i.id == iid)) {
                            i.mix = mix;
                        }
                    });
                });
            });
        if let Some(payload) = row.response.dnd_release_payload::<SlotDrag>() {
            if payload.mixer == id && payload.index != k {
                drop = Some((payload.index, k));
            }
        }
        if row.response.dnd_hover_payload::<SlotDrag>().is_some() {
            ui.painter().line_segment([row.response.rect.left_top(), row.response.rect.right_top()], Stroke::new(2.0_f32, theme::ACCENT_2));
        }
    }
    if let Some((from, to)) = drop {
        st.session.edit("Reorder effects", |p| {
            if let Some(c) = p.mixer.get_mut(id) {
                let e = c.inserts.remove(from);
                c.inserts.insert(to.min(c.inserts.len()), e);
            }
        });
    }
    if m.inserts.len() < MAX_INSERTS {
        egui::Frame::NONE.stroke(Stroke::new(1.0_f32, theme::GRID)).corner_radius(5.0).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
            ui.allocate_ui_with_layout(Vec2::new(ui.available_width(), SLOT_H - 4.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.label(RichText::new(format!("{}", m.inserts.len() + 1)).small().monospace().color(theme::TEXT_DIM));
                let w = ui.available_width() - 16.0;
                add_effect_menu_sized(ui, st, id, "+  Add an effect to this empty slot", None, Vec2::new(w, SLOT_H - 6.0));
            });
        });
    } else {
        ui.label(RichText::new(format!("All {MAX_INSERTS} slots used")).small().color(theme::TEXT_DIM));
    }

    // Routing.
    ui.add_space(8.0);
    ui.label(RichText::new("ROUTING").small().color(theme::TEXT_DIM));
    if is_master {
        ui.label(RichText::new("The master goes to the audio output.").small().color(theme::TEXT_DIM));
    } else {
        let name_of = |t: Id| all.iter().find(|c| c.id == t).map(|c| if c.id == master { "Master".to_string() } else { c.name.clone() }).unwrap_or("?".into());
        ui.horizontal(|ui| {
            ui.label(RichText::new("Main out").small());
            let cur = main_target(m, master).map(name_of).unwrap_or_else(|| "(none)".into());
            egui::ComboBox::from_id_salt(("mix.main", id)).selected_text(RichText::new(cur).small()).width(150.0).show_ui(ui, |ui| {
                if ui.selectable_label(!m.main_out, "(none – sends only)").clicked() {
                    st.session.edit("Route output", |p| {
                        p.mixer.get_mut(id).iter_mut().for_each(|c| {
                            c.main_out = false;
                            c.output = None;
                        })
                    });
                }
                for c in all.iter().filter(|c| c.id != id && c.kind == ChannelKind::Audio) {
                    let t = c.id;
                    if ui.selectable_label(main_target(m, master) == Some(t), name_of(t)).clicked() {
                        st.session.edit("Route output", |p| {
                            p.mixer.get_mut(id).iter_mut().for_each(|c| {
                                c.main_out = true;
                                c.output = if t == master { None } else { Some(t) };
                                c.sends.retain(|s| s.target != t);
                            })
                        });
                    }
                }
            });
        });
        for s in &m.sends {
            let target = s.target;
            ui.horizontal(|ui| {
                let mut lvl = s.level;
                let r = ui.add(egui::Slider::new(&mut lvl, 0.0..=1.0).show_value(false).text(RichText::new(format!("➡ {}", name_of(target))).small()));
                widgets::apply(st, &r, "Send level", |p| {
                    if let Some(x) = p.mixer.get_mut(id).and_then(|c| c.sends.iter_mut().find(|x| x.target == target)) {
                        x.level = lvl;
                    }
                });
                if widgets::toggle(ui, s.pre_fader, "pre", theme::ACCENT_2).on_hover_text("Pre-fader send").clicked() {
                    st.session.edit("Send pre/post", |p| {
                        if let Some(x) = p.mixer.get_mut(id).and_then(|c| c.sends.iter_mut().find(|x| x.target == target)) {
                            x.pre_fader = !x.pre_fader;
                        }
                    });
                }
                if ui.small_button("x").on_hover_text("Remove route").clicked() {
                    st.session.edit("Remove route", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.sends.retain(|x| x.target != target)));
                }
            });
        }
        ui.menu_button(RichText::new("+ route to…").small(), |ui| {
            for c in all.iter().filter(|c| c.id != id && c.kind == ChannelKind::Audio && route_of(m, c.id, master) == Route::None) {
                if ui.button(name_of(c.id)).clicked() {
                    toggle_route(st, id, c.id);
                    ui.close_menu();
                }
            }
        });
        let inputs: Vec<String> = all.iter().filter(|c| c.id != id && route_of(c, id, master) != Route::None).map(|c| c.name.clone()).collect();
        if !inputs.is_empty() {
            ui.label(RichText::new(format!("Receives from: {}", inputs.join(", "))).small().color(theme::TEXT_DIM));
        }
        let sources: Vec<String> = st.session.project.channels.iter().filter(|c| c.mixer == id).map(|c| c.name.clone()).chain(st.session.project.tracks.iter().filter(|t| t.mixer == id).map(|t| t.name.clone())).collect();
        if !sources.is_empty() {
            ui.label(RichText::new(format!("Channels / tracks: {}", sources.join(", "))).small().color(theme::TEXT_DIM));
        }
    }
    options(ui, st, m, all);
}

/// Drag payload for reordering effect slots.
#[derive(Clone, Copy)]
struct SlotDrag {
    mixer: Id,
    index: usize,
}

fn slot_menu(ui: &mut Ui, st: &mut State, id: Id, iid: Id, k: usize, m: &MixerChannel, all: &[MixerChannel]) {
    if ui.button("Open editor").clicked() {
        if !st.ui.effect_windows.contains(&(id, iid)) {
            st.ui.effect_windows.push((id, iid));
        }
        ui.close_menu();
    }
    add_effect_menu(ui, st, id, "Replace with", Some(iid));
    let cur_sc = m.inserts[k].sidechain;
    ui.menu_button("Sidechain from", |ui| {
        if ui.selectable_label(cur_sc.is_none(), "(none)").clicked() {
            set_sidechain(st, id, iid, None);
            ui.close_menu();
        }
        for c in all.iter().filter(|c| c.id != id && c.kind == ChannelKind::Audio) {
            if ui.selectable_label(cur_sc == Some(c.id), &c.name).clicked() {
                set_sidechain(st, id, iid, Some(c.id));
                ui.close_menu();
            }
        }
    });
    ui.separator();
    if k > 0 && ui.button("Move up").clicked() {
        st.session.edit("Reorder effects", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.inserts.swap(k, k - 1)));
        ui.close_menu();
    }
    if k + 1 < m.inserts.len() && ui.button("Move down").clicked() {
        st.session.edit("Reorder effects", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.inserts.swap(k, k + 1)));
        ui.close_menu();
    }
    if ui.button("Remove").clicked() {
        st.session.edit("Remove effect", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.inserts.retain(|i| i.id != iid)));
        st.ui.effect_windows.retain(|w| *w != (id, iid));
        ui.close_menu();
    }
}

fn set_sidechain(st: &mut State, id: Id, iid: Id, src: Option<Id>) {
    st.session.edit("Sidechain", |p| {
        if let Some(i) = p.mixer.get_mut(id).and_then(|c| c.inserts.iter_mut().find(|i| i.id == iid)) {
            i.sidechain = src;
        }
    });
}

fn options(ui: &mut Ui, st: &mut State, m: &MixerChannel, all: &[MixerChannel]) {
    let id = m.id;
    let master = all[0].id;
    let is_master = id == master;
    let vca = m.kind == ChannelKind::Vca;
    ui.add_space(8.0);
    ui.label(RichText::new("OPTIONS").small().color(theme::TEXT_DIM));
    ui.horizontal_wrapped(|ui| {
        if !vca {
            if widgets::toggle(ui, m.phase_invert, "Ø", theme::ACCENT_2).on_hover_text("Invert polarity").clicked() {
                st.session.edit("Phase", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.phase_invert = !c.phase_invert));
            }
            if widgets::toggle(ui, m.swap_lr, "🔀", theme::ACCENT_2).on_hover_text("Swap left / right").clicked() {
                st.session.edit("Swap L/R", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.swap_lr = !c.swap_lr));
            }
        }
        if !is_master && widgets::toggle(ui, m.solo_safe, "solo safe", theme::PLAYHEAD).on_hover_text("Stays audible when others solo (e.g. reverb returns)").clicked() {
            st.session.edit("Solo safe", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.solo_safe = !c.solo_safe));
        }
    });
    let vcas: Vec<&MixerChannel> = all.iter().filter(|c| c.kind == ChannelKind::Vca).collect();
    if !is_master && !vca && !vcas.is_empty() {
        let cur = m.vca.and_then(|v| all.iter().find(|c| c.id == v)).map(|c| c.name.clone()).unwrap_or("no VCA".into());
        egui::ComboBox::from_id_salt(("mix.vca", id)).selected_text(RichText::new(cur).small()).show_ui(ui, |ui| {
            if ui.selectable_label(m.vca.is_none(), "no VCA").clicked() {
                st.session.edit("VCA", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.vca = None));
            }
            for v in vcas {
                let vid = v.id;
                if ui.selectable_label(m.vca == Some(vid), &v.name).clicked() {
                    st.session.edit("VCA", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.vca = Some(vid)));
                }
            }
        });
    }
    if let (Some(_), false, false) = (st.session.project.surround, is_master, vca) {
        if main_target(m, master) == Some(master) {
            ui.label(RichText::new("SURROUND").small().color(theme::TEXT_DIM));
            surround_pad(ui, st, id, m.surround, 180.0);
        }
    }
    if !is_master && ui.button(RichText::new("🗑 Delete insert").small()).clicked() {
        st.session.edit("Delete mixer channel", |p| {
            p.mixer.channels.retain(|c| c.id != id);
            for c in p.mixer.channels.iter_mut() {
                c.sends.retain(|s| s.target != id);
                if c.output == Some(id) {
                    c.output = None;
                }
                if c.vca == Some(id) {
                    c.vca = None;
                }
                for i in c.inserts.iter_mut() {
                    if i.sidechain == Some(id) {
                        i.sidechain = None;
                    }
                }
            }
            let first = p.mixer.master.id;
            for t in p.tracks.iter_mut() {
                if t.mixer == id {
                    t.mixer = first;
                }
            }
            for c in p.channels.iter_mut() {
                if c.mixer == id {
                    c.mixer = first;
                }
            }
        });
    }
}

fn surround_pad(ui: &mut Ui, st: &mut State, id: Id, sp: SurroundPan, width: f32) {
    let size = width - 12.0;
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::click_and_drag());
    let p = ui.painter();
    p.rect_filled(rect, 4.0, theme::BG);
    p.circle_stroke(rect.center(), size * 0.45, egui::Stroke::new(1.0_f32, theme::GRID));
    // Speakers.
    if let Some(f) = st.session.project.surround {
        for (a, n) in dragon_engine::surround::layout(f).iter().zip(dragon_engine::surround::channel_names(f)) {
            if let Some(a) = a {
                let rad = a.to_radians();
                let pos = rect.center() + egui::vec2(rad.sin(), -rad.cos()) * size * 0.45;
                p.text(pos, egui::Align2::CENTER_CENTER, *n, egui::FontId::proportional(8.0), theme::TEXT_DIM);
            }
        }
    }
    let to_pos = |x: f32, y: f32| rect.center() + egui::vec2(x, -y) * size * 0.42;
    p.circle_filled(to_pos(sp.x, sp.y), 4.0 + sp.z * 3.0, theme::ACCENT_2);
    if let Some(pos) = r.interact_pointer_pos().filter(|_| r.dragged() || r.clicked()) {
        let d = (pos - rect.center()) / (size * 0.42);
        let (x, y) = (d.x.clamp(-1.0, 1.0), (-d.y).clamp(-1.0, 1.0));
        st.session.gesture("Surround pan", |p| {
            p.mixer.get_mut(id).iter_mut().for_each(|c| {
                c.surround.x = x;
                c.surround.y = y;
            })
        });
    }
    if r.double_clicked() {
        st.session.edit("Surround pan", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.surround = SurroundPan::default()));
    }
    ui.horizontal(|ui| {
        let mut lfe = sp.lfe;
        let r = ui.add(egui::Slider::new(&mut lfe, 0.0..=1.0).text(RichText::new("LFE").small()).show_value(false));
        widgets::apply(st, &r, "LFE send", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.surround.lfe = lfe));
    });
    let mut spread = sp.spread;
    let r = ui.add(egui::Slider::new(&mut spread, 0.0..=180.0).text(RichText::new("spread").small()).show_value(false));
    widgets::apply(st, &r, "Surround spread", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.surround.spread = spread));
    // Height: used by the 7.1.4 bed and by Atmos (ADM) objects.
    let mut z = sp.z;
    let r = ui.add(egui::Slider::new(&mut z, 0.0..=1.0).text(RichText::new("height").small()).show_value(false));
    widgets::apply(st, &r, "Surround height", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.surround.z = z));
}

fn learn_menu(ui: &mut Ui, st: &mut State, t: Target) {
    if ui.button("MIDI learn").clicked() {
        st.ui.midi_learn = Some(t.clone());
        ui.close_menu();
    }
    if ui.button("Create automation lane").clicked() {
        crate::views::automation::add_lane(st, t);
        ui.close_menu();
    }
}

/// A menu of every effect: adds a new slot, or with `replace` swaps the effect in an existing slot (keeping
/// its position, bypass and mix).
pub fn add_effect_menu(ui: &mut Ui, st: &mut State, mixer: Id, label: &str, replace: Option<Id>) {
    add_effect_menu_sized(ui, st, mixer, label, replace, Vec2::ZERO);
}

/// [`add_effect_menu`] with a button of at least `size` (the big empty effect slot).
pub fn add_effect_menu_sized(ui: &mut Ui, st: &mut State, mixer: Id, label: &str, replace: Option<Id>, size: Vec2) {
    let put = |st: &mut State, effect: String, params: std::collections::BTreeMap<String, f32>, plugin: Option<PluginRef>| {
        let iid = st.session.edit(if replace.is_some() { "Replace effect" } else { "Add effect" }, |p| {
            let iid = p.next_id();
            if let Some(c) = p.mixer.get_mut(mixer) {
                let new = Insert { id: iid, effect, enabled: true, mix: 1.0, params, sidechain: None, plugin, preset: String::new(), mods: Vec::new() };
                match replace.and_then(|r| c.inserts.iter().position(|i| i.id == r)) {
                    Some(k) => {
                        let old = &c.inserts[k];
                        c.inserts[k] = Insert { enabled: old.enabled, mix: old.mix, sidechain: old.sidechain, ..new };
                    }
                    None => c.inserts.push(new),
                }
            }
            iid
        });
        if let Some(r) = replace {
            st.ui.effect_windows.retain(|w| *w != (mixer, r));
        }
        st.ui.effect_windows.push((mixer, iid));
    };
    let text = if size == Vec2::ZERO { RichText::new(label).small().color(theme::ACCENT_2) } else { RichText::new(label).color(theme::ACCENT_2) };
    let button = egui::Button::new(text).min_size(size).fill(if size == Vec2::ZERO { theme::PANEL_2 } else { Color32::TRANSPARENT });
    egui::menu::menu_custom_button(ui, button, |ui| {
        let mut cats: Vec<&str> = dragon_engine::effects::registry().iter().map(|d| d.category).collect();
        cats.sort();
        cats.dedup();
        for cat in cats {
            ui.menu_button(cat, |ui| {
                for d in dragon_engine::effects::registry().iter().filter(|d| d.category == cat) {
                    if ui.button(d.name).on_hover_text(d.description).clicked() {
                        put(st, d.id.to_string(), d.defaults(), None);
                        ui.close_menu();
                    }
                }
            });
        }
        if replace.is_none() && ui.button("Browse the effect gallery…").on_hover_text("See every stock effect in action").clicked() {
            st.ui.mixer = mixer;
            st.ui.fx_gallery = true;
            ui.close_menu();
        }
        ui.separator();
        let fx: Vec<_> = st.settings.plugins.iter().filter(|p| !p.instrument).cloned().collect();
        ui.menu_button("Plugins", |ui| {
            if fx.is_empty() {
                ui.label(RichText::new("No effect plugins – scan in View › Plugin manager").small());
            }
            for info in fx {
                if ui.button(format!("{} · {}", info.name, info.vendor)).clicked() {
                    put(st, "plugin".into(), Default::default(), Some(info.to_ref()));
                    ui.close_menu();
                }
            }
        });
    });
}
