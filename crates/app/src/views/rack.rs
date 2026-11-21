//! Channel rack: instrument channels with the step sequencer for the selected pattern, swing, pattern
//! length, channel volume / pan / mute / solo, mixer routing and per-step velocity.

use super::widgets::{self, knob};
use crate::state::State;
use crate::theme;
use dragon_engine::device::Command as Cmd;
use dragon_engine::model::*;
use dragon_engine::render::PlayMode;
use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use std::path::PathBuf;

pub fn show(ui: &mut Ui, st: &mut State) {
    if ui.rect_contains_pointer(ui.max_rect()) {
        st.ui.editor = crate::state::Editor::Rack;
    }
    let pat = st.ui.pattern;
    let Some(pattern) = st.session.project.pattern(pat).cloned() else {
        ui.label("No pattern – add one with Add › Pattern.");
        return;
    };
    // ---- header
    ui.horizontal(|ui| {
        let mut name = pattern.name.clone();
        if ui.add(egui::TextEdit::singleline(&mut name).desired_width(110.0)).changed() {
            st.session.gesture("Rename pattern", |p| p.pattern_mut(pat).iter_mut().for_each(|x| x.name = name.clone()));
        }
        if ui.button("▶ Pattern").on_hover_text("Loop this pattern").clicked() {
            st.ui.pattern_mode = true;
            st.audio.send(Cmd::Stop);
            st.audio.send(Cmd::Play(PlayMode::Pattern(pat)));
        }
        ui.separator();
        ui.label("Steps");
        let mut steps = pattern.steps;
        let r = ui.add(egui::DragValue::new(&mut steps).range(1..=64));
        widgets::apply(st, &r, "Pattern length", |p| p.pattern_mut(pat).iter_mut().for_each(|x| x.steps = steps));
        let mut swing = pattern.swing;
        let r = knob(ui, &mut swing, 0.0, 1.0, 0.0, "swing", widgets::pct);
        widgets::apply(st, &r, "Swing", |p| p.pattern_mut(pat).iter_mut().for_each(|x| x.swing = swing));
        ui.separator();
        ui.menu_button("+ Channel", |ui| {
            for (id, label) in [("add.sampler", "Sampler…"), ("add.synth", "Synth"), ("add.wavetable", "Dragon Wave (wavetable)"), ("add.guitar", "Dragon Strings (guitar)"), ("add.drums", "Drum pads")] {
                if ui.button(label).clicked() {
                    st.ctx.run(&format!("dragon.{id}"), serde_json::Value::Null);
                    ui.close_menu();
                }
            }
            ui.separator();
            let plugins: Vec<_> = st.settings.plugins.iter().filter(|p| p.instrument).cloned().collect();
            if plugins.is_empty() {
                ui.label(RichText::new("No instrument plugins scanned").small());
            }
            for info in plugins {
                if ui.button(format!("{} ({:?})", info.name, info.format)).clicked() {
                    let r = info.to_ref();
                    let name = info.name.clone();
                    st.ui.channel = Some(st.session.edit("Add plugin instrument", |p| p.add_channel(&name, Instrument::Plugin(r))));
                    ui.close_menu();
                }
            }
        });
        ui.menu_button("Pattern ⏷", |ui| pattern_menu(ui, st, pat, None));
        // Fill the selected channels' steps (every step, every 2nd, 4th …).
        let targets: Vec<Id> = if st.ui.rack_sel.is_empty() { st.ui.channel.into_iter().collect() } else { st.ui.rack_sel.clone() };
        ui.add_enabled_ui(!targets.is_empty(), |ui| {
            ui.menu_button("Fill ⏷", |ui| fill_menu(ui, st, &targets)).response.on_hover_text("Fill the selected channel's steps: every step, every 2nd, every 4th … (also on a channel's right-click menu)");
        });
        if ui.button("Clear").clicked() {
            st.session.edit("Clear pattern", |p| p.pattern_mut(pat).iter_mut().for_each(|x| x.channels.clear()));
        }
    });
    if st.session.in_gesture() && !ui.ctx().input(|i| i.pointer.any_down()) && !ui.ctx().wants_keyboard_input() {
        st.session.end_gesture();
    }
    ui.separator();

    // ---- rows
    let channels: Vec<RackChannel> = st.session.project.channels.clone();
    let mixers: Vec<(Id, String)> = st.session.project.mixer.channels.iter().map(|m| (m.id, m.name.clone())).collect();
    let steps = pattern.steps as usize;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let mut rows: Vec<(Id, Rect)> = Vec::new();
        if let Some(g) = st.ui.rack_group.clone() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Group: {g}")).strong());
                if ui.small_button("Show all").on_hover_text("PgUp / PgDn cycle the groups").clicked() {
                    st.ui.rack_group = None;
                }
            });
        }
        let group = st.ui.rack_group.clone();
        for c in channels.iter().filter(|c| group.as_ref().is_none_or(|g| &c.group == g)) {
            let cid = c.id;
            let selected = st.ui.channel == Some(cid) || st.ui.rack_sel.contains(&cid);
            if c.zipped {
                // Zipped (FL Alt+Z): one thin row with the name; click to select, Alt+U unzips.
                let r = ui.add(
                    egui::Button::new(RichText::new(format!("▶ {}", c.name)).small())
                        .fill(theme::rgb(c.color).gamma_multiply(if selected { 0.8 } else { 0.25 }))
                        .min_size(Vec2::new(ui.available_width().min(260.0), 14.0)),
                );
                if r.clicked() {
                    if ui.input(|i| i.modifiers.ctrl) {
                        st.ui.rack_sel.push(cid);
                    } else {
                        st.ui.rack_sel = vec![cid];
                        st.ui.channel = Some(cid);
                    }
                }
                r.on_hover_text("Zipped – Alt+U unzips");
                continue;
            }
            let row = ui.horizontal(|ui| {
                ui.set_height(30.0);
                if widgets::toggle(ui, !c.muted, "⚫", theme::OK).on_hover_text("Mute").clicked() {
                    st.session.edit("Mute channel", |p| p.channel_mut(cid).iter_mut().for_each(|c| c.muted = !c.muted));
                }
                let mut pan = c.pan;
                let r = mini_knob(ui, &mut pan, -1.0, 1.0, "pan");
                widgets::apply(st, &r, "Channel pan", |p| p.channel_mut(cid).iter_mut().for_each(|c| c.pan = pan));
                let mut vol = c.volume;
                let r = mini_knob(ui, &mut vol, 0.0, 1.25, "vol");
                widgets::apply(st, &r, "Channel volume", |p| p.channel_mut(cid).iter_mut().for_each(|c| c.volume = vol));
                let mx = mixers.iter().find(|m| m.0 == c.mixer).map(|m| m.1.clone()).unwrap_or("–".into());
                if st.ui.rack_mixer_sel {
                    ui.add_sized([62.0, 20.0], egui::Label::new(RichText::new(format!("➡{}", short(&mx, 7))).small()).truncate()).on_hover_text("Mixer channel (right-click the name for routing)");
                    ui.menu_button(RichText::new("…").small(), |ui| {
                        for (id, n) in &mixers {
                            if ui.selectable_label(*id == c.mixer, n).clicked() {
                                let id = *id;
                                st.session.edit("Route channel", |p| p.channel_mut(cid).iter_mut().for_each(|c| c.mixer = id));
                                ui.close_menu();
                            }
                        }
                    });
                }
                let name_btn = egui::Button::new(RichText::new(short(&c.name, 14)).color(if selected { Color32::BLACK } else { Color32::WHITE }))
                    .fill(if selected { theme::rgb(c.color) } else { theme::rgb(c.color).gamma_multiply(0.35) })
                    .min_size(Vec2::new(118.0, 24.0));
                let nb = ui.add(name_btn).on_hover_text(format!("{} – {}\nclick: select · double-click: settings · right-click: menu", c.name, c.instrument.kind_name()));
                if nb.clicked() {
                    // FL: Ctrl+click adds the channel to the selection.
                    if ui.input(|i| i.modifiers.ctrl) {
                        if let Some(k) = st.ui.rack_sel.iter().position(|x| *x == cid) {
                            st.ui.rack_sel.remove(k);
                        } else {
                            if st.ui.rack_sel.is_empty() {
                                st.ui.rack_sel.extend(st.ui.channel);
                            }
                            st.ui.rack_sel.push(cid);
                        }
                    } else {
                        st.ui.rack_sel = vec![cid];
                    }
                    st.ui.channel = Some(cid);
                    st.ui.piano_channel = Some(cid);
                }
                if nb.double_clicked() && !st.ui.instrument_windows.contains(&cid) {
                    st.ui.instrument_windows.push(cid);
                }
                nb.context_menu(|ui| channel_menu(ui, st, c));
                // Steps or notes indicator.
                let pc = pattern.channels.get(&cid);
                let has_notes = pc.map(|x| !x.notes.is_empty()).unwrap_or(false);
                if has_notes {
                    let (rect, r) = ui.allocate_exact_size(Vec2::new(steps as f32 * 22.0, 24.0), Sense::click());
                    mini_roll(ui.painter(), rect, &pc.unwrap().notes, pattern.length_beats());
                    if r.double_clicked() || r.clicked() {
                        st.ui.piano_channel = Some(cid);
                        st.ui.channel = Some(cid);
                        st.ctx.open("dragon.piano", None);
                    }
                } else {
                    step_row(ui, st, pat, cid, pc.map(|x| x.steps.clone()).unwrap_or_default(), steps);
                }
            });
            // Drop a sample on a channel to replace its sound (from the browser; files from the desktop are
            // handled in the app's drop handler through the row rectangles below).
            rows.push((cid, row.response.rect));
            if row.response.dnd_hover_payload::<PathBuf>().is_some() {
                ui.painter().rect_stroke(row.response.rect.expand(1.0), 3.0, Stroke::new(2.0_f32, theme::ACCENT_2), egui::StrokeKind::Outside);
                ui.painter().text(row.response.rect.right_center() - Vec2::new(6.0, 0.0), egui::Align2::RIGHT_CENTER, "replace sound", egui::FontId::proportional(11.0), theme::ACCENT_2);
            }
            if let Some(path) = super::widgets::dnd_release::<PathBuf>(&row.response) {
                st.replace_sample(cid, &path);
            }
        }
        let frame = ui.ctx().cumulative_pass_nr();
        ui.data_mut(|d| d.insert_temp(egui::Id::new("rack.rows"), (frame, rows)));
        // Drop zone for new samplers.
        let (rect, r) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::hover());
        let frame = ui.ctx().cumulative_pass_nr();
        ui.data_mut(|d| d.insert_temp(egui::Id::new("rack.dropzone"), (frame, rect)));
        ui.painter().rect_stroke(rect.shrink(2.0), 4.0, Stroke::new(1.0_f32, if r.dnd_hover_payload::<PathBuf>().is_some() { theme::ACCENT_2 } else { theme::GRID }), egui::StrokeKind::Inside);
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "Drop a sample here to add a sampler channel · drop it on a channel to replace its sound", egui::FontId::proportional(11.0), theme::TEXT_DIM);
        if let Some(path) = super::widgets::dnd_release::<PathBuf>(&r) {
            st.add_sampler(&path);
        }
    });
}

