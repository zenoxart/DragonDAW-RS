//! Floating editor windows: effect editors (stock effects with presets, plugins with a generic parameter
//! editor and their own GUI), instrument editors (sampler, synth, drum pads, plugins) and audio clip
//! properties.

use super::widgets::{self, knob};
use crate::state::State;
use crate::theme;
use dragon_engine::effects::{self, ParamKind};
use dragon_engine::model::*;
use egui::{RichText, Ui};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub fn show(ctx: &egui::Context, st: &mut State) {
    let effects_open = st.ui.effect_windows.clone();
    for (m, i) in effects_open {
        let mut open = true;
        let title = effect_title(st, m, i);
        egui::Window::new(title).id(egui::Id::new(("fx", m, i))).open(&mut open).resizable(true).default_width(470.0).show(ctx, |ui| effect_editor(ui, st, m, i));
        if !open || st.session.project.mixer.get(m).and_then(|c| c.inserts.iter().find(|x| x.id == i)).is_none() {
            st.ui.effect_windows.retain(|w| *w != (m, i));
        }
    }
    let inst = st.ui.instrument_windows.clone();
    for c in inst {
        let mut open = true;
        let title = st.session.project.channel(c).map(|x| format!("{} – {}", x.name, x.instrument.kind_name())).unwrap_or_default();
        egui::Window::new(title).id(egui::Id::new(("inst", c))).open(&mut open).resizable(true).default_width(460.0).show(ctx, |ui| instrument_editor(ui, st, c));
        if !open || st.session.project.channel(c).is_none() {
            st.ui.instrument_windows.retain(|w| *w != c);
        }
    }
    if let Some(cid) = st.ui.clip_window {
        let mut open = true;
        egui::Window::new("Audio clip").id(egui::Id::new("clipwin")).open(&mut open).default_width(320.0).show(ctx, |ui| clip_editor(ui, st, cid));
        if !open {
            st.ui.clip_window = None;
        }
    }
    if st.session.in_gesture() && !ctx.input(|i| i.pointer.any_down()) && !ctx.wants_keyboard_input() {
        st.session.end_gesture();
    }
}

fn effect_title(st: &State, m: Id, i: Id) -> String {
    let p = &st.session.project;
    let ch = p.mixer.get(m).map(|c| c.name.clone()).unwrap_or_default();
    let ins = p.mixer.get(m).and_then(|c| c.inserts.iter().find(|x| x.id == i));
    let name = ins.map(|x| x.plugin.as_ref().map(|p| p.name.clone()).unwrap_or_else(|| effects::find(&x.effect).map(|d| d.name.to_string()).unwrap_or(x.effect.clone()))).unwrap_or_default();
    format!("{name} · {ch}")
}

fn with_insert(p: &mut Project, m: Id, i: Id, f: impl FnOnce(&mut Insert)) {
    if let Some(x) = p.mixer.get_mut(m).and_then(|c| c.inserts.iter_mut().find(|x| x.id == i)) {
        f(x);
    }
}

fn presets_dir(st: &State, effect: &str) -> PathBuf {
    st.ctx.paths.config.join("Presets").join(effect)
}

