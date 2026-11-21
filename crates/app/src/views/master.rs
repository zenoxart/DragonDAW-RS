//! Mastering: loudness (momentary / short-term / integrated LUFS, true peak) against a target, a spectrum
//! analyser and goniometer of the master, a one-click mastering chain, and the reference-track comparison.

use super::widgets;
use crate::state::State;
use crate::theme;
use egui::{Pos2, RichText, Stroke, Ui, Vec2};

const TARGETS: &[(&str, f32)] = &[("Streaming (Spotify/YouTube)", -14.0), ("Apple Music", -16.0), ("Podcast", -16.0), ("Broadcast EBU R128", -23.0), ("Club / loud master", -9.0)];

pub fn show(ui: &mut Ui, st: &mut State) {
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        meters(ui, st);
        ui.separator();
        if st.session.project.mode == "album" {
            super::album::show(ui, st);
            ui.separator();
        }
        master_chain(ui, st);
        ui.separator();
        super::reference::show(ui, st);
    });
}

/// The master's effect chain: a signal-flow strip (click a block to open it, its switch bypasses it) and
/// the full slot list (add, drag to reorder, chains, sidechains) as in the mixer.
fn master_chain(ui: &mut Ui, st: &mut State) {
    let meters = st.audio.meters().snapshot();
    let all: Vec<dragon_engine::model::MixerChannel> = std::iter::once(st.session.project.mixer.master.clone()).chain(st.session.project.mixer.channels.iter().cloned()).collect();
    let master = all[0].clone();
    ui.heading("Master chain");
    ui.add_space(4.0);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(380.0);
            super::mixer::insert_panel(ui, st, &master, &all, &meters);
        });
    });
}

