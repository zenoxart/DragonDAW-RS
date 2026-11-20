//! The Dragon Wave editor: oscillators with their wavetables drawn in 3D, sub and noise, two filters with
//! their response curves, envelopes, LFOs, the modulation matrix, macros, the FX rack and voicing.

use super::widgets::{self, knob_colored};
use crate::state::State;
use crate::theme;
use dragon_engine::effects::{self, ParamKind};
use dragon_engine::model::{Id, Instrument, Project};
use dragon_engine::wt::{self, *};
use egui::{Color32, Pos2, RichText, Sense, Stroke, Ui, Vec2};

const OSC_COL: [Color32; 3] = [Color32::from_rgb(0xff, 0x8a, 0x3c), Color32::from_rgb(0x4d, 0xc3, 0xff), Color32::from_rgb(0xb3, 0x88, 0xff)];
const MOD_COL: Color32 = Color32::from_rgb(0x5d, 0xe0, 0xa0);

fn set(p: &mut Project, cid: Id, f: impl FnOnce(&mut WavetableParams)) {
    if let Some(Instrument::Wavetable(w)) = p.channel_mut(cid).map(|c| &mut c.instrument) {
        f(w);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Osc,
    Mod,
    Fx,
    Voice,
}

/// A knob for one value of the synth.
#[allow(clippy::too_many_arguments)]
fn kn(ui: &mut Ui, st: &mut State, cid: Id, v: f32, min: f32, max: f32, default: f32, label: &str, fmt: impl Fn(f32) -> String, color: Color32, write: impl FnOnce(&mut WavetableParams, f32)) {
    let mut x = v;
    let r = knob_colored(ui, &mut x, min, max, default, label, fmt, color);
    if r.hovered() || r.dragged() {
        st.hint("Dragon Wave", label, format!("{x:.2}"));
    }
    widgets::apply(st, &r, &format!("Dragon Wave: {label}"), |p| set(p, cid, |w| write(w, x)));
}

fn panel(ui: &mut Ui, title: &str, color: Color32, width: f32, body: impl FnOnce(&mut Ui)) {
    egui::Frame::NONE.fill(theme::PANEL_2).stroke(Stroke::new(1.0_f32, color.gamma_multiply(0.35))).corner_radius(8.0).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
        ui.set_width(width);
        ui.vertical(|ui| {
            ui.set_width(width);
            ui.label(RichText::new(title).small().strong().color(color));
            body(ui);
        });
    });
}

pub fn editor(ui: &mut Ui, st: &mut State, cid: Id, p: WavetableParams) {
    let tab_id = egui::Id::new(("wave.tab", cid));
    let mut tab = ui.data(|d| d.get_temp::<u8>(tab_id)).map(|t| [Tab::Osc, Tab::Mod, Tab::Fx, Tab::Voice][t as usize % 4]).unwrap_or_default();
    // Header: name, preset, macros, master.
    ui.horizontal(|ui| {
        ui.label(RichText::new("DRAGON WAVE").size(16.0).strong().color(theme::ACCENT));
        ui.label(RichText::new("wavetable synth").small().color(theme::TEXT_DIM));
        ui.add_space(10.0);
        let presets = wt::presets();
        egui::ComboBox::from_id_salt(("wave.preset", cid))
            .selected_text(&p.preset)
            .width(150.0)
            .show_ui(ui, |ui| {
                for (name, preset) in presets {
                    if ui.selectable_label(p.preset == name, name).clicked() {
                        st.session.edit(&format!("Preset: {name}"), |pr| set(pr, cid, |w| *w = preset));
                    }
                }
            })
            .response
            .on_hover_text("Factory presets");
        ui.add_space(10.0);
        for k in 0..4 {
            let name = p.macro_names[k].clone();
            kn(ui, st, cid, p.macros[k], 0.0, 1.0, 0.0, &name, widgets::pct, MOD_COL, move |w, v| w.macros[k] = v);
        }
        ui.add_space(6.0);
        kn(ui, st, cid, p.master, 0.0, 1.5, 0.7, "master", widgets::db_fmt, theme::ACCENT, |w, v| w.master = v);
    });
    ui.horizontal(|ui| {
        for (t, name) in [(Tab::Osc, "Oscillators & filters"), (Tab::Mod, "Modulation"), (Tab::Fx, "FX"), (Tab::Voice, "Voicing")] {
            let on = tab == t;
            let b = egui::Button::new(RichText::new(name).strong().color(if on { Color32::BLACK } else { theme::TEXT })).fill(if on { theme::ACCENT } else { theme::PANEL_2 }).min_size(Vec2::new(150.0, 26.0));
            if ui.add(b).clicked() {
                tab = t;
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(tab_id, tab as u8));
    ui.add_space(4.0);
    match tab {
        Tab::Osc => osc_tab(ui, st, cid, &p),
        Tab::Mod => mod_tab(ui, st, cid, &p),
        Tab::Fx => fx_tab(ui, st, cid, &p),
        Tab::Voice => voice_tab(ui, st, cid, &p),
    }
}

// ------------------------------------------------------------------------------------------------ oscillators

/// The wavetable drawn as stacked frames in perspective, the current position lit.
fn table_view(ui: &mut Ui, src: &TableSource, pos: f32, color: Color32, width: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 120.0), Sense::click_and_drag());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 6.0, Color32::from_rgb(0x0c, 0x0d, 0x12));
    let t = wt::table(src);
    let n = t.len();
    let shown = n.min(24);
    let depth = Vec2::new(rect.width() * 0.25, -rect.height() * 0.45);
    let w = rect.width() * 0.7;
    let h = rect.height() * 0.22;
    let base = Pos2::new(rect.left() + 8.0, rect.bottom() - h - 8.0);
    let cur = (pos * (n - 1) as f32).round() as usize;
    let frame_line = |k: usize, z: f32| -> Vec<Pos2> {
        let f = &t.frames[k];
        let off = base + depth * z;
        (0..=64).map(|i| Pos2::new(off.x + i as f32 / 64.0 * w, off.y - f[(i * (f.len() - 1)) / 64] * h)).collect()
    };
    for s in (0..shown).rev() {
        let k = if shown == 1 { 0 } else { s * (n - 1) / (shown - 1) };
        let z = k as f32 / (n - 1).max(1) as f32;
        p.add(egui::Shape::line(frame_line(k, z), Stroke::new(1.0_f32, color.gamma_multiply(0.18 + 0.25 * (1.0 - z)))));
    }
    let z = cur as f32 / (n - 1).max(1) as f32;
    p.add(egui::Shape::line(frame_line(cur, z), Stroke::new(2.2_f32, color)));
    p.text(rect.right_top() + Vec2::new(-6.0, 4.0), egui::Align2::RIGHT_TOP, format!("{}  ·  frame {}/{}", t.name, cur + 1, n), egui::FontId::proportional(10.0), theme::TEXT_DIM);
    resp
}

