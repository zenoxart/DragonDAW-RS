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
        // A stock editor fits its content; full screen fills the whole app window (and the app goes full screen).
        let full = st.ui.fx_fullscreen == Some((m, i));
        // Full screen uses a window of its own, so the fitted window keeps its size and place for when you leave.
        let wid = if full { egui::Id::new(("fx.full", m, i)) } else { egui::Id::new(("fx", m, i)) };
        let mut w = egui::Window::new(title).id(wid).open(&mut open).resizable(true).vscroll(true);
        w = if full {
            w.fixed_rect(ctx.screen_rect().shrink(4.0)).collapsible(false)
        } else {
            // Wide editors (all four multiband columns side by side) open wider.
            let wide = st.session.project.mixer.get(m).and_then(|c| c.inserts.iter().find(|x| x.id == i)).is_some_and(|x| x.effect == "multiband");
            w.default_width(if wide { 980.0 } else { 600.0 }).default_height((ctx.screen_rect().height() - 120.0).min(820.0)).min_width(480.0).max_height(ctx.screen_rect().height() - 60.0)
        };
        // The smallest the window may get: wide enough for the panel's widest control and tall enough for
        // all of the editor (as measured last frame), within the screen.
        let need_id = egui::Id::new(("fx.need", m, i));
        if !full {
            if let Some([need_w, need_h, win_w]) = ctx.data(|d| d.get_temp::<[f32; 3]>(need_id)) {
                let screen = ctx.screen_rect();
                let max_h = screen.height() - 120.0;
                // Too tall for the screen: as wide as the screen allows, so the sections pack into fewer rows.
                let grow = if need_h > max_h && win_w < screen.width() - 41.0 { screen.width() - 40.0 } else { 0.0 };
                w = w.min_width(need_w.max(grow).clamp(480.0, screen.width() - 40.0)).min_height(need_h.min(max_h));
            }
        }
        let panel_need = egui::Id::new(super::fx_panel::NEED_KEY);
        ctx.data_mut(|d| d.remove::<f32>(panel_need));
        let mut measured = [0.0f32; 2];
        let shown = w.show(ctx, |ui| {
            effect_editor(ui, st, m, i);
            measured = [ui.ctx().data(|d| d.get_temp::<f32>(panel_need)).unwrap_or(0.0) + 24.0, ui.min_rect().height() + 8.0];
        });
        if let Some(r) = shown {
            ctx.data_mut(|d| d.insert_temp(need_id, [measured[0], measured[1], r.response.rect.width()]));
        }
        if !open || st.session.project.mixer.get(m).and_then(|c| c.inserts.iter().find(|x| x.id == i)).is_none() {
            st.ui.effect_windows.retain(|w| *w != (m, i));
            if st.ui.fx_fullscreen == Some((m, i)) {
                st.ui.fx_fullscreen = None;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            }
        }
    }
    let inst = st.ui.instrument_windows.clone();
    for c in inst {
        let mut open = true;
        let title = st.session.project.channel(c).map(|x| format!("{} – {}", x.name, x.instrument.kind_name())).unwrap_or_default();
        let wide = matches!(st.session.project.channel(c).map(|x| &x.instrument), Some(Instrument::Wavetable(_) | Instrument::Guitar(_) | Instrument::Piano(_)));
        egui::Window::new(title).id(egui::Id::new(("inst", c))).open(&mut open).resizable(true).default_width(if wide { 1300.0 } else { 460.0 }).show(ctx, |ui| instrument_editor(ui, st, c));
        if !open || st.session.project.channel(c).is_none() {
            st.ui.instrument_windows.retain(|w| *w != c);
        }
    }
    if let Some(cid) = st.ui.clip_window {
        let mut open = true;
        egui::Window::new("Audio clip")
            .id(egui::Id::new("clipwin"))
            .open(&mut open)
            .default_width(600.0)
            .default_height(760.0)
            .min_width(460.0)
            .resizable(true)
            .vscroll(true)
            .show(ctx, |ui| super::clip_view::show(ui, st, cid));
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

/// Pro-Q style comparison with another EQ in the project: spectrum collisions on the display, and EQ match
/// (learn both spectra for a while, then fill free bands so this track's tone follows the reference).
fn eq_compare(ui: &mut Ui, st: &mut State, m: Id, i: Id, ins: &Insert) {
    use dragon_engine::dsp::spectrum::BANDS;
    let others: Vec<(Id, String)> = st
        .session
        .project
        .mixer
        .channels
        .iter()
        .chain(std::iter::once(&st.session.project.mixer.master))
        .flat_map(|c| c.inserts.iter().filter(|x| x.effect == "eq" && x.id != i).map(move |x| (x.id, c.name.clone())))
        .collect();
    let ref_key = egui::Id::new(("eq.ref", i));
    let learn_key = egui::Id::new(("eq.learn", i));
    let hero_key = egui::Id::new(("fx.hero", m, i));
    let mut reference: Option<Id> = ui.data(|d| d.get_temp(ref_key)).flatten();
    let data = st.audio.meters().snapshot().fx_data;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Compare with").small());
        let name = reference.and_then(|r| others.iter().find(|o| o.0 == r)).map(|o| format!("{} · EQ", o.1)).unwrap_or("–".into());
        egui::ComboBox::from_id_salt(("eq.refbox", i)).selected_text(name).width(140.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut reference, None, "–");
            for (id, n) in &others {
                ui.selectable_value(&mut reference, Some(*id), format!("{n} · EQ"));
            }
        });
        if others.is_empty() {
            ui.label(RichText::new("(add an EQ on another track to compare or match)").small().color(theme::TEXT_DIM));
        }
        ui.data_mut(|d| d.insert_temp(ref_key, reference));
        // Collisions: hand the reference's output spectrum to the display.
        let other = reference.and_then(|r| data.get(&r)).filter(|t| t.len() >= BANDS * 2).map(|t| t[BANDS..BANDS * 2].to_vec());
        ui.data_mut(|d| d.insert_temp(hero_key.with("collide"), other.clone()));
        // Match: average both spectra while learning.
        // Match references besides another EQ: the sidechain input, or an audio file.
        let file_key = egui::Id::new(("eq.fileref", i));
        let mut file_ref: Option<(String, Vec<f32>)> = ui.data(|d| d.get_temp(file_key)).flatten();
        let mut use_sc: bool = ui.data(|d| d.get_temp(egui::Id::new(("eq.scref", i)))).unwrap_or(false);
        ui.menu_button("Match to…", |ui| {
            if ui.selectable_label(use_sc, "Sidechain input").on_hover_text("Route a track into this EQ's sidechain (mixer) – its sound is the reference").clicked() {
                use_sc = !use_sc;
                file_ref = None;
                ui.close_menu();
            }
            if ui.button("Audio file…").clicked() {
                if let Some(path) = rfd::FileDialog::new().add_filter("Audio", &["wav", "flac", "mp3", "ogg", "aif", "aiff"]).pick_file() {
                    match st.session.cache.get(&path, 48_000) {
                        Ok(d) => {
                            file_ref = Some((path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), dragon_engine::dsp::spectrum::average(&d.l, &d.r, 48_000.0)));
                            use_sc = false;
                        }
                        Err(e) => st.toast(format!("Cannot read the reference: {e}"), redhive_contracts::ui::Severity::Error),
                    }
                }
                ui.close_menu();
            }
        });
        if let Some((name, _)) = &file_ref {
            ui.label(RichText::new(format!("reference: {name}")).small().color(theme::TEXT_DIM));
        } else if use_sc {
            ui.label(RichText::new("reference: sidechain").small().color(theme::TEXT_DIM));
        }
        ui.data_mut(|d| d.insert_temp(file_key, file_ref.clone()));
        ui.data_mut(|d| d.insert_temp(egui::Id::new(("eq.scref", i)), use_sc));
        let sc_spec: Option<Vec<f32>> = if use_sc {
            let o = BANDS * 2 + dragon_engine::effects::eq::EQ_BANDS;
            data.get(&i).filter(|t| t.len() >= o + BANDS).map(|t| t[o..o + BANDS].to_vec())
        } else {
            None
        };
        let live_ref = sc_spec.or(other.clone());
        let mut learn: (bool, u32, Vec<f32>, Vec<f32>) = ui.data(|d| d.get_temp(learn_key)).unwrap_or((false, 0, vec![0.0; BANDS], vec![0.0; BANDS]));
        if reference.is_some() || use_sc || file_ref.is_some() {
            if ui.selectable_label(learn.0, if learn.0 { format!("⏺ Learning ({} s)", learn.1 / 30) } else { "Learn".into() }).on_hover_text("Play the track (and the reference); the spectra are averaged").clicked() {
                learn = if learn.0 { (false, learn.1, learn.2, learn.3) } else { (true, 0, vec![0.0; BANDS], vec![0.0; BANDS]) };
            }
            if learn.0 {
                if let Some(own) = data.get(&i).filter(|t| t.len() >= BANDS) {
                    let rf = file_ref.as_ref().map(|f| f.1.clone()).or(live_ref.clone());
                    if let Some(o) = rf {
                        for b in 0..BANDS {
                            learn.2[b] += own[b];
                            learn.3[b] += o[b];
                        }
                        learn.1 += 1;
                    }
                }
                ui.ctx().request_repaint();
            }
            let ready = !learn.0 && learn.1 > 10;
            if ui.add_enabled(ready, egui::Button::new("Apply match")).on_hover_text("Fills free bands with bells so this track's tone follows the reference").clicked() {
                let n = learn.1 as f32;
                let own: Vec<f32> = learn.2.iter().map(|x| x / n).collect();
                let rf: Vec<f32> = learn.3.iter().map(|x| x / n).collect();
                let def = effects::find("eq").unwrap();
                let free: Vec<usize> = (1..=dragon_engine::effects::eq::EQ_BANDS).filter(|b| effects::value(def, &ins.params, &format!("b{b}_on")) < 0.5).collect();
                let fit = dragon_engine::effects::eq::match_bands(&own, &rf, free.len().min(8), 12.0, 1.0, 48_000.0);
                let count = fit.len();
                st.session.edit("EQ match", |p| {
                    with_insert(p, m, i, |x| {
                        for (b, (f, g, q)) in free.iter().zip(fit) {
                            for (k, v) in [("on", 1.0), ("type", 0.0), ("freq", f), ("gain", (g * 10.0).round() / 10.0), ("q", q), ("stereo", 0.0), ("dyn", 0.0)] {
                                x.params.insert(format!("b{b}_{k}"), v);
                            }
                        }
                    })
                });
                st.ui.status = format!("EQ match: {count} bands");
            }
        }
        ui.data_mut(|d| d.insert_temp(learn_key, learn));
    });
}