fn effect_editor(ui: &mut Ui, st: &mut State, m: Id, i: Id) {
    let Some(ins) = st.session.project.mixer.get(m).and_then(|c| c.inserts.iter().find(|x| x.id == i)).cloned() else { return };
    let gr = st.audio.meters().snapshot().gain_reduction.get(&i).copied().unwrap_or(0.0);
    let cat = effects::find(&ins.effect).map(|d| d.category).unwrap_or("Plugin");
    let ccol = super::mixer::category_color(cat);
    // Header: category stripe, power LED, mix and gain reduction.
    egui::Frame::NONE.fill(theme::PANEL_2).corner_radius(6.0).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
        ui.horizontal(|ui| {
            let (lr, lresp) = ui.allocate_exact_size(egui::Vec2::splat(22.0), egui::Sense::click());
            let led = if ins.enabled { theme::OK } else { theme::GRID_BAR };
            if ins.enabled {
                ui.painter().circle_filled(lr.center(), 10.0, led.gamma_multiply(0.25));
            }
            ui.painter().circle_filled(lr.center(), 6.5, led);
            if lresp.on_hover_text(if ins.enabled { "On – click to bypass" } else { "Bypassed – click to enable" }).clicked() {
                let en = !ins.enabled;
                st.session.edit("Bypass effect", |p| with_insert(p, m, i, |x| x.enabled = en));
            }
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(RichText::new(cat.to_uppercase()).size(9.5).color(ccol).strong());
                ui.label(RichText::new(effect_title(st, m, i)).size(14.0).strong());
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut mix = ins.mix;
                let r = widgets::knob_colored(ui, &mut mix, 0.0, 1.0, 1.0, "mix", widgets::pct, ccol);
                widgets::apply(st, &r, "Effect mix", |p| with_insert(p, m, i, |x| x.mix = mix));
                if gr > 0.05 {
                    ui.label(RichText::new(format!("GR −{gr:.1} dB")).color(theme::WARN).monospace());
                }
            });
        });
    });
    let def = effects::find(&ins.effect);
    if def.map(|d| d.uses_sidechain).unwrap_or(ins.plugin.is_some()) {
        let names: Vec<(Id, String)> = st.session.project.mixer.channels.iter().filter(|c| c.id != m).map(|c| (c.id, c.name.clone())).collect();
        let cur = ins.sidechain.and_then(|s| names.iter().find(|n| n.0 == s)).map(|n| n.1.clone()).unwrap_or("none".into());
        egui::ComboBox::from_label("Sidechain input").selected_text(cur).show_ui(ui, |ui| {
            if ui.selectable_label(ins.sidechain.is_none(), "none").clicked() {
                st.session.edit("Sidechain", |p| with_insert(p, m, i, |x| x.sidechain = None));
            }
            for (id, n) in &names {
                if ui.selectable_label(ins.sidechain == Some(*id), n).clicked() {
                    let s = *id;
                    st.session.edit("Sidechain", |p| with_insert(p, m, i, |x| x.sidechain = Some(s)));
                }
            }
        });
    }
    ui.separator();
    if let Some(pl) = &ins.plugin {
        plugin_params(ui, st, pl.clone(), PluginSlot::Insert(m, i));
        return;
    }
    let Some(def) = def else {
        ui.label("Unknown effect");
        return;
    };
    let time = if st.settings.reduced_motion { None } else { Some(ui.input(|i| i.time) as f32) };
    let (edits, _) = super::fx_ui::hero(ui, &super::fx_ui::Hero { def, params: &ins.params, gr, enabled: ins.enabled, color: ccol, time });
    if !edits.is_empty() {
        st.session.gesture(def.name, |p| {
            with_insert(p, m, i, |x| {
                for (k, v) in edits {
                    x.params.insert(k, v);
                }
            })
        });
    }
    if time.is_some() && ins.enabled {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
    }
    ui.label(RichText::new(def.description).small().color(theme::TEXT_DIM));
    // Group "Band 3 freq" / "Band 3 gain" … into one row per band, and parameters sharing a first word
    // ("Early …", "Late …") into a section; other parameters share a row.
    let first_word_count = |w: &str| def.params.iter().filter(|p| p.name.split(' ').next() == Some(w)).count();
    let group_of = |name: &str| -> String {
        let mut it = name.split(' ');
        match (it.next(), it.next()) {
            (Some(a), Some(b)) if b.chars().all(|c| c.is_ascii_digit()) => format!("{a} {b}"),
            (Some(a), Some(_)) if first_word_count(a) >= 3 => a.to_string(),
            _ => String::new(),
        }
    };
    let mut groups: Vec<(String, Vec<&effects::ParamInfo>)> = Vec::new();
    for prm in def.params {
        let g = group_of(prm.name);
        match groups.iter_mut().find(|x| x.0 == g) {
            Some(x) => x.1.push(prm),
            None => groups.push((g, vec![prm])),
        }
    }
    for (group, params) in groups {
        // The reverb's early and late parts: a heading and the response of the part's EQ.
        if def.id == "reverb" && (group == "Early" || group == "Late") {
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(RichText::new(if group == "Early" { "Early reflections" } else { "Late reverb" }).strong());
                reverb_part_curve(ui, &ins.params, if group == "Early" { "er" } else { "late" });
            });
        }
        egui::Frame::NONE.fill(theme::PANEL).corner_radius(6.0).inner_margin(egui::Margin::symmetric(6, 4)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                if !group.is_empty() {
                    ui.add_sized([46.0, 20.0], egui::Label::new(RichText::new(&group).small().strong().color(ccol)));
                }
                for prm in params {
                    let short = prm.name.strip_prefix(group.as_str()).map(str::trim).filter(|x| !x.is_empty()).unwrap_or(prm.name);
                    let mut v = effects::value(def, &ins.params, prm.id);
                    let key = prm.id.to_string();
                    match prm.kind {
                        ParamKind::Choice(names) => {
                            let mut idx = v.round() as usize;
                            egui::ComboBox::from_id_salt(("fxc", i, prm.id)).selected_text(format!("{}: {}", short, names.get(idx).copied().unwrap_or("?"))).show_ui(ui, |ui| {
                                for (k, n) in names.iter().enumerate() {
                                    ui.selectable_value(&mut idx, k, *n);
                                }
                            });
                            if idx != v.round() as usize {
                                st.session.edit(prm.name, |p| {
                                    with_insert(p, m, i, |x| {
                                        x.params.insert(key, idx as f32);
                                    })
                                });
                            }
                        }
                        ParamKind::Toggle => {
                            let mut b = v > 0.5;
                            if ui.checkbox(&mut b, short).changed() {
                                st.session.edit(prm.name, |p| {
                                    with_insert(p, m, i, |x| {
                                        x.params.insert(key, if b { 1.0 } else { 0.0 });
                                    })
                                });
                            }
                        }
                        _ => {
                            let mut n = prm.normalize(v);
                            let r = widgets::knob_colored(ui, &mut n, 0.0, 1.0, prm.normalize(prm.default), short, |x| prm.format(prm.denormalize(x)), ccol);
                            v = prm.denormalize(n);
                            widgets::apply(st, &r, prm.name, |p| {
                                with_insert(p, m, i, |x| {
                                    x.params.insert(key.clone(), v);
                                })
                            });
                            let target = Target::InsertParam { channel: m, insert: i, param: key };
                            r.context_menu(|ui| {
                                if ui.button("MIDI learn").clicked() {
                                    st.ui.midi_learn = Some(target.clone());
                                    ui.close_menu();
                                }
                                if ui.button("Create automation lane").clicked() {
                                    crate::views::automation::add_lane(st, target.clone());
                                    ui.close_menu();
                                }
                            });
                        }
                    }
                }
            });
        });
    }
    ui.separator();
    // Presets.
    ui.horizontal(|ui| {
        ui.label("Preset");
        let dir = presets_dir(st, def.id);
        let list: Vec<PathBuf> = std::fs::read_dir(&dir).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect()).unwrap_or_default();
        egui::ComboBox::from_id_salt(("preset", i)).selected_text(if ins.preset.is_empty() { "(default)".to_string() } else { ins.preset.clone() }).show_ui(ui, |ui| {
            if ui.selectable_label(false, "(default)").clicked() {
                let d = def.defaults();
                st.session.edit("Load preset", |p| {
                    with_insert(p, m, i, |x| {
                        x.params = d;
                        x.preset.clear();
                    })
                });
            }
            for f in &list {
                let name = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                if ui.selectable_label(ins.preset == name, &name).clicked() {
                    if let Some(params) = st.ctx.paths.load_from::<BTreeMap<String, f32>>(f) {
                        st.session.edit("Load preset", |p| {
                            with_insert(p, m, i, |x| {
                                x.params = params;
                                x.preset = name;
                            })
                        });
                    }
                }
            }
        });
        let id = egui::Id::new(("presetname", i));
        let mut name: String = ui.data_mut(|d| d.get_temp(id).unwrap_or_default());
        ui.add(egui::TextEdit::singleline(&mut name).hint_text("name").desired_width(90.0));
        if ui.button("Save").clicked() && !name.trim().is_empty() {
            let _ = std::fs::create_dir_all(&dir);
            st.ctx.paths.save_to(&dir.join(format!("{}.json", crate::state::sanitize(name.trim()))), &ins.params);
            let n = name.trim().to_string();
            st.session.edit("Save preset", |p| with_insert(p, m, i, |x| x.preset = n));
            name.clear();
        }
        ui.data_mut(|d| d.insert_temp(id, name));
    });
}