/// Pattern operations (FL Studio): clone, make unique, split by channel, split at a bar, change the source
/// pattern of a clip, delete. With `clip` the menu belongs to a playlist clip (make unique / source pattern act
/// on it, or on all selected pattern clips when it is selected).
pub fn pattern_menu(ui: &mut Ui, st: &mut State, pat: Id, clip: Option<Id>) {
    let Some(pattern) = st.session.project.pattern(pat).cloned() else {
        ui.label("Pattern not found");
        return;
    };
    ui.label(RichText::new(&pattern.name).strong().color(theme::rgb(pattern.color)));
    let mut name = pattern.name.clone();
    if ui.add(egui::TextEdit::singleline(&mut name).desired_width(140.0).hint_text("name")).changed() {
        st.session.gesture("Rename pattern", |p| p.pattern_mut(pat).iter_mut().for_each(|x| x.name = name.clone()));
    }
    ui.separator();
    if let Some(cid) = clip {
        let clips: Vec<Id> = if st.ui.clips.contains(&cid) { st.ui.clips.clone() } else { vec![cid] };
        let n = clips.len();
        if ui.button(if n > 1 { format!("Make unique ({n} clips)") } else { "Make unique".into() }).on_hover_text("Give the clip its own copy of the pattern, so editing it does not change the others").clicked() {
            let new = st.session.edit("Make unique", |p| dragon_engine::edit::make_unique(p, &clips));
            if let Some(&first) = new.first() {
                st.ui.pattern = first;
            }
            close(ui);
        }
        ui.menu_button("Source pattern", |ui| {
            for (id, nm) in st.session.project.patterns.iter().map(|x| (x.id, x.name.clone())).collect::<Vec<_>>() {
                if ui.selectable_label(id == pat, nm).clicked() {
                    st.session.edit("Change source pattern", |p| dragon_engine::edit::set_clip_pattern(p, &clips, id));
                    close(ui);
                }
            }
        });
    }
    if ui.button("Clone pattern").on_hover_text("A new, independent copy of the pattern").clicked() {
        if let Some(id) = st.session.edit("Clone pattern", |p| dragon_engine::edit::clone_pattern(p, pat)) {
            st.ui.pattern = id;
        }
        close(ui);
    }
    let used = pattern.channels.values().filter(|c| c.steps.iter().any(|s| s.on) || !c.notes.is_empty()).count();
    if ui.add_enabled(used >= 2, egui::Button::new("Split by channel")).on_hover_text("One pattern per channel; the playlist clips are replaced by stacked clips of the parts").clicked() {
        let parts = st.session.edit("Split by channel", |p| dragon_engine::edit::split_by_channel(p, pat));
        if let Some(&first) = parts.first() {
            st.ui.pattern = first;
        }
        close(ui);
    }
    let len = pattern.length_beats();
    let has_notes = pattern.channels.values().any(|c| !c.notes.is_empty());
    let bpb = 4.0;
    ui.menu_button("Split at", |ui| {
        let step = if has_notes { bpb } else { 1.0 };
        let mut at = step;
        let mut any = false;
        while at < len - 1e-9 {
            let label = if has_notes || (at % bpb).abs() < 1e-9 { format!("bar {}", (at / bpb) as u32 + 1) } else { format!("beat {}", at as u32 + 1) };
            if ui.button(label).clicked() {
                if let Some((a, _)) = st.session.edit("Split pattern", |p| dragon_engine::edit::split_pattern_at(p, pat, at)) {
                    st.ui.pattern = a;
                }
                close(ui);
            }
            any = true;
            at += step;
        }
        if !any {
            ui.label(RichText::new("The pattern is too short to split").small());
        }
    });
    if ui.button("Open in channel rack").clicked() {
        st.ui.pattern = pat;
        st.ctx.open("dragon.rack", None);
        close(ui);
    }
    ui.separator();
    if ui.add_enabled(st.session.project.patterns.len() > 1, egui::Button::new("Delete pattern")).on_hover_text("Removes the pattern and its playlist clips").clicked() {
        if let Some(next) = st.session.edit("Delete pattern", |p| dragon_engine::edit::delete_pattern(p, pat)) {
            st.ui.pattern = next;
        }
        close(ui);
    }
}

