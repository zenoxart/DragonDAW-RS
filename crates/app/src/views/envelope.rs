//! FL Studio-style envelope editor: the DAHDSR curve as a graph with draggable points (attack peak, hold
//! end, decay / sustain point, release end), tension handles on the attack and decay / release segments,
//! knobs underneath, and envelope presets (built-in shapes plus your own, saved as JSON).

use super::widgets::{self, knob};
use crate::state::State;
use crate::theme;
use dragon_engine::model::{Envelope, Project};
use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};

/// Built-in shapes.
pub fn builtin_presets() -> Vec<(&'static str, Envelope)> {
    let e =
        |delay: f32, attack: f32, hold: f32, decay: f32, sustain: f32, release: f32, at: f32, rt: f32| Envelope { enabled: true, delay, attack, hold, decay, sustain, release, attack_tension: at, release_tension: rt };
    vec![
        ("Default (full sample)", e(0.0, 0.0, 0.0, 0.0, 1.0, 0.05, 0.0, 0.0)),
        ("Pluck", e(0.0, 0.002, 0.0, 0.25, 0.0, 0.1, 0.0, -0.6)),
        ("Punch", e(0.0, 0.001, 0.04, 0.15, 0.35, 0.12, 0.0, -0.4)),
        ("Short gate", e(0.0, 0.002, 0.08, 0.01, 0.0, 0.01, 0.0, 0.0)),
        ("Pad", e(0.0, 0.6, 0.0, 0.8, 0.7, 1.5, 0.4, -0.2)),
        ("Swell", e(0.0, 1.2, 0.0, 0.0, 1.0, 0.4, 0.8, 0.0)),
        ("Tail cut", e(0.0, 0.0, 0.0, 0.0, 1.0, 0.02, 0.0, 0.0)),
        ("Delayed fade-in", e(0.15, 0.3, 0.0, 0.0, 1.0, 0.3, 0.3, 0.0)),
    ]
}

fn presets_dir(st: &State) -> std::path::PathBuf {
    st.ctx.paths.config.join("Presets").join("Envelopes")
}