/// Frequency response of one reverb part's EQ (low cut, high cut, bell).
fn reverb_part_curve(ui: &mut Ui, params: &BTreeMap<String, f32>, prefix: &str) {
    use dragon_engine::dsp::{Biquad, BiquadKind};
    let def = effects::find("reverb").unwrap();
    let v = |id: &str| effects::value(def, params, &format!("{prefix}_{id}"));
    let rate = 48000.0;
    let (lo, hi, freq, gain) = (v("lo"), v("hi"), v("freq"), v("gain"));
    let mut fs: Vec<Biquad> = Vec::new();
    if lo > 20.5 {
        fs.push(Biquad::new(BiquadKind::HighPass, lo, 0.707, 0.0, rate));
    }
    if hi < 19_900.0 {
        fs.push(Biquad::new(BiquadKind::LowPass, hi, 0.707, 0.0, rate));
    }
    if gain.abs() > 0.05 {
        fs.push(Biquad::new(BiquadKind::Peak, freq, 0.8, gain, rate));
    }
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().clamp(120.0, 260.0), 40.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 3.0, theme::BG);
    p.line_segment([rect.left_center(), rect.right_center()], egui::Stroke::new(1.0_f32, theme::GRID));
    let pts: Vec<egui::Pos2> = (0..64)
        .map(|k| {
            let f = 20.0 * 1000f32.powf(k as f32 / 63.0);
            let db: f32 = fs.iter().map(|b| b.response_db(f, rate)).sum::<f32>().clamp(-18.0, 18.0);
            egui::pos2(rect.left() + rect.width() * k as f32 / 63.0, rect.center().y - db / 18.0 * rect.height() * 0.48)
        })
        .collect();
    p.add(egui::Shape::line(pts, egui::Stroke::new(1.5_f32, if prefix == "er" { theme::ACCENT_2 } else { theme::PLAYHEAD })));
}

#[derive(Clone, Copy)]
enum PluginSlot {
    Insert(Id, Id),
    Channel(Id),
}