/// Closes the menu or popup the pattern menu is shown in.
fn close(ui: &mut Ui) {
    ui.close_menu();
    ui.memory_mut(|m| m.close_popup());
}

fn short(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n - 1).collect::<String>())
    }
}

fn mini_knob(ui: &mut Ui, v: &mut f32, min: f32, max: f32, what: &str) -> egui::Response {
    let (rect, mut r) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::click_and_drag());
    if r.dragged() {
        *v = (*v - r.drag_delta().y / 120.0 * (max - min)).clamp(min, max);
        r.mark_changed();
    }
    if r.double_clicked() {
        *v = if min < 0.0 { 0.0 } else { 0.8 };
        r.mark_changed();
    }
    let n = (*v - min) / (max - min);
    let c = rect.center();
    let p = ui.painter();
    p.circle_filled(c, 8.0, theme::PANEL_2);
    let a = std::f32::consts::PI * (0.75 + 1.5 * n);
    p.line_segment([c, Pos2::new(c.x + 7.0 * a.cos(), c.y + 7.0 * a.sin())], Stroke::new(2.0_f32, theme::ACCENT_2));
    let txt = if what == "pan" { widgets::pan_fmt(*v) } else { widgets::db_fmt(*v) };
    r.on_hover_text(format!("{what}: {txt}"))
}