/// Your saved envelope presets (name, envelope), sorted by name.
pub fn user_presets(st: &State) -> Vec<(String, Envelope)> {
    let mut v: Vec<(String, Envelope)> = std::fs::read_dir(presets_dir(st))
        .map(|rd| {
            rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).filter_map(|p| Some((p.file_stem()?.to_string_lossy().to_string(), st.ctx.paths.load_from::<Envelope>(&p)?))).collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

pub fn save_preset(st: &State, name: &str, e: &Envelope) {
    let dir = presets_dir(st);
    let _ = std::fs::create_dir_all(&dir);
    st.ctx.paths.save_to(&dir.join(format!("{}.json", crate::state::sanitize(name))), e);
}

/// Seconds shown for the sustain part of the graph.
fn sustain_view(e: &Envelope) -> f32 {
    ((e.delay + e.attack + e.hold + e.decay + e.release) * 0.35).clamp(0.15, 2.0)
}

/// The graph; returns the edited envelope while a point or handle is dragged.
pub fn graph(ui: &mut Ui, id: egui::Id, e: &Envelope, color: Color32, size: Vec2) -> Option<Envelope> {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 6.0, Color32::from_rgb(0x12, 0x14, 0x18));
    let pr = rect.shrink2(Vec2::new(10.0, 10.0));
    let sus = sustain_view(e);
    let held = e.delay + e.attack + e.hold + e.decay + sus;
    let total = (held + e.release).max(0.05);
    let x_of = |t: f32| pr.left() + t / total * pr.width();
    let t_of = |x: f32| ((x - pr.left()) / pr.width() * total).max(0.0);
    let y_of = |v: f32| pr.bottom() - v * pr.height();
    // Grid: quarter lines and second marks.
    for k in 1..4 {
        let y = pr.top() + pr.height() * k as f32 / 4.0;
        p.line_segment([Pos2::new(pr.left(), y), Pos2::new(pr.right(), y)], Stroke::new(1.0_f32, Color32::from_white_alpha(10)));
    }
    // The curve.
    let pts: Vec<Pos2> = (0..=200).map(|k| total * k as f32 / 200.0).map(|t| Pos2::new(x_of(t), y_of(e.level_at(t, held)))).collect();
    {
        use egui::epaint::{Mesh, Vertex, WHITE_UV};
        let mut mesh = Mesh::default();
        for w in pts.windows(2) {
            let i = mesh.vertices.len() as u32;
            for (pos, c) in
                [(w[0], color.gamma_multiply(0.4)), (w[1], color.gamma_multiply(0.4)), (Pos2::new(w[1].x, pr.bottom()), color.gamma_multiply(0.02)), (Pos2::new(w[0].x, pr.bottom()), color.gamma_multiply(0.02))]
            {
                mesh.vertices.push(Vertex { pos, uv: WHITE_UV, color: c });
            }
            mesh.indices.extend_from_slice(&[i, i + 1, i + 2, i, i + 2, i + 3]);
        }
        p.add(egui::Shape::mesh(mesh));
    }
    p.add(egui::Shape::line(pts, Stroke::new(2.0_f32, color)));
    // Note-off marker.
    p.extend(egui::Shape::dashed_line(&[Pos2::new(x_of(held), pr.top()), Pos2::new(x_of(held), pr.bottom())], Stroke::new(1.0_f32, Color32::from_white_alpha(50)), 3.0, 3.0));
    p.text(Pos2::new(x_of(held) + 3.0, pr.top()), egui::Align2::LEFT_TOP, "note off", egui::FontId::proportional(9.0), Color32::from_white_alpha(90));
    // Points: 0 delay end, 1 attack peak, 2 hold end, 3 decay end (sustain), 4 release end.
    let nodes = [
        Pos2::new(x_of(e.delay), y_of(0.0)),
        Pos2::new(x_of(e.delay + e.attack), y_of(1.0)),
        Pos2::new(x_of(e.delay + e.attack + e.hold), y_of(1.0)),
        Pos2::new(x_of(e.delay + e.attack + e.hold + e.decay), y_of(e.sustain)),
        Pos2::new(x_of(held + e.release), y_of(0.0)),
    ];
    // Tension handles at the middle of the attack and the release.
    let mid_a = Pos2::new(x_of(e.delay + e.attack * 0.5), y_of(e.level_at(e.delay + e.attack * 0.5, held)));
    let mid_r = Pos2::new(x_of(held + e.release * 0.5), y_of(e.level_at(held + e.release * 0.5, held)));
    let mut out = None;
    let names = ["delay", "attack", "hold", "decay / sustain", "release"];
    for (k, n) in nodes.iter().enumerate() {
        let r = ui.interact(Rect::from_center_size(*n, Vec2::splat(14.0)), id.with(("node", k)), Sense::drag());
        let hot = r.hovered() || r.dragged();
        p.circle_filled(*n, if hot { 6.0 } else { 4.5 }, if hot { Color32::WHITE } else { color });
        p.circle_stroke(*n, 6.5, Stroke::new(1.0_f32, color.gamma_multiply(0.5)));
        if hot {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        let r = r.on_hover_text(format!("Drag the {} point", names[k]));
        if let Some(pos) = r.dragged().then(|| r.interact_pointer_pos()).flatten() {
            let t = t_of(pos.x);
            let mut x = *e;
            match k {
                0 => x.delay = t.min(4.0),
                1 => x.attack = (t - e.delay).clamp(0.0, 8.0),
                2 => x.hold = (t - e.delay - e.attack).clamp(0.0, 4.0),
                3 => {
                    x.decay = (t - e.delay - e.attack - e.hold).clamp(0.0, 8.0);
                    x.sustain = ((pr.bottom() - pos.y) / pr.height()).clamp(0.0, 1.0);
                }
                _ => x.release = (t - held).clamp(0.0, 12.0),
            }
            x.enabled = true;
            out = Some(x);
        }
    }
    for (k, c) in [mid_a, mid_r].into_iter().enumerate() {
        let r = ui.interact(Rect::from_center_size(c, Vec2::splat(12.0)), id.with(("tension", k)), Sense::drag());
        let hot = r.hovered() || r.dragged();
        p.circle_stroke(c, if hot { 5.0 } else { 3.5 }, Stroke::new(1.5_f32, if hot { Color32::WHITE } else { color.gamma_multiply(0.8) }));
        let r = r.on_hover_text("Drag up / down to bend the curve");
        if r.dragged() {
            let d = -r.drag_delta().y / 80.0;
            let mut x = out.unwrap_or(*e);
            if k == 0 {
                x.attack_tension = (x.attack_tension - d).clamp(-1.0, 1.0);
            } else {
                x.release_tension = (x.release_tension + d).clamp(-1.0, 1.0);
            }
            x.enabled = true;
            out = Some(x);
        }
    }
    if !e.enabled {
        p.rect_filled(rect, 6.0, Color32::from_black_alpha(90));
        p.text(rect.center(), egui::Align2::CENTER_CENTER, "envelope off – drag a point to switch it on", egui::FontId::proportional(11.0), theme::TEXT_DIM);
    }
    ui.data_mut(|d| d.insert_temp(id.with("nodes"), nodes.to_vec()));
    out
}

/// Full editor: graph, presets, knobs. `set` writes the envelope into the project.
pub fn editor(ui: &mut Ui, st: &mut State, label: &str, env: Envelope, color: Color32, set: impl Fn(&mut Project, Envelope) + Copy) {
    let id = ui.id().with(("env", label));
    ui.horizontal(|ui| {
        ui.label(RichText::new(label.to_uppercase()).size(10.0).strong().color(color));
        let mut e = env;
        if ui.checkbox(&mut e.enabled, "on").changed() {
            st.session.edit(label, |p| set(p, e));
        }
        // Presets.
        let user = user_presets(st);
        egui::ComboBox::from_id_salt(id.with("preset")).selected_text("Presets").width(110.0).show_ui(ui, |ui| {
            ui.label(RichText::new("BUILT-IN").small().color(theme::TEXT_DIM));
            for (name, pe) in builtin_presets() {
                if ui.selectable_label(false, name).clicked() {
                    st.session.edit("Envelope preset", |p| set(p, pe));
                }
            }
            if !user.is_empty() {
                ui.separator();
                ui.label(RichText::new("YOURS").small().color(theme::TEXT_DIM));
            }
            for (name, pe) in &user {
                if ui.selectable_label(false, name).clicked() {
                    let pe = *pe;
                    st.session.edit("Envelope preset", |p| set(p, pe));
                }
            }
        });
        let nid = id.with("name");
        let mut name: String = ui.data_mut(|d| d.get_temp(nid).unwrap_or_default());
        ui.add(egui::TextEdit::singleline(&mut name).hint_text("preset name").desired_width(90.0));
        if ui.button("Save").on_hover_text("Save this envelope as a preset").clicked() && !name.trim().is_empty() {
            save_preset(st, name.trim(), &env);
            st.ui.status = format!("Saved envelope preset \"{}\"", name.trim());
            name.clear();
        }
        ui.data_mut(|d| d.insert_temp(nid, name));
    });
    let w = ui.available_width().max(280.0);
    if let Some(e) = graph(ui, id.with("graph"), &env, color, Vec2::new(w, 120.0)) {
        st.session.gesture(label, |p| set(p, e));
    }
    ui.horizontal_wrapped(|ui| {
        let secs = |v: f32| if v < 1.0 { format!("{:.0} ms", v * 1000.0) } else { format!("{v:.2} s") };
        let fields: [(&str, f32, f32, f32); 6] =
            [("delay", env.delay, 0.0, 2.0), ("attack", env.attack, 0.0, 4.0), ("hold", env.hold, 0.0, 2.0), ("decay", env.decay, 0.0, 4.0), ("sustain", env.sustain, 0.0, 1.0), ("release", env.release, 0.0, 8.0)];
        for (k, (name, v, min, max)) in fields.into_iter().enumerate() {
            let mut x = v;
            let r = if name == "sustain" { knob(ui, &mut x, min, max, 1.0, name, widgets::pct) } else { knob(ui, &mut x, min, max, 0.0, name, secs) };
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
        }
        let mut at = env.attack_tension;
        let r = knob(ui, &mut at, -1.0, 1.0, 0.0, "att. bend", |v| format!("{v:+.2}"));
        widgets::apply(st, &r, label, |p| set(p, Envelope { attack_tension: at, ..env }));
        let mut rt = env.release_tension;
        let r = knob(ui, &mut rt, -1.0, 1.0, 0.0, "rel. bend", |v| format!("{v:+.2}"));
        widgets::apply(st, &r, label, |p| set(p, Envelope { release_tension: rt, ..env }));
    });
    if st.session.in_gesture() && !ui.input(|i| i.pointer.any_down()) {
        st.session.end_gesture();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_curve_matches_the_generator() {
        let e = Envelope { enabled: true, delay: 0.0, attack: 0.01, hold: 0.0, decay: 0.01, sustain: 0.5, release: 0.01, attack_tension: 0.0, release_tension: 0.0 };
        let mut g = dragon_engine::dsp::EnvGen::new(e, 1000.0);
        g.trigger();
        let mut out = Vec::new();
        for i in 0..60 {
            if i == 40 {
                g.release();
            }
            out.push(g.next());
        }
        for (i, v) in out.iter().enumerate() {
            let t = (i + 1) as f32 / 1000.0;
            assert!((v - e.level_at(t, 0.04)).abs() < 0.12, "t {t}: generator {v} vs graph {}", e.level_at(t, 0.04));
        }
    }

    /// Dragging the decay / sustain point lowers the sustain level and lengthens the decay.
    #[test]
    fn dragging_the_sustain_point_edits_the_envelope() {
        let ctx = egui::Context::default();
        let e = Envelope { enabled: true, delay: 0.0, attack: 0.05, hold: 0.0, decay: 0.2, sustain: 0.8, release: 0.3, attack_tension: 0.0, release_tension: 0.0 };
        let id = egui::Id::new("t");
        let mut last: Option<Envelope> = None;
        let mut frame = |events: Vec<egui::Event>, last: &mut Option<Envelope>| {
            let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(600.0, 300.0))), events, ..Default::default() };
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    if let Some(x) = graph(ui, id, &e, Color32::WHITE, Vec2::new(400.0, 120.0)) {
                        *last = Some(x);
                    }
                });
            });
        };
        frame(vec![], &mut last);
        let nodes: Vec<Pos2> = ctx.data(|d| d.get_temp(id.with("nodes"))).unwrap();
        let a = nodes[3];
        let b = a + egui::vec2(30.0, 40.0);
        frame(vec![egui::Event::PointerMoved(a)], &mut last);
        frame(vec![egui::Event::PointerButton { pos: a, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }], &mut last);
        for k in 1..=5 {
            frame(vec![egui::Event::PointerMoved(a + (b - a) * (k as f32 / 5.0))], &mut last);
        }
        let x = last.expect("edited");
        assert!(x.sustain < 0.6, "sustain lowered: {}", x.sustain);
        assert!(x.decay > 0.2, "decay longer: {}", x.decay);
    }
}