fn osc_panel(ui: &mut Ui, st: &mut State, cid: Id, p: &WavetableParams, o: usize, width: f32) {
    let osc = p.osc[o].clone();
    let col = OSC_COL[o];
    panel(ui, ["OSC A", "OSC B", "OSC C"][o], col, width, |ui| {
        ui.horizontal(|ui| {
            if widgets::toggle(ui, osc.on, if osc.on { "On" } else { "Off" }, col).clicked() {
                st.session.edit("Oscillator on / off", |pr| set(pr, cid, |w| w.osc[o].on = !osc.on));
            }
            egui::ComboBox::from_id_salt(("wave.table", cid, o)).selected_text(osc.table.name()).width(width - 120.0).show_ui(ui, |ui| {
                for name in wt::FACTORY {
                    let src = TableSource::Factory(name.into());
                    if ui.selectable_label(osc.table == src, name).clicked() {
                        st.session.edit("Wavetable", |pr| set(pr, cid, |w| w.osc[o].table = src));
                    }
                }
            });
            if ui.small_button("Import…").on_hover_text("Any audio file: wavetables (cycles of 2048 samples) or a sound cut into cycles").clicked() {
                if let Some(f) = rfd::FileDialog::new().add_filter("Audio", &["wav", "flac", "aif", "aiff", "mp3", "ogg"]).pick_file() {
                    st.session.edit("Import wavetable", |pr| set(pr, cid, |w| w.osc[o].table = TableSource::File(f)));
                }
            }
        });
        // Drag the table up / down to scan through it.
        let r = table_view(ui, &osc.table, osc.pos, if osc.on { col } else { theme::TEXT_DIM }, width);
        if r.dragged() {
            let pos = (osc.pos - r.drag_delta().y / 150.0).clamp(0.0, 1.0);
            st.session.gesture("Wavetable position", |pr| set(pr, cid, |w| w.osc[o].pos = pos));
        }
        if r.drag_stopped() {
            st.session.end_gesture();
        }
        if let Some(path) = widgets::dnd_release::<std::path::PathBuf>(&r) {
            let path = (*path).clone();
            st.session.edit("Import wavetable", |pr| set(pr, cid, |w| w.osc[o].table = TableSource::File(path)));
        }
        r.on_hover_text("Drag up / down to move through the table; drop an audio file to use it as a wavetable");
        ui.horizontal_wrapped(|ui| {
            kn(ui, st, cid, osc.pos, 0.0, 1.0, 0.0, "position", widgets::pct, col, move |w, v| w.osc[o].pos = v);
            ui.vertical(|ui| {
                let mut warp = osc.warp;
                egui::ComboBox::from_id_salt(("wave.warp", cid, o)).selected_text(warp.name()).width(64.0).show_ui(ui, |ui| {
                    for x in Warp::ALL {
                        ui.selectable_value(&mut warp, x, x.name());
                    }
                });
                if warp != osc.warp {
                    st.session.edit("Warp", |pr| set(pr, cid, |w| w.osc[o].warp = warp));
                }
                ui.label(RichText::new("warp").small().color(theme::TEXT_DIM));
            });
            kn(ui, st, cid, osc.warp_amount, 0.0, 1.0, 0.0, "warp amt", widgets::pct, col, move |w, v| w.osc[o].warp_amount = v);
            kn(ui, st, cid, osc.level, 0.0, 1.0, 0.75, "level", widgets::pct, col, move |w, v| w.osc[o].level = v);
            kn(ui, st, cid, osc.pan, -1.0, 1.0, 0.0, "pan", widgets::pan_fmt, col, move |w, v| w.osc[o].pan = v);
        });
        ui.horizontal_wrapped(|ui| {
            kn(ui, st, cid, osc.octave as f32, -4.0, 4.0, 0.0, "octave", |v| format!("{:+.0}", v.round()), col, move |w, v| w.osc[o].octave = v.round() as i32);
            kn(ui, st, cid, osc.semi as f32, -12.0, 12.0, 0.0, "semi", |v| format!("{:+.0} st", v.round()), col, move |w, v| w.osc[o].semi = v.round() as i32);
            kn(ui, st, cid, osc.fine, -100.0, 100.0, 0.0, "fine", |v| format!("{v:+.0} ct"), col, move |w, v| w.osc[o].fine = v);
            kn(ui, st, cid, osc.unison as f32, 1.0, 16.0, 1.0, "unison", |v| format!("{:.0} v", v.round()), col, move |w, v| w.osc[o].unison = v.round() as u32);
            kn(ui, st, cid, osc.detune, 0.0, 1.0, 0.25, "detune", widgets::pct, col, move |w, v| w.osc[o].detune = v);
        });
        ui.horizontal_wrapped(|ui| {
            kn(ui, st, cid, osc.blend, 0.0, 1.0, 0.75, "blend", widgets::pct, col, move |w, v| w.osc[o].blend = v);
            kn(ui, st, cid, osc.width, 0.0, 1.0, 1.0, "width", widgets::pct, col, move |w, v| w.osc[o].width = v);
            kn(ui, st, cid, osc.phase, 0.0, 1.0, 0.0, "phase", |v| format!("{:.0}°", v * 360.0), col, move |w, v| w.osc[o].phase = v);
            kn(ui, st, cid, osc.random_phase, 0.0, 1.0, 1.0, "random", widgets::pct, col, move |w, v| w.osc[o].random_phase = v);
            route_combo(ui, st, cid, ("osc", o), osc.route, move |w, r| w.osc[o].route = r);
        });
    });
}