fn step_row(ui: &mut Ui, st: &mut State, pat: Id, cid: Id, cur: Vec<Step>, steps: usize) {
    for i in 0..steps {
        let s = cur.get(i).copied().unwrap_or_default();
        let beat_group = (i / 4) % 2 == 0;
        let (rect, r) = ui.allocate_exact_size(Vec2::new(20.0, 24.0), Sense::click_and_drag());
        let off = if beat_group { Color32::from_gray(58) } else { Color32::from_gray(44) };
        let on = theme::ACCENT.lerp_to_gamma(theme::ACCENT_2, s.velocity.clamp(0.0, 1.0));
        ui.painter().rect_filled(rect.shrink(1.5), 3.0, if s.on { on } else { off });
        if s.on && s.velocity < 0.99 {
            let h = (rect.height() - 6.0) * s.velocity;
            ui.painter().rect_filled(Rect::from_min_max(Pos2::new(rect.right() - 4.0, rect.bottom() - 3.0 - h), Pos2::new(rect.right() - 2.0, rect.bottom() - 3.0)), 0.0, Color32::WHITE);
        }
        // Click toggles, right-click clears, drag over steps paints, Alt+wheel sets velocity.
        let painting = ui.input(|i| i.pointer.primary_down()) && r.hovered() && ui.ctx().dragged_id().is_some();
        let toggle = r.clicked() || r.drag_started();
        if toggle || r.secondary_clicked() || (painting && !s.on) {
            let val = if r.secondary_clicked() {
                false
            } else if toggle {
                !s.on
            } else {
                true
            };
            let edit = |p: &mut Project| {
                if let Some(pt) = p.pattern_mut(pat) {
                    let pc = pt.channels.entry(cid).or_default();
                    if pc.steps.len() < steps {
                        pc.steps.resize(steps, Step::default());
                    }
                    pc.steps[i].on = val;
                }
            };
            if painting && !toggle {
                st.session.gesture("Paint steps", edit);
            } else {
                st.session.edit("Toggle step", edit);
            }
            if val {
                st.audio.send(Cmd::Live(dragon_engine::render::LiveEvent::NoteOn { channel: cid, note: 60, velocity: s.velocity.max(0.1) }));
                st.audio.send(Cmd::Live(dragon_engine::render::LiveEvent::NoteOff { channel: cid, note: 60 }));
            }
        }
        if s.on && r.hovered() {
            let wheel = ui.input(|i| if i.modifiers.alt { i.smooth_scroll_delta.y } else { 0.0 });
            if wheel != 0.0 {
                let v = (s.velocity + wheel / 400.0).clamp(0.05, 1.0);
                st.session.gesture("Step velocity", |p| {
                    if let Some(x) = p.pattern_mut(pat).and_then(|pt| pt.channels.get_mut(&cid)).and_then(|pc| pc.steps.get_mut(i)) {
                        x.velocity = v;
                    }
                });
            }
        }
        r.on_hover_text(format!("Step {} · velocity {:.0}% (Alt+wheel)", i + 1, s.velocity * 100.0));
    }
}