fn plugin_params(ui: &mut Ui, st: &mut State, pl: PluginRef, slot: PluginSlot) {
    let inst_effect = match slot {
        PluginSlot::Insert(_, i) => st.session.pool.effect_by_id(i).map(Ok),
        PluginSlot::Channel(c) => st.session.pool.instrument_by_id(c).map(Err),
    };
    ui.label(RichText::new(format!("{:?} · {}", pl.format, pl.path.display())).small().color(theme::TEXT_DIM));
    let Some(inst) = inst_effect else {
        ui.colored_label(theme::REC, "The plugin could not be loaded (see the status bar warning).");
        return;
    };
    let (params, values): (Vec<_>, Vec<_>) = match &inst {
        Ok(e) => {
            let g = e.lock().unwrap();
            let ps = g.plugin_params();
            let vs = ps.iter().map(|p| g.plugin_param(p.id).unwrap_or(p.default)).collect();
            (ps, vs)
        }
        Err(i) => {
            let g = i.lock().unwrap();
            let ps = g.plugin_params();
            let vs = ps.iter().map(|p| g.plugin_param(p.id).unwrap_or(p.default)).collect();
            (ps, vs)
        }
    };
    let key = match slot {
        PluginSlot::Insert(m, i) => (m, Some(i)),
        PluginSlot::Channel(c) => (c, None),
    };
    // The plugin's own GUI opens by itself whenever this window appears (FL Studio behaviour); the generic
    // controls below are for plugins without a GUI or as a fallback.
    let seen_id = egui::Id::new(("plugin.window.seen", key));
    let pass = ui.ctx().cumulative_pass_nr();
    let last_seen: Option<u64> = ui.data(|d| d.get_temp(seen_id));
    ui.data_mut(|d| d.insert_temp(seen_id, pass));
    let appeared = last_seen.map(|f| pass.saturating_sub(f) > 2).unwrap_or(true);
    let error_id = egui::Id::new(("plugin.window.error", key));
    let open_gui = |st: &mut State, ui: &Ui| -> bool {
        let (ok, err) = match &inst {
            Ok(e) => {
                let mut g = e.lock().unwrap();
                let ok = g.editor(true);
                (ok, g.editor_error())
            }
            Err(i) => {
                let mut g = i.lock().unwrap();
                let ok = g.editor(true);
                (ok, g.editor_error())
            }
        };
        if ok {
            if !st.ui.native_editors.contains(&key) {
                st.ui.native_editors.push(key);
            }
            crate::views::quests::event(st, crate::views::quests::Event::PluginGui);
            ui.data_mut(|d| d.remove::<String>(error_id));
        } else {
            ui.data_mut(|d| d.insert_temp(error_id, err.unwrap_or_else(|| "the plugin has no editor the host can open on this platform".into())));
        }
        ok
    };
    if appeared && !st.ui.native_editors.contains(&key) {
        open_gui(st, ui);
    }
    let gui_open = st.ui.native_editors.contains(&key);
    let error: Option<String> = ui.data(|d| d.get_temp(error_id));
    ui.horizontal(|ui| {
        if gui_open {
            let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter().circle_filled(dot.center(), 4.0, theme::OK);
            ui.label(RichText::new("Plugin window open").color(theme::OK));
            if ui.button("Close plugin window").clicked() {
                match &inst {
                    Ok(e) => e.lock().unwrap().editor(false),
                    Err(i) => i.lock().unwrap().editor(false),
                };
                st.ui.native_editors.retain(|k| *k != key);
            }
        } else if ui.button("Open plugin window").on_hover_text("The plugin's own interface").clicked() && !open_gui(st, ui) {
            let why: String = ui.data(|d| d.get_temp(error_id)).unwrap_or_default();
            st.toast(format!("The plugin window could not be opened: {why}"), redhive_contracts::ui::Severity::Warning);
        }
        if ui.button("Store state in project").on_hover_text("Saves the plugin's full internal state (also done automatically on save)").clicked() {
            st.capture_plugin_states();
        }
    });
    if let (false, Some(e)) = (gui_open, &error) {
        ui.label(RichText::new(format!("Plugin window unavailable: {e}. Use the generic controls below.")).small().color(theme::WARN));
    }
    let generic = egui::CollapsingHeader::new(format!("Generic controls ({})", params.len())).id_salt(("plugin.generic", key)).default_open(!gui_open && error.is_some()).open(if appeared {
        Some(!gui_open && error.is_some())
    } else {
        None
    });
    generic.show(ui, |ui| {
        let filter_id = egui::Id::new(("plfilter", pl.id.clone()));
        let mut filter: String = ui.data_mut(|d| d.get_temp(filter_id).unwrap_or_default());
        ui.add(egui::TextEdit::singleline(&mut filter).hint_text("filter parameters…"));
        ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            if params.is_empty() {
                ui.label("The plugin exposes no parameters.");
            }
            for (prm, v) in params.iter().zip(values) {
                if !filter.is_empty() && !prm.name.to_lowercase().contains(&filter.to_lowercase()) {
                    continue;
                }
                let mut val = pl.params.get(&prm.id).copied().unwrap_or(v) as f32;
                let r = ui.add(egui::Slider::new(&mut val, 0.0..=1.0).text(&prm.name));
                let pid = prm.id;
                widgets::apply(st, &r, "Plugin parameter", |p| {
                    let r = match slot {
                        PluginSlot::Insert(m, i) => p.mixer.get_mut(m).and_then(|c| c.inserts.iter_mut().find(|x| x.id == i)).and_then(|x| x.plugin.as_mut()),
                        PluginSlot::Channel(c) => match p.channel_mut(c).map(|c| &mut c.instrument) {
                            Some(Instrument::Plugin(r)) => Some(r),
                            _ => None,
                        },
                    };
                    if let Some(r) = r {
                        r.params.insert(pid, val as f64);
                    }
                });
                if prm.automatable {
                    let target = Target::PluginParam {
                        channel: match slot {
                            PluginSlot::Insert(m, _) => m,
                            PluginSlot::Channel(c) => c,
                        },
                        insert: match slot {
                            PluginSlot::Insert(_, i) => Some(i),
                            _ => None,
                        },
                        param: pid,
                    };
                    r.context_menu(|ui| {
                        if ui.button("MIDI learn").clicked() {
                            st.ui.midi_learn = Some(target.clone());
                            ui.close_menu();
                        }
                        if ui.button("Create automation lane").clicked() {
                            crate::views::automation::add_lane(st, target.clone());
                            ui.close_menu();
                        }
                    });
                }
            }
        });
    });
}

fn with_channel(p: &mut Project, c: Id, f: impl FnOnce(&mut RackChannel)) {
    if let Some(x) = p.channel_mut(c) {
        f(x);
    }
}

fn env_editor(ui: &mut Ui, st: &mut State, label: &str, env: Envelope, set: impl Fn(&mut Project, Envelope) + Copy) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).small().color(theme::TEXT_DIM));
        let mut e = env;
        if ui.checkbox(&mut e.enabled, "on").changed() {
            st.session.edit(label, |p| set(p, e));
        }
    });
    ui.horizontal_wrapped(|ui| {
        let secs = |v: f32| if v < 1.0 { format!("{:.0} ms", v * 1000.0) } else { format!("{v:.2} s") };
        let fields: [(&str, f32, f32, f32); 6] =
            [("delay", env.delay, 0.0, 2.0), ("attack", env.attack, 0.0, 4.0), ("hold", env.hold, 0.0, 2.0), ("decay", env.decay, 0.0, 4.0), ("sustain", env.sustain, 0.0, 1.0), ("release", env.release, 0.0, 8.0)];
        for (k, (name, v, min, max)) in fields.into_iter().enumerate() {
            let mut x = v;
            let r = if name == "sustain" { knob(ui, &mut x, min, max, 1.0, name, widgets::pct) } else { knob(ui, &mut x, min, max, 0.0, name, secs) };
            if r.changed() {
                let mut e = env;
                match k {
                    0 => e.delay = x,
                    1 => e.attack = x,
                    2 => e.hold = x,
                    3 => e.decay = x,
                    4 => e.sustain = x,
                    _ => e.release = x,
                }
                widgets::apply(st, &r, label, |p| set(p, e));
            } else {
                widgets::apply(st, &r, label, |_| {});
            }
        }
    });
}

