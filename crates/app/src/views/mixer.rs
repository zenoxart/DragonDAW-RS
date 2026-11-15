//! Mixer: master + channel strips with insert slots (stock effects and CLAP/VST3 plugins), sends, output
//! routing, VCA assignment, pan / stereo width, phase invert, L/R swap, mute / solo / solo-safe, faders
//! and peak / gain-reduction meters.

use super::widgets::{self, knob};
use crate::state::State;
use crate::theme;
use dragon_engine::model::*;
use egui::{Color32, RichText, Ui, Vec2};

pub fn show(ui: &mut Ui, st: &mut State) {
    let meters = st.audio.meters().snapshot();
    let all: Vec<MixerChannel> = std::iter::once(st.session.project.mixer.master.clone()).chain(st.session.project.mixer.channels.iter().cloned()).collect();
    let names: Vec<(Id, String, ChannelKind)> = all.iter().map(|m| (m.id, m.name.clone(), m.kind)).collect();
    let master = st.session.project.mixer.master.id;
    egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            for m in &all {
                strip(ui, st, m, m.id == master, &names, &meters);
            }
            ui.vertical(|ui| {
                if ui.button("+ Bus").clicked() {
                    st.ctx.run("dragon.add.mixer", serde_json::Value::Null);
                }
                if ui.button("+ VCA").clicked() {
                    st.ctx.run("dragon.add.vca", serde_json::Value::Null);
                }
            });
        });
    });
    if st.session.in_gesture() && !ui.ctx().input(|i| i.pointer.any_down()) && !ui.ctx().wants_keyboard_input() {
        st.session.end_gesture();
    }
}

