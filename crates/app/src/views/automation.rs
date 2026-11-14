//! Automation lanes (mixer, sends, effect / plugin parameters, instrument parameters, tempo) with a point
//! editor and read / write / touch / latch / trim modes, plus the list of MIDI controller mappings.

use crate::state::{target_name, State};
use crate::theme;
use dragon_engine::model::*;
use egui::{Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};

/// Adds a lane for `t` (or shows the existing one) seeded with the current value.
pub fn add_lane(st: &mut State, t: Target) {
    if st.session.project.automation.iter().any(|l| l.target == t) {
        st.ctx.open("dragon.automation", None);
        return;
    }
    let (min, max) = dragon_engine::midi::target_range(&t);
    let v = current_value(&st.session.project, &t).unwrap_or((min + max) / 2.0);
    st.session.edit("Add automation lane", |p| {
        let id = p.next_id();
        p.automation.push(AutomationLane { id, target: t, points: vec![AutoPoint { beat: 0.0, value: v, tension: 0.0 }], mode: AutomationMode::Read, visible: true });
    });
    st.ctx.open("dragon.automation", None);
}

fn current_value(p: &Project, t: &Target) -> Option<f32> {
    Some(match t {
        Target::MixerVolume(id) => p.mixer.get(*id)?.volume,
        Target::MixerPan(id) => p.mixer.get(*id)?.pan,
        Target::MixerWidth(id) => p.mixer.get(*id)?.width,
        Target::ChannelVolume(id) => p.channel(*id)?.volume,
        Target::ChannelPan(id) => p.channel(*id)?.pan,
        Target::Tempo => p.bpm as f32,
        Target::SynthCutoff(id) => match &p.channel(*id)?.instrument {
            Instrument::Synth(s) => s.cutoff,
            _ => return None,
        },
        Target::InsertParam { channel, insert, param } => *p.mixer.get(*channel)?.inserts.iter().find(|i| i.id == *insert)?.params.get(param)?,
        _ => return None,
    })
}

