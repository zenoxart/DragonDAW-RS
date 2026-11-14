//! Mastering: loudness (momentary / short-term / integrated LUFS, true peak) against a target, a spectrum
//! analyser and goniometer of the master, and a one-click mastering chain.

use super::widgets;
use crate::state::State;
use crate::theme;
use dragon_engine::model::Insert;
use egui::{Pos2, RichText, Stroke, Ui, Vec2};

const TARGETS: &[(&str, f32)] = &[("Streaming (Spotify/YouTube)", -14.0), ("Apple Music", -16.0), ("Podcast", -16.0), ("Broadcast EBU R128", -23.0), ("Club / loud master", -9.0)];

pub fn show(ui: &mut Ui, st: &mut State) {
    let m = st.audio.meters().snapshot();
    let target = st.session.project.loudness_target;
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(250.0);
            ui.heading("Loudness");
            let row = |ui: &mut Ui, n: &str, v: f32, unit: &str, warn: bool| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(n).color(theme::TEXT_DIM));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(if v < -69.0 { "–".into() } else { format!("{v:.1} {unit}") }).monospace().size(16.0).color(if warn { theme::WARN } else { egui::Color32::WHITE }));
                    });
                });
            };
            row(ui, "Momentary", m.momentary, "LUFS", false);
            row(ui, "Short-term", m.short_term, "LUFS", false);
            row(ui, "Integrated", m.integrated, "LUFS", m.integrated > target + 1.0);
            let tp = if m.momentary < -69.0 && m.true_peak == 0.0 { -70.0 } else { m.true_peak };
            row(ui, "True peak", tp, "dBTP", tp > -1.0);
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Target");
                let mut t = target;
                let r = ui.add(egui::DragValue::new(&mut t).range(-36.0..=-5.0).speed(0.1).suffix(" LUFS"));
                widgets::apply(st, &r, "Loudness target", |p| p.loudness_target = t);
                ui.menu_button("…", |ui| {
                    for (n, v) in TARGETS {
                        if ui.button(format!("{n}  {v}")).clicked() {
                            st.session.edit("Loudness target", |p| p.loudness_target = *v);
                            ui.close_menu();
                        }
                    }
                });
            });
            if m.integrated > -69.0 {
                let diff = target - m.integrated;
                ui.label(RichText::new(if diff.abs() < 0.5 { "On target".to_string() } else if diff > 0.0 { format!("{diff:.1} dB below target") } else { format!("{:.1} dB above target", -diff) }).color(if diff.abs() < 1.0 { theme::OK } else { theme::WARN }));
            } else {
                ui.label(RichText::new("Play the song to measure.").color(theme::TEXT_DIM));
            }
            ui.label(RichText::new("Export can normalise to the target (File › Export › Normalise: loudness).").small().color(theme::TEXT_DIM));
            ui.separator();
            if ui.button("Add mastering chain to master").on_hover_text("EQ ➡ multiband compressor ➡ stereo imager ➡ limiter (−1 dBTP)").clicked() {
                st.session.edit("Mastering chain", |p| {
                    for id in ["eq", "multiband", "imager", "limiter"] {
                        if p.mixer.master.inserts.iter().any(|i| i.effect == id) {
                            continue;
                        }
                        let iid = p.next_id();
                        let mut params = dragon_engine::effects::find(id).map(|d| d.defaults()).unwrap_or_default();
                        if id == "limiter" {
                            params.insert("ceiling".into(), -1.0);
                        }
                        p.mixer.master.inserts.push(Insert { id: iid, effect: id.into(), enabled: true, mix: 1.0, params, sidechain: None, plugin: None, preset: String::new() });
                    }
                    // Keep the limiter last.
                    p.mixer.master.inserts.sort_by_key(|i| i.effect == "limiter");
                });
                st.ui.mixer = st.session.project.mixer.master.id;
            }
        });
        ui.separator();
        ui.vertical(|ui| {
            ui.heading("Spectrum");
            spectrum(ui, &m.scope, st.audio.rate() as f32);
        });
        ui.vertical(|ui| {
            ui.heading("Master");
            let (l, r) = m.strips.get(&st.session.project.mixer.master.id).copied().unwrap_or((0.0, 0.0));
            ui.horizontal(|ui| {
                widgets::meter(ui, l, r, 170.0);
                let mut v = st.session.project.mixer.master.volume;
                let resp = widgets::fader(ui, &mut v, 2.0, 170.0);
                widgets::apply(st, &resp, "Master volume", |p| p.mixer.master.volume = v);
            });
        });
    });
}

fn spectrum(ui: &mut Ui, scope: &[f32], rate: f32) {
    let w = ui.available_width().clamp(200.0, 700.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 180.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 4.0, theme::BG);
    for db in [-60.0f32, -40.0, -20.0, 0.0] {
        let y = rect.top() + (-db / 72.0) * rect.height();
        p.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(1.0_f32, theme::GRID));
        p.text(Pos2::new(rect.left() + 2.0, y), egui::Align2::LEFT_BOTTOM, format!("{db:.0}"), egui::FontId::monospace(9.0), theme::TEXT_DIM);
    }
    for f in [100.0f32, 1000.0, 10000.0] {
        let x = rect.left() + (f / 20.0).log10() / 3.0 * rect.width();
        p.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(1.0_f32, theme::GRID));
        p.text(Pos2::new(x + 2.0, rect.bottom()), egui::Align2::LEFT_BOTTOM, if f >= 1000.0 { format!("{}k", f / 1000.0) } else { format!("{f}") }, egui::FontId::monospace(9.0), theme::TEXT_DIM);
    }
    if scope.len() < 256 {
        return;
    }
    // Goertzel over log-spaced bins with a Hann window.
    let n = scope.len().min(4096);
    let x = &scope[scope.len() - n..];
    let win: Vec<f32> = (0..n).map(|i| x[i] * (0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos())).collect();
    let bins = 96;
    let pts: Vec<Pos2> = (0..bins)
        .map(|k| {
            let f = 20.0 * 1000f32.powf(k as f32 / (bins - 1) as f32);
            let w = std::f32::consts::TAU * f / rate;
            let coeff = 2.0 * w.cos();
            let (mut s1, mut s2) = (0.0f32, 0.0f32);
            for v in &win {
                let s0 = v + coeff * s1 - s2;
                s2 = s1;
                s1 = s0;
            }
            let pow = s1 * s1 + s2 * s2 - coeff * s1 * s2;
            let mag = pow.max(1e-12).sqrt() / (n as f32 / 4.0);
            let db = dragon_engine::dsp::gain_to_db(mag).clamp(-72.0, 0.0);
            Pos2::new(rect.left() + k as f32 / (bins - 1) as f32 * rect.width(), rect.top() + (-db / 72.0) * rect.height())
        })
        .collect();
    let mut fill = pts.clone();
    fill.push(rect.right_bottom());
    fill.push(rect.left_bottom());
    p.add(egui::Shape::line(pts, Stroke::new(1.5_f32, theme::ACCENT_2)));
}