/// FabFilter-style modulation: LFOs and envelope followers, each moving one parameter by a depth.
fn modulation(ui: &mut Ui, st: &mut State, m: Id, i: Id, def: &effects::EffectDef, ins: &Insert) {
    use dragon_engine::model::{ModSource, Modulation};
    let header = if ins.mods.is_empty() { "Modulation".to_string() } else { format!("Modulation ({})", ins.mods.len()) };
    egui::CollapsingHeader::new(header).id_salt(("fx.mods", i)).default_open(!ins.mods.is_empty()).show(ui, |ui| {
        let targets: Vec<&effects::ParamInfo> = def.params.iter().filter(|p| !matches!(p.kind, effects::ParamKind::Toggle | effects::ParamKind::Choice(_))).collect();
        let mut mods = ins.mods.clone();
        let mut changed = false;
        let mut remove = None;
        for (k, md) in mods.iter_mut().enumerate() {
            egui::Frame::NONE.fill(theme::PANEL_2).corner_radius(4.0).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (name, color) = match md.source {
                        ModSource::Lfo { .. } => ("LFO", egui::Color32::from_rgb(0x4b, 0xc0, 0xf0)),
                        ModSource::Envelope { .. } => ("ENV", egui::Color32::from_rgb(0x5d, 0xe0, 0xa8)),
                        ModSource::Xy { .. } => ("XY", egui::Color32::from_rgb(0xf5, 0xc8, 0x42)),
                        ModSource::MidiNote { .. } => ("NOTE", egui::Color32::from_rgb(0xc8, 0x7d, 0xf0)),
                        ModSource::MidiVelocity => ("VEL", egui::Color32::from_rgb(0xf0, 0x7d, 0xb4)),
                    };
                    ui.label(RichText::new(format!("{name} {}", k + 1)).strong().color(color));
                    ui.label(">");
                    let cur = targets.iter().find(|p| p.id == md.target).map(|p| p.name).unwrap_or("?");
                    egui::ComboBox::from_id_salt(("fx.mod.t", i, k)).selected_text(cur).width(150.0).show_ui(ui, |ui| {
                        for p in &targets {
                            if ui.selectable_label(p.id == md.target, p.name).clicked() {
                                md.target = p.id.to_string();
                                changed = true;
                            }
                        }
                    });
                    changed |= ui.add(egui::Slider::new(&mut md.depth, -1.0..=1.0).text("depth")).changed();
                    if ui.small_button("🗙").on_hover_text("Remove").clicked() {
                        remove = Some(k);
                    }
                });
                ui.horizontal(|ui| match &mut md.source {
                    ModSource::Lfo { rate, shape, sync } => {
                        egui::ComboBox::from_id_salt(("fx.mod.s", i, k)).selected_text(ModSource::SHAPES[(*shape as usize).min(4)]).width(80.0).show_ui(ui, |ui| {
                            for (n, label) in ModSource::SHAPES.iter().enumerate() {
                                changed |= ui.selectable_value(shape, n as u8, *label).changed();
                            }
                        });
                        changed |= ui.checkbox(sync, "sync").changed();
                        let (lo, hi, unit) = if *sync { (0.0625, 4.0, " /beat") } else { (0.02, 20.0, " Hz") };
                        changed |= ui.add(egui::Slider::new(rate, lo..=hi).logarithmic(true).suffix(unit)).changed();
                    }
                    ModSource::Envelope { attack, release, gain } => {
                        changed |= ui.add(egui::Slider::new(attack, 0.1..=200.0).logarithmic(true).suffix(" ms").text("att")).changed();
                        changed |= ui.add(egui::Slider::new(release, 5.0..=2000.0).logarithmic(true).suffix(" ms").text("rel")).changed();
                        changed |= ui.add(egui::Slider::new(gain, -24.0..=24.0).suffix(" dB").text("gain")).changed();
                    }
                    ModSource::Xy { axis } => {
                        changed |= ui.selectable_value(axis, 0, "X axis").changed();
                        changed |= ui.selectable_value(axis, 1, "Y axis").changed();
                    }
                    ModSource::MidiNote { lo, hi } => {
                        changed |= ui.add(egui::Slider::new(lo, 0..=126).text("low note")).changed();
                        changed |= ui.add(egui::Slider::new(hi, 1..=127).text("high note")).changed();
                    }
                    ModSource::MidiVelocity => {
                        ui.label(RichText::new("the last note's velocity").color(theme::TEXT_DIM));
                    }
                });
                ui.horizontal(|ui| {
                    changed |= ui.add(egui::Slider::new(&mut md.slew, 0.0..=2000.0).logarithmic(true).suffix(" ms").text("slew")).on_hover_text("Glide on the source: smooths jumps (0 = instant)").changed();
                });
            });
        }
        if let Some(k) = remove {
            mods.remove(k);
            changed = true;
        }
        ui.horizontal(|ui| {
            let first = targets.first().map(|p| p.id.to_string()).unwrap_or_default();
            if ui.button("+ LFO").clicked() {
                mods.push(Modulation { source: ModSource::Lfo { rate: 1.0, shape: 0, sync: true }, target: first.clone(), depth: 0.3, slew: 0.0 });
                changed = true;
            }
            if ui.button("+ Envelope follower").clicked() {
                mods.push(Modulation { source: ModSource::Envelope { attack: 5.0, release: 150.0, gain: 0.0 }, target: first.clone(), depth: 0.5, slew: 0.0 });
                changed = true;
            }
            if ui.button("+ XY pad").on_hover_text("Moves the target with the XY pad; right-click the pad for MIDI learn or automation").clicked() {
                mods.push(Modulation { source: ModSource::Xy { axis: 0 }, target: first.clone(), depth: 0.5, slew: 0.0 });
                changed = true;
            }
            if ui.button("+ MIDI note").on_hover_text("Follows the pitch of the last note played (patterns, piano roll, live MIDI)").clicked() {
                mods.push(Modulation { source: ModSource::MidiNote { lo: 36, hi: 96 }, target: first.clone(), depth: 0.5, slew: 0.0 });
                changed = true;
            }
            if ui.button("+ Velocity").on_hover_text("Follows the velocity of the last note played").clicked() {
                mods.push(Modulation { source: ModSource::MidiVelocity, target: first, depth: 0.5, slew: 0.0 });
                changed = true;
            }
        });
        // The XY pad (when a route uses it): drag to move both axes.
        if mods.iter().any(|x| matches!(x.source, ModSource::Xy { .. })) {
            let get = |k: &str| ins.params.get(k).copied().unwrap_or(0.5);
            let (mut x, mut y) = (get("__xy_x"), get("__xy_y"));
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(140.0, 140.0), egui::Sense::click_and_drag());
            let p = ui.painter();
            p.rect_filled(rect, 6.0, theme::PANEL_2);
            for k in 1..4 {
                let f = k as f32 / 4.0;
                p.line_segment([egui::pos2(rect.left() + rect.width() * f, rect.top()), egui::pos2(rect.left() + rect.width() * f, rect.bottom())], egui::Stroke::new(1.0_f32, theme::GRID));
                p.line_segment([egui::pos2(rect.left(), rect.top() + rect.height() * f), egui::pos2(rect.right(), rect.top() + rect.height() * f)], egui::Stroke::new(1.0_f32, theme::GRID));
            }
            if let Some(pos) = resp.interact_pointer_pos().filter(|_| resp.dragged() || resp.clicked()) {
                x = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                y = ((rect.bottom() - pos.y) / rect.height()).clamp(0.0, 1.0);
                st.ui.last_target = Some(dragon_engine::model::Target::InsertParam { channel: m, insert: i, param: "__xy_x".into() });
                st.session.gesture("XY pad", |p| {
                    with_insert(p, m, i, |ins| {
                        ins.params.insert("__xy_x".into(), x);
                        ins.params.insert("__xy_y".into(), y);
                    })
                });
            }
            let dot = egui::pos2(rect.left() + x * rect.width(), rect.bottom() - y * rect.height());
            p.circle_filled(dot, 7.0, egui::Color32::from_rgb(0xf5, 0xc8, 0x42));
            p.text(rect.left_top() + egui::vec2(4.0, 2.0), egui::Align2::LEFT_TOP, format!("XY {x:.2} / {y:.2}"), egui::FontId::proportional(9.0), theme::TEXT_DIM);
            resp.context_menu(|ui| {
                for (axis, name) in [("__xy_x", "X"), ("__xy_y", "Y")] {
                    let target = dragon_engine::model::Target::InsertParam { channel: m, insert: i, param: axis.into() };
                    if ui.button(format!("MIDI learn {name}")).clicked() {
                        st.ui.midi_learn = Some(target.clone());
                        ui.close_menu();
                    }
                    if ui.button(format!("Automation lane for {name}")).clicked() {
                        crate::views::automation::add_lane(st, target);
                        ui.close_menu();
                    }
                }
            });
        }
        if changed {
            st.session.gesture("Modulation", |p| with_insert(p, m, i, |x| x.mods = mods));
        }
        if !ui.ctx().input(|i| i.pointer.any_down()) {
            st.session.end_gesture();
        }
    });
}