fn route_combo(ui: &mut Ui, st: &mut State, cid: Id, salt: (&str, usize), cur: Route, write: impl FnOnce(&mut WavetableParams, Route)) {
    ui.vertical(|ui| {
        let mut r = cur;
        egui::ComboBox::from_id_salt(("wave.route", cid, salt)).selected_text(r.name()).width(70.0).show_ui(ui, |ui| {
            for x in Route::ALL {
                ui.selectable_value(&mut r, x, x.name());
            }
        });
        if r != cur {
            st.session.edit("Routing", |pr| set(pr, cid, |w| write(w, r)));
        }
        ui.label(RichText::new("to").small().color(theme::TEXT_DIM));
    });
}

/// Approximate magnitude (dB) of a filter at `f` Hz, for drawing.
pub fn filter_db(kind: FilterKind, fc: f32, reso: f32, f: f32) -> f32 {
    let x = f / fc.max(1.0);
    let q = |r: f32| 1.0 / (2.0 - 1.95 * r.clamp(0.0, 1.0));
    let lp = |r: f32| 1.0 / ((1.0 - x * x).powi(2) + (x / q(r)).powi(2)).sqrt();
    let hp = |r: f32| x * x / ((1.0 - x * x).powi(2) + (x / q(r)).powi(2)).sqrt();
    let bp = |r: f32| (x / q(r)) / ((1.0 - x * x).powi(2) + (x / q(r)).powi(2)).sqrt();
    let g = match kind {
        FilterKind::Lp12 => lp(reso),
        FilterKind::Lp24 => lp(reso * 0.6).powi(2),
        FilterKind::Hp12 => hp(reso),
        FilterKind::Hp24 => hp(reso * 0.6).powi(2),
        FilterKind::Bp => bp(reso),
        FilterKind::Notch => ((1.0 - x * x).abs()) / ((1.0 - x * x).powi(2) + (x / q(reso)).powi(2)).sqrt(),
        FilterKind::CombPlus | FilterKind::CombMinus => {
            let fb = 0.2 + reso * 0.75;
            let w = std::f32::consts::TAU * x;
            let s = if kind == FilterKind::CombPlus { -fb } else { fb };
            (1.0 - fb * 0.5) / ((1.0 + s * w.cos()).powi(2) + (s * w.sin()).powi(2)).sqrt()
        }
        FilterKind::Formant => {
            const V: [[f32; 3]; 5] = [[800.0, 1150.0, 2900.0], [400.0, 1600.0, 2700.0], [270.0, 2100.0, 3000.0], [450.0, 800.0, 2830.0], [325.0, 700.0, 2530.0]];
            let k = ((fc.max(20.0) / 20.0).log2() / 10.0).clamp(0.0, 1.0) * 4.0;
            let i = (k as usize).min(3);
            let t = k - i as f32;
            let qq = 1.0 / (2.0 - 1.95 * (0.5 + reso * 0.45));
            (0..3)
                .map(|j| {
                    let c = V[i][j] * (1.0 - t) + V[i + 1][j] * t;
                    let y = f / c;
                    [1.0, 0.7, 0.4][j] * (y / qq) / ((1.0 - y * y).powi(2) + (y / qq).powi(2)).sqrt()
                })
                .sum::<f32>()
                * 1.6
        }
    };
    20.0 * g.max(1e-5).log10()
}