fn instrument_editor(ui: &mut Ui, st: &mut State, cid: Id) {
    let Some(ch) = st.session.project.channel(cid).cloned() else { return };
    let ccol = theme::rgb(ch.color);
    egui::Frame::NONE.fill(theme::PANEL_2).corner_radius(6.0).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
        ui.horizontal(|ui| {
            let (bar, _) = ui.allocate_exact_size(egui::vec2(5.0, 40.0), egui::Sense::hover());
            ui.painter().rect_filled(bar, 2.0, ccol);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(RichText::new(ch.instrument.kind_name().to_uppercase()).size(9.5).color(ccol).strong());
                let mut name = ch.name.clone();
                if ui.add(egui::TextEdit::singleline(&mut name).desired_width(140.0).font(egui::TextStyle::Heading)).changed() {
                    st.session.gesture("Rename channel", |p| with_channel(p, cid, |c| c.name = name));
                }
            });
            let mut col = [ch.color.0, ch.color.1, ch.color.2];
            if ui.color_edit_button_srgb(&mut col).changed() {
                st.session.gesture("Colour", |p| with_channel(p, cid, |c| c.color = Rgb(col[0], col[1], col[2])));
            }
            let mut pitch = ch.pitch;
            let r = knob(ui, &mut pitch, -24.0, 24.0, 0.0, "pitch", |v| format!("{v:+.2} st"));
            widgets::apply(st, &r, "Channel pitch", |p| with_channel(p, cid, |c| c.pitch = pitch));
            let mut vol = ch.volume;
            let r = knob(ui, &mut vol, 0.0, 1.25, 0.8, "volume", widgets::db_fmt);
            widgets::apply(st, &r, "Channel volume", |p| with_channel(p, cid, |c| c.volume = vol));
            let mut pan = ch.pan;
            let r = knob(ui, &mut pan, -1.0, 1.0, 0.0, "pan", widgets::pan_fmt);
            widgets::apply(st, &r, "Channel pan", |p| with_channel(p, cid, |c| c.pan = pan));
        });
    });
    ui.horizontal(|ui| {
        let (mut cg, mut cb, mut cs) = (ch.cut_group, ch.cut_by, ch.cut_self);
        let a = ui.add(egui::DragValue::new(&mut cg).range(0..=16).prefix("cut group "));
        let b = ui.add(egui::DragValue::new(&mut cb).range(0..=16).prefix("cut by "));
        let c = ui.checkbox(&mut cs, "cut itself");
        if a.changed() || b.changed() || c.changed() {
            st.session.edit("Cut groups", |p| {
                with_channel(p, cid, |x| {
                    x.cut_group = cg;
                    x.cut_by = cb;
                    x.cut_self = cs;
                })
            });
        }
    });
    ui.separator();
    match ch.instrument.clone() {
        Instrument::Sampler(s) => sampler_editor(ui, st, cid, s, ch.envelope),
        Instrument::Synth(s) => synth_editor(ui, st, cid, s),
        Instrument::DrumPads(pads) => drum_editor(ui, st, cid, pads),
        Instrument::Plugin(pl) => plugin_params(ui, st, pl, PluginSlot::Channel(cid)),
    }
}

fn set_sampler(p: &mut Project, cid: Id, f: impl FnOnce(&mut SamplerParams)) {
    if let Some(Instrument::Sampler(s)) = p.channel_mut(cid).map(|c| &mut c.instrument) {
        f(s);
    }
}

fn sampler_editor(ui: &mut Ui, st: &mut State, cid: Id, s: SamplerParams, env: Envelope) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(s.path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or("(no sample)".into())).strong());
        if ui.button("Load…").clicked() {
            if let Some(f) = rfd::FileDialog::new().add_filter("Audio", &["wav", "flac", "mp3", "ogg", "aif", "aiff"]).pick_file() {
                st.session.edit("Load sample", |p| set_sampler(p, cid, |x| x.path = f));
            }
        }
        if ui.button("Slice to new channel").clicked() {
            st.ui.channel = Some(cid);
            st.ctx.run("dragon.tools.slice", serde_json::Value::Null);
        }
    });
    // Waveform with start / end.
    if let Some(w) = st.waveform(&s.path) {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 70.0), egui::Sense::click_and_drag());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 3.0, theme::BG);
        let n = w.len().max(1);
        for x in 0..rect.width() as usize {
            let b0 = x * n / rect.width() as usize;
            let b1 = ((x + 1) * n / rect.width() as usize).max(b0 + 1).min(n);
            let (lo, hi) = w[b0.min(n - 1)..b1].iter().fold((0.0f32, 0.0f32), |(l, h), (a, b)| (l.min(*a), h.max(*b)));
            let xx = rect.left() + x as f32;
            p.line_segment([egui::pos2(xx, rect.center().y - hi * rect.height() * 0.48), egui::pos2(xx, rect.center().y - lo * rect.height() * 0.48)], egui::Stroke::new(1.0_f32, theme::ACCENT_2));
        }
        let sx = rect.left() + s.start * rect.width();
        let ex = rect.left() + s.end * rect.width();
        p.rect_filled(egui::Rect::from_x_y_ranges(rect.left()..=sx, rect.y_range()), 0.0, egui::Color32::from_black_alpha(140));
        p.rect_filled(egui::Rect::from_x_y_ranges(ex..=rect.right(), rect.y_range()), 0.0, egui::Color32::from_black_alpha(140));
        for sl in &s.slices {
            let x = rect.left() + sl * rect.width();
            p.line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())], egui::Stroke::new(1.0_f32, theme::PLAYHEAD));
        }
        if let Some(pos) = resp.interact_pointer_pos() {
            if resp.dragged() || resp.clicked() {
                let v = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                let start = (pos.x - sx).abs() < (pos.x - ex).abs();
                st.session.gesture("Sample range", |p| set_sampler(p, cid, |x| if start { x.start = v.min(x.end - 0.001) } else { x.end = v.max(x.start + 0.001) }));
            }
        }
        if let Some(path) = resp.dnd_release_payload::<PathBuf>() {
            let path = (*path).clone();
            st.session.edit("Load sample", |p| set_sampler(p, cid, |x| x.path = path));
        }
    }
    ui.horizontal_wrapped(|ui| {
        let mut root = s.root_note as f32;
        let r = knob(ui, &mut root, 0.0, 127.0, 60.0, "root", |v| dragon_engine::instruments::note_name(v.round() as u8));
        widgets::apply(st, &r, "Root note", |p| set_sampler(p, cid, |x| x.root_note = root.round() as u8));
        let mut fine = s.fine;
        let r = knob(ui, &mut fine, -1.0, 1.0, 0.0, "fine", |v| format!("{:+.0} ct", v * 100.0));
        widgets::apply(st, &r, "Fine tune", |p| set_sampler(p, cid, |x| x.fine = fine));
        let mut glide = s.glide;
        let r = knob(ui, &mut glide, 0.0, 1.0, 0.0, "glide", |v| format!("{:.0} ms", v * 1000.0));
        widgets::apply(st, &r, "Glide", |p| set_sampler(p, cid, |x| x.glide = glide));
        let (mut lp, mut rev) = (s.looped, s.reverse);
        if ui.checkbox(&mut lp, "loop").changed() {
            st.session.edit("Loop sample", |p| set_sampler(p, cid, |x| x.looped = lp));
        }
        if ui.checkbox(&mut rev, "reverse").changed() {
            st.session.edit("Reverse sample", |p| set_sampler(p, cid, |x| x.reverse = rev));
        }
    });
    env_editor(ui, st, "Volume envelope", env, move |p, e| with_channel(p, cid, |c| c.envelope = e));
}