fn presets_dir(st: &State, effect: &str) -> PathBuf {
    st.ctx.paths.config.join("Presets").join(effect)
}

/// `c` lighter (`k` > 1) or darker (`k` < 1).
fn shade(c: egui::Color32, k: f32) -> egui::Color32 {
    let f = |v: u8| ((v as f32 * k).clamp(0.0, 255.0)) as u8;
    egui::Color32::from_rgb(f(c.r()), f(c.g()), f(c.b()))
}

/// A small (i) that shows `text` when hovered.
fn info_dot(ui: &mut Ui, text: &str, accent: egui::Color32) {
    let (ir, iresp) = ui.allocate_exact_size(egui::Vec2::splat(16.0), egui::Sense::hover());
    let col = if iresp.hovered() { accent } else { theme::TEXT_DIM };
    ui.painter().circle_stroke(ir.center(), 6.5, egui::Stroke::new(1.2_f32, col));
    ui.painter().text(ir.center(), egui::Align2::CENTER_CENTER, "i", egui::FontId::proportional(10.0), col);
    iresp.on_hover_text(text);
}

pub(super) fn effect_editor(ui: &mut Ui, st: &mut State, m: Id, i: Id) {
    let Some(ins) = st.session.project.mixer.get(m).and_then(|c| c.inserts.iter().find(|x| x.id == i)).cloned() else { return };
    let gr = st.audio.meters().snapshot().gain_reduction.get(&i).copied().unwrap_or(0.0);
    let cat = effects::find(&ins.effect).map(|d| d.category).unwrap_or("Plugin");
    // The Soft Clipper wears gold.
    let ccol = if ins.effect == "clipper" {
        egui::Color32::from_rgb(0xf0, 0xc0, 0x5a)
    } else if ins.plugin.is_none() {
        super::analog::finish_of(&ins.effect).accent().unwrap_or_else(|| super::mixer::category_color(cat))
    } else {
        super::mixer::category_color(cat)
    };
    // Header, after the design reference: colour stripe, "Dragon <name> | category", A/B compare, power
    // button and the mix knob.
    let fx_name = effects::find(&ins.effect).map(|d| d.name.to_string()).or_else(|| ins.plugin.as_ref().map(|p| p.name.clone())).unwrap_or_default();
    let brand = fx_name.clone();
    egui::Frame::NONE.fill(theme::PANEL_2).corner_radius(6.0).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
        ui.horizontal(|ui| {
            // The controls are laid out first (from the right); the name takes what is left and is shortened
            // with "…" in narrow windows instead of running under the buttons.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Power button: a ring with a stem, orange when on.
                let (pr, presp) = ui.allocate_exact_size(egui::Vec2::splat(24.0), egui::Sense::click());
                let pc = if ins.enabled { egui::Color32::from_rgb(0xf0, 0x8a, 0x4b) } else { theme::TEXT_DIM };
                let c = pr.center();
                let pts: Vec<egui::Pos2> = (0..=24).map(|k| -std::f32::consts::FRAC_PI_2 + 0.6 + (std::f32::consts::TAU - 1.2) * k as f32 / 24.0).map(|a| egui::pos2(c.x + 7.5 * a.cos(), c.y + 7.5 * a.sin())).collect();
                if presp.hovered() || ins.enabled {
                    ui.painter().circle_filled(c, 11.0, pc.gamma_multiply(0.15));
                }
                ui.painter().add(egui::Shape::line(pts, egui::Stroke::new(2.0_f32, pc)));
                ui.painter().line_segment([egui::pos2(c.x, c.y - 9.0), egui::pos2(c.x, c.y - 1.0)], egui::Stroke::new(2.0_f32, pc));
                if presp.on_hover_text(if ins.enabled { "On – click to bypass" } else { "Bypassed – click to switch on" }).clicked() {
                    let en = !ins.enabled;
                    st.session.edit("Bypass effect", |p| with_insert(p, m, i, |x| x.enabled = en));
                }
                ui.add_space(4.0);
                // A/B compare: two settings to flip between while listening.
                let ab_id = egui::Id::new(("fx.ab", m, i));
                let (active_b, other): (bool, Option<BTreeMap<String, f32>>) = ui.data(|d| d.get_temp(ab_id)).unwrap_or((false, None));
                for (is_b, label) in [(true, "B"), (false, "A")] {
                    let on = active_b == is_b;
                    let b = ui.add(
                        egui::Button::new(RichText::new(label).strong().color(if on { egui::Color32::BLACK } else { theme::TEXT }))
                            .fill(if on { egui::Color32::from_rgb(0xf0, 0x8a, 0x4b) } else { theme::PANEL })
                            .min_size(egui::vec2(24.0, 22.0)),
                    );
                    if b.on_hover_text("A/B compare: flip between two settings of this effect").clicked() && !on {
                        let cur = ins.params.clone();
                        let next = other.clone().unwrap_or_else(|| cur.clone());
                        st.session.edit("A/B compare", |p| with_insert(p, m, i, |x| x.params = next));
                        ui.data_mut(|d| d.insert_temp(ab_id, (is_b, Some(cur))));
                    }
                }
                ui.add_space(6.0);
                let full = st.ui.fx_fullscreen == Some((m, i));
                if ui.add(egui::Button::new(RichText::new("⛶").size(14.0)).selected(full).min_size(egui::vec2(24.0, 22.0))).on_hover_text(if full { "Leave full screen" } else { "Full screen" }).clicked() {
                    st.ui.fx_fullscreen = if full { None } else { Some((m, i)) };
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Fullscreen(!full));
                }
                ui.add_space(4.0);
                let mut mix = ins.mix;
                let r = widgets::mini_knob(ui, &mut mix, 0.0, 1.0, 1.0, ccol).on_hover_text(format!("Mix (dry/wet) {:.0}%", mix * 100.0));
                widgets::apply(st, &r, "Effect mix", |p| with_insert(p, m, i, |x| x.mix = mix));
                ui.label(RichText::new("MIX").size(9.5).color(theme::TEXT_DIM));
                if gr > 0.05 {
                    ui.label(RichText::new(format!("GR −{gr:.1} dB")).color(theme::WARN).monospace());
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let (bar, _) = ui.allocate_exact_size(egui::vec2(4.0, 22.0), egui::Sense::hover());
                    ui.painter().rect_filled(bar, 2.0, if ins.enabled { ccol } else { theme::GRID_BAR });
                    let name = ui.add(egui::Label::new(RichText::new(&brand).size(15.0).strong().color(theme::TEXT)).truncate());
                    ui.data_mut(|d| d.insert_temp(egui::Id::new(("fx.header.name", m, i)), name.rect));
                    ui.add(egui::Label::new(RichText::new(format!("|  {cat}")).size(11.0).color(theme::TEXT_DIM)).truncate());
                    // What it does, behind an (i).
                    if let Some(d) = effects::find(&ins.effect) {
                        info_dot(ui, d.description, ccol);
                    }
                });
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
    // Tabs: the controls, or the effect's Hammerstein graph with the current settings.
    if super::hammerstein::tabs(ui, egui::Id::new(("fx.tab", m, i)), ccol) {
        if !st.quests.progress.done.contains("hammerstein") {
            crate::views::quests::event(st, crate::views::quests::Event::Hammerstein);
        }
        ui.separator();
        let bpm = st.session.project.bpm;
        super::hammerstein::view(ui, &ins, bpm, egui::Id::new(("fx.hammerstein", m, i)), ccol);
        return;
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
    let tele = st.audio.meters().snapshot().fx_data.get(&i).cloned().unwrap_or_else(|| vec![-1.0]);
    // The device: display and controls on one faceplate in the effect's own finish – the display in its
    // bezel, the control groups engraved into the same face, screws in the corners.
    // An analog rack unit: the faceplate (painted behind once its size is known), the nameplate with
    // lamp and VU meters, then the display and the controls.
    let finish = super::analog::finish_of(def.id);
    let face = finish.base();
    let plate_at = ui.painter().add(egui::Shape::Noop);
    let ear = super::analog::EAR as i8;
    // The unit's own output (RMS, as a VU reads it), measured by the engine right after this insert.
    let lv = st.audio.meters().snapshot().insert_rms(i);
    let level = lv.map(|(l, r, _)| (l, r)).unwrap_or((0.0, 0.0));
    // The display reads the same measured output (its meters show real values, not a demo).
    ui.data_mut(|d| d.insert_temp(egui::Id::new(("fx.hero", m, i)).with("out_level"), lv.unwrap_or((0.0, 0.0, 0.0))));
    let dev = egui::Frame::NONE.inner_margin(egui::Margin { left: 14 + ear, right: 14 + ear, top: 8, bottom: 14 }).show(ui, |ui| {
        super::analog::nameplate(ui, &super::analog::Plate { id: def.id, name: def.name, category: def.category, enabled: ins.enabled, gr, level, time, key: egui::Id::new(("fx.unit", m, i)) });
        ui.add_space(6.0);
        let bezel = egui::Frame::NONE.fill(egui::Color32::from_rgb(0x08, 0x09, 0x0b)).corner_radius(7.0).inner_margin(egui::Margin::same(3)).stroke(if finish.gnostic() {
            egui::Stroke::new(2.0_f32, super::analog::GNOSTIC_GOLD.gamma_multiply(0.75))
        } else {
            egui::Stroke::new(2.0_f32, shade(face, 0.55))
        });
        let (edits, _) =
            bezel.show(ui, |ui| super::fx_ui::hero(ui, &super::fx_ui::Hero { def, params: &ins.params, gr, enabled: ins.enabled, color: ccol, time, tele: &tele, key: egui::Id::new(("fx.hero", m, i)) })).inner;
        ui.add_space(8.0);
        if let Some((k, v)) = edits.last() {
            if let Some(prm) = def.params.iter().find(|p| p.id == k.as_str()) {
                st.hint(def.name, prm.name, prm.format(*v));
            }
        }
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
        // The designed panel (fx_panel); the generic grid below only for an effect without one.
        let eq_sel_key = egui::Id::new(("fx.hero", m, i)).with("eq.sel");
        let band_key = if def.id == "eq" { eq_sel_key } else { egui::Id::new(("fx.hero", m, i)).with("band") };
        let mut band: usize = ui.data(|d| d.get_temp(band_key)).unwrap_or_else(|| {
            if def.id == "eq" {
                (1..=dragon_engine::effects::eq::EQ_BANDS).find(|b| effects::value(def, &ins.params, &format!("b{b}_on")) > 0.5).unwrap_or(1)
            } else {
                1
            }
        });
        if def.id == "eq" && super::fx_panel::panel("eq", 1).is_some() {
            // A recessed strip in the plate, so its controls read on any finish.
            egui::Frame::NONE.fill(egui::Color32::from_black_alpha(150)).corner_radius(5.0).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                eq_compare(ui, st, m, i, &ins);
            });
            ui.add_space(6.0);
        }
        let params = ins.params.clone();
        let shown = {
            let mut touch = |_ui: &mut Ui, t: super::fx_panel::Touch| {
                let prm = t.prm;
                let key = prm.id.to_string();
                let v = t.value;
                if t.resp.hovered() || t.resp.dragged() || t.resp.changed() {
                    st.hint(def.name, prm.name, prm.format(v));
                }
                match prm.kind {
                    ParamKind::Choice(_) | ParamKind::Toggle => {
                        if t.resp.changed() {
                            st.session.edit(prm.name, |p| {
                                with_insert(p, m, i, |x| {
                                    x.params.insert(key.clone(), v);
                                })
                            });
                        }
                    }
                    _ => {
                        let r = t.resp.on_hover_text(format!("{} – drag, Shift = fine, double-click or Ctrl+click resets, right-click to type a value", prm.name));
                        widgets::apply(st, &r, prm.name, |p| {
                            with_insert(p, m, i, |x| {
                                x.params.insert(key.clone(), v);
                            })
                        });
                        let target = Target::InsertParam { channel: m, insert: i, param: key.clone() };
                        if r.changed() || r.drag_started() {
                            st.ui.last_target = Some(target.clone());
                        }
                        param_menu(&r, st, m, i, prm, v, target);
                    }
                }
            };
            let mut custom = |ui: &mut Ui, name: &str| match name {
                "reverb.er" => reverb_part_curve(ui, &params, "er"),
                "reverb.late" => reverb_part_curve(ui, &params, "late"),
                n if n.starts_with("mb.gr") => {
                    let band = n[5..].parse::<usize>().unwrap_or(1).clamp(1, 4) - 1;
                    super::fx_ui::band_meter(ui, &tele, band, super::fx_panel::BAND_COLORS[band]);
                }
                _ => {}
            };
            super::fx_panel::show(ui, super::fx_panel::View { def, params: &params, accent: ccol, salt: egui::Id::new(("fx.panel", m, i)) }, &mut band, &mut touch, &mut custom)
        };
        ui.data_mut(|d| d.insert_temp(band_key, band));
        shown
    });
    ui.painter().set(plate_at, egui::Shape::Vec(super::analog::faceplate(ui.ctx(), dev.response.rect, finish)));
    let shown = dev.inner;
    if !shown {
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
        // Pro-Q style: the EQ shows one band's controls at a time – the band last touched on the display, or the
        // one picked in the band strip.
        let eq_sel_key = egui::Id::new(("fx.hero", m, i)).with("eq.sel");
        let eq_band: Option<usize> = (def.id == "eq").then(|| {
            let n = dragon_engine::effects::eq::EQ_BANDS;
            let on = |b: usize| effects::value(def, &ins.params, &format!("b{b}_on")) > 0.5;
            let mut sel: usize = ui.data(|d| d.get_temp(eq_sel_key)).unwrap_or_else(|| (1..=n).find(|b| on(*b)).unwrap_or(1));
            // A recessed strip in the plate, so its controls read on any finish.
            egui::Frame::NONE.fill(egui::Color32::from_black_alpha(150)).corner_radius(5.0).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                eq_compare(ui, st, m, i, &ins);
            });
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Band").small());
                for b in 1..=n {
                    let txt = RichText::new(format!("{b}")).small().color(if on(b) { theme::TEXT } else { theme::TEXT_DIM });
                    if ui.selectable_label(sel == b, txt).on_hover_text(if on(b) { "on" } else { "off – select it, then switch it on" }).clicked() {
                        sel = b;
                        ui.data_mut(|d| d.insert_temp(eq_sel_key, b));
                    }
                }
            });
            sel
        });
        let mut groups: Vec<(String, Vec<&effects::ParamInfo>)> = Vec::new();
        for prm in def.params {
            if let Some(b) = eq_band {
                let prefix = format!("b{b}_");
                if prm.id.starts_with('b') && prm.id[1..2].chars().all(|c| c.is_ascii_digit()) && !prm.id.starts_with(&prefix) {
                    continue;
                }
            }
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
                                let r = widgets::fx_knob(ui, &mut n, prm.normalize(prm.default), short, &prm.format(v), ccol)
                                    .on_hover_text(format!("{} – drag, Shift = fine, double-click or Ctrl+click resets, right-click to type a value", prm.name));
                                v = prm.denormalize(n);
                                widgets::apply(st, &r, prm.name, |p| {
                                    with_insert(p, m, i, |x| {
                                        x.params.insert(key.clone(), v);
                                    })
                                });
                                let target = Target::InsertParam { channel: m, insert: i, param: key.clone() };
                                if r.changed() || r.drag_started() {
                                    st.ui.last_target = Some(target.clone());
                                }
                                param_menu(&r, st, m, i, prm, v, target);
                            }
                        }
                    }
                });
            });
        }
    }
    ui.separator();
    modulation(ui, st, m, i, def, &ins);
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