fn mini_roll(p: &egui::Painter, rect: Rect, notes: &[Note], len_beats: f64) {
    p.rect_filled(rect, 3.0, theme::PANEL_2);
    let (lo, hi) = notes.iter().fold((127u8, 0u8), |(l, h), n| (l.min(n.pitch), h.max(n.pitch)));
    let span = hi.saturating_sub(lo).max(12) as f32;
    let len = (len_beats * PPQ as f64).max(1.0) as f32;
    for n in notes {
        let x0 = rect.left() + n.start as f32 / len * rect.width();
        let x1 = rect.left() + n.end() as f32 / len * rect.width();
        let y = rect.bottom() - 3.0 - (n.pitch - lo) as f32 / span * (rect.height() - 6.0);
        p.line_segment([Pos2::new(x0, y), Pos2::new(x1.max(x0 + 2.0), y)], Stroke::new(2.0_f32, theme::ACCENT_2));
    }
}

/// The step fills: (name, every n steps, starting at step, what it is good for).
pub const FILLS: [(&str, usize, usize, &str); 7] = [
    ("Each step", 1, 0, "Every 16th: rolls, fast hats"),
    ("Every 2 steps", 2, 0, "Every 8th: hi-hats, shakers"),
    ("Every 3 steps", 3, 0, "Dotted 8ths: syncopated hats, percs"),
    ("Every 4 steps", 4, 0, "Every beat: four-on-the-floor kick"),
    ("Every 8 steps", 8, 0, "Every half bar: kick on 1 and 3"),
    ("Off-beats", 4, 2, "Between the beats: open hats, house bass"),
    ("Backbeat (2 and 4)", 8, 4, "Beats 2 and 4: claps and snares"),
];