pub fn show(ui: &mut Ui, st: &mut State) {
    ui.horizontal(|ui| {
        ui.menu_button("+ Lane", |ui| {
            let p = st.session.project.clone();
            if ui.button("Tempo").clicked() {
                add_lane(st, Target::Tempo);
                ui.close_menu();
            }
            ui.menu_button("Mixer", |ui| {
                for m in p.mixer.all() {
                    ui.menu_button(&m.name, |ui| {
                        for (n, t) in [("Volume", Target::MixerVolume(m.id)), ("Pan", Target::MixerPan(m.id)), ("Width", Target::MixerWidth(m.id)), ("Mute", Target::MixerMute(m.id))] {
                            if ui.button(n).clicked() {
                                add_lane(st, t);
                                ui.close_menu();
                            }
                        }
                        for s in &m.sends {
                            if ui.button(format!("Send ➡ {}", p.mixer.get(s.target).map(|x| x.name.clone()).unwrap_or_default())).clicked() {
                                add_lane(st, Target::SendLevel { channel: m.id, target: s.target });
                                ui.close_menu();
                            }
                        }
                        for i in &m.inserts {
                            if let Some(def) = dragon_engine::effects::find(&i.effect) {
                                ui.menu_button(def.name, |ui| {
                                    for prm in def.params {
                                        if ui.button(prm.name).clicked() {
                                            add_lane(st, Target::InsertParam { channel: m.id, insert: i.id, param: prm.id.into() });
                                            ui.close_menu();
                                        }
                                    }
                                    if ui.button("Dry/wet").clicked() {
                                        add_lane(st, Target::InsertMix { channel: m.id, insert: i.id });
                                        ui.close_menu();
                                    }
                                });
                            }
                        }
                    });
                }
            });
            ui.menu_button("Channels", |ui| {
                for c in &p.channels {
                    ui.menu_button(&c.name, |ui| {
                        let mut opts = vec![("Volume", Target::ChannelVolume(c.id)), ("Pan", Target::ChannelPan(c.id)), ("Pitch", Target::ChannelPitch(c.id))];
                        if matches!(c.instrument, Instrument::Synth(_)) {
                            opts.push(("Cutoff", Target::SynthCutoff(c.id)));
                            opts.push(("Resonance", Target::SynthResonance(c.id)));
                        }
                        for (n, t) in opts {
                            if ui.button(n).clicked() {
                                add_lane(st, t);
                                ui.close_menu();
                            }
                        }
                    });
                }
            });
        });
        ui.label(RichText::new("Click adds a point · drag moves · right-click deletes · Alt+drag bends the curve · right-click any knob for “Create automation lane”").small().color(theme::TEXT_DIM));
    });
    ui.separator();
    let lanes = st.session.project.automation.clone();
    let len = st.session.project.song_length().max(16.0) + 8.0;
    let beat_now = st.beat_now();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for lane in &lanes {
            let lid = lane.id;
            ui.horizontal(|ui| {
                ui.label(RichText::new(target_name(&st.session.project, &lane.target)).strong());
                let mut mode = lane.mode;
                egui::ComboBox::from_id_salt(("amode", lid)).selected_text(format!("{mode:?}")).width(70.0).show_ui(ui, |ui| {
                    for m in [AutomationMode::Off, AutomationMode::Read, AutomationMode::Write, AutomationMode::Touch, AutomationMode::Latch, AutomationMode::Trim] {
                        ui.selectable_value(&mut mode, m, format!("{m:?}"));
                    }
                });
                if mode != lane.mode {
                    st.session.edit("Automation mode", |p| p.automation.iter_mut().filter(|l| l.id == lid).for_each(|l| l.mode = mode));
                }
                if ui.small_button("clear").clicked() {
                    st.session.edit("Clear automation", |p| p.automation.iter_mut().filter(|l| l.id == lid).for_each(|l| l.points.truncate(1)));
                }
                if ui.small_button("🗑").clicked() {
                    st.session.edit("Delete lane", |p| p.automation.retain(|l| l.id != lid));
                }
            });
            lane_editor(ui, st, lane, len, beat_now);
            ui.add_space(6.0);
        }
        ui.separator();
        ui.heading("MIDI controller mappings");
        let maps = st.session.project.midi_mappings.clone();
        if maps.is_empty() {
            ui.label(RichText::new("Right-click a knob or fader and choose “MIDI learn”, then move a controller.").small().color(theme::TEXT_DIM));
        }
        for (k, m) in maps.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!("ch {} · CC {} ➡ {}", m.channel + 1, m.controller, target_name(&st.session.project, &m.target)));
                if ui.small_button("remove").clicked() {
                    st.session.edit("Remove MIDI mapping", |p| {
                        p.midi_mappings.remove(k);
                    });
                }
            });
        }
    });
}