/// The right-click menu of an effect parameter: type an exact value, reset, MIDI learn, automation lane.
fn param_menu(r: &egui::Response, st: &mut State, m: Id, i: Id, prm: &effects::ParamInfo, v: f32, target: Target) {
    let key = prm.id.to_string();
    r.context_menu(|ui| {
        // Type an exact value (FabFilter style); units and a k suffix are accepted.
        let buf_id = ui.id().with(("fx.type", &target));
        let mut buf: String = ui.data(|d| d.get_temp(buf_id)).unwrap_or_else(|| prm.format(v));
        let te = ui.add(egui::TextEdit::singleline(&mut buf).desired_width(110.0).hint_text("value"));
        ui.data_mut(|d| d.insert_temp(buf_id, buf.clone()));
        if te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            if let Some(x) = widgets::parse_value(&buf) {
                let x = x.clamp(prm.min.min(prm.max), prm.max.max(prm.min));
                let key = key.clone();
                st.session.edit(prm.name, |p| {
                    with_insert(p, m, i, |ins| {
                        ins.params.insert(key, x);
                    })
                });
            }
            ui.data_mut(|d| d.remove::<String>(buf_id));
            ui.close_menu();
        }
        if ui.button("Reset to default").clicked() {
            let key = key.clone();
            st.session.edit(prm.name, |p| {
                with_insert(p, m, i, |ins| {
                    ins.params.insert(key, prm.default);
                })
            });
            ui.close_menu();
        }
        ui.separator();
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
            let g = e.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let ps = g.plugin_params();
            let vs = ps.iter().map(|p| g.plugin_param(p.id).unwrap_or(p.default)).collect();
            (ps, vs)
        }
        Err(i) => {
            let g = i.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
                let mut g = e.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let ok = g.editor(true);
                (ok, g.editor_error())
            }
            Err(i) => {
                let mut g = i.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
                    Ok(e) => e.lock().unwrap_or_else(std::sync::PoisonError::into_inner).editor(false),
                    Err(i) => i.lock().unwrap_or_else(std::sync::PoisonError::into_inner).editor(false),
                };
                st.ui.native_editors.retain(|k| *k != key);
            }
            // Full screen for the plugin's own window (resizable editors follow the new size).
            let fs_id = egui::Id::new(("plugin.window.fs", key));
            let mut fs: bool = ui.data(|d| d.get_temp(fs_id)).unwrap_or(false);
            if ui.toggle_value(&mut fs, "⛶ Full screen").on_hover_text("Plugin window full screen (editors that can resize fill it)").changed() {
                let now = match &inst {
                    Ok(e) => e.lock().unwrap_or_else(std::sync::PoisonError::into_inner).editor_fullscreen(fs),
                    Err(i) => i.lock().unwrap_or_else(std::sync::PoisonError::into_inner).editor_fullscreen(fs),
                };
                ui.data_mut(|d| d.insert_temp(fs_id, now));
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
                if r.hovered() || r.dragged() {
                    st.hint(&pl.name, &prm.name, format!("{:.0} %", val * 100.0));
                }
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
    super::envelope::editor(ui, st, label, env, theme::ACCENT_2, set);
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
        Instrument::Wavetable(w) => super::wave_ui::editor(ui, st, cid, *w),
        Instrument::Guitar(g) => super::guitar_ui::editor(ui, st, cid, *g),
        Instrument::Piano(g) => super::piano_ui::editor(ui, st, cid, *g),
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
        // FL style: the volume envelope drawn over the sample (as if the note is held for the whole
        // sample), and the waveform dimmed where the envelope turns it down.
        if env.enabled {
            let rate = st.audio.rate() as f32;
            let secs = (n * crate::state::WAVE_BUCKET) as f32 / rate.max(1.0);
            let play = ((s.end - s.start).max(0.001)) * secs;
            let held = play;
            let mut pts = Vec::new();
            let mut x = sx;
            while x <= ex {
                let t = (x - sx) / (ex - sx).max(1.0) * play;
                let g = env.level_at(t, held);
                pts.push(egui::pos2(x, rect.bottom() - 2.0 - g * (rect.height() - 4.0)));
                // Shade the part of the waveform the envelope removes.
                let cut = (1.0 - g).clamp(0.0, 1.0);
                if cut > 0.02 {
                    p.line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())], egui::Stroke::new(1.0_f32, theme::BG.gamma_multiply(cut * 0.85)));
                }
                x += 1.0;
            }
            p.add(egui::Shape::line(pts, egui::Stroke::new(1.5_f32, theme::WARN)));
        }
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
        if let Some(path) = super::widgets::dnd_release::<PathBuf>(&resp) {
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
            if let Some(path) = super::widgets::dnd_release::<PathBuf>(&r) {
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