fn filter_panel(ui: &mut Ui, st: &mut State, cid: Id, p: &WavetableParams, k: usize, width: f32) {
    let f = p.filters[k].clone();
    let col = Color32::from_rgb(0xff, 0x5a, 0x6e);
    panel(ui, ["FILTER 1", "FILTER 2"][k], col, width, |ui| {
        ui.horizontal(|ui| {
            if widgets::toggle(ui, f.on, if f.on { "On" } else { "Off" }, col).clicked() {
                st.session.edit("Filter on / off", |pr| set(pr, cid, |w| w.filters[k].on = !f.on));
            }
            let mut kind = f.kind;
            egui::ComboBox::from_id_salt(("wave.fkind", cid, k)).selected_text(kind.name()).width(110.0).show_ui(ui, |ui| {
                for x in FilterKind::ALL {
                    ui.selectable_value(&mut kind, x, x.name());
                }
            });
            if kind != f.kind {
                st.session.edit("Filter type", |pr| set(pr, cid, |w| w.filters[k].kind = kind));
            }
        });
        // Response curve; drag it: sideways = cutoff, up / down = resonance.
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 70.0), Sense::click_and_drag());
        let pa = ui.painter_at(rect);
        pa.rect_filled(rect, 5.0, Color32::from_rgb(0x0c, 0x0d, 0x12));
        let x_of = |hz: f32| rect.left() + (hz / 20.0).log2() / 10.0 * rect.width();
        let hz_of = |x: f32| 20.0 * 2f32.powf((x - rect.left()) / rect.width() * 10.0);
        let y_of = |db: f32| rect.center().y - db.clamp(-36.0, 24.0) / 36.0 * rect.height() * 0.5;
        let pts: Vec<Pos2> = (0..=80)
            .map(|i| {
                let x = rect.left() + i as f32 / 80.0 * rect.width();
                let db = if f.on {
                    let wet = filter_db(f.kind, f.cutoff, f.resonance, hz_of(x));
                    let g = 10f32.powf(wet / 20.0) * f.mix + (1.0 - f.mix);
                    20.0 * g.max(1e-5).log10()
                } else {
                    0.0
                };
                Pos2::new(x, y_of(db))
            })
            .collect();
        pa.add(egui::Shape::line(pts, Stroke::new(1.8_f32, if f.on { col } else { theme::TEXT_DIM })));
        let cx = x_of(f.cutoff);
        pa.circle_filled(Pos2::new(cx, y_of(0.0)), 4.0, if f.on { Color32::WHITE } else { theme::TEXT_DIM });
        if resp.dragged() {
            let d = resp.drag_delta();
            let cutoff = (f.cutoff * 2f32.powf(d.x / rect.width() * 10.0)).clamp(20.0, 20000.0);
            let reso = (f.resonance - d.y / 120.0).clamp(0.0, 0.99);
            st.session.gesture("Filter", |pr| {
                set(pr, cid, |w| {
                    w.filters[k].cutoff = cutoff;
                    w.filters[k].resonance = reso;
                })
            });
        }
        if resp.drag_stopped() {
            st.session.end_gesture();
        }
        resp.on_hover_text("Drag: sideways = cutoff, up / down = resonance");
        ui.horizontal_wrapped(|ui| {
            kn(ui, st, cid, f.cutoff, 20.0, 20000.0, 2000.0, "cutoff", |v| if v >= 1000.0 { format!("{:.1} kHz", v / 1000.0) } else { format!("{v:.0} Hz") }, col, move |w, v| w.filters[k].cutoff = v);
            kn(ui, st, cid, f.resonance, 0.0, 0.99, 0.2, "reso", widgets::pct, col, move |w, v| w.filters[k].resonance = v);
            kn(ui, st, cid, f.drive, 0.0, 1.0, 0.0, "drive", widgets::pct, col, move |w, v| w.filters[k].drive = v);
            kn(ui, st, cid, f.mix, 0.0, 1.0, 1.0, "mix", widgets::pct, col, move |w, v| w.filters[k].mix = v);
            kn(ui, st, cid, f.keytrack, 0.0, 1.0, 0.0, "key", widgets::pct, col, move |w, v| w.filters[k].keytrack = v);
        });
    });
}