fn set_synth(p: &mut Project, cid: Id, f: impl FnOnce(&mut SynthParams)) {
    if let Some(Instrument::Synth(s)) = p.channel_mut(cid).map(|c| &mut c.instrument) {
        f(s);
    }
}

fn synth_editor(ui: &mut Ui, st: &mut State, cid: Id, s: SynthParams) {
    let time = if st.settings.reduced_motion { None } else { Some(ui.input(|i| i.time) as f32) };
    let color = st.session.project.channel(cid).map(|c| theme::rgb(c.color)).unwrap_or(theme::ACCENT);
    super::fx_ui::synth_hero(ui, &s, color, time);
    if time.is_some() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
    }
    for k in 0..3 {
        let o = s.osc[k];
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("OSC {}", k + 1)).small().color(theme::ACCENT_2));
            let mut w = o.wave;
            egui::ComboBox::from_id_salt(("osc", cid, k)).selected_text(format!("{w:?}")).width(80.0).show_ui(ui, |ui| {
                for x in [Waveform::Saw, Waveform::Square, Waveform::Triangle, Waveform::Sine, Waveform::Noise] {
                    ui.selectable_value(&mut w, x, format!("{x:?}"));
                }
            });
            if w != o.wave {
                st.session.edit("Waveform", |p| set_synth(p, cid, |x| x.osc[k].wave = w));
            }
            let mut lvl = o.level;
            let r = knob(ui, &mut lvl, 0.0, 1.0, 0.0, "level", widgets::pct);
            widgets::apply(st, &r, "Osc level", |p| set_synth(p, cid, |x| x.osc[k].level = lvl));
            let mut co = o.coarse as f32;
            let r = knob(ui, &mut co, -36.0, 36.0, 0.0, "coarse", |v| format!("{:+.0} st", v.round()));
            widgets::apply(st, &r, "Osc coarse", |p| set_synth(p, cid, |x| x.osc[k].coarse = co.round() as i32));
            let mut fi = o.fine;
            let r = knob(ui, &mut fi, -1.0, 1.0, 0.0, "fine", |v| format!("{:+.0} ct", v * 100.0));
            widgets::apply(st, &r, "Osc fine", |p| set_synth(p, cid, |x| x.osc[k].fine = fi));
            let mut pw = o.pulse_width;
            let r = knob(ui, &mut pw, 0.05, 0.95, 0.5, "pw", widgets::pct);
            widgets::apply(st, &r, "Pulse width", |p| set_synth(p, cid, |x| x.osc[k].pulse_width = pw));
        });
    }
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        let mut f = s.filter;
        egui::ComboBox::from_id_salt(("flt", cid)).selected_text(format!("{f:?}")).width(90.0).show_ui(ui, |ui| {
            for x in [FilterMode::LowPass, FilterMode::HighPass, FilterMode::BandPass] {
                ui.selectable_value(&mut f, x, format!("{x:?}"));
            }
        });
        if f != s.filter {
            st.session.edit("Filter mode", |p| set_synth(p, cid, |x| x.filter = f));
        }
        let hz = |v: f32| if v >= 1000.0 { format!("{:.1} kHz", v / 1000.0) } else { format!("{v:.0} Hz") };
        let mut cut = (s.cutoff.max(20.0) / 20.0).log2() / 10.0;
        let r = knob(ui, &mut cut, 0.0, 1.0, 1.0, "cutoff", |v| hz(20.0 * 2f32.powf(v * 10.0)));
        let cutoff = 20.0 * 2f32.powf(cut * 10.0);
        widgets::apply(st, &r, "Cutoff", |p| set_synth(p, cid, |x| x.cutoff = cutoff));
        r.context_menu(|ui| {
            if ui.button("Create automation lane").clicked() {
                crate::views::automation::add_lane(st, Target::SynthCutoff(cid));
                ui.close_menu();
            }
            if ui.button("MIDI learn").clicked() {
                st.ui.midi_learn = Some(Target::SynthCutoff(cid));
                ui.close_menu();
            }
        });
        let fields: [(&str, f32, f32, f32, &str); 9] = [
            ("reso", s.resonance, 0.0, 1.0, "resonance"),
            ("env amt", s.filter_env, -1.0, 1.0, "filter_env"),
            ("unison", s.unison as f32, 1.0, 8.0, "unison"),
            ("detune", s.detune, 0.0, 1.0, "detune"),
            ("lfo rate", s.lfo_rate, 0.0, 20.0, "lfo_rate"),
            ("lfo➡pitch", s.lfo_pitch, 0.0, 1.0, "lfo_pitch"),
            ("lfo➡filter", s.lfo_filter, 0.0, 1.0, "lfo_filter"),
            ("glide", s.glide, 0.0, 1.0, "glide"),
            ("gain", s.gain, 0.0, 2.0, "gain"),
        ];
        for (label, v, min, max, key) in fields {
            let mut x = v;
            let r = knob(ui, &mut x, min, max, v, label, |v| format!("{v:.2}"));
            widgets::apply(st, &r, label, |p| {
                set_synth(p, cid, |s| match key {
                    "resonance" => s.resonance = x,
                    "filter_env" => s.filter_env = x,
                    "unison" => s.unison = x.round().max(1.0) as u32,
                    "detune" => s.detune = x,
                    "lfo_rate" => s.lfo_rate = x,
                    "lfo_pitch" => s.lfo_pitch = x,
                    "lfo_filter" => s.lfo_filter = x,
                    "glide" => s.glide = x,
                    _ => s.gain = x,
                })
            });
        }
        let mut poly = s.polyphony;
        if ui.add(egui::DragValue::new(&mut poly).range(1..=32).prefix("voices ")).changed() {
            st.session.edit("Polyphony", |p| set_synth(p, cid, |x| x.polyphony = poly));
        }
    });
    env_editor(ui, st, "Filter envelope", s.filter_adsr, move |p, e| set_synth(p, cid, |x| x.filter_adsr = e));
    env_editor(ui, st, "Amp envelope", s.amp_adsr, move |p, e| set_synth(p, cid, |x| x.amp_adsr = e));
}