fn meters(ui: &mut Ui, st: &mut State) {
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
                ui.label(
                    RichText::new(if diff.abs() < 0.5 {
                        "On target".to_string()
                    } else if diff > 0.0 {
                        format!("{diff:.1} dB below target")
                    } else {
                        format!("{:.1} dB above target", -diff)
                    })
                    .color(if diff.abs() < 1.0 { theme::OK } else { theme::WARN }),
                );
            } else {
                ui.label(RichText::new("Play the song to measure.").color(theme::TEXT_DIM));
            }
            ui.label(RichText::new("Export can normalise to the target (File › Export › Normalise: loudness).").small().color(theme::TEXT_DIM));
            ui.separator();
            if ui.button("Add mastering chain to master").on_hover_text("Compressor (gentle glue) ➡ Obsidian soft clipper ➡ Philosopher's Stone – all stock").clicked() {
                st.session.edit("Mastering chain", crate::modes::add_stone_chain);
                st.ui.mixer = st.session.project.mixer.master.id;
            }
        });
        ui.add_space(14.0);
        ui.vertical(|ui| {
            ui.heading("Spectrum");
            spectrum_3d(ui, &m.scope, st.audio.rate() as f32, !st.settings.reduced_motion);
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

/// A surface point: (frequency 0 … 1, age 0 … 1, level 0 … 1).
type Vertex3 = (f32, f32, f32);

/// The 3D spectrum's starting view (yaw, pitch in radians): from the front left, looking down a little.
const DEFAULT_VIEW: (f32, f32) = (-0.45, 0.55);

/// Seconds of history the 3D spectrum shows.
const HISTORY: f64 = 5.0;
/// Frequency bins of the 3D spectrum (log spaced, 20 Hz … 20 kHz).
const BINS: usize = 72;
/// The level floor (dBFS).
const FLOOR: f32 = -72.0;

/// The level (dBFS) of `scope`'s most recent samples in `BINS` log-spaced bands (Goertzel, Hann window).
fn analyse(scope: &[f32], rate: f32) -> Vec<f32> {
    if scope.len() < 256 {
        return vec![FLOOR; BINS];
    }
    let n = scope.len().min(2048);
    let x = &scope[scope.len() - n..];
    let win: Vec<f32> = (0..n).map(|i| x[i] * (0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos())).collect();
    (0..BINS)
        .map(|k| {
            let f = 20.0 * 1000f32.powf(k as f32 / (BINS - 1) as f32);
            if f >= rate * 0.5 {
                return FLOOR;
            }
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
            dragon_engine::dsp::gain_to_db(mag).clamp(FLOOR, 0.0)
        })
        .collect()
}

/// Heat colour of a level 0 … 1 (black → violet → red → orange → yellow → white).
fn heat(t: f32) -> egui::Color32 {
    const STOPS: [(f32, [u8; 3]); 6] = [(0.0, [8, 6, 20]), (0.25, [70, 18, 110]), (0.5, [190, 40, 70]), (0.7, [245, 110, 30]), (0.88, [252, 200, 50]), (1.0, [255, 250, 220])];
    let t = t.clamp(0.0, 1.0);
    let k = STOPS.iter().position(|s| s.0 >= t).unwrap_or(STOPS.len() - 1).max(1);
    let (a, b) = (STOPS[k - 1], STOPS[k]);
    let q = ((t - a.0) / (b.0 - a.0).max(1e-6)).clamp(0.0, 1.0);
    let mix = |i: usize| (a.1[i] as f32 + (b.1[i] as f32 - a.1[i] as f32) * q) as u8;
    egui::Color32::from_rgb(mix(0), mix(1), mix(2))
}

type History = std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<(f64, Vec<f32>)>>>;

/// The master spectrum as a real-time 3D heat landscape: frequency across, level up, the last five seconds
/// running into the depth (now at the front).
fn spectrum_3d(ui: &mut Ui, scope: &[f32], rate: f32, animate: bool) {
    let hist: History = ui.data_mut(|d| d.get_temp_mut_or_default::<History>(egui::Id::new("master.spectrum3d")).clone());
    let now = ui.input(|i| i.time);
    {
        let mut h = hist.lock().unwrap_or_else(|e| e.into_inner());
        if h.back().is_none_or(|(t, _)| now - t >= 1.0 / 30.0) {
            h.push_back((now, analyse(scope, rate)));
        }
        while h.front().is_some_and(|(t, _)| now - t > HISTORY) {
            h.pop_front();
        }
    }
    if animate {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
    }
    let w = ui.available_width().clamp(320.0, 760.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 260.0), egui::Sense::click_and_drag());
    // The view turns with a left-drag (yaw across, tilt up/down); a double-click resets it.
    let view_id = egui::Id::new("master.spectrum3d.view");
    let (mut yaw, mut pitch): (f32, f32) = ui.data(|d| d.get_temp(view_id)).unwrap_or(DEFAULT_VIEW);
    if resp.dragged_by(egui::PointerButton::Primary) {
        let d = resp.drag_delta();
        yaw = (yaw + d.x * 0.008).clamp(-1.5, 1.5);
        pitch = (pitch + d.y * 0.008).clamp(0.0, 1.45);
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    if resp.double_clicked() {
        (yaw, pitch) = DEFAULT_VIEW;
    }
    ui.data_mut(|d| d.insert_temp(view_id, (yaw, pitch)));
    let resp = resp.on_hover_text("Drag to rotate · double-click to reset the view");
    let _ = resp;
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 4.0, egui::Color32::from_rgb(0x0b, 0x0c, 0x12));
    // A real 3D projection: f (0 … 1 across), age (0 = now at the front … 1 = five seconds ago, into the
    // depth), level (0 … 1 up); turned by yaw / pitch and drawn orthographically.
    let (cy, sy, cp, sp) = (yaw.cos(), yaw.sin(), pitch.cos(), pitch.sin());
    let scale = (rect.width() / 2.0).min(rect.height() / 1.25);
    let centre = Pos2::new(rect.center().x, rect.center().y + rect.height() * 0.08);
    let rot = move |f: f32, age: f32, lvl: f32| {
        let (x, y, z) = ((f - 0.5) * 1.6, lvl * 0.7 - 0.2, (age - 0.5) * 1.1);
        let (x1, z1) = (x * cy + z * sy, -x * sy + z * cy);
        (x1, y * cp + z1 * sp, z1 * cp - y * sp)
    };
    let proj = move |f: f32, age: f32, lvl: f32| {
        let (x, up, _) = rot(f, age, lvl);
        Pos2::new(centre.x + x * scale, centre.y - up * scale)
    };
    let depth = move |f: f32, age: f32, lvl: f32| rot(f, age, lvl).2;
    let grid = Stroke::new(1.0_f32, egui::Color32::from_white_alpha(22));
    let small = egui::FontId::monospace(9.0);
    // Floor grid and the back wall.
    for f in [100.0f32, 1000.0, 10000.0] {
        let x = (f / 20.0).log10() / 3.0;
        p.line_segment([proj(x, 0.0, 0.0), proj(x, 1.0, 0.0)], grid);
        p.text(proj(x, 0.0, 0.0) + Vec2::new(0.0, 3.0), egui::Align2::CENTER_TOP, if f >= 1000.0 { format!("{}k", f / 1000.0) } else { format!("{f}") }, small.clone(), theme::TEXT_DIM);
    }
    for s in [1.0f32, 2.0, 3.0, 4.0, 5.0] {
        let a = s / HISTORY as f32;
        p.line_segment([proj(0.0, a, 0.0), proj(1.0, a, 0.0)], grid);
        p.text(proj(1.0, a, 0.0) + Vec2::new(4.0, 0.0), egui::Align2::LEFT_CENTER, format!("-{s:.0}s"), small.clone(), theme::TEXT_DIM);
    }
    for db in [-60.0f32, -48.0, -36.0, -24.0, -12.0, 0.0] {
        let l = (db - FLOOR) / -FLOOR;
        p.line_segment([proj(0.0, 1.0, l), proj(1.0, 1.0, l)], grid);
        p.line_segment([proj(0.0, 0.0, l), proj(0.0, 1.0, l)], grid);
        p.text(proj(0.0, 0.0, l) - Vec2::new(4.0, 0.0), egui::Align2::RIGHT_CENTER, format!("{db:.0}"), small.clone(), theme::TEXT_DIM);
    }
    p.line_segment([proj(0.0, 0.0, 0.0), proj(1.0, 0.0, 0.0)], Stroke::new(1.0_f32, egui::Color32::from_white_alpha(60)));
    p.line_segment([proj(0.0, 0.0, 0.0), proj(0.0, 0.0, 1.0)], Stroke::new(1.0_f32, egui::Color32::from_white_alpha(60)));
    p.line_segment([proj(1.0, 0.0, 0.0), proj(1.0, 1.0, 0.0)], Stroke::new(1.0_f32, egui::Color32::from_white_alpha(60)));
    p.text(proj(0.5, 0.0, 0.0) + Vec2::new(0.0, 14.0), egui::Align2::CENTER_TOP, "frequency", egui::FontId::proportional(9.5), theme::TEXT_DIM);
    p.text(proj(0.0, 0.0, 1.0) + Vec2::new(0.0, -4.0), egui::Align2::CENTER_BOTTOM, "dB", egui::FontId::proportional(9.5), theme::TEXT_DIM);
    p.text(proj(1.0, 0.55, 0.0) + Vec2::new(18.0, 6.0), egui::Align2::LEFT_CENTER, "time", egui::FontId::proportional(9.5), theme::TEXT_DIM);

    // The surface, from the oldest frame to now (later strips are drawn over the ones behind them).
    let h = hist.lock().unwrap_or_else(|e| e.into_inner());
    let frames: Vec<(f32, &Vec<f32>)> = h.iter().map(|(t, v)| (((now - t) / HISTORY) as f32, v)).collect();
    let lvl = |db: f32| ((db - FLOOR) / -FLOOR).clamp(0.0, 1.0);
    // Painter's algorithm: the quads furthest from the viewer first, whatever the rotation.
    let mut quads: Vec<(f32, [Vertex3; 4])> = Vec::with_capacity(frames.len() * BINS);
    for pair in frames.windows(2) {
        let ((a0, v0), (a1, v1)) = (pair[0], pair[1]);
        for k in 0..BINS - 1 {
            let f0 = k as f32 / (BINS - 1) as f32;
            let f1 = (k + 1) as f32 / (BINS - 1) as f32;
            let q = [(f0, a0, lvl(v0[k])), (f1, a0, lvl(v0[k + 1])), (f1, a1, lvl(v1[k + 1])), (f0, a1, lvl(v1[k]))];
            let d = q.iter().map(|(f, a, l)| depth(*f, *a, *l)).sum::<f32>();
            quads.push((d, q));
        }
    }
    quads.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut mesh = egui::epaint::Mesh::default();
    for (_, q) in &quads {
        let base = mesh.vertices.len() as u32;
        for (f, age, l) in q {
            // Older rows fade towards the background.
            let c = heat(*l).gamma_multiply(1.0 - 0.55 * age.clamp(0.0, 1.0));
            mesh.colored_vertex(proj(*f, *age, *l), c);
        }
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    p.add(egui::Shape::mesh(mesh));
    // The current spectrum as a bright line along the front.
    if let Some((age, v)) = frames.last() {
        let pts: Vec<Pos2> = v.iter().enumerate().map(|(k, db)| proj(k as f32 / (BINS - 1) as f32, *age, lvl(*db))).collect();
        p.add(egui::Shape::line(pts, Stroke::new(1.5_f32, egui::Color32::from_rgb(255, 236, 200))));
    }
}