fn osc_tab(ui: &mut Ui, st: &mut State, cid: Id, p: &WavetableParams) {
    let w = ((ui.available_width() - 24.0) / 3.0).max(240.0);
    ui.horizontal_top(|ui| {
        for o in 0..3 {
            osc_panel(ui, st, cid, p, o, w - 16.0);
        }
    });
    ui.add_space(6.0);
    ui.horizontal_top(|ui| {
        let small = ((ui.available_width() - 40.0) * 0.2).max(170.0);
        let sub = p.sub.clone();
        let col = Color32::from_rgb(0xe0, 0xc2, 0x4a);
        panel(ui, "SUB", col, small - 16.0, |ui| {
            ui.horizontal(|ui| {
                if widgets::toggle(ui, sub.on, if sub.on { "On" } else { "Off" }, col).clicked() {
                    st.session.edit("Sub on / off", |pr| set(pr, cid, |w| w.sub.on = !sub.on));
                }
                let mut shape = sub.shape;
                egui::ComboBox::from_id_salt(("wave.sub", cid)).selected_text(format!("{shape:?}")).width(80.0).show_ui(ui, |ui| {
                    for x in [SubShape::Sine, SubShape::Triangle, SubShape::Saw, SubShape::Square] {
                        ui.selectable_value(&mut shape, x, format!("{x:?}"));
                    }
                });
                if shape != sub.shape {
                    st.session.edit("Sub shape", |pr| set(pr, cid, |w| w.sub.shape = shape));
                }
            });
            ui.horizontal_wrapped(|ui| {
                kn(ui, st, cid, sub.octave as f32, -3.0, 1.0, -1.0, "octave", |v| format!("{:+.0}", v.round()), col, |w, v| w.sub.octave = v.round() as i32);
                kn(ui, st, cid, sub.level, 0.0, 1.0, 0.6, "level", widgets::pct, col, |w, v| w.sub.level = v);
                kn(ui, st, cid, sub.pan, -1.0, 1.0, 0.0, "pan", widgets::pan_fmt, col, |w, v| w.sub.pan = v);
                route_combo(ui, st, cid, ("sub", 0), sub.route, |w, r| w.sub.route = r);
            });
        });
        let noise = p.noise.clone();
        let col = Color32::from_rgb(0xa0, 0xa8, 0xb8);
        panel(ui, "NOISE", col, small - 16.0, |ui| {
            if widgets::toggle(ui, noise.on, if noise.on { "On" } else { "Off" }, col).clicked() {
                st.session.edit("Noise on / off", |pr| set(pr, cid, |w| w.noise.on = !noise.on));
            }
            ui.horizontal_wrapped(|ui| {
                kn(
                    ui,
                    st,
                    cid,
                    noise.color,
                    0.0,
                    1.0,
                    0.5,
                    "colour",
                    |v| {
                        if v < 0.4 {
                            "dark".into()
                        } else if v > 0.6 {
                            "bright".into()
                        } else {
                            "white".into()
                        }
                    },
                    col,
                    |w, v| w.noise.color = v,
                );
                kn(ui, st, cid, noise.level, 0.0, 1.0, 0.3, "level", widgets::pct, col, |w, v| w.noise.level = v);
                kn(ui, st, cid, noise.pan, -1.0, 1.0, 0.0, "pan", widgets::pan_fmt, col, |w, v| w.noise.pan = v);
                route_combo(ui, st, cid, ("noise", 0), noise.route, |w, r| w.noise.route = r);
            });
        });
        let fw = ((ui.available_width() - 30.0) / 2.0).max(220.0);
        for k in 0..2 {
            filter_panel(ui, st, cid, p, k, fw - 16.0);
        }
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("Filters").small().color(theme::TEXT_DIM));
        for (r, name, tip) in [(FilterRouting::Serial, "1 ➡ 2", "Filter 1 into filter 2"), (FilterRouting::Parallel, "1 + 2", "Both filters side by side")] {
            if widgets::toggle(ui, p.filter_routing == r, name, theme::ACCENT_2).on_hover_text(tip).clicked() && p.filter_routing != r {
                st.session.edit("Filter routing", |pr| set(pr, cid, |w| w.filter_routing = r));
            }
        }
    });
}

// ------------------------------------------------------------------------------------------------ modulation

fn mod_tab(ui: &mut Ui, st: &mut State, cid: Id, p: &WavetableParams) {
    let id = egui::Id::new(("wave.modsel", cid));
    let (mut env_k, mut lfo_k) = ui.data(|d| d.get_temp::<(usize, usize)>(id)).unwrap_or((0, 0));
    ui.horizontal_top(|ui| {
        let half = ((ui.available_width() - 20.0) / 2.0).max(320.0);
        ui.vertical(|ui| {
            ui.set_width(half);
            ui.horizontal(|ui| {
                for k in 0..4 {
                    if widgets::toggle(ui, env_k == k, &format!("ENV {}", k + 1), MOD_COL).on_hover_text(if k == 0 { "Env 1 shapes the level of every note" } else { "Free envelope: use it in the matrix" }).clicked() {
                        env_k = k;
                    }
                }
            });
            let label = if env_k == 0 { "Env 1 · amp".to_string() } else { format!("Env {}", env_k + 1) };
            let k = env_k;
            super::envelope::editor(ui, st, &label, p.env[k], MOD_COL, move |pr, e| set(pr, cid, |w| w.env[k] = e));
        });
        ui.vertical(|ui| {
            ui.set_width(half);
            ui.horizontal(|ui| {
                for k in 0..4 {
                    if widgets::toggle(ui, lfo_k == k, &format!("LFO {}", k + 1), MOD_COL).clicked() {
                        lfo_k = k;
                    }
                }
            });
            lfo_editor(ui, st, cid, p, lfo_k, half);
        });
    });
    ui.data_mut(|d| d.insert_temp(id, (env_k, lfo_k)));
    ui.add_space(6.0);
    matrix(ui, st, cid, p);
}