fn drum_editor(ui: &mut Ui, st: &mut State, cid: Id, pads: Vec<Pad>) {
    ui.label(RichText::new("Drop samples on pads · click to audition · right-click for options").small().color(theme::TEXT_DIM));
    egui::Grid::new(("pads", cid)).spacing([6.0, 6.0]).show(ui, |ui| {
        for (k, pad) in pads.iter().enumerate() {
            let has = !pad.layers.is_empty();
            let b = egui::Button::new(RichText::new(format!("{}\n{}", pad.name, dragon_engine::instruments::note_name(pad.note))).small())
                .fill(if has { theme::ACCENT.gamma_multiply(0.5) } else { theme::PANEL_2 })
                .min_size(egui::vec2(96.0, 46.0));
            let r = ui.add(b);
            if r.clicked() {
                st.audio.send(dragon_engine::device::Command::Live(dragon_engine::render::LiveEvent::NoteOn { channel: cid, note: pad.note, velocity: 0.9 }));
                st.audio.send(dragon_engine::device::Command::Live(dragon_engine::render::LiveEvent::NoteOff { channel: cid, note: pad.note }));
            }
            if let Some(path) = r.dnd_release_payload::<PathBuf>() {
                let path = (*path).clone();
                let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                st.session.edit("Assign pad", |p| {
                    if let Some(Instrument::DrumPads(v)) = p.channel_mut(cid).map(|c| &mut c.instrument) {
                        v[k].layers = vec![PadLayer { path, velocity_low: 0.0, velocity_high: 1.0 }];
                        v[k].name = name;
                    }
                });
            }
            r.context_menu(|ui| {
                if ui.button("Load sample…").clicked() {
                    if let Some(f) = rfd::FileDialog::new().add_filter("Audio", &["wav", "flac", "mp3", "ogg", "aif", "aiff"]).pick_file() {
                        st.session.edit("Assign pad", |p| {
                            if let Some(Instrument::DrumPads(v)) = p.channel_mut(cid).map(|c| &mut c.instrument) {
                                v[k].name = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                                v[k].layers = vec![PadLayer { path: f, velocity_low: 0.0, velocity_high: 1.0 }];
                            }
                        });
                    }
                    ui.close_menu();
                }
                if ui.button("Add velocity layer…").clicked() {
                    if let Some(f) = rfd::FileDialog::new().add_filter("Audio", &["wav", "flac", "mp3", "ogg", "aif", "aiff"]).pick_file() {
                        st.session.edit("Add layer", |p| {
                            if let Some(Instrument::DrumPads(v)) = p.channel_mut(cid).map(|c| &mut c.instrument) {
                                let n = v[k].layers.len() as f32 + 1.0;
                                v[k].layers.push(PadLayer { path: f, velocity_low: 0.0, velocity_high: 1.0 });
                                for (i, l) in v[k].layers.iter_mut().enumerate() {
                                    l.velocity_low = i as f32 / n;
                                    l.velocity_high = (i + 1) as f32 / n;
                                }
                            }
                        });
                    }
                    ui.close_menu();
                }
                let mut pad2 = pad.clone();
                let mut changed = ui.add(egui::Slider::new(&mut pad2.gain, 0.0..=2.0).text("gain")).changed();
                changed |= ui.add(egui::Slider::new(&mut pad2.pan, -1.0..=1.0).text("pan")).changed();
                changed |= ui.add(egui::Slider::new(&mut pad2.pitch, -24.0..=24.0).text("pitch")).changed();
                changed |= ui.add(egui::Slider::new(&mut pad2.choke, 0..=8).text("choke group")).changed();
                changed |= ui.add(egui::Slider::new(&mut pad2.note, 0..=127).text("note")).changed();
                if changed {
                    st.session.gesture("Pad settings", |p| {
                        if let Some(Instrument::DrumPads(v)) = p.channel_mut(cid).map(|c| &mut c.instrument) {
                            v[k] = pad2;
                        }
                    });
                }
            });
            if k % 4 == 3 {
                ui.end_row();
            }
        }
    });
}