fn strip(ui: &mut Ui, st: &mut State, m: &MixerChannel, is_master: bool, names: &[(Id, String, ChannelKind)], meters: &dragon_engine::render::MeterValues) {
    let id = m.id;
    let selected = st.ui.mixer == id;
    let width = if is_master { 128.0 } else { 112.0 };
    let frame = egui::Frame::NONE.fill(if selected { theme::PANEL_2 } else { theme::PANEL }).stroke(egui::Stroke::new(1.0_f32, if selected { theme::ACCENT } else { theme::GRID })).corner_radius(4.0).inner_margin(5.0);
    let resp = frame.show(ui, |ui| {
        ui.set_width(width);
        ui.vertical(|ui| {
            // Name & colour.
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::new(4.0, 16.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 1.0, theme::rgb(m.color));
                let mut name = m.name.clone();
                let resp = ui.add_enabled(!is_master, egui::TextEdit::singleline(&mut name).desired_width(width - 22.0).frame(false));
                if resp.changed() {
                    st.session.gesture("Rename channel", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.name = name.clone()));
                }
                if resp.clicked() {
                    st.ui.mixer = id;
                }
            });
            let vca = m.kind == ChannelKind::Vca;
            if !vca {
                // Inserts.
                ui.label(RichText::new("INSERTS").small().color(theme::TEXT_DIM));
                for ins in &m.inserts {
                    let iid = ins.id;
                    ui.horizontal(|ui| {
                        if widgets::toggle(ui, ins.enabled, "🔘", theme::OK).on_hover_text("Bypass").clicked() {
                            st.session.edit("Bypass effect", |p| {
                                if let Some(i) = p.mixer.get_mut(id).and_then(|c| c.inserts.iter_mut().find(|i| i.id == iid)) {
                                    i.enabled = !i.enabled;
                                }
                            });
                        }
                        let label = ins.plugin.as_ref().map(|p| p.name.clone()).or_else(|| dragon_engine::effects::find(&ins.effect).map(|d| d.name.to_string())).unwrap_or_else(|| ins.effect.clone());
                        let gr = meters.gain_reduction.get(&iid).copied().unwrap_or(0.0);
                        let text = if gr > 0.1 { format!("{label}  −{gr:.1}") } else { label };
                        let b = ui.add(egui::Button::new(RichText::new(text).small()).min_size(Vec2::new(width - 30.0, 16.0)));
                        if b.clicked() && !st.ui.effect_windows.contains(&(id, iid)) {
                            st.ui.effect_windows.push((id, iid));
                        }
                        b.context_menu(|ui| {
                            if ui.button("Move up").clicked() {
                                st.session.edit("Reorder effects", |p| {
                                    if let Some(c) = p.mixer.get_mut(id) {
                                        if let Some(k) = c.inserts.iter().position(|i| i.id == iid) {
                                            if k > 0 {
                                                c.inserts.swap(k, k - 1);
                                            }
                                        }
                                    }
                                });
                                ui.close_menu();
                            }
                            if ui.button("Move down").clicked() {
                                st.session.edit("Reorder effects", |p| {
                                    if let Some(c) = p.mixer.get_mut(id) {
                                        if let Some(k) = c.inserts.iter().position(|i| i.id == iid) {
                                            if k + 1 < c.inserts.len() {
                                                c.inserts.swap(k, k + 1);
                                            }
                                        }
                                    }
                                });
                                ui.close_menu();
                            }
                            if ui.button("Remove").clicked() {
                                st.session.edit("Remove effect", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.inserts.retain(|i| i.id != iid)));
                                st.ui.effect_windows.retain(|w| *w != (id, iid));
                                ui.close_menu();
                            }
                        });
                    });
                }
                add_effect_menu(ui, st, id);
                // Sends.
                if !is_master {
                    ui.label(RichText::new("SENDS").small().color(theme::TEXT_DIM));
                    for s in &m.sends {
                        let target = s.target;
                        ui.horizontal(|ui| {
                            let tn = names.iter().find(|n| n.0 == target).map(|n| n.1.clone()).unwrap_or("?".into());
                            let mut lvl = s.level;
                            let r = ui.add(egui::Slider::new(&mut lvl, 0.0..=1.0).show_value(false).text(RichText::new(tn).small()));
                            widgets::apply(st, &r, "Send level", |p| {
                                if let Some(x) = p.mixer.get_mut(id).and_then(|c| c.sends.iter_mut().find(|x| x.target == target)) {
                                    x.level = lvl;
                                }
                            });
                            r.context_menu(|ui| {
                                let mut pre = s.pre_fader;
                                if ui.checkbox(&mut pre, "Pre-fader").changed() {
                                    st.session.edit("Send pre/post", |p| {
                                        if let Some(x) = p.mixer.get_mut(id).and_then(|c| c.sends.iter_mut().find(|x| x.target == target)) {
                                            x.pre_fader = pre;
                                        }
                                    });
                                }
                                if ui.button("Remove send").clicked() {
                                    st.session.edit("Remove send", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.sends.retain(|x| x.target != target)));
                                    ui.close_menu();
                                }
                            });
                        });
                    }
                    ui.menu_button(RichText::new("+ send").small(), |ui| {
                        for (tid, tn, k) in names {
                            if *tid != id && *k == ChannelKind::Audio && !m.sends.iter().any(|s| s.target == *tid) && ui.button(tn).clicked() {
                                let t = *tid;
                                st.session.edit("Add send", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.sends.push(MixerSend { target: t, level: 0.5, pre_fader: false })));
                                ui.close_menu();
                            }
                        }
                    });
                    // Output.
                    let out = m.output.and_then(|o| names.iter().find(|n| n.0 == o)).map(|n| n.1.clone()).unwrap_or("Master".into());
                    egui::ComboBox::from_id_salt(("mix.out", id)).selected_text(RichText::new(format!("➡ {out}")).small()).width(width - 8.0).show_ui(ui, |ui| {
                        if ui.selectable_label(m.output.is_none(), "Master").clicked() {
                            st.session.edit("Route output", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.output = None));
                        }
                        for (tid, tn, k) in names {
                            if *tid != id && *k == ChannelKind::Audio && !is_master_id(st, *tid) && ui.selectable_label(m.output == Some(*tid), tn).clicked() {
                                let t = *tid;
                                st.session.edit("Route output", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.output = Some(t)));
                            }
                        }
                    });
                    let vcas: Vec<&(Id, String, ChannelKind)> = names.iter().filter(|n| n.2 == ChannelKind::Vca).collect();
                    if !vcas.is_empty() {
                        let cur = m.vca.and_then(|v| names.iter().find(|n| n.0 == v)).map(|n| n.1.clone()).unwrap_or("no VCA".into());
                        egui::ComboBox::from_id_salt(("mix.vca", id)).selected_text(RichText::new(cur).small()).width(width - 8.0).show_ui(ui, |ui| {
                            if ui.selectable_label(m.vca.is_none(), "no VCA").clicked() {
                                st.session.edit("VCA", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.vca = None));
                            }
                            for (vid, vn, _) in vcas {
                                if ui.selectable_label(m.vca == Some(*vid), vn).clicked() {
                                    let v = *vid;
                                    st.session.edit("VCA", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.vca = Some(v)));
                                }
                            }
                        });
                    }
                }
                // Pan & width.
                ui.horizontal(|ui| {
                    let mut pan = m.pan;
                    let r = knob(ui, &mut pan, -1.0, 1.0, 0.0, "pan", widgets::pan_fmt);
                    widgets::apply(st, &r, "Pan", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.pan = pan));
                    r.context_menu(|ui| learn_menu(ui, st, Target::MixerPan(id)));
                    let mut w = m.width;
                    let r = knob(ui, &mut w, 0.0, 2.0, 1.0, "width", widgets::pct);
                    widgets::apply(st, &r, "Stereo width", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.width = w));
                });
            }
            // Surround panner (top-level channels of a surround project).
            if let (Some(_), false, false) = (st.session.project.surround, is_master, vca) {
                if m.output.is_none() {
                    surround_pad(ui, st, id, m.surround, width);
                }
            }
            // Fader + meter.
            ui.horizontal(|ui| {
                let mut v = m.volume;
                let r = widgets::fader(ui, &mut v, 2.0, 150.0);
                widgets::apply(st, &r, "Volume", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.volume = v));
                r.context_menu(|ui| learn_menu(ui, st, Target::MixerVolume(id)));
                let (l, rr) = meters.strips.get(&id).copied().unwrap_or((0.0, 0.0));
                widgets::meter(ui, l, rr, 150.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(format!("{:.1}", dragon_engine::dsp::gain_to_db(v))).small().monospace());
                    let pk = dragon_engine::dsp::gain_to_db(l.max(rr).max(1e-6));
                    ui.label(RichText::new(if pk < -99.0 { "-∞".into() } else { format!("{pk:.1}") }).small().monospace().color(theme::meter_color(l.max(rr))));
                });
            });
            ui.horizontal(|ui| {
                if widgets::toggle(ui, m.muted, "M", theme::WARN).on_hover_text("Mute").clicked() {
                    st.session.edit("Mute", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.muted = !c.muted));
                }
                if !is_master {
                    if widgets::toggle(ui, m.solo, "S", theme::OK).on_hover_text("Solo").clicked() {
                        st.session.edit("Solo", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.solo = !c.solo));
                    }
                    if widgets::toggle(ui, m.solo_safe, "SS", theme::PLAYHEAD).on_hover_text("Solo safe (stays audible when others solo, e.g. reverb returns)").clicked() {
                        st.session.edit("Solo safe", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.solo_safe = !c.solo_safe));
                    }
                }
                if !vca {
                    if widgets::toggle(ui, m.phase_invert, "Ø", theme::ACCENT_2).on_hover_text("Invert polarity").clicked() {
                        st.session.edit("Phase", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.phase_invert = !c.phase_invert));
                    }
                    if widgets::toggle(ui, m.swap_lr, "🔀", theme::ACCENT_2).on_hover_text("Swap left / right").clicked() {
                        st.session.edit("Swap L/R", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.swap_lr = !c.swap_lr));
                    }
                }
            });
            if !is_master {
                ui.horizontal(|ui| {
                    if ui.small_button("🗑").on_hover_text("Delete channel").clicked() {
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
                    let mut col = [m.color.0, m.color.1, m.color.2];
                    if ui.color_edit_button_srgb(&mut col).changed() {
                        st.session.gesture("Colour", |p| p.mixer.get_mut(id).iter_mut().for_each(|c| c.color = Rgb(col[0], col[1], col[2])));
                    }
                });
            }
        });
    });
    if resp.response.contains_pointer() && ui.input(|i| i.pointer.primary_clicked()) {
        st.ui.mixer = id;
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

fn is_master_id(st: &State, id: Id) -> bool {
    st.session.project.mixer.master.id == id
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

pub fn add_effect_menu(ui: &mut Ui, st: &mut State, mixer: Id) {
    ui.menu_button(RichText::new("+ effect").small().color(theme::ACCENT_2), |ui| {
        let mut cats: Vec<&str> = dragon_engine::effects::registry().iter().map(|d| d.category).collect();
        cats.dedup();
        cats.sort();
        cats.dedup();
        for cat in cats {
            ui.menu_button(cat, |ui| {
                for d in dragon_engine::effects::registry().iter().filter(|d| d.category == cat) {
                    if ui.button(d.name).on_hover_text(d.description).clicked() {
                        let eid = d.id.to_string();
                        let params = d.defaults();
                        let iid = st.session.edit("Add effect", |p| {
                            let iid = p.next_id();
                            if let Some(c) = p.mixer.get_mut(mixer) {
                                c.inserts.push(Insert { id: iid, effect: eid, enabled: true, mix: 1.0, params, sidechain: None, plugin: None, preset: String::new() });
                            }
                            iid
                        });
                        st.ui.effect_windows.push((mixer, iid));
                        ui.close_menu();
                    }
                }
            });
        }
        let fx: Vec<_> = st.settings.plugins.iter().filter(|p| !p.instrument).cloned().collect();
        ui.menu_button("Plugins", |ui| {
            if fx.is_empty() {
                ui.label(RichText::new("No effect plugins – scan in View › Plugin manager").small());
            }
            for info in fx {
                if ui.button(format!("{} · {}", info.name, info.vendor)).clicked() {
                    let r = info.to_ref();
                    let iid = st.session.edit("Add plugin", |p| {
                        let iid = p.next_id();
                        if let Some(c) = p.mixer.get_mut(mixer) {
                            c.inserts.push(Insert { id: iid, effect: "plugin".into(), enabled: true, mix: 1.0, params: Default::default(), sidechain: None, plugin: Some(r), preset: String::new() });
                        }
                        iid
                    });
                    st.ui.effect_windows.push((mixer, iid));
                    ui.close_menu();
                }
            }
        });
    });
    let _ = Color32::WHITE;
}