fn lfo_editor(ui: &mut Ui, st: &mut State, cid: Id, p: &WavetableParams, k: usize, width: f32) {
    let l = p.lfo[k].clone();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 70.0), Sense::hover());
    let pa = ui.painter_at(rect);
    pa.rect_filled(rect, 5.0, Color32::from_rgb(0x0c, 0x0d, 0x12));
    let pts: Vec<Pos2> = (0..=120)
        .map(|i| {
            let ph = i as f32 / 120.0 * 2.0;
            let v = l.shape.at(ph);
            let v = if l.unipolar { v * 0.5 + 0.5 } else { v };
            Pos2::new(rect.left() + i as f32 / 120.0 * rect.width(), rect.center().y - v * rect.height() * 0.4)
        })
        .collect();
    pa.add(egui::Shape::line(pts, Stroke::new(1.8_f32, MOD_COL)));
    ui.horizontal_wrapped(|ui| {
        let mut shape = l.shape;
        egui::ComboBox::from_id_salt(("wave.lfo.shape", cid, k)).selected_text(shape.name()).width(96.0).show_ui(ui, |ui| {
            for x in LfoShape::ALL {
                ui.selectable_value(&mut shape, x, x.name());
            }
        });
        if shape != l.shape {
            st.session.edit("LFO shape", |pr| set(pr, cid, |w| w.lfo[k].shape = shape));
        }
        if widgets::toggle(ui, l.sync, "sync", MOD_COL).on_hover_text("Lock the rate to the tempo").clicked() {
            st.session.edit("LFO sync", |pr| set(pr, cid, |w| w.lfo[k].sync = !l.sync));
        }
        if l.sync {
            let cur = LFO_BEATS.iter().find(|b| (b.0 - l.beats).abs() < 1e-3).map(|b| b.1).unwrap_or("1/4");
            egui::ComboBox::from_id_salt(("wave.lfo.beats", cid, k)).selected_text(cur).width(64.0).show_ui(ui, |ui| {
                for (b, name) in LFO_BEATS {
                    if ui.selectable_label((b - l.beats).abs() < 1e-3, name).clicked() {
                        st.session.edit("LFO rate", |pr| set(pr, cid, |w| w.lfo[k].beats = b));
                    }
                }
            });
        } else {
            kn(ui, st, cid, l.rate, 0.01, 40.0, 2.0, "rate", |v| format!("{v:.2} Hz"), MOD_COL, move |w, v| w.lfo[k].rate = v);
        }
        let mut mode = l.mode;
        egui::ComboBox::from_id_salt(("wave.lfo.mode", cid, k)).selected_text(format!("{mode:?}")).width(70.0).show_ui(ui, |ui| {
            for (x, tip) in [(LfoMode::Retrig, "Restarts with every note"), (LfoMode::Free, "Runs on for all notes"), (LfoMode::OneShot, "One cycle per note, then holds")] {
                ui.selectable_value(&mut mode, x, format!("{x:?}")).on_hover_text(tip);
            }
        });
        if mode != l.mode {
            st.session.edit("LFO mode", |pr| set(pr, cid, |w| w.lfo[k].mode = mode));
        }
        kn(ui, st, cid, l.delay, 0.0, 4.0, 0.0, "delay", |v| format!("{v:.2} s"), MOD_COL, move |w, v| w.lfo[k].delay = v);
        kn(ui, st, cid, l.rise, 0.0, 4.0, 0.0, "rise", |v| format!("{v:.2} s"), MOD_COL, move |w, v| w.lfo[k].rise = v);
        if widgets::toggle(ui, l.unipolar, "0…1", MOD_COL).on_hover_text("Unipolar (0 … 1) instead of −1 … 1").clicked() {
            st.session.edit("LFO polarity", |pr| set(pr, cid, |w| w.lfo[k].unipolar = !l.unipolar));
        }
    });
}