fn lane_editor(ui: &mut Ui, st: &mut State, lane: &AutomationLane, len: f64, beat_now: f64) {
    let (min, max) = dragon_engine::midi::target_range(&lane.target);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 80.0), Sense::click_and_drag());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 3.0, theme::BG);
    let x_of = |b: f64| rect.left() + (b / len) as f32 * rect.width();
    let b_of = |x: f32| ((x - rect.left()) / rect.width()) as f64 * len;
    let y_of = |v: f32| rect.bottom() - (v - min) / (max - min) * rect.height();
    let v_of = |y: f32| (min + (rect.bottom() - y) / rect.height() * (max - min)).clamp(min, max);
    let bpb = st.session.project.beats_per_bar(0.0);
    let mut b = 0.0;
    while b < len {
        p.line_segment([Pos2::new(x_of(b), rect.top()), Pos2::new(x_of(b), rect.bottom())], Stroke::new(1.0_f32, theme::GRID));
        b += bpb * 4.0;
    }
    let curve: Vec<Pos2> = (0..=rect.width() as usize / 2).map(|i| {
        let x = rect.left() + i as f32 * 2.0;
        Pos2::new(x, y_of(lane.value_at(b_of(x)).unwrap_or(min)))
    }).collect();
    p.add(egui::Shape::line(curve, Stroke::new(2.0_f32, if lane.mode == AutomationMode::Off { theme::TEXT_DIM } else { theme::ACCENT_2 })));
    let pts: Vec<Pos2> = lane.points.iter().map(|pt| Pos2::new(x_of(pt.beat), y_of(pt.value))).collect();
    for q in &pts {
        p.circle_filled(*q, 4.0, egui::Color32::WHITE);
    }
    p.line_segment([Pos2::new(x_of(beat_now), rect.top()), Pos2::new(x_of(beat_now), rect.bottom())], Stroke::new(1.0_f32, theme::PLAYHEAD));
    let lid = lane.id;
    let hit = |pos: Pos2| pts.iter().position(|q| q.distance(pos) < 7.0);
    let drag_id = egui::Id::new(("adrag", lid));
    if resp.drag_started() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let idx = hit(pos).or_else(|| {
                // Insert a point on drag start in empty space.
                let b = b_of(pos.x).max(0.0);
                let v = v_of(pos.y);
                st.session.gesture("Automation", |p| {
                    if let Some(l) = p.automation.iter_mut().find(|l| l.id == lid) {
                        l.points.push(AutoPoint { beat: b, value: v, tension: 0.0 });
                        l.points.sort_by(|a, c| a.beat.total_cmp(&c.beat));
                    }
                });
                st.session.project.automation.iter().find(|l| l.id == lid).and_then(|l| l.points.iter().position(|q| (q.beat - b).abs() < 1e-9))
            });
            ui.data_mut(|d| d.insert_temp(drag_id, idx));
        }
    }
    if resp.dragged() {
        if let (Some(Some(i)), Some(pos)) = (ui.data(|d| d.get_temp::<Option<usize>>(drag_id)), resp.interact_pointer_pos()) {
            let alt = ui.input(|i| i.modifiers.alt);
            let b = b_of(pos.x).max(0.0);
            let v = v_of(pos.y);
            let dy = resp.drag_delta().y;
            st.session.gesture("Automation", |p| {
                if let Some(pt) = p.automation.iter_mut().find(|l| l.id == lid).and_then(|l| l.points.get_mut(i)) {
                    if alt {
                        pt.tension = (pt.tension - dy / 60.0).clamp(-1.0, 1.0);
                    } else {
                        pt.beat = b;
                        pt.value = v;
                    }
                }
            });
        }
    }
    if resp.drag_stopped() {
        st.session.edit("Sort automation", |p| {
            if let Some(l) = p.automation.iter_mut().find(|l| l.id == lid) {
                l.points.sort_by(|a, c| a.beat.total_cmp(&c.beat));
            }
        });
        st.session.end_gesture();
    }
    if resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            if hit(pos).is_none() {
                let (b, v) = (b_of(pos.x).max(0.0), v_of(pos.y));
                st.session.edit("Add automation point", |p| {
                    if let Some(l) = p.automation.iter_mut().find(|l| l.id == lid) {
                        l.points.push(AutoPoint { beat: b, value: v, tension: 0.0 });
                        l.points.sort_by(|a, c| a.beat.total_cmp(&c.beat));
                    }
                });
            }
        }
    }
    if resp.secondary_clicked() {
        if let Some(i) = resp.interact_pointer_pos().and_then(hit) {
            st.session.edit("Delete automation point", |p| {
                if let Some(l) = p.automation.iter_mut().find(|l| l.id == lid) {
                    if l.points.len() > 1 {
                        l.points.remove(i);
                    }
                }
            });
        }
    }
    let _ = Rect::NOTHING;
}