/// Sets channel `cid`'s steps in `pat` to every `every`th step from `start` (the rest off); the pattern's
/// step count decides the length. Velocities of steps that stay on are kept.
pub fn fill_steps(p: &mut Project, pat: Id, cid: Id, every: usize, start: usize) {
    let Some(pt) = p.pattern_mut(pat) else { return };
    let n = pt.steps.max(1) as usize;
    let pc = pt.channels.entry(cid).or_default();
    pc.steps.resize(n.max(pc.steps.len()), Step::default());
    for (i, s) in pc.steps.iter_mut().enumerate() {
        s.on = i < n && i >= start && (i - start).is_multiple_of(every.max(1));
    }
}

/// The fill choices for `targets` (one undo step).
fn fill_menu(ui: &mut Ui, st: &mut State, targets: &[Id]) {
    let pat = st.ui.pattern;
    let what = match targets {
        [one] => st.session.project.channel(*one).map(|c| c.name.clone()).unwrap_or_else(|| "this channel".into()),
        _ => format!("{} channels", targets.len()),
    };
    ui.label(RichText::new(format!("Fill {what}")).small().color(theme::TEXT_DIM));
    for (name, every, start, tip) in FILLS {
        if ui.button(name).on_hover_text(tip).clicked() {
            st.session.edit(&format!("Fill: {}", name.to_lowercase()), |p| {
                for cid in targets {
                    fill_steps(p, pat, *cid, every, start);
                }
            });
            close(ui);
        }
    }
    ui.separator();
    if ui.button("Invert").on_hover_text("On steps off, off steps on").clicked() {
        st.session.edit("Invert steps", |p| {
            if let Some(pt) = p.pattern_mut(pat) {
                let n = pt.steps as usize;
                for cid in targets {
                    let pc = pt.channels.entry(*cid).or_default();
                    pc.steps.resize(n.max(pc.steps.len()), Step::default());
                    pc.steps.iter_mut().take(n).for_each(|s| s.on = !s.on);
                }
            }
        });
        close(ui);
    }
    for (label, dir) in [("Shift left", -1i32), ("Shift right", 1)] {
        if ui.button(label).on_hover_text("Move every step one place (wraps around)").clicked() {
            st.session.edit(label, |p| {
                if let Some(pt) = p.pattern_mut(pat) {
                    let n = pt.steps as usize;
                    for cid in targets {
                        let pc = pt.channels.entry(*cid).or_default();
                        pc.steps.resize(n.max(pc.steps.len()), Step::default());
                        if n > 0 {
                            if dir < 0 {
                                pc.steps[..n].rotate_left(1);
                            } else {
                                pc.steps[..n].rotate_right(1);
                            }
                        }
                    }
                }
            });
            close(ui);
        }
    }
    if ui.button("Clear").clicked() {
        st.session.edit("Clear steps", |p| {
            if let Some(pt) = p.pattern_mut(pat) {
                for cid in targets {
                    if let Some(pc) = pt.channels.get_mut(cid) {
                        pc.steps.iter_mut().for_each(|s| s.on = false);
                    }
                }
            }
        });
        close(ui);
    }
}