fn matrix(ui: &mut Ui, st: &mut State, cid: Id, p: &WavetableParams) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("MODULATION MATRIX").small().strong().color(MOD_COL));
        ui.label(RichText::new("a source moves a destination by the amount (− reverses it)").small().color(theme::TEXT_DIM));
        if ui.button("+ Add").clicked() {
            st.session.edit("Add modulation", |pr| set(pr, cid, |w| w.matrix.push(ModSlot { source: ModSource::Lfo(0), dest: ModDest::OscPos(0), amount: 0.5, on: true })));
        }
    });
    if p.matrix.is_empty() {
        ui.label(RichText::new("Nothing modulated yet. Try: LFO 1 ➡ Osc A position, or Env 2 ➡ Filter 1 cutoff.").small().color(theme::TEXT_DIM));
    }
    egui::Grid::new(("wave.matrix", cid)).num_columns(6).spacing([8.0, 4.0]).striped(true).show(ui, |ui| {
        for (i, s) in p.matrix.clone().into_iter().enumerate() {
            if widgets::toggle(ui, s.on, if s.on { "on" } else { "off" }, MOD_COL).clicked() {
                st.session.edit("Modulation on / off", |pr| {
                    set(pr, cid, |w| {
                        if let Some(x) = w.matrix.get_mut(i) {
                            x.on = !s.on
                        }
                    })
                });
            }
            egui::ComboBox::from_id_salt(("wave.src", cid, i)).selected_text(s.source.name()).width(110.0).show_ui(ui, |ui| {
                for x in ModSource::all() {
                    if ui.selectable_label(x == s.source, x.name()).clicked() {
                        st.session.edit("Modulation source", |pr| {
                            set(pr, cid, |w| {
                                if let Some(m) = w.matrix.get_mut(i) {
                                    m.source = x
                                }
                            })
                        });
                    }
                }
            });
            ui.label("➡");
            egui::ComboBox::from_id_salt(("wave.dst", cid, i)).selected_text(s.dest.name()).width(150.0).show_ui(ui, |ui| {
                for x in ModDest::all() {
                    if ui.selectable_label(x == s.dest, x.name()).clicked() {
                        st.session.edit("Modulation destination", |pr| {
                            set(pr, cid, |w| {
                                if let Some(m) = w.matrix.get_mut(i) {
                                    m.dest = x
                                }
                            })
                        });
                    }
                }
            });
            let mut a = s.amount;
            let r = ui.add(egui::Slider::new(&mut a, -1.0..=1.0).show_value(true).custom_formatter(|v, _| format!("{:+.0}%", v * 100.0)));
            widgets::apply(st, &r, "Modulation amount", |pr| {
                set(pr, cid, |w| {
                    if let Some(m) = w.matrix.get_mut(i) {
                        m.amount = a
                    }
                })
            });
            if ui.small_button("×").on_hover_text("Remove").clicked() {
                st.session.edit("Remove modulation", |pr| {
                    set(pr, cid, |w| {
                        if i < w.matrix.len() {
                            w.matrix.remove(i);
                        }
                    })
                });
            }
            ui.end_row();
        }
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Macro names").small().color(theme::TEXT_DIM));
        for k in 0..4 {
            let mut n = p.macro_names[k].clone();
            if ui.add(egui::TextEdit::singleline(&mut n).desired_width(80.0)).changed() {
                st.session.gesture("Macro name", |pr| set(pr, cid, |w| w.macro_names[k] = n));
            }
        }
    });
}

// ------------------------------------------------------------------------------------------------ FX and voicing

fn fx_tab(ui: &mut Ui, st: &mut State, cid: Id, p: &WavetableParams) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("FX RACK").small().strong().color(theme::ACCENT_2));
        ui.label(RichText::new("runs after the voices, top to bottom").small().color(theme::TEXT_DIM));
        ui.menu_button("+ Add effect", |ui| {
            for id in FX_CHOICES {
                if let Some(def) = effects::find(id) {
                    if ui.button(def.name).on_hover_text(def.description).clicked() {
                        st.session.edit("Add synth effect", |pr| set(pr, cid, |w| w.fx.push(FxSlot { id: id.into(), on: true, params: Default::default() })));
                        ui.close_menu();
                    }
                }
            }
        });
    });
    if p.fx.is_empty() {
        ui.label(RichText::new("No effects yet – add distortion, chorus, delay, reverb …").small().color(theme::TEXT_DIM));
    }
    let n = p.fx.len();
    for (i, f) in p.fx.clone().into_iter().enumerate() {
        let Some(def) = effects::find(&f.id) else { continue };
        let col = theme::ACCENT_2;
        egui::Frame::NONE.fill(theme::PANEL_2).corner_radius(6.0).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                if widgets::toggle(ui, f.on, if f.on { "On" } else { "Off" }, col).clicked() {
                    st.session.edit("Effect on / off", |pr| {
                        set(pr, cid, |w| {
                            if let Some(x) = w.fx.get_mut(i) {
                                x.on = !f.on
                            }
                        })
                    });
                }
                ui.label(RichText::new(def.name).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("×").on_hover_text("Remove").clicked() {
                        st.session.edit("Remove synth effect", |pr| {
                            set(pr, cid, |w| {
                                if i < w.fx.len() {
                                    w.fx.remove(i);
                                }
                            })
                        });
                    }
                    if i + 1 < n && ui.small_button("down").clicked() {
                        st.session.edit("Move effect", |pr| set(pr, cid, |w| w.fx.swap(i, i + 1)));
                    }
                    if i > 0 && ui.small_button("up").clicked() {
                        st.session.edit("Move effect", |pr| set(pr, cid, |w| w.fx.swap(i, i - 1)));
                    }
                });
            });
            // The main controls first; the rest folded away so long effects stay tidy.
            let (main, more) = def.params.split_at(def.params.len().min(8));
            fx_params(ui, st, cid, i, &f, main, col);
            if !more.is_empty() {
                egui::CollapsingHeader::new(RichText::new(format!("More controls ({})", more.len())).small()).id_salt(("wave.fxmore", cid, i)).show(ui, |ui| fx_params(ui, st, cid, i, &f, more, col));
            }
        });
        ui.add_space(4.0);
    }
}