fn clip_editor(ui: &mut Ui, st: &mut State, cid: Id) {
    let Some((tid, clip)) = st.session.project.tracks.iter().find_map(|t| t.clips.iter().find(|c| c.id == cid).map(|c| (t.id, c.clone()))) else {
        st.ui.clip_window = None;
        return;
    };
    let Some(a) = clip.audio().cloned() else { return };
    let set = move |p: &mut Project, f: &dyn Fn(&mut Clip)| {
        if let Some(c) = p.track_mut(tid).and_then(|t| t.clips.iter_mut().find(|c| c.id == cid)) {
            f(c);
        }
    };
    ui.label(RichText::new(a.source.display().to_string()).small().color(theme::TEXT_DIM));
    let mut name = clip.name.clone();
    if ui.text_edit_singleline(&mut name).changed() {
        st.session.gesture("Rename clip", |p| set(p, &|c| c.name = name.clone()));
    }
    egui::Grid::new("clipgrid").num_columns(2).show(ui, |ui| {
        let mut g = a.gain_db;
        ui.label("Gain");
        let r = ui.add(egui::Slider::new(&mut g, -48.0..=24.0).suffix(" dB"));
        widgets::apply(st, &r, "Clip gain", |p| {
            set(p, &|c| {
                if let Some(x) = c.audio_mut() {
                    x.gain_db = g
                }
            })
        });
        ui.end_row();
        let mut pan = a.pan;
        ui.label("Pan");
        let r = ui.add(egui::Slider::new(&mut pan, -1.0..=1.0));
        widgets::apply(st, &r, "Clip pan", |p| {
            set(p, &|c| {
                if let Some(x) = c.audio_mut() {
                    x.pan = pan
                }
            })
        });
        ui.end_row();
        let mut fi = a.fade_in as f32;
        ui.label("Fade in");
        let r = ui.add(egui::Slider::new(&mut fi, 0.0..=10.0).suffix(" s").logarithmic(true));
        widgets::apply(st, &r, "Fade in", |p| {
            set(p, &|c| {
                if let Some(x) = c.audio_mut() {
                    x.fade_in = fi as f64
                }
            })
        });
        ui.end_row();
        let mut fo = a.fade_out as f32;
        ui.label("Fade out");
        let r = ui.add(egui::Slider::new(&mut fo, 0.0..=10.0).suffix(" s").logarithmic(true));
        widgets::apply(st, &r, "Fade out", |p| {
            set(p, &|c| {
                if let Some(x) = c.audio_mut() {
                    x.fade_out = fo as f64
                }
            })
        });
        ui.end_row();
        ui.label("Fade curve");
        let mut fc = a.fade_curve;
        ui.horizontal(|ui| {
            for (v, n) in [(FadeCurve::Linear, "linear"), (FadeCurve::EqualPower, "equal power"), (FadeCurve::Exponential, "exponential")] {
                ui.selectable_value(&mut fc, v, n);
            }
        });
        if fc != a.fade_curve {
            st.session.edit("Fade curve", |p| {
                set(p, &|c| {
                    if let Some(x) = c.audio_mut() {
                        x.fade_curve = fc
                    }
                })
            });
        }
        ui.end_row();
        let mut pitch = a.pitch;
        ui.label("Pitch");
        let r = ui.add(egui::Slider::new(&mut pitch, -24.0..=24.0).suffix(" st"));
        widgets::apply(st, &r, "Clip pitch", |p| {
            set(p, &|c| {
                if let Some(x) = c.audio_mut() {
                    x.pitch = pitch
                }
            })
        });
        ui.end_row();
        ui.label("Time");
        let mut mode = a.stretch;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut mode, StretchMode::Resample, "resample");
            ui.selectable_value(&mut mode, StretchMode::Stretch, "stretch (keep pitch)");
        });
        if mode != a.stretch {
            st.session.edit("Stretch mode", |p| {
                set(p, &|c| {
                    if let Some(x) = c.audio_mut() {
                        x.stretch = mode
                    }
                })
            });
        }
        ui.end_row();
        let mut speed = a.speed as f32;
        ui.label("Speed");
        let r = ui.add(egui::Slider::new(&mut speed, 0.25..=4.0).logarithmic(true));
        widgets::apply(st, &r, "Clip speed", |p| {
            set(p, &|c| {
                if let Some(x) = c.audio_mut() {
                    x.speed = speed as f64
                }
            })
        });
        ui.end_row();
    });
    ui.horizontal(|ui| {
        let mut rev = a.reverse;
        if ui.checkbox(&mut rev, "Reverse").changed() {
            st.session.edit("Reverse clip", |p| {
                set(p, &|c| {
                    if let Some(x) = c.audio_mut() {
                        x.reverse = rev
                    }
                })
            });
        }
        let mut muted = clip.muted;
        if ui.checkbox(&mut muted, "Muted").changed() {
            st.session.edit("Mute clip", |p| set(p, &|c| c.muted = muted));
        }
        if ui.button("Fit to project tempo").on_hover_text("Detects the clip's tempo and stretches it to the project tempo").clicked() {
            if let Ok(d) = st.session.cache.get(&a.source, st.audio.rate()) {
                let (bpm, _) = dragon_engine::analysis::detect_bpm(&d);
                match bpm {
                    Some(b) => {
                        let target = st.session.project.bpm;
                        st.session.edit("Fit to tempo", |p| {
                            set(p, &|c| {
                                if let Some(x) = c.audio_mut() {
                                    x.source_bpm = Some(b);
                                    x.stretch = StretchMode::Stretch;
                                    x.speed = target / b;
                                }
                            })
                        });
                        st.ui.status = format!("Detected {b:.1} BPM");
                    }
                    None => st.toast("No clear tempo found in this clip", redhive_contracts::ui::Severity::Info),
                }
            }
        }
    });
}