fn channel_menu(ui: &mut Ui, st: &mut State, c: &RackChannel) {
    let cid = c.id;
    if ui.button("Settings…").clicked() {
        if !st.ui.instrument_windows.contains(&cid) {
            st.ui.instrument_windows.push(cid);
        }
        ui.close_menu();
    }
    if ui.button("Open in piano roll").clicked() {
        st.ui.piano_channel = Some(cid);
        st.ctx.open("dragon.piano", None);
        ui.close_menu();
    }
    if ui.button("Convert steps to notes").clicked() {
        let pat = st.ui.pattern;
        st.session.edit("Steps to notes", |p| {
            if let Some(pc) = p.pattern_mut(pat).and_then(|pt| pt.channels.get_mut(&cid)) {
                dragon_engine::edit::steps_to_notes(pc, 60);
            }
        });
        ui.close_menu();
    }
    ui.menu_button("Fill steps", |ui| {
        let targets = if st.ui.rack_sel.contains(&cid) { st.ui.rack_sel.clone() } else { vec![cid] };
        fill_menu(ui, st, &targets);
    });
    if ui.button("Solo").clicked() {
        st.session.edit("Solo channel", |p| p.channel_mut(cid).iter_mut().for_each(|c| c.solo = !c.solo));
        ui.close_menu();
    }
    if ui.button("Duplicate").clicked() {
        st.session.edit("Duplicate channel", |p| {
            if let Some(mut ch) = p.channel(cid).cloned() {
                ch.id = p.next_id();
                ch.name = format!("{} 2", ch.name);
                p.channels.push(ch);
            }
        });
        ui.close_menu();
    }
    if ui.button("MIDI learn volume").clicked() {
        st.ui.midi_learn = Some(Target::ChannelVolume(cid));
        ui.close_menu();
    }
    ui.separator();
    if ui.button("Delete channel").clicked() {
        st.session.edit("Delete channel", |p| {
            p.channels.retain(|c| c.id != cid);
            for pt in p.patterns.iter_mut() {
                pt.channels.remove(&cid);
            }
        });
        if st.ui.channel == Some(cid) {
            st.ui.channel = st.session.project.channels.first().map(|c| c.id);
        }
        ui.close_menu();
    }
}

#[cfg(test)]
mod fill_tests {
    use super::*;

    #[test]
    fn fills_set_every_nth_step_from_a_start() {
        let mut p = Project::new_default();
        let pat = p.patterns[0].id;
        let ch = p.add_channel("Hat", Instrument::Synth(Default::default()));
        let on = |p: &Project| -> Vec<usize> { p.pattern(pat).unwrap().channels[&ch].steps.iter().enumerate().filter(|(_, s)| s.on).map(|(i, _)| i).collect() };
        fill_steps(&mut p, pat, ch, 4, 0);
        assert_eq!(on(&p), [0, 4, 8, 12]);
        fill_steps(&mut p, pat, ch, 2, 0);
        assert_eq!(on(&p).len(), 8);
        fill_steps(&mut p, pat, ch, 1, 0);
        assert_eq!(on(&p).len(), 16);
        fill_steps(&mut p, pat, ch, 4, 2);
        assert_eq!(on(&p), [2, 6, 10, 14], "off-beats");
        fill_steps(&mut p, pat, ch, 8, 4);
        assert_eq!(on(&p), [4, 12], "backbeat");
        // A longer pattern is filled to its end.
        p.pattern_mut(pat).unwrap().steps = 32;
        fill_steps(&mut p, pat, ch, 8, 0);
        assert_eq!(on(&p), [0, 8, 16, 24]);
    }
}