fn fx_params(ui: &mut Ui, st: &mut State, cid: Id, i: usize, f: &FxSlot, params: &[effects::ParamInfo], col: Color32) {
    ui.horizontal_wrapped(|ui| {
        for prm in params {
            let v = f.params.get(prm.id).copied().unwrap_or(prm.default);
            let key = prm.id.to_string();
            match prm.kind {
                ParamKind::Toggle => {
                    if ui.checkbox(&mut (v > 0.5), prm.name).clicked() {
                        st.session.edit(prm.name, |pr| {
                            set(pr, cid, |w| {
                                if let Some(x) = w.fx.get_mut(i) {
                                    x.params.insert(key, if v > 0.5 { 0.0 } else { 1.0 });
                                }
                            })
                        });
                    }
                }
                ParamKind::Choice(names) => {
                    let cur = names.get(v.round() as usize).copied().unwrap_or("?");
                    egui::ComboBox::from_id_salt(("wave.fxc", cid, i, prm.id)).selected_text(cur).width(90.0).show_ui(ui, |ui| {
                        for (j, nm) in names.iter().enumerate() {
                            if ui.selectable_label(j == v.round() as usize, *nm).clicked() {
                                let key = key.clone();
                                st.session.edit(prm.name, |pr| {
                                    set(pr, cid, |w| {
                                        if let Some(x) = w.fx.get_mut(i) {
                                            x.params.insert(key, j as f32);
                                        }
                                    })
                                });
                            }
                        }
                    });
                }
                _ => {
                    let prm = *prm;
                    // Long names get shortened so neighbouring labels don't run together.
                    let short = if prm.name.chars().count() > 9 { format!("{}.", prm.name.chars().take(8).collect::<String>().trim_end()) } else { prm.name.to_string() };
                    ui.add_space(2.0);
                    kn(
                        ui,
                        st,
                        cid,
                        v,
                        prm.min,
                        prm.max,
                        prm.default,
                        &short,
                        move |x| prm.format(x),
                        col,
                        move |w, x| {
                            if let Some(s) = w.fx.get_mut(i) {
                                s.params.insert(key, x);
                            }
                        },
                    );
                }
            }
        }
    });
}

fn voice_tab(ui: &mut Ui, st: &mut State, cid: Id, p: &WavetableParams) {
    ui.horizontal_wrapped(|ui| {
        let col = theme::ACCENT_2;
        kn(ui, st, cid, p.polyphony as f32, 1.0, 32.0, 16.0, "voices", |v| format!("{:.0}", v.round()), col, |w, v| w.polyphony = v.round() as u32);
        if widgets::toggle(ui, p.mono, "Mono", col).on_hover_text("One note at a time; the last held note comes back when you let go").clicked() {
            st.session.edit("Mono", |pr| set(pr, cid, |w| w.mono = !p.mono));
        }
        if widgets::toggle(ui, p.legato, "Legato", col).on_hover_text("Mono: overlapping notes don't restart the envelopes").clicked() {
            st.session.edit("Legato", |pr| set(pr, cid, |w| w.legato = !p.legato));
        }
        kn(ui, st, cid, p.glide, 0.0, 1.0, 0.0, "glide", |v| format!("{:.0} ms", v * 1000.0), col, |w, v| w.glide = v);
        kn(ui, st, cid, p.velocity, 0.0, 1.0, 0.5, "velocity", widgets::pct, col, |w, v| w.velocity = v);
    });
    ui.label(RichText::new("Velocity: how much softer quiet notes play. Glide: pitch slides between notes (mono, or slide notes in the piano roll).").small().color(theme::TEXT_DIM));
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Rect;

    #[test]
    fn filter_curves_have_the_right_shape() {
        assert!(filter_db(FilterKind::Lp24, 1000.0, 0.2, 100.0).abs() < 1.0);
        assert!(filter_db(FilterKind::Lp24, 1000.0, 0.2, 8000.0) < -30.0);
        assert!(filter_db(FilterKind::Hp12, 1000.0, 0.2, 100.0) < -30.0);
        assert!(filter_db(FilterKind::Bp, 1000.0, 0.5, 1000.0) > filter_db(FilterKind::Bp, 1000.0, 0.5, 100.0) + 10.0);
    }

    #[test]
    fn the_editor_draws_every_tab() {
        let mut st = State::new(redhive_client_framework::AppContext::new("en"), crate::state::Settings { show_splash: false, ..Default::default() });
        st.new_project();
        let cid = st.session.edit("x", |p| p.add_channel("Wave", Instrument::Wavetable(Box::new(wt::presets()[1].1.clone()))));
        let ctx = egui::Context::default();
        for tab in 0..4u8 {
            let _ = ctx.run(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1300.0, 900.0))), ..Default::default() }, |ctx| {
                ctx.data_mut(|d| d.insert_temp(egui::Id::new(("wave.tab", cid)), tab));
                egui::CentralPanel::default().show(ctx, |ui| {
                    let Some(Instrument::Wavetable(p)) = st.session.project.channel(cid).map(|c| c.instrument.clone()) else { panic!() };
                    editor(ui, &mut st, cid, *p);
                });
            });
        }
    }
}
