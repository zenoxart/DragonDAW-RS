//! The look of the stock effect editors: a header (category stripe, power LED, gain-reduction meter) and an
//! animated "hero" display per effect that shows what it does: EQ / filter response, compressor transfer
//! curve with a gain-reduction ball, delay echo taps, reverb decay tail, saturation curve, LFO motion,
//! stereo field and so on. Everything is drawn procedurally so it scales and stays sharp; the motion
//! stops when "Reduce motion" is on.

use crate::theme;
use dragon_engine::dsp::{Biquad, BiquadKind};
use dragon_engine::effects::{self, EffectDef};
use egui::{Color32, Pos2, Rect, Shape, Stroke, Ui, Vec2};
use std::collections::BTreeMap;
use std::f32::consts::{PI, TAU};

const RATE: f32 = 48_000.0;

/// What the hero display needs to know about the insert.
pub struct Hero<'a> {
    pub def: &'a EffectDef,
    pub params: &'a BTreeMap<String, f32>,
    /// Current gain reduction in dB (dynamics).
    pub gr: f32,
    pub enabled: bool,
    pub color: Color32,
    /// Seconds since start; `None` freezes the animation.
    pub time: Option<f32>,
    /// Live values the effect reports (see `Effect::telemetry`); empty when there are none.
    pub tele: &'a [f32],
    /// Identifies the insert (keeps per-insert display history such as Aurum Tune's pitch trace).
    pub key: egui::Id,
}

impl Hero<'_> {
    pub(super) fn v(&self, id: &str) -> f32 {
        effects::value(self.def, self.params, id)
    }
    pub(super) fn t(&self) -> f32 {
        self.time.unwrap_or(0.0)
    }
}

/// Log-frequency x position (20 Hz … 20 kHz) inside `r`.
pub(super) fn fx(r: Rect, f: f32) -> f32 {
    r.left() + r.width() * ((f.max(20.0) / 20.0).log10() / 3.0).clamp(0.0, 1.0)
}

pub(super) fn freq_at(r: Rect, x: f32) -> f32 {
    20.0 * 1000f32.powf(((x - r.left()) / r.width()).clamp(0.0, 1.0))
}

/// A filled curve: a glowing line over a vertical gradient that fades towards `base_y` (the look of the
/// reference sheet).
pub(super) fn filled_curve(p: &egui::Painter, pts: &[Pos2], base_y: f32, col: Color32) {
    use egui::epaint::{Mesh, Vertex, WHITE_UV};
    let mut mesh = Mesh::default();
    let top = col.gamma_multiply(0.42);
    let bottom = col.gamma_multiply(0.02);
    for w in pts.windows(2) {
        let i = mesh.vertices.len() as u32;
        for (pos, c) in [(w[0], top), (w[1], top), (Pos2::new(w[1].x, base_y), bottom), (Pos2::new(w[0].x, base_y), bottom)] {
            mesh.vertices.push(Vertex { pos, uv: WHITE_UV, color: c });
        }
        mesh.indices.extend_from_slice(&[i, i + 1, i + 2, i, i + 2, i + 3]);
    }
    p.add(Shape::mesh(mesh));
    p.add(Shape::line(pts.to_vec(), Stroke::new(6.0_f32, col.gamma_multiply(0.14))));
    p.add(Shape::line(pts.to_vec(), Stroke::new(2.0_f32, col)));
}

/// The graph area inside a display, leaving room for the value axis (left) and the frequency /
/// time axis (bottom).
pub(super) fn plot(r: Rect) -> Rect {
    Rect::from_min_max(r.min + Vec2::new(32.0, 10.0), r.max - Vec2::new(10.0, 18.0))
}

/// Labels down the left edge: `(value, text)` placed with `y_of`.
pub(super) fn y_axis(p: &egui::Painter, pr: Rect, marks: &[(f32, &str)], y_of: impl Fn(f32) -> f32) {
    for (v, t) in marks {
        let y = y_of(*v);
        p.line_segment([Pos2::new(pr.left(), y), Pos2::new(pr.right(), y)], Stroke::new(1.0_f32, Color32::from_white_alpha(if *v == 0.0 { 28 } else { 10 })));
        p.text(Pos2::new(pr.left() - 5.0, y), egui::Align2::RIGHT_CENTER, *t, egui::FontId::proportional(9.0), Color32::from_white_alpha(110));
    }
}

/// Labels along the bottom edge: `(value, text)` placed with `x_of`.
pub(super) fn x_axis(p: &egui::Painter, pr: Rect, marks: &[(f32, String)], x_of: impl Fn(f32) -> f32) {
    for (v, t) in marks {
        let x = x_of(*v);
        p.line_segment([Pos2::new(x, pr.top()), Pos2::new(x, pr.bottom())], Stroke::new(1.0_f32, Color32::from_white_alpha(10)));
        p.text(Pos2::new(x, pr.bottom() + 3.0), egui::Align2::CENTER_TOP, t, egui::FontId::proportional(9.0), Color32::from_white_alpha(110));
    }
}

pub(super) fn grid(p: &egui::Painter, r: Rect, log_freq: bool) {
    super::fx_mod::screen(p, r, 6.0);
    let pr = plot(r);
    if log_freq {
        let marks: Vec<(f32, String)> = [20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0, 20000.0].iter().map(|f| (*f, if *f >= 1000.0 { format!("{}k", f / 1000.0) } else { format!("{f}") })).collect();
        x_axis(p, pr, &marks, |f| fx(pr, f));
    } else {
        let g = Stroke::new(1.0_f32, Color32::from_white_alpha(10));
        for k in 1..8 {
            let x = pr.left() + pr.width() * k as f32 / 8.0;
            p.line_segment([Pos2::new(x, pr.top()), Pos2::new(x, pr.bottom())], g);
        }
    }
}

/// Draws the effect's hero display. Some displays are also controls: drag an EQ band's node (wheel = Q),
/// the filter's cutoff point (up/down = resonance) or a dynamics threshold line. Returns the parameter
/// changes the gesture made, and whether a drag is running.
pub fn hero(ui: &mut Ui, h: &Hero) -> (Vec<(String, f32)>, bool) {
    let w = ui.available_width().max(260.0);
    let base = match h.def.id {
        "tune" => 230.0,
        "saturator" => 260.0,
        "clean_voice" | "class_a_strip" => 200.0,
        "alter" => 200.0,
        "orbit" => 220.0,
        "drum_bus" => 200.0,
        "dragon_particle" => 200.0,
        "analyzer" => 210.0,
        "tuner" | "amp" | "convolver" => 190.0,
        _ => 170.0,
    };
    let height = hero_height(h.def.id, base, w, ui.available_height(), ui.ctx().screen_rect().height());
    let (r, resp) = ui.allocate_exact_size(Vec2::new(w, height), egui::Sense::click_and_drag());
    let edits = interact(ui, r, &resp, h);
    if let Some((x, y)) = xy_params(h.def.id) {
        let name = |id: Option<&str>| id.and_then(|id| h.def.params.iter().find(|p| p.id == id)).map(|p| p.name).unwrap_or("-");
        let _ = resp.clone().on_hover_text(format!("Drag: left / right = {}, up / down = {} (Shift = fine, double-click resets)", name(x), name(y)));
    } else if h.def.id == "multiband" {
        let _ = resp.clone().on_hover_text("Drag a crossover sideways, a band up / down for its threshold (Shift = fine, double-click resets)");
    } else if !hit_regions(h.def.id, r).is_empty() {
        let _ = resp.clone().on_hover_text("Drag a control up / down (Shift = fine), wheel, double-click resets");
    }
    let p = ui.painter_at(r);
    // Every display is a screen in its unit's phosphor.
    let finish = super::analog::finish_of(h.def.id);
    let phosphor = super::fx_mod::phosphor(finish);
    super::fx_mod::set_screen(phosphor);
    let themed = !matches!(h.def.id, "orbit" | "clipper" | "chorus" | "flanger" | "phaser" | "tremolo");
    let col = if !h.enabled {
        theme::TEXT_DIM
    } else if themed {
        phosphor
    } else {
        h.color
    };
    match h.def.id {
        "eq" => eq(&p, r, h, col),
        "filter" => filter(&p, r, h, col),
        "compressor" | "limiter" | "gate" | "deesser" => dynamics(&p, r, h, col),
        "multiband" => multiband(&p, r, h, col),
        "transient" => transient(&p, r, h, col),
        "delay" => super::fx_cave::cavern(&p, r, h),
        "reverb" => super::fx_cave::lair(&p, r, h),
        "saturator" => super::fx_crucible::draw(&p, r, h),
        "bitcrusher" => shaper(&p, r, h, col),
        "clipper" => clipper(&p, r, h, col),
        "multisat" => multisat(&p, r, h, col),
        "chorus" | "flanger" | "phaser" | "tremolo" => super::fx_mod::draw(&p, r, h),
        "imager" => phase_meter(&p, r, h, col),
        "utility" => stereo(&p, r, h, col),
        "tune" => tune(&p, r, h, col),
        "dragon_particle" => particle(&p, r, h, col),
        "clean_voice" => clean_voice(&p, r, h, col),
        "class_a_strip" => class_a_strip(&p, r, h, col),
        "drum_bus" => drum_bus(&p, r, h, col),
        "alter" => alter(&p, r, h, col),
        "orbit" => orbit(&p, r, h, col),
        id if super::fx_extra::draws(id) => super::fx_extra::draw(&p, r, h, col),
        _ => {
            grid(&p, r, false);
            let pts: Vec<Pos2> = (0..=96).map(|k| Pos2::new(r.left() + r.width() * k as f32 / 96.0, r.center().y + (k as f32 * 0.25 + h.t() * 3.0).sin() * r.height() * 0.3)).collect();
            filled_curve(&p, &pts, r.bottom(), col);
        }
    }
    if !h.enabled {
        p.rect_filled(r, 6.0, Color32::from_black_alpha(110));
        p.text(r.center(), egui::Align2::CENTER_CENTER, "BYPASSED", egui::FontId::proportional(18.0), theme::TEXT_DIM);
    }
    // The gnostic units: a sacred-geometry sigil turning slowly behind everything, in gold light.
    if finish.gnostic() && h.def.id != "dragon_particle" {
        super::fx_mod::sigil(&p, r, h.t(), phosphor);
    }
    if themed {
        super::fx_mod::crt_glass(&p, r);
    }
    p.rect_stroke(r, 6.0, Stroke::new(1.0_f32, col.gamma_multiply(0.4)), egui::StrokeKind::Inside);
    (edits, resp.dragged())
}

/// The insert's measured output (RMS left, RMS right, peak), when the display shows a live insert.
fn out_level(ctx: &egui::Context, h: &Hero) -> Option<(f32, f32, f32)> {
    ctx.data(|d| d.get_temp::<(f32, f32, f32)>(h.key.with("out_level")))
}

/// Square area of the transfer-curve displays (dynamics, shaper), with room for axis labels.
fn square(r: Rect) -> Rect {
    Rect::from_min_size(r.min + Vec2::new(34.0, 10.0), Vec2::splat(r.height() - 32.0))
}

fn interact(ui: &Ui, r: Rect, resp: &egui::Response, h: &Hero) -> Vec<(String, f32)> {
    let mut out = Vec::new();
    if h.def.id == "eq" {
        // The band panel stays while the mouse is on it (off the graph).
        eq_band_panel(ui, r, h, 18.0, &mut out);
    }
    let Some(pos) = resp.interact_pointer_pos().or_else(|| resp.hover_pos()) else { return out };
    let grab = resp.id.with("grab");
    match h.def.id {
        "eq" => {
            let r = plot(r);
            let range = 18.0;
            let y_of = |db: f32| r.center().y - db.clamp(-range, range) / range * r.height() * 0.5;
            let node = |b: usize| Pos2::new(fx(r, h.v(&format!("b{b}_freq"))), y_of(h.v(&format!("b{b}_gain"))));
            let near = |at: Pos2| {
                (1..=dragon_engine::effects::eq::EQ_BANDS).filter(|b| h.v(&format!("b{b}_on")) > 0.5).min_by(|a, b| node(*a).distance(at).total_cmp(&node(*b).distance(at))).filter(|b| node(*b).distance(at) < 16.0)
            };
            let nearest = near(pos);
            if resp.drag_started() {
                let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
                ui.data_mut(|d| d.insert_temp(grab, near(origin).unwrap_or(0)));
                if let Some(b) = near(origin) {
                    ui.data_mut(|d| d.insert_temp(h.key.with("eq.sel"), b));
                } else if let Some(f) = spectrum_peak(h.tele, r, origin.x) {
                    // Spectrum grab: dragging a peak of the analyzer makes a narrow band there.
                    if let Some(b) = (1..=dragon_engine::effects::eq::EQ_BANDS).find(|b| h.v(&format!("b{b}_on")) < 0.5) {
                        out.extend([
                            (format!("b{b}_on"), 1.0),
                            (format!("b{b}_type"), 0.0),
                            (format!("b{b}_freq"), f),
                            (format!("b{b}_gain"), 0.0),
                            (format!("b{b}_q"), 4.0),
                            (format!("b{b}_dyn"), 0.0),
                            (format!("b{b}_stereo"), 0.0),
                        ]);
                        ui.data_mut(|d| d.insert_temp(grab, b));
                        ui.data_mut(|d| d.insert_temp(h.key.with("eq.sel"), b));
                    }
                }
            }
            let held: usize = ui.data(|d| d.get_temp(grab)).unwrap_or(0);
            if resp.dragged() && held > 0 {
                let (shift, ctrl) = ui.input(|i| (i.modifiers.shift, i.modifiers.command));
                let ty = h.v(&format!("b{held}_type")).round() as usize;
                let d = resp.drag_delta();
                // Shift: fine moves (a tenth of the mouse); Ctrl: gain / Q only (frequency locked)
                // modifiers. Pass filters, band pass and notch have no gain: up/down is their Q.
                let fine = if shift { 0.1 } else { 1.0 };
                if !ctrl {
                    let f0 = h.v(&format!("b{held}_freq"));
                    let f = if shift { f0 * 2f32.powf(d.x * fine / (r.width() / 10.0)) } else { freq_at(r, pos.x) };
                    out.push((format!("b{held}_freq"), f.clamp(20.0, 20_000.0)));
                }
                if matches!(ty, 3..=6) {
                    let q0 = h.v(&format!("b{held}_q"));
                    out.push((format!("b{held}_q"), (q0 * 2f32.powf(-d.y * fine / 60.0)).clamp(0.1, 18.0)));
                } else if shift || ctrl {
                    let g0 = h.v(&format!("b{held}_gain"));
                    out.push((format!("b{held}_gain"), (g0 - d.y * fine / (r.height() * 0.5) * range).clamp(-range, range)));
                } else {
                    let db = ((r.center().y - pos.y) / (r.height() * 0.5) * range).clamp(-range, range);
                    out.push((format!("b{held}_gain"), (db * 10.0).round() / 10.0));
                }
            }
            if let Some(b) = nearest {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                let s = ui.input(|i| i.smooth_scroll_delta.y);
                if s != 0.0 {
                    out.push((format!("b{b}_q"), (h.v(&format!("b{b}_q")) * (1.0 + s / 300.0)).clamp(0.1, 18.0)));
                }
                // Alt+click removes the band (style).
                if resp.clicked() && ui.input(|i| i.modifiers.alt) {
                    out.push((format!("b{b}_on"), 0.0));
                }
                if !resp.dragged() {
                    let ty = EQ_TYPES.get(h.v(&format!("b{b}_type")).round() as usize).copied().unwrap_or("Bell");
                    egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), resp.id.with("eq.tip"), |ui| {
                        ui.label(format!("Band {b} · {ty}\n{} · {:+.1} dB · Q {:.2}", hz_text(h.v(&format!("b{b}_freq"))), h.v(&format!("b{b}_gain")), h.v(&format!("b{b}_q"))));
                        if dragon_engine::effects::eq::has_slope(BiquadKind::from_index(h.v(&format!("b{b}_type")))) {
                            ui.label(format!("Slope {}", dragon_engine::effects::eq::SLOPES.get(h.v(&format!("b{b}_slope")).round() as usize).copied().unwrap_or("")));
                        }
                        ui.label(egui::RichText::new("drag · Shift = fine · Ctrl = gain only · wheel = Q · right-click = shape / slope · Alt+click = remove").size(10.0).color(theme::TEXT_DIM));
                    });
                }
            } else if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                if r.contains(pos) {
                    let f = freq_at(r, pos.x);
                    let midi = 69.0 + 12.0 * (f / 440.0).log2();
                    let note = midi.round().clamp(0.0, 127.0) as u8;
                    let cents = ((midi - midi.round()) * 100.0).round();
                    let db = (r.center().y - pos.y) / (r.height() * 0.5) * range;
                    egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), resp.id.with("eq.where"), |ui| {
                        ui.label(format!("{} · {} {cents:+.0} ct · {db:+.1} dB", hz_text(f), dragon_engine::instruments::note_name(note)));
                    });
                }
            }
            // Double-click on empty space adds a band there: a cut near the edges, a bell elsewhere.
            if resp.double_clicked() && nearest.is_none() && r.contains(pos) {
                if let Some(b) = (1..=dragon_engine::effects::eq::EQ_BANDS).find(|b| h.v(&format!("b{b}_on")) < 0.5) {
                    let f = freq_at(r, pos.x).clamp(20.0, 20_000.0);
                    let edge = (pos.x - r.left()) / r.width();
                    let ty = if edge < 0.08 {
                        4.0
                    } else if edge > 0.92 {
                        3.0
                    } else {
                        0.0
                    };
                    let db = if ty == 0.0 { ((r.center().y - pos.y) / (r.height() * 0.5) * range).clamp(-range, range) } else { 0.0 };
                    ui.data_mut(|d| d.insert_temp(h.key.with("eq.sel"), b));
                    out.extend([(format!("b{b}_on"), 1.0), (format!("b{b}_type"), ty), (format!("b{b}_freq"), f), (format!("b{b}_gain"), (db * 10.0).round() / 10.0), (format!("b{b}_q"), 0.9)]);
                }
            }
            // Right-click on a band: shape, Q presets, gain reset and removal.
            if resp.clicked() {
                // A click on a band selects it (its panel opens); on empty space it closes the panel.
                ui.data_mut(|d| d.insert_temp(h.key.with("eq.sel"), nearest.unwrap_or(0)));
            }
            let menu_id = resp.id.with("eq.menu");
            if resp.secondary_clicked() {
                ui.data_mut(|d| d.insert_temp(menu_id, nearest.unwrap_or(0)));
            }
            let menu_band: usize = ui.data(|d| d.get_temp(menu_id)).unwrap_or(0);
            if menu_band > 0 {
                let b = menu_band;
                resp.context_menu(|ui| {
                    ui.label(egui::RichText::new(format!("Band {b}")).strong());
                    let cur = h.v(&format!("b{b}_type")).round() as usize;
                    for (k, name) in EQ_TYPES.iter().enumerate() {
                        if ui.selectable_label(k == cur, *name).clicked() {
                            out.push((format!("b{b}_type"), k as f32));
                            ui.close_menu();
                        }
                    }
                    if dragon_engine::effects::eq::has_slope(BiquadKind::from_index(cur as f32)) {
                        ui.menu_button("Slope", |ui| {
                            let cs = h.v(&format!("b{b}_slope")).round() as usize;
                            for (k, name) in dragon_engine::effects::eq::SLOPES.iter().enumerate() {
                                if ui.selectable_label(k == cs, *name).clicked() {
                                    out.push((format!("b{b}_slope"), k as f32));
                                    ui.close_menu();
                                }
                            }
                        });
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Q");
                        for q in [0.5f32, 0.9, 2.0, 5.0, 10.0] {
                            if ui.small_button(format!("{q}")).clicked() {
                                out.push((format!("b{b}_q"), q));
                                ui.close_menu();
                            }
                        }
                    });
                    ui.menu_button("Stereo placement", |ui| {
                        let cur = h.v(&format!("b{b}_stereo")).round() as usize;
                        for (k, name) in dragon_engine::effects::eq::PLACEMENTS.iter().enumerate() {
                            if ui.selectable_label(k == cur, *name).clicked() {
                                out.push((format!("b{b}_stereo"), k as f32));
                                ui.close_menu();
                            }
                        }
                    });
                    ui.menu_button("Dynamic", |ui| {
                        let cur = h.v(&format!("b{b}_dyn"));
                        for (label, v) in [("Off (static)", 0.0f32), ("Cut up to −3 dB", -3.0), ("Cut up to −6 dB", -6.0), ("Cut up to −12 dB", -12.0), ("Boost up to +6 dB", 6.0)] {
                            if ui.selectable_label((cur - v).abs() < 0.05, label).clicked() {
                                out.push((format!("b{b}_dyn"), v));
                                ui.close_menu();
                            }
                        }
                        ui.label(egui::RichText::new(format!("threshold {:.0} dB (knob below)", h.v(&format!("b{b}_thr")))).small());
                    });
                    let soloed = h.v("solo").round() as usize == b;
                    if ui.selectable_label(soloed, "🎧 Solo (listen to this band)").clicked() {
                        out.push(("solo".into(), if soloed { 0.0 } else { b as f32 }));
                        ui.close_menu();
                    }
                    if ui.button("Reset gain").clicked() {
                        out.push((format!("b{b}_gain"), 0.0));
                        ui.close_menu();
                    }
                    if ui.button("Remove band").clicked() {
                        out.push((format!("b{b}_on"), 0.0));
                        ui.close_menu();
                    }
                });
            }
        }
        "saturator" => out.extend(super::fx_crucible::interact(ui, r, resp, h)),
        "delay" => out.extend(super::fx_cave::cavern_interact(ui, r, resp, h)),
        "reverb" => {
            let dr = decay_rect(r);
            let y_of = |m: f32| dr.center().y - (m.max(0.01).log2() / DR_RANGE).clamp(-1.0, 1.0) * (dr.height() * 0.4);
            let node = |k: usize| Pos2::new(fx(dr, h.v(&format!("dr_f{k}"))), y_of(h.v(&format!("dr_{k}"))));
            if resp.drag_started() {
                let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
                let hit = (1..=dragon_engine::effects::reverb::DR_BANDS).min_by(|a, b| node(*a).distance(origin).total_cmp(&node(*b).distance(origin))).filter(|k| node(*k).distance(origin) < 12.0);
                ui.data_mut(|d| d.insert_temp(grab, hit.unwrap_or(0)));
            }
            let held: usize = ui.data(|d| d.get_temp(grab)).unwrap_or(0);
            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
            if !dr.contains(origin) {
                super::fx_cave::lair_interact(ui, r, resp, h, &mut out);
            }
            if resp.dragged() && held > 0 {
                let u = ((dr.center().y - pos.y) / (dr.height() * 0.4)).clamp(-1.0, 1.0);
                out.push((format!("dr_{held}"), 2f32.powf(u * DR_RANGE)));
                // Free nodes: sideways moves the point's frequency.
                out.push((format!("dr_f{held}"), freq_at(dr, pos.x).clamp(20.0, 20_000.0)));
            }
            if resp.double_clicked() {
                if let Some(k) = (1..=dragon_engine::effects::reverb::DR_BANDS).find(|k| node(*k).distance(pos) < 12.0) {
                    out.push((format!("dr_{k}"), 1.0));
                    out.push((format!("dr_f{k}"), dragon_engine::effects::reverb::DR_FREQS[k - 1]));
                }
            }
            let hover = (1..=dragon_engine::effects::reverb::DR_BANDS).find(|k| node(*k).distance(pos) < 12.0);
            if hover.is_some() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
            }
            // Wheel over a band: its Q (width).
            if let Some(k) = hover {
                let s = ui.input(|i| i.raw_scroll_delta.y);
                if s != 0.0 {
                    let q = h.v(&format!("dr_q{k}"));
                    out.push((format!("dr_q{k}"), (q * if s > 0.0 { 1.15 } else { 1.0 / 1.15 }).clamp(0.3, 6.0)));
                }
            }
            // Right-click a band: its shape.
            let menu_id = resp.id.with("dr.menu");
            if resp.secondary_clicked() {
                ui.data_mut(|d| d.insert_temp(menu_id, hover.unwrap_or(0)));
            }
            let menu_band: usize = ui.data(|d| d.get_temp(menu_id)).unwrap_or(0);
            if menu_band > 0 {
                let k = menu_band;
                resp.context_menu(|ui| {
                    ui.label(egui::RichText::new(format!("Decay band {k}")).strong());
                    let cur = h.v(&format!("dr_t{k}")).round() as usize;
                    for (t, name) in dragon_engine::effects::reverb::DR_SHAPES.iter().enumerate() {
                        if ui.selectable_label(t == cur, *name).clicked() {
                            out.push((format!("dr_t{k}"), t as f32));
                            ui.close_menu();
                        }
                    }
                    if ui.button("Reset band").clicked() {
                        out.push((format!("dr_{k}"), 1.0));
                        out.push((format!("dr_q{k}"), 1.0));
                        ui.close_menu();
                    }
                });
            }
        }
        "multisat" => {
            // Drag a crossover line: it follows the pointer in frequency.
            let r = plot(r);
            let n = h.v("bands").round() as usize + 1;
            let xs = ["x1", "x2", "x3"];
            if resp.drag_started() {
                let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
                let hit = (0..n.saturating_sub(1)).min_by(|a, b| (fx(r, h.v(xs[*a])) - origin.x).abs().total_cmp(&(fx(r, h.v(xs[*b])) - origin.x).abs())).filter(|k| (fx(r, h.v(xs[*k])) - origin.x).abs() < 10.0);
                ui.data_mut(|d| d.insert_temp(grab, hit.map(|k| k + 1).unwrap_or(0)));
            }
            let held: usize = ui.data(|d| d.get_temp(grab)).unwrap_or(0);
            if resp.dragged() && held > 0 {
                out.push((xs[held - 1].to_string(), freq_at(r, pos.x).clamp(30.0, 16_000.0)));
            } else if resp.dragged() {
                // A band itself: up / down is its drive.
                let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
                if r.contains(origin) {
                    let band = 1 + (0..n.saturating_sub(1)).filter(|k| fx(r, h.v(xs[*k])) < origin.x).count();
                    let id = format!("b{band}_drive");
                    if let Some(prm) = h.def.params.iter().find(|p| p.id == id) {
                        let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
                        let nv = (prm.normalize(h.v(&id)) - resp.drag_delta().y / r.height() * fine).clamp(0.0, 1.0);
                        out.push((id, prm.denormalize(nv)));
                    }
                }
            }
            if (0..n.saturating_sub(1)).any(|k| (fx(r, h.v(xs[k])) - pos.x).abs() < 10.0) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
        }
        "dragon_particle" => {
            let c = r.center();
            let rad = (r.height() * 0.5 - 22.0).max(30.0);
            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
            if resp.dragged() && origin.distance(c) < rad + 10.0 {
                // Turn the stone: sideways (a full turn across its circumference), or up / down.
                let dd = resp.drag_delta();
                let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
                let d = (dd.x / (TAU * rad * 0.86) - dd.y / 220.0) * fine;
                out.push(("amount".into(), (h.v("amount") + d).clamp(0.0, 1.0)));
            }
            if pos.distance(c) < rad + 10.0 {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                let s = ui.input(|i| i.smooth_scroll_delta.y);
                if s != 0.0 {
                    out.push(("amount".into(), (h.v("amount") + s / 800.0).clamp(0.0, 1.0)));
                }
            }
        }
        "orbit" => {
            let (field, shapes, modes) = orbit_layout(r);
            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
            if resp.clicked() {
                if let Some(k) = shapes.iter().position(|b| b.contains(pos)) {
                    out.push(("shape".into(), k as f32));
                } else if let Some(k) = modes.iter().position(|b| b.contains(pos)) {
                    out.push(("mode".into(), k as f32));
                }
            }
            if field.contains(pos) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
                let s = ui.input(|i| i.smooth_scroll_delta.y);
                if s != 0.0 {
                    out.push(("width".into(), (h.v("width") + s / 6.0).clamp(0.0, 100.0)));
                }
                if resp.double_clicked() {
                    out.push(("center".into(), 0.0));
                    out.push(("width".into(), 70.0));
                }
            }
            if resp.dragged() && field.contains(origin) {
                // Left / right: centre; up / down: width.
                let d = resp.drag_delta();
                if d.x != 0.0 {
                    out.push(("center".into(), (h.v("center") + d.x / field.width() * 200.0).clamp(-100.0, 100.0)));
                }
                if d.y != 0.0 {
                    out.push(("width".into(), (h.v("width") - d.y / field.height() * 100.0).clamp(0.0, 100.0)));
                }
            }
        }
        "class_a_strip" => {
            // The one-fader noise suppressor: drag (or wheel) the cap, double-click for 0 %.
            let (track, _) = ns_fader(r);
            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
            let zone = track.expand2(Vec2::new(14.0, 10.0));
            if resp.dragged() && zone.contains(origin) {
                let v = (track.bottom() - pos.y) / track.height() * 100.0;
                out.push(("ns".into(), v.clamp(0.0, 100.0).round()));
            }
            if zone.contains(pos) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                let s = ui.input(|i| i.smooth_scroll_delta.y);
                if s != 0.0 {
                    out.push(("ns".into(), (h.v("ns") + s / 8.0).clamp(0.0, 100.0).round()));
                }
                if resp.double_clicked() {
                    out.push(("ns".into(), 0.0));
                }
            } else if !zone.contains(origin) || !resp.dragged() {
                region_drag(ui, resp, h, pos, &hit_regions("class_a_strip", r), &mut out);
            }
        }
        "filter" => {
            let r = plot(r);
            if resp.dragged() {
                out.push(("cutoff".into(), freq_at(r, pos.x).clamp(20.0, 20_000.0)));
                out.push(("resonance".into(), (1.0 - (pos.y - r.top()) / r.height()).clamp(0.0, 0.98)));
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            }
        }
        "multiband" => multiband_interact(ui, r, resp, h, pos, &mut out),
        "compressor" | "gate" | "deesser" => {
            let sq = square(r);
            if resp.dragged() && sq.expand(10.0).contains(resp.interact_pointer_pos().unwrap_or(pos)) {
                let lo = if h.def.id == "gate" { -80.0 } else { -60.0 };
                let db = (-60.0 + (pos.x - sq.left()) / sq.width() * 60.0).clamp(lo, 0.0);
                out.push(("threshold".into(), (db * 2.0).round() / 2.0));
            }
            if sq.contains(pos) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
        }
        id => {
            // Drawn controls (Drum Bus tiles and bars, the strip's sections, Shapeshifter's sliders): drag one
            // up / down, wheel, double-click resets.
            let hits = hit_regions(id, r);
            if !hits.is_empty() {
                region_drag(ui, resp, h, pos, &hits, &mut out);
            } else if let Some((x, y)) = xy_params(id) {
                // The display as an XY pad over the effect's two main controls.
                let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
                if r.contains(pos) {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
                }
                if resp.dragged() && r.contains(origin) {
                    let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
                    let d = resp.drag_delta() * fine;
                    for (id, delta) in [(x, d.x / r.width()), (y, -d.y / r.height())] {
                        if let (Some(id), true) = (id, delta != 0.0) {
                            if let Some(prm) = h.def.params.iter().find(|p| p.id == id) {
                                let n = (prm.normalize(h.v(id)) + delta).clamp(0.0, 1.0);
                                out.push((id.to_string(), prm.denormalize(n)));
                            }
                        }
                    }
                }
                if resp.double_clicked() {
                    for id in [x, y].into_iter().flatten() {
                        if let Some(prm) = h.def.params.iter().find(|p| p.id == id) {
                            out.push((id.to_string(), prm.default));
                        }
                    }
                }
            }
        }
    }
    out
}

/// The two parameters a display moves when dragged (x, y): its effect's main controls.
pub fn xy_params(id: &str) -> Option<(Option<&'static str>, Option<&'static str>)> {
    Some(match id {
        "limiter" => (Some("ceiling"), Some("gain")),
        "chorus" | "flanger" | "tremolo" => (Some("rate"), Some("depth")),
        "phaser" => (Some("center"), Some("depth")),
        "bitcrusher" => (Some("downsample"), Some("bits")),
        "utility" => (Some("pan"), Some("gain")),
        "imager" => (Some("width"), Some("mono_below")),
        "tune" => (Some("amount"), Some("speed")),
        "clean_voice" => (Some("presence"), Some("comp")),
        "convolver" => (Some("size"), Some("mix")),
        "volume_shaper" => (Some("offset"), Some("depth")),
        "amp" => (Some("drive"), Some("master")),
        // The meters: dragging zooms the dB range / moves the reference pitch.
        "analyzer" => (None, Some("range")),
        "tuner" => (Some("reference"), None),
        _ => return None,
    })
}

/// Controls drawn on a display, as (area, parameter): dragged up / down like a fader.
pub fn hit_regions(id: &str, r: Rect) -> Vec<(Rect, &'static str)> {
    match id {
        "clipper" => {
            let g = clip_geom(r);
            vec![(g.input, "input"), (g.output, "post"), (g.screen, "threshold")]
        }
        "drum_bus" => {
            let mut v = vec![
                (Rect::from_min_max(Pos2::new(r.left() + 8.0, r.top() + 14.0), Pos2::new(r.left() + 36.0, r.bottom() - 14.0)), "input"),
                (Rect::from_min_max(Pos2::new(r.right() - 36.0, r.top() + 14.0), Pos2::new(r.right() - 8.0, r.bottom() - 14.0)), "output"),
            ];
            let body = Rect::from_min_max(Pos2::new(r.left() + 46.0, r.top() + 8.0), Pos2::new(r.right() - 46.0, r.bottom() - 8.0));
            let top = Rect::from_min_max(body.min, Pos2::new(body.right(), body.top() + body.height() * 0.55));
            let tw = top.width() / 4.0;
            for (k, id) in ["drive", "harmonics", "comp_mix", "lift"].into_iter().enumerate() {
                v.push((Rect::from_min_size(Pos2::new(top.left() + k as f32 * tw + 3.0, top.top()), Vec2::new(tw - 6.0, top.height())), id));
            }
            let bottom = Rect::from_min_max(Pos2::new(body.left(), top.bottom() + 8.0), body.max);
            let half = bottom.width() / 2.0;
            for (x0, ids) in [(bottom.left(), &["heq1", "heq2", "heq3", "heq4", "heq5"][..]), (bottom.left() + half, &["sub", "low", "loud", "grit"][..])] {
                let area = Rect::from_min_max(Pos2::new(x0 + 3.0, bottom.top()), Pos2::new(x0 + half - 3.0, bottom.bottom()));
                let bw = (area.width() - 12.0) / ids.len() as f32;
                for (k, id) in ids.iter().enumerate() {
                    v.push((Rect::from_min_max(Pos2::new(area.left() + 6.0 + k as f32 * bw, area.top() + 12.0), Pos2::new(area.left() + 6.0 + (k + 1) as f32 * bw, area.bottom())), id));
                }
            }
            v
        }
        "class_a_strip" => {
            // After the NS column: preamp (drive), EQ (mid gain), opto comp (threshold: drag down = more).
            let (_, nsr) = ns_fader(r);
            let r = Rect::from_min_max(Pos2::new(nsr.right(), r.top()), r.max);
            let w3 = (r.width() - 40.0) / 3.0;
            let sec = |k: usize| Rect::from_min_size(Pos2::new(r.left() + 10.0 + k as f32 * (w3 + 10.0), r.top() + 10.0), Vec2::new(w3, r.height() - 20.0));
            vec![(sec(0), "drive"), (sec(1), "mid_gain"), (sec(2), "threshold")]
        }
        "alter" => {
            let s = |x: f32| Rect::from_min_max(Pos2::new(x - 18.0, r.top() + 20.0), Pos2::new(x + 18.0, r.bottom() - 18.0));
            vec![(s(r.left() + 34.0), "pitch"), (s(r.right() - 34.0), "formant")]
        }
        "transient" => {
            let half = r.width() / 2.0;
            vec![(Rect::from_min_size(r.min, Vec2::new(half, r.height())), "attack"), (Rect::from_min_size(r.min + Vec2::new(half, 0.0), Vec2::new(half, r.height())), "sustain")]
        }
        _ => Vec::new(),
    }
}

fn region_drag(ui: &Ui, resp: &egui::Response, h: &Hero, pos: Pos2, hits: &[(Rect, &'static str)], out: &mut Vec<(String, f32)>) {
    let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
    let param = |id: &str| h.def.params.iter().find(|p| p.id == id);
    if let Some((rect, id)) = hits.iter().find(|(r, _)| r.contains(pos)) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        let _ = rect;
        if let Some(prm) = param(id) {
            let s = ui.input(|i| i.smooth_scroll_delta.y);
            if s != 0.0 {
                out.push((id.to_string(), prm.denormalize((prm.normalize(h.v(id)) + s / 600.0).clamp(0.0, 1.0))));
            }
            if resp.double_clicked() {
                out.push((id.to_string(), prm.default));
            }
        }
    }
    if resp.dragged() {
        if let Some((rect, id)) = hits.iter().find(|(r, _)| r.contains(origin)) {
            if let Some(prm) = param(id) {
                let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
                // A full sweep over the control's height (at least 120 px).
                let n = prm.normalize(h.v(id)) - resp.drag_delta().y / rect.height().max(120.0) * fine;
                out.push((id.to_string(), prm.denormalize(n.clamp(0.0, 1.0))));
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------- EQ

/// The band panel: a small floating bar under the selected band with its shape, slope, frequency,
/// gain and Q as draggable values, plus solo and remove. Hidden while a band is dragged.
fn eq_band_panel(ui: &Ui, r: Rect, h: &Hero, range: f32, out: &mut Vec<(String, f32)>) {
    use dragon_engine::effects::eq::{has_slope, SLOPES};
    let pr = plot(r);
    let b: usize = ui.data(|d| d.get_temp(h.key.with("eq.sel"))).unwrap_or(0);
    if b == 0 || h.v(&format!("b{b}_on")) < 0.5 || ui.input(|i| i.pointer.primary_down()) && ui.ctx().dragged_id().is_some() {
        return;
    }
    let k = |n: &str| format!("b{b}_{n}");
    let y_of = |db: f32| pr.center().y - db.clamp(-range, range) / range * pr.height() * 0.5;
    let node = Pos2::new(fx(pr, h.v(&k("freq"))), y_of(h.v(&k("gain"))));
    let size = Vec2::new(372.0, 30.0);
    let below = node.y + 18.0 + size.y < r.bottom();
    let at = Pos2::new((node.x - size.x / 2.0).clamp(r.left() + 4.0, r.right() - size.x - 4.0), if below { node.y + 18.0 } else { node.y - 18.0 - size.y });
    let col = BAND_COLORS[(b - 1) % 7];
    egui::Area::new(h.key.with(("eq.panel", b))).order(egui::Order::Foreground).fixed_pos(at).show(ui.ctx(), |ui| {
        egui::Frame::NONE.fill(Color32::from_rgba_unmultiplied(18, 18, 24, 235)).stroke(Stroke::new(1.0_f32, col.gamma_multiply(0.8))).corner_radius(8.0).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(egui::RichText::new(format!("{b}")).strong().color(col));
                let ty = h.v(&k("type")).round() as usize;
                egui::ComboBox::from_id_salt(h.key.with(("eq.panel.type", b))).width(84.0).selected_text(EQ_TYPES.get(ty).copied().unwrap_or("Bell")).show_ui(ui, |ui| {
                    for (n, name) in EQ_TYPES.iter().enumerate() {
                        if ui.selectable_label(n == ty, *name).clicked() {
                            out.push((k("type"), n as f32));
                        }
                    }
                });
                if has_slope(BiquadKind::from_index(ty as f32)) {
                    let sl = h.v(&k("slope")).round() as usize;
                    egui::ComboBox::from_id_salt(h.key.with(("eq.panel.slope", b))).width(66.0).selected_text(SLOPES.get(sl).copied().unwrap_or("12 dB/oct").replace(" dB/oct", " dB")).show_ui(ui, |ui| {
                        for (n, name) in SLOPES.iter().enumerate() {
                            if ui.selectable_label(n == sl, *name).clicked() {
                                out.push((k("slope"), n as f32));
                            }
                        }
                    });
                }
                let mut f = h.v(&k("freq"));
                let fspeed = f * 0.005;
                if ui.add(egui::DragValue::new(&mut f).range(20.0..=20_000.0).speed(fspeed).custom_formatter(|v, _| hz_text(v as f32)).custom_parser(parse_hz)).changed() {
                    out.push((k("freq"), f));
                }
                if !matches!(ty, 3..=6) {
                    let mut g = h.v(&k("gain"));
                    if ui.add(egui::DragValue::new(&mut g).range(-range..=range).speed(0.1).suffix(" dB").fixed_decimals(1)).changed() {
                        out.push((k("gain"), g));
                    }
                }
                let mut q = h.v(&k("q"));
                let qspeed = q * 0.01;
                if ui.add(egui::DragValue::new(&mut q).range(0.1..=18.0).speed(qspeed).prefix("Q ").fixed_decimals(2)).changed() {
                    out.push((k("q"), q));
                }
                let soloed = h.v("solo").round() as usize == b;
                if ui.selectable_label(soloed, "🎧").on_hover_text("Solo: listen to this band").clicked() {
                    out.push(("solo".into(), if soloed { 0.0 } else { b as f32 }));
                }
                if ui.small_button("🗑").on_hover_text("Remove the band").clicked() {
                    out.push((k("on"), 0.0));
                }
            });
        });
    });
}

/// "1.2k", "800 Hz", "2 kHz" → Hz.
fn parse_hz(t: &str) -> Option<f64> {
    let t = t.trim().to_lowercase().replace("hz", "");
    let t = t.trim();
    match t.strip_suffix('k') {
        Some(n) => n.trim().parse::<f64>().ok().map(|v| v * 1000.0),
        None => t.parse::<f64>().ok(),
    }
}

const EQ_TYPES: [&str; 7] = ["Bell", "Low shelf", "High shelf", "Low pass", "High pass", "Band pass", "Notch"];

fn hz_text(f: f32) -> String {
    if f >= 1000.0 {
        format!("{:.2} kHz", f / 1000.0)
    } else {
        format!("{f:.0} Hz")
    }
}

/// A peak of the output spectrum near `x` (within 12 px): a local maximum standing 3 dB over its
/// surroundings. Returns its frequency.
fn spectrum_peak(tele: &[f32], pr: Rect, x: f32) -> Option<f32> {
    use dragon_engine::dsp::spectrum::{band_freq, BANDS, FLOOR};
    if tele.len() < BANDS * 2 {
        return None;
    }
    let post = &tele[BANDS..BANDS * 2];
    (3..BANDS - 3)
        .filter(|b| (fx(pr, band_freq(*b)) - x).abs() < 12.0 && post[*b] > FLOOR + 20.0)
        .filter(|b| post[*b] >= post[b - 1] && post[*b] >= post[b + 1] && post[*b] > post[b - 3].max(post[b + 3]) + 3.0)
        .max_by(|a, b| post[*a].total_cmp(&post[*b]))
        .map(band_freq)
}

/// The live analyzer behind the EQ curve: the input spectrum as a faint fill, the output as a line.
/// `tele` holds the input bands then the output bands (see `dsp::spectrum`).
fn spectrum(p: &egui::Painter, pr: Rect, tele: &[f32]) {
    use dragon_engine::dsp::spectrum::{band_freq, BANDS};
    if tele.len() < BANDS * 2 {
        return;
    }
    let y = |db: f32| pr.bottom() - ((db + 84.0) / 90.0).clamp(0.0, 1.0) * pr.height();
    let pts = |off: usize| -> Vec<Pos2> { (0..BANDS).map(|b| Pos2::new(fx(pr, band_freq(b)), y(tele[off + b]))).collect() };
    let pre = pts(0);
    for w in pre.windows(2) {
        let q = [w[0], w[1], Pos2::new(w[1].x, pr.bottom()), Pos2::new(w[0].x, pr.bottom())];
        p.add(Shape::convex_polygon(q.to_vec(), Color32::from_rgba_unmultiplied(120, 140, 170, 22), Stroke::NONE));
    }
    p.add(Shape::line(pts(BANDS), Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(200, 215, 235, 90))));
}

const BAND_COLORS: [Color32; 7] = [
    Color32::from_rgb(0xf0, 0x52, 0x6a),
    Color32::from_rgb(0xf0, 0x8a, 0x4b),
    Color32::from_rgb(0xf5, 0xc8, 0x42),
    Color32::from_rgb(0x5d, 0xe0, 0xa8),
    Color32::from_rgb(0x4b, 0xc0, 0xf0),
    Color32::from_rgb(0x9b, 0x7b, 0xf5),
    Color32::from_rgb(0xe0, 0x6e, 0xd8),
];

fn eq(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    grid(p, r, true);
    let range = 18.0;
    let pr = plot(r);
    let y_of = |db: f32| pr.center().y - db.clamp(-range, range) / range * pr.height() * 0.5;
    y_axis(p, pr, &[(12.0, "+12"), (6.0, "+6"), (0.0, "0"), (-6.0, "-6"), (-12.0, "-12")], y_of);
    let bands: Vec<(usize, dragon_engine::effects::eq::BandFilter, f32, f32)> = (1..=dragon_engine::effects::eq::EQ_BANDS)
        .filter(|b| h.v(&format!("b{b}_on")) > 0.5)
        .map(|b| {
            let f = h.v(&format!("b{b}_freq"));
            let g = h.v(&format!("b{b}_gain"));
            let q = h.v(&format!("b{b}_q"));
            let slope = h.v(&format!("b{b}_slope")).round().max(0.0) as usize;
            (b - 1, dragon_engine::effects::eq::BandFilter::new(BiquadKind::from_index(h.v(&format!("b{b}_type"))), f, q, g, slope, RATE), f, g)
        })
        .collect();
    let selected: usize = p.ctx().data(|d| d.get_temp(h.key.with("eq.sel"))).unwrap_or(0);
    let auto = h.v("auto_gain") > 0.5;
    let out = h.v("output") + if auto { dragon_engine::effects::eq::ParametricEq::auto_gain_for(h.params, RATE) } else { 0.0 };
    let solo = h.v("solo").round() as usize;
    let p = p.with_clip_rect(pr.expand(8.0).intersect(r));
    if h.tele.is_empty() && h.time.is_some() {
        // Gallery: a moving, music-like demo spectrum, shaped by the curve for the output.
        use dragon_engine::dsp::spectrum::{band_freq, BANDS};
        let t = h.t();
        let pre: Vec<f32> = (0..BANDS).map(|b| -48.0 - 10.0 * ((b as f32 / BANDS as f32) - 0.3).abs() * 3.0 + 6.0 * (b as f32 * 0.7 + t * 3.0).sin() * (b as f32 * 0.13 + t).cos()).collect();
        let post: Vec<f32> = (0..BANDS).map(|b| pre[b] + bands.iter().map(|x| x.1.response_db(band_freq(b), RATE)).sum::<f32>()).collect();
        spectrum(&p, pr, &[pre, post].concat());
    } else {
        spectrum(&p, pr, h.tele);
    }
    // Collisions with another track's EQ: where both are loud, the overlap glows red.
    let other: Option<Vec<f32>> = p.ctx().data(|d| d.get_temp(h.key.with("collide"))).flatten();
    if let (Some(o), true) = (other, h.tele.len() >= dragon_engine::dsp::spectrum::BANDS * 2) {
        use dragon_engine::dsp::spectrum::{band_freq, BANDS};
        let own = &h.tele[BANDS..BANDS * 2];
        let y = |db: f32| pr.bottom() - ((db + 84.0) / 90.0).clamp(0.0, 1.0) * pr.height();
        let peak = own.iter().chain(o.iter()).fold(-120.0f32, |m, x| m.max(*x));
        for b in 0..BANDS - 1 {
            let both = own[b].min(o[b]);
            if both > peak - 24.0 {
                let strength = ((both - (peak - 24.0)) / 24.0).clamp(0.0, 1.0);
                let r = Rect::from_min_max(Pos2::new(fx(pr, band_freq(b)), y(both)), Pos2::new(fx(pr, band_freq(b + 1)), pr.bottom()));
                p.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(0xf0, 0x40, 0x40, (40.0 + 90.0 * strength) as u8));
            }
        }
        p.add(Shape::line((0..BANDS).map(|b| Pos2::new(fx(pr, band_freq(b)), y(o[b]))).collect(), Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(0xf0, 0x90, 0x60, 120))));
    }
    // Piano scale along the frequency axis: C keys marked, black keys darker.
    {
        let strip = Rect::from_min_max(Pos2::new(pr.left(), pr.bottom() - 7.0), pr.max);
        for note in 16u8..=127 {
            let f = 440.0 * 2f32.powf((note as f32 - 69.0) / 12.0);
            let f2 = 440.0 * 2f32.powf((note as f32 - 68.0) / 12.0);
            if f < 20.0 || f2 > 20_000.0 {
                continue;
            }
            let black = matches!(note % 12, 1 | 3 | 6 | 8 | 10);
            let r = Rect::from_min_max(Pos2::new(fx(pr, f * 2f32.powf(-1.0 / 24.0)), strip.top()), Pos2::new(fx(pr, f2 * 2f32.powf(-1.0 / 24.0)), strip.bottom()));
            p.rect_filled(r, 0.0, if black { Color32::from_gray(30) } else { Color32::from_gray(150) });
            if note % 12 == 0 {
                p.line_segment([r.left_top(), r.left_bottom()], Stroke::new(1.0_f32, theme::ACCENT));
            }
        }
    }
    // Each band's own shape, faint; the selected band filled in its colour, then the sum.
    for (k, bq, _, _) in &bands {
        let pts: Vec<Pos2> = (0..=120).map(|i| pr.left() + pr.width() * i as f32 / 120.0).map(|x| Pos2::new(x, y_of(bq.response_db(freq_at(pr, x), RATE)))).collect();
        let sel = selected == k + 1;
        if sel {
            let zero = y_of(0.0);
            for w in pts.windows(2) {
                let (a, b) = (w[0], w[1]);
                p.add(Shape::convex_polygon(vec![Pos2::new(a.x, zero), a, b, Pos2::new(b.x, zero)], BAND_COLORS[*k % 7].gamma_multiply(0.16), Stroke::NONE));
            }
        }
        p.add(Shape::line(pts, Stroke::new(if sel { 2.0_f32 } else { 1.0 }, BAND_COLORS[*k % 7].gamma_multiply(if sel { 0.95 } else { 0.4 }))));
    }
    let pts: Vec<Pos2> = (0..=180)
        .map(|i| pr.left() + pr.width() * i as f32 / 180.0)
        .map(|x| {
            let f = freq_at(pr, x);
            Pos2::new(x, y_of(bands.iter().map(|b| b.1.response_db(f, RATE)).sum::<f32>() + out))
        })
        .collect();
    filled_curve(&p, &pts, pr.bottom(), col);
    for (k, bq, f, _) in &bands {
        let c = Pos2::new(fx(pr, *f), y_of(bq.response_db(*f, RATE)));
        let pulse = 1.0 + 0.15 * (h.t() * 2.0 + *k as f32).sin();
        p.circle_filled(c, 10.0 * pulse, BAND_COLORS[*k % 7].gamma_multiply(0.22));
        let rad = if selected == k + 1 { 8.0 } else { 6.5 };
        p.circle_filled(c, rad, BAND_COLORS[*k % 7]);
        p.circle_stroke(c, rad, Stroke::new(if selected == k + 1 { 2.5_f32 } else { 1.5 }, Color32::from_white_alpha(if selected == k + 1 { 255 } else { 200 })));
        p.text(c, egui::Align2::CENTER_CENTER, format!("{}", k + 1), egui::FontId::proportional(8.5), Color32::BLACK);
        // Cuts and shelves show their slope under the node.
        let kind = BiquadKind::from_index(h.v(&format!("b{}_type", k + 1)));
        if dragon_engine::effects::eq::has_slope(kind) {
            let sl = dragon_engine::effects::eq::SLOPES.get(h.v(&format!("b{}_slope", k + 1)).round() as usize).copied().unwrap_or("12 dB/oct");
            let below = if c.y < pr.bottom() - 26.0 { 16.0 } else { -16.0 };
            p.text(c + Vec2::new(0.0, below), egui::Align2::CENTER_CENTER, sl.replace("/oct", ""), egui::FontId::proportional(9.5), BAND_COLORS[*k % 7]);
        }
        if solo == k + 1 {
            p.circle_stroke(c, 11.0, Stroke::new(2.0_f32, Color32::WHITE));
        }
        let b = k + 1;
        let range = h.v(&format!("b{b}_dyn"));
        if range.abs() > 0.05 {
            // the dynamic range as a bar from the static gain, the live change as a dot.
            let g = h.v(&format!("b{b}_gain"));
            let x = c.x;
            p.line_segment([Pos2::new(x, y_of(g)), Pos2::new(x, y_of(g + range))], Stroke::new(3.0_f32, BAND_COLORS[*k % 7].gamma_multiply(0.5)));
            let live = h.tele.get(dragon_engine::dsp::spectrum::BANDS * 2 + k).copied().unwrap_or(0.0);
            p.circle_filled(Pos2::new(x, y_of(g + live)), 3.5, Color32::WHITE);
        }
        let place = h.v(&format!("b{b}_stereo")).round() as usize;
        if place > 0 {
            let tag = ["", "M", "S", "L", "R"][place.min(4)];
            p.text(c + Vec2::new(9.0, -9.0), egui::Align2::LEFT_BOTTOM, tag, egui::FontId::proportional(9.0), BAND_COLORS[*k % 7]);
        }
    }
    let mut tags = Vec::new();
    if solo > 0 {
        tags.push(format!("SOLO {solo}"));
    }
    if auto {
        tags.push(format!("AUTO GAIN {:+.1} dB", out - h.v("output")));
    }
    if h.v("phase") > 0.5 {
        tags.push("LINEAR PHASE".into());
    }
    if !tags.is_empty() {
        p.text(Pos2::new(pr.right() - 4.0, pr.top() + 2.0), egui::Align2::RIGHT_TOP, tags.join(" · "), egui::FontId::proportional(9.5), theme::WARN);
    }
}

/// Multiband saturator: the bands across the frequency axis, each with its style, drive (bar, live value
/// with dynamics as a dot) and level; crossovers are draggable lines.
fn multisat(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    use dragon_engine::effects::multisat::{shape, BANDS, SAT_STYLES};
    grid(p, r, true);
    let pr = plot(r);
    let n = (h.v("bands").round() as usize + 1).clamp(1, BANDS);
    let xs: Vec<f32> = ["x1", "x2", "x3"].iter().take(n - 1).map(|k| h.v(k)).collect();
    let edges: Vec<f32> = std::iter::once(pr.left()).chain(xs.iter().map(|f| fx(pr, *f))).chain(std::iter::once(pr.right())).collect();
    for b in 0..n {
        let band = Rect::from_min_max(Pos2::new(edges[b] + 2.0, pr.top() + 4.0), Pos2::new(edges[b + 1] - 2.0, pr.bottom() - 4.0));
        if band.width() < 8.0 {
            continue;
        }
        let k = b + 1;
        let c = BAND_COLORS[(b * 2) % 7];
        p.rect_filled(band, 4.0, c.gamma_multiply(0.08));
        let drive = h.v(&format!("b{k}_drive"));
        let live = h.tele.get(BANDS + b).copied().filter(|_| h.tele.len() >= BANDS * 2).unwrap_or(drive);
        let fh = (drive / 36.0).clamp(0.0, 1.0) * band.height() * 0.6;
        let bar = Rect::from_min_max(Pos2::new(band.left() + 6.0, band.bottom() - fh), Pos2::new(band.left() + 14.0, band.bottom()));
        p.rect_filled(bar, 2.0, c);
        p.circle_filled(Pos2::new(bar.center().x, band.bottom() - (live / 36.0).clamp(0.0, 1.2) * band.height() * 0.6), 3.0, Color32::WHITE);
        // The style's transfer curve, small.
        let style = h.v(&format!("b{k}_style")).round() as usize;
        let g = dragon_engine::dsp::db_to_gain(drive);
        let cr = Rect::from_min_size(Pos2::new(band.left() + 20.0, band.top() + 18.0), Vec2::new((band.width() - 26.0).clamp(10.0, 60.0), (band.height() * 0.45).min(50.0)));
        p.rect_stroke(cr, 2.0, Stroke::new(1.0_f32, Color32::from_white_alpha(20)), egui::StrokeKind::Inside);
        let pts: Vec<Pos2> =
            (0..=40).map(|i| -1.0 + 2.0 * i as f32 / 40.0).map(|x| Pos2::new(cr.center().x + x * cr.width() * 0.5, cr.center().y - (shape(style, x * g) / g.sqrt()).clamp(-1.2, 1.2) * cr.height() * 0.42)).collect();
        p.add(Shape::line(pts, Stroke::new(1.5_f32, c)));
        p.text(Pos2::new(band.left() + 4.0, band.top() + 2.0), egui::Align2::LEFT_TOP, format!("{k} · {}", SAT_STYLES.get(style).unwrap_or(&"")), egui::FontId::proportional(9.0), c);
        p.text(Pos2::new(band.right() - 4.0, band.bottom() - 2.0), egui::Align2::RIGHT_BOTTOM, format!("{drive:.0} dB"), egui::FontId::monospace(9.0), theme::TEXT);
    }
    for (k, f) in xs.iter().enumerate() {
        let x = fx(pr, *f);
        p.line_segment([Pos2::new(x, pr.top()), Pos2::new(x, pr.bottom())], Stroke::new(2.0_f32, col));
        p.text(Pos2::new(x + 3.0, pr.bottom() - 16.0 - 11.0 * k as f32), egui::Align2::LEFT_BOTTOM, hz_text(*f), egui::FontId::proportional(8.5), theme::TEXT_DIM);
    }
}

fn filter(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    grid(p, r, true);
    let pr = plot(r);
    let kind_of = |t: f32| match t.round() as i32 {
        0 => BiquadKind::LowPass,
        1 => BiquadKind::HighPass,
        2 => BiquadKind::BandPass,
        3 => BiquadKind::Notch,
        4 => BiquadKind::Peak,
        5 => BiquadKind::LowShelf,
        _ => BiquadKind::HighShelf,
    };
    let stages = |s: f32| [1, 2, 4][(s.round().max(0.0) as usize).min(2)];
    // The cutoff wobbles with the LFO like the sound does.
    let lfo = (h.t() * TAU * h.v("lfo_rate")).sin() * h.v("lfo_depth");
    let cutoff = (h.v("cutoff") * 2f32.powf(lfo)).clamp(20.0, 20_000.0);
    let unit = |t: f32, f: f32, res: f32, slope: f32| {
        let q = 0.707 + res * 12.0;
        (Biquad::new(kind_of(t), f, q, h.v("gain"), RATE), stages(slope))
    };
    let a = unit(h.v("type"), cutoff, h.v("resonance"), h.v("slope"));
    let b_on = h.v("b_on") >= 0.5;
    let b_cut = if h.v("b_follow") >= 0.5 { (h.v("b_cutoff") * cutoff / h.v("cutoff").max(1.0)).clamp(20.0, 20_000.0) } else { h.v("b_cutoff") };
    let b = unit(h.v("b_type"), b_cut, h.v("b_resonance"), h.v("b_slope"));
    let parallel = h.v("routing") >= 0.5;
    let resp = |f: f32| {
        let ra = a.0.response_db(f, RATE) * a.1 as f32;
        if !b_on {
            return ra;
        }
        let rb = b.0.response_db(f, RATE) * b.1 as f32;
        if parallel {
            // Sum of the two paths (in amplitude), halved.
            20.0 * ((10f32.powf(ra / 20.0) + 10f32.powf(rb / 20.0)) * 0.5).max(1e-6).log10()
        } else {
            ra + rb
        }
    };
    let (top, bottom) = (12.0, -24.0);
    let y_of = |db: f32| pr.top() + (top - db.clamp(bottom, top)) / (top - bottom) * pr.height();
    y_axis(p, pr, &[(12.0, "+12"), (6.0, "+6"), (0.0, "0"), (-6.0, "-6"), (-12.0, "-12"), (-18.0, "-18"), (-24.0, "-24")], y_of);
    let p = p.with_clip_rect(pr.expand(10.0).intersect(r));
    let pts: Vec<Pos2> = (0..=180).map(|i| pr.left() + pr.width() * i as f32 / 180.0).map(|x| Pos2::new(x, y_of(resp(freq_at(pr, x))))).collect();
    filled_curve(&p, &pts, pr.bottom(), col);
    // The cutoff nodes: a ring with a glow (A), and a smaller one for filter B.
    let c = Pos2::new(fx(pr, cutoff), y_of(resp(cutoff)));
    p.circle_filled(c, 11.0, col.gamma_multiply(0.25));
    p.circle_filled(c, 6.0, theme::BG);
    p.circle_stroke(c, 6.0, Stroke::new(2.0_f32, Color32::WHITE));
    if b_on {
        let c = Pos2::new(fx(pr, b_cut), y_of(resp(b_cut)));
        p.circle_filled(c, 5.0, theme::BG);
        p.circle_stroke(c, 5.0, Stroke::new(2.0_f32, theme::WARN));
    }
}

// --------------------------------------------------------------------------------------- dynamics

fn dynamics(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    super::fx_mod::screen(p, r, 6.0);
    // Left: transfer curve (input dB → output dB). Right: big gain-reduction meter.
    let sq = square(r);
    let lo = -60.0;
    let to_x = |db: f32| sq.left() + (db - lo) / -lo * sq.width();
    let to_y = |db: f32| sq.bottom() - ((db - lo) / -lo).clamp(0.0, 1.0) * sq.height();
    p.rect_stroke(sq, 3.0, Stroke::new(1.0_f32, Color32::from_white_alpha(20)), egui::StrokeKind::Inside);
    y_axis(p, sq, &[(0.0, "0"), (-12.0, "-12"), (-24.0, "-24"), (-36.0, "-36"), (-48.0, "-48")], to_y);
    x_axis(p, sq, &[(-48.0, "-48".into()), (-36.0, "-36".into()), (-24.0, "-24".into()), (-12.0, "-12".into()), (0.0, "0".into())], to_x);
    // The 1:1 line, dashed.
    p.extend(Shape::dashed_line(&[Pos2::new(sq.left(), sq.bottom()), Pos2::new(sq.right(), sq.top())], Stroke::new(1.0_f32, Color32::from_white_alpha(45)), 4.0, 4.0));
    let (thr, ratio, knee, makeup) = match h.def.id {
        "limiter" => (h.v("ceiling") - h.v("gain"), 100.0, 0.0, h.v("gain")),
        "gate" => (h.v("threshold"), 1.0 / h.v("ratio").clamp(1.0, 8.0), h.v("knee"), 0.0),
        "deesser" => (h.v("threshold"), 1.0 + h.v("range") / 6.0, 0.0, 0.0),
        _ => (h.v("threshold"), h.v("ratio"), h.v("knee"), h.v("makeup")),
    };
    let curve = |x: f32| -> f32 {
        let y = if h.def.id == "gate" {
            if x < thr {
                thr + (x - thr) / ratio
            } else {
                x
            }
        } else if knee > 0.0 && (x - thr).abs() < knee / 2.0 {
            let d = x - thr + knee / 2.0;
            x + (1.0 / ratio - 1.0) * d * d / (2.0 * knee)
        } else if x > thr {
            thr + (x - thr) / ratio
        } else {
            x
        };
        y + makeup
    };
    let pts: Vec<Pos2> = (0..=80).map(|k| lo + -lo * k as f32 / 80.0).map(|x| Pos2::new(to_x(x), to_y(curve(x)))).collect();
    filled_curve(p, &pts, sq.bottom(), col);
    p.line_segment([Pos2::new(to_x(thr), sq.top()), Pos2::new(to_x(thr), sq.bottom())], Stroke::new(1.0_f32, theme::WARN.gamma_multiply(0.6)));
    let tx = to_x(thr);
    let (anchor, x) = if tx > sq.center().x { (egui::Align2::RIGHT_BOTTOM, tx - 3.0) } else { (egui::Align2::LEFT_BOTTOM, tx + 3.0) };
    p.text(Pos2::new(x, sq.bottom() - 3.0), anchor, format!("thr {thr:.0} dB"), egui::FontId::proportional(9.0), theme::WARN);
    // The ball rides the curve at the level implied by the current gain reduction.
    // The ball sits at the real input level: the measured output peak plus the reduction, minus make-up
    // (in a mixer window); only the gallery, which has no signal, lets it sway as a demo.
    let x_in = match out_level(p.ctx(), h) {
        Some((_, _, pk)) => (gain_to_db_ui(pk) + h.gr - h.v("makeup")).clamp(lo, 0.0),
        None if h.gr > 0.05 => (thr + h.gr * ratio / (ratio - 1.0).max(0.01)).min(0.0),
        None => thr - 6.0 + 3.0 * (h.t() * 1.3).sin(),
    };
    let ball = Pos2::new(to_x(x_in), to_y(curve(x_in)));
    p.circle_filled(ball, 8.0, col.gamma_multiply(0.3));
    p.circle_filled(ball, 4.5, Color32::WHITE);
    // Segmented, LED-style gain-reduction meter (orange), then the reading.
    let meter = Rect::from_min_max(Pos2::new(sq.right() + 34.0, sq.top() + 14.0), Pos2::new(sq.right() + 50.0, sq.bottom()));
    p.text(Pos2::new(meter.center().x, r.top() + 4.0), egui::Align2::CENTER_TOP, "GR", egui::FontId::proportional(9.0), theme::TEXT_DIM);
    let segs = 16;
    let seg_h = meter.height() / segs as f32;
    let lit = ((h.gr / 24.0).clamp(0.0, 1.0) * segs as f32).round() as usize;
    for k in 0..segs {
        let y0 = meter.top() + k as f32 * seg_h;
        let sr = Rect::from_min_max(Pos2::new(meter.left(), y0 + 1.0), Pos2::new(meter.right(), y0 + seg_h - 1.0));
        p.rect_filled(sr, 1.5, if k < lit { Color32::from_rgb(0xf0, 0x8a, 0x4b) } else { Color32::from_white_alpha(14) });
        if k % 4 == 0 {
            p.text(Pos2::new(meter.right() + 4.0, sr.center().y), egui::Align2::LEFT_CENTER, format!("-{}", k * 24 / segs), egui::FontId::proportional(8.0), Color32::from_white_alpha(90));
        }
    }
    let m = Rect::from_min_max(Pos2::new(meter.right() + 30.0, sq.top()), Pos2::new(r.right() - 12.0, sq.bottom()));
    let history = matches!(h.def.id, "compressor" | "limiter");
    p.text(Pos2::new(m.left(), m.top() + 4.0), egui::Align2::LEFT_TOP, "GAIN REDUCTION", egui::FontId::proportional(9.5), theme::TEXT_DIM);
    p.text(
        Pos2::new(if history { m.right() } else { m.left() }, if history { m.top() } else { m.top() + 22.0 }),
        if history { egui::Align2::RIGHT_TOP } else { egui::Align2::LEFT_TOP },
        if h.gr > 0.05 { format!("−{:.1} dB", h.gr) } else { "0.0 dB".into() },
        egui::FontId::monospace(if history { 15.0 } else { 26.0 }),
        if h.gr > 0.1 { theme::WARN } else { theme::TEXT_DIM },
    );
    if history {
        level_history(p, Rect::from_min_max(Pos2::new(m.left(), m.top() + 22.0), Pos2::new(m.right(), m.bottom() - 18.0)), h, col, |x| curve(x) - makeup);
    }
    let caption = match h.def.id {
        "limiter" => format!("{} · ceiling {:.1} dB · +{:.1} dB drive", dragon_engine::effects::dynamics::LIMITER_STYLES.get(h.v("style").round() as usize).copied().unwrap_or("Allround"), h.v("ceiling"), h.v("gain")),
        "gate" => format!(
            "{} · range {:.0} dB · hold {:.0} ms · hyst {:.0} dB{}",
            dragon_engine::effects::dynamics::GATE_STYLES.get(h.v("style").round() as usize).copied().unwrap_or("Classic"),
            h.v("range"),
            h.v("hold"),
            h.v("hysteresis"),
            if h.v("flip") >= 0.5 { " · flipped" } else { "" }
        ),
        "deesser" => format!(
            "{} · {} · {:.1}-{:.1} kHz · range {:.0} dB",
            dragon_engine::effects::dynamics::DS_MODES.get(h.v("mode").round() as usize).copied().unwrap_or("Single vocal"),
            if h.v("processing") >= 0.5 { "wide band" } else { "split band" },
            h.v("freq") / 1000.0,
            h.v("freq_hi") / 1000.0,
            h.v("range")
        ),
        _ => {
            let style = dragon_engine::effects::dynamics::COMP_STYLES.get(h.v("style").round() as usize).copied().unwrap_or("Clean");
            let look = if h.v("lookahead") > 0.05 { format!(" · look-ahead {:.1} ms", h.v("lookahead")) } else { String::new() };
            format!("{style} · {ratio:.1}:1 · {:.0} / {:.0} ms{look}", h.v("attack"), h.v("release"))
        }
    };
    p.text(Pos2::new(m.left(), m.bottom()), egui::Align2::LEFT_BOTTOM, caption, egui::FontId::proportional(10.5), theme::TEXT);
}

/// Scrolling history: input level (grey), output level (colour) and gain reduction
/// (orange, hanging from the top), on one dB scale. `static_gr` gives the curve for the gallery demo.
fn level_history(p: &egui::Painter, g: Rect, h: &Hero, col: Color32, static_out: impl Fn(f32) -> f32) {
    const LEN: usize = 240;
    super::fx_mod::inset(p, g, 3.0);
    let hist_id = h.key.with("comp.hist");
    let mut hist: std::collections::VecDeque<[f32; 3]> = p.ctx().data(|d| d.get_temp(hist_id)).unwrap_or_default();
    if h.time.is_some() {
        let v = if h.tele.len() >= 3 {
            [h.tele[0], h.tele[1], h.tele[2]]
        } else {
            // Gallery: a drum-like level, compressed by the static curve.
            let t = h.t() % 0.5;
            let x = -36.0 + 32.0 * (-t / 0.12).exp();
            let y = static_out(x);
            [x, y + h.v("makeup"), (x - y).max(0.0)]
        };
        hist.push_back(v);
        while hist.len() > LEN {
            hist.pop_front();
        }
        p.ctx().data_mut(|d| d.insert_temp(hist_id, hist.clone()));
    }
    let lo = -60.0;
    let y = |db: f32| g.bottom() - ((db - lo) / -lo).clamp(0.0, 1.0) * g.height();
    for db in [-12.0, -24.0, -36.0, -48.0] {
        p.line_segment([Pos2::new(g.left(), y(db)), Pos2::new(g.right(), y(db))], Stroke::new(1.0_f32, Color32::from_white_alpha(10)));
        p.text(Pos2::new(g.left() + 2.0, y(db)), egui::Align2::LEFT_BOTTOM, format!("{db:.0}"), egui::FontId::proportional(7.5), Color32::from_white_alpha(60));
    }
    let n = hist.len();
    let dx = g.width() / LEN as f32;
    let xs = |i: usize| g.right() - (n - i) as f32 * dx;
    for (i, v) in hist.iter().enumerate() {
        let x = xs(i);
        p.rect_filled(Rect::from_min_max(Pos2::new(x, y(v[0])), Pos2::new(x + dx, g.bottom())), 0.0, Color32::from_white_alpha(26));
        if v[2] > 0.05 {
            let gh = (v[2] / -lo) * g.height();
            p.rect_filled(Rect::from_min_max(Pos2::new(x, g.top()), Pos2::new(x + dx, g.top() + gh)), 0.0, Color32::from_rgba_unmultiplied(0xf0, 0x5a, 0x3b, 150));
        }
    }
    if n > 1 {
        let out: Vec<Pos2> = hist.iter().enumerate().map(|(i, v)| Pos2::new(xs(i) + dx * 0.5, y(v[1]))).collect();
        p.add(Shape::line(out, Stroke::new(1.3_f32, col)));
    }
    p.rect_stroke(g, 3.0, Stroke::new(1.0_f32, Color32::from_white_alpha(18)), egui::StrokeKind::Inside);
}

fn transient(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    use dragon_engine::effects::dynamics::{ts_shape, ts_sus_shape, TS_SHAPES, TS_SUS_SHAPES};
    super::fx_mod::screen(p, r, 6.0);
    let att = [h.v("attack") / 100.0, h.v("att_dur") / 1000.0, h.v("att_shape").round()];
    let sus = [h.v("sustain") / 100.0, h.v("sus_dur") / 1000.0, h.v("sus_shape").round()];
    let weights = |t: f32| -> (f32, f32) {
        let (ad, sd) = (att[1].max(0.001), sus[1].max(0.01));
        let wa = if t < ad { ts_shape(att[2] as usize, t / ad) } else { 0.0 };
        let s0 = ad * 0.6;
        let ws = if t >= s0 && t < s0 + sd { ((t - s0) / 0.005).min(1.0) * ts_sus_shape(sus[2] as usize, (t - s0) / sd, sus[0]) } else { 0.0 };
        (wa, ws)
    };
    // ---- top: scrolling level (grey) and gain change (attack orange-ish, sustain colour)
    let top = Rect::from_min_max(r.min + Vec2::new(10.0, 10.0), Pos2::new(r.right() - 10.0, r.top() + r.height() * 0.55));
    super::fx_mod::inset(p, top, 4.0);
    let hist_id = h.key.with("ts.hist");
    let mut hist: std::collections::VecDeque<[f32; 4]> = p.ctx().data(|d| d.get_temp(hist_id)).unwrap_or_default();
    if h.time.is_some() {
        let v = if h.tele.len() >= 4 {
            [h.tele[0], h.tele[1], h.tele[2], h.tele[3]]
        } else {
            // Gallery: a hit every 0.5 s.
            let t = (h.t() % 0.5).max(0.0);
            let lvl = 0.8 * (-t / 0.09).exp();
            let (wa, ws) = weights(t);
            [lvl, wa, ws, att[0] * 18.0 * wa + sus[0] * 18.0 * ws]
        };
        hist.push_back(v);
        while hist.len() > 200 {
            hist.pop_front();
        }
        p.ctx().data_mut(|d| d.insert_temp(hist_id, hist.clone()));
    }
    let n = hist.len();
    let bw = top.width() / 200.0;
    let mid = top.center().y;
    let attack_col = Color32::from_rgb(0xf0, 0x8a, 0x4b);
    for (i, v) in hist.iter().enumerate() {
        let x = top.right() - (n - i) as f32 * bw;
        let lv = (dragon_engine::dsp::gain_to_db(v[0].max(1e-4)) + 48.0).max(0.0) / 48.0;
        let hgt = lv * top.height() * 0.46;
        p.rect_filled(Rect::from_min_max(Pos2::new(x, mid - hgt), Pos2::new(x + bw * 0.8, mid + hgt)), 0.0, Color32::from_white_alpha(28));
        // The processed level: input scaled by the gain change.
        let out = (lv + v[3] / 48.0).clamp(0.0, 1.2);
        let oh = out * top.height() * 0.46;
        let c = if v[1] > v[2] { attack_col } else { col };
        if v[1] > 0.01 || v[2] > 0.01 {
            p.rect_filled(Rect::from_min_max(Pos2::new(x, mid - oh), Pos2::new(x + bw * 0.8, mid - hgt.min(oh))), 0.0, c.gamma_multiply(0.8));
            p.rect_filled(Rect::from_min_max(Pos2::new(x, mid + hgt.min(oh)), Pos2::new(x + bw * 0.8, mid + oh)), 0.0, c.gamma_multiply(0.8));
        }
    }
    p.line_segment([Pos2::new(top.left(), mid), Pos2::new(top.right(), mid)], Stroke::new(1.0_f32, Color32::from_white_alpha(20)));
    // ---- bottom: the attack and sustain gain envelopes
    let bot = Rect::from_min_max(Pos2::new(r.left() + 10.0, top.bottom() + 8.0), r.max - Vec2::new(10.0, 8.0));
    let half = bot.width() / 2.0 - 4.0;
    for (k, (name, amt, dur, shape, color)) in [("ATTACK", att[0], att[1], att[2], attack_col), ("SUSTAIN", sus[0], sus[1], sus[2], col)].into_iter().enumerate() {
        let pr = Rect::from_min_size(Pos2::new(bot.left() + k as f32 * (half + 8.0), bot.top()), Vec2::new(half, bot.height()));
        super::fx_mod::inset(p, pr, 4.0);
        let cy = pr.center().y + 6.0;
        let span = att[1] * 0.6 + sus[1];
        let pts: Vec<Pos2> = (0..=80)
            .map(|i| {
                let t = span * i as f32 / 80.0;
                let (wa, ws) = weights(t);
                let w = if k == 0 { wa } else { ws };
                Pos2::new(pr.left() + 6.0 + (pr.width() - 12.0) * i as f32 / 80.0, cy - amt * w * (pr.height() * 0.32))
            })
            .collect();
        p.line_segment([Pos2::new(pr.left() + 4.0, cy), Pos2::new(pr.right() - 4.0, cy)], Stroke::new(1.0_f32, Color32::from_white_alpha(25)));
        filled_curve(p, &pts, cy, color);
        p.text(pr.left_top() + Vec2::new(6.0, 4.0), egui::Align2::LEFT_TOP, name, egui::FontId::proportional(9.5), color);
        p.text(
            pr.right_top() + Vec2::new(-6.0, 4.0),
            egui::Align2::RIGHT_TOP,
            format!("{:+.0}% · {:.0} ms · {}", amt * 100.0, dur * 1000.0, if k == 0 { TS_SHAPES } else { TS_SUS_SHAPES }.get(shape as usize).unwrap_or(&"")),
            egui::FontId::proportional(9.5),
            theme::TEXT,
        );
    }
}

// ------------------------------------------------------------------------------------ time based

/// Where the reverb's decay-rate curve sits: the right pane of Dragon's Lair.
fn decay_rect(r: Rect) -> Rect {
    super::fx_cave::lair_layout(r).2.shrink(4.0)
}

const DR_RANGE: f32 = 2.0; // log2: ×0.25 … ×4

/// Decay-rate EQ: six bands (bell, shelves, notch) with free frequency, ×0.25 … ×4 of the
/// decay time and Q (wheel); right-click a band for its shape.
pub(super) fn decay_curve(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    let y_of = |m: f32| r.center().y - (m.max(0.01).log2() / DR_RANGE).clamp(-1.0, 1.0) * (r.height() * 0.4);
    p.line_segment([Pos2::new(r.left(), r.center().y), Pos2::new(r.right(), r.center().y)], Stroke::new(1.0_f32, Color32::from_white_alpha(30)));
    // The curve the reverb uses (sum of the bands' shapes), and a dot per band at its value.
    let bands = dragon_engine::effects::reverb::dr_bands(|k| h.v(k));
    let line: Vec<Pos2> = (0..=120).map(|i| r.left() + r.width() * i as f32 / 120.0).map(|x| Pos2::new(x, y_of(dragon_engine::effects::reverb::dr_curve(&bands, freq_at(r, x))))).collect();
    p.add(Shape::line(line, Stroke::new(1.5_f32, col)));
    for b in &bands {
        let c = Pos2::new(fx(r, b.freq), y_of(b.mul));
        p.circle_filled(c, 4.0, col);
        p.circle_stroke(c, 4.0, Stroke::new(1.0_f32, Color32::WHITE));
    }
    p.text(r.left_top() + Vec2::new(4.0, 2.0), egui::Align2::LEFT_TOP, "DECAY RATE", egui::FontId::proportional(8.5), theme::TEXT_DIM);
    p.text(Pos2::new(r.right() - 4.0, y_of(4.0)), egui::Align2::RIGHT_CENTER, "×4", egui::FontId::proportional(8.0), theme::TEXT_DIM);
    p.text(Pos2::new(r.right() - 4.0, y_of(0.25)), egui::Align2::RIGHT_CENTER, "×¼", egui::FontId::proportional(8.0), theme::TEXT_DIM);
}

// ---------------------------------------------------------------------------------- distortion

fn shaper(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    super::fx_mod::screen(p, r, 6.0);
    let sq = square(r);
    p.rect_stroke(sq, 3.0, Stroke::new(1.0_f32, Color32::from_white_alpha(20)), egui::StrokeKind::Inside);
    let crush = h.def.id == "bitcrusher";
    let shape = |x: f32| -> f32 {
        if crush {
            let steps = 2f32.powf(h.v("bits") - 1.0);
            (x * steps).round() / steps
        } else {
            let drive = dragon_engine::dsp::db_to_gain(h.v("drive"));
            let mode = h.v("mode").round() as usize;
            if mode == 0 {
                let t = dragon_engine::dsp::tube::TUBES[(h.v("tube").round() as usize).min(dragon_engine::dsp::tube::TUBES.len() - 1)];
                dragon_engine::dsp::tube::tube_curve(&t, h.v("bias"), x * drive).clamp(-1.2, 1.2)
            } else {
                effects::saturation_curve(mode, x * drive).clamp(-1.2, 1.2)
            }
        }
    };
    let c = sq.center();
    let half = sq.width() / 2.0;
    let ax = |v: f32| c.x + v * half;
    let ay = |v: f32| c.y - v * half;
    y_axis(p, sq, &[(1.0, "+1.0"), (0.5, "+0.5"), (0.0, "0"), (-0.5, "-0.5"), (-1.0, "-1.0")], ay);
    x_axis(p, sq, &[(-1.0, "-1".into()), (-0.5, "-0.5".into()), (0.0, "0".into()), (0.5, "+0.5".into()), (1.0, "+1".into())], ax);
    p.extend(Shape::dashed_line(&[Pos2::new(sq.left(), sq.bottom()), Pos2::new(sq.right(), sq.top())], Stroke::new(1.0_f32, Color32::from_white_alpha(50)), 4.0, 4.0));
    let pts: Vec<Pos2> = (0..=120).map(|k| -1.0 + 2.0 * k as f32 / 120.0).map(|x| Pos2::new(ax(x), ay(shape(x).clamp(-1.0, 1.0)))).collect();
    let p2 = p.with_clip_rect(sq.expand(2.0));
    filled_curve(&p2, &pts, c.y, col);
    // The dot rides the curve at the real level going in (the measured output peak traced back through
    // the curve); in the gallery, with no signal, a test sine moves it.
    let xin = match out_level(p.ctx(), h) {
        Some((_, _, pk)) => {
            let (mut a, mut b) = (0.0f32, 1.0f32);
            for _ in 0..24 {
                let m = 0.5 * (a + b);
                if shape(m) < pk {
                    a = m;
                } else {
                    b = m;
                }
            }
            0.5 * (a + b)
        }
        None => (h.t() * 2.0).sin() * 0.9,
    };
    let dot = Pos2::new(ax(xin), ay(shape(xin).clamp(-1.0, 1.0)));
    p.circle_filled(dot, 7.0, col.gamma_multiply(0.3));
    p.circle_filled(dot, 3.5, Color32::WHITE);
    // Right: a sine going in, and what comes out, scrolling.
    let w = Rect::from_min_max(Pos2::new(sq.right() + 24.0, r.top() + 10.0), Pos2::new(r.right() - 10.0, r.bottom() - 10.0));
    let ph = h.t() * 2.0;
    let mid = w.center().y;
    let amp = w.height() * 0.42;
    let ds = if crush { h.v("downsample").max(1.0) } else { 1.0 };
    let ins: Vec<Pos2> = (0..=160).map(|k| k as f32 / 160.0).map(|u| Pos2::new(w.left() + u * w.width(), mid - (u * TAU * 2.0 + ph).sin() * amp * 0.9)).collect();
    let outs: Vec<Pos2> = (0..=160)
        .map(|k| {
            let kk = ((k as f32 / ds).floor() * ds) / 160.0;
            let u = k as f32 / 160.0;
            Pos2::new(w.left() + u * w.width(), mid - shape((kk * TAU * 2.0 + ph).sin() * 0.9) * amp)
        })
        .collect();
    p.add(Shape::line(ins, Stroke::new(1.0_f32, Color32::from_white_alpha(40))));
    p.add(Shape::line(outs, Stroke::new(2.0_f32, col)));
    let label = if crush {
        format!("{:.0} bit · ÷{:.0}", h.v("bits"), h.v("downsample"))
    } else if h.v("mode").round() as usize == 0 {
        let t = dragon_engine::dsp::tube::TUBES[(h.v("tube").round() as usize).min(dragon_engine::dsp::tube::TUBES.len() - 1)];
        format!("{} · {:+.1} dB drive", t.name, h.v("drive"))
    } else {
        format!("{:+.1} dB drive", h.v("drive"))
    };
    p.text(w.right_top(), egui::Align2::RIGHT_TOP, label, egui::FontId::proportional(10.5), theme::TEXT);
}

// ----------------------------------------------------------------------------------- modulation

const POWER_TUBE: &[u8] = include_bytes!("../../assets/fx/power_tube.png");
const GAUGE: &[u8] = include_bytes!("../../assets/fx/gauge.png");
const BRASS_PLATE: &[u8] = include_bytes!("../../assets/fx/brass_plate.png");
const GLASS_TUBE: &[u8] = include_bytes!("../../assets/fx/glass_tube.png");

// --------------------------------------------------------------------------------------- utility

fn stereo(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    super::fx_mod::screen(p, r, 6.0);
    let width = h.v("width");
    let mono = h.def.id == "utility" && h.v("mono") > 0.5;
    let pan = if h.def.id == "utility" { h.v("pan") } else { 0.0 };
    let c = Pos2::new(r.center().x + pan * r.width() * 0.25, r.bottom() - 10.0);
    let rad = r.height() - 22.0;
    // The stereo field as a half-disc; the image's spread as a coloured wedge.
    let arc: Vec<Pos2> = (0..=60).map(|k| PI + PI * k as f32 / 60.0).map(|a| Pos2::new(r.center().x + a.cos() * rad, c.y + a.sin() * rad)).collect();
    p.add(Shape::line(arc, Stroke::new(1.0_f32, Color32::from_white_alpha(30))));
    let spread = if mono { 0.02 } else { (width / 2.0).clamp(0.02, 1.0) * PI * 0.5 };
    let mut wedge = vec![c];
    wedge.extend((0..=30).map(|k| -PI / 2.0 - spread + 2.0 * spread * k as f32 / 30.0).map(|a| Pos2::new(c.x + a.cos() * rad, c.y + a.sin() * rad)));
    p.add(Shape::convex_polygon(wedge, col.gamma_multiply(0.18), Stroke::new(1.5_f32, col)));
    // Particles drift inside the wedge.
    for k in 0..36 {
        let u = ((k as f32 * 0.6180339).fract() * 2.0 - 1.0) * spread;
        let d = ((h.t() * 0.25 + k as f32 * 0.137).fract()) * rad;
        let a = -PI / 2.0 + u;
        p.circle_filled(Pos2::new(c.x + a.cos() * d, c.y + a.sin() * d), 2.0, col.gamma_multiply(1.0 - d / rad));
    }
    p.text(r.left_top() + Vec2::new(8.0, 6.0), egui::Align2::LEFT_TOP, "L", egui::FontId::proportional(11.0), theme::TEXT_DIM);
    p.text(r.right_top() + Vec2::new(-8.0, 6.0), egui::Align2::RIGHT_TOP, "R", egui::FontId::proportional(11.0), theme::TEXT_DIM);
    let label = if mono { "mono".to_string() } else { format!("width {:.0}%", width * 100.0) };
    p.text(Pos2::new(r.center().x, r.top() + 6.0), egui::Align2::CENTER_TOP, label, egui::FontId::proportional(11.0), theme::TEXT);
}

/// The stereo imager as a phase meter: a goniometer (mid vertical, side horizontal) of the output, a
/// correlation meter from −1 (out of phase) to +1 (mono) and a left / right balance bar.
fn phase_meter(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    super::fx_mod::screen(p, r, 6.0);
    // Live data, or a synthetic signal with the set width in the gallery.
    let demo: Vec<f32>;
    let data: &[f32] = if h.tele.len() > 3 {
        h.tele
    } else {
        let w = h.v("width");
        let t = h.t();
        let mut d = vec![0.0f32; 3];
        let (mut lr, mut ll, mut rr) = (0.0f32, 0.0f32, 0.0f32);
        for k in 0..192 {
            let x = t * 3.0 + k as f32 * 0.07;
            let m = (x * 2.3).sin() * 0.6 + (x * 5.1).sin() * 0.2;
            let sd = ((x * 3.7).sin() * 0.35 + (x * 7.3).cos() * 0.15) * w;
            let (l, rr_) = (m + sd, m - sd);
            lr += l * rr_;
            ll += l * l;
            rr += rr_ * rr_;
            d.push(l * 0.8);
            d.push(rr_ * 0.8);
        }
        d[0] = (lr / (ll * rr).sqrt().max(1e-9)).clamp(-1.0, 1.0);
        d[1] = ((rr - ll) / (rr + ll).max(1e-9)).clamp(-1.0, 1.0);
        d[2] = w * 0.5;
        demo = d;
        &demo
    };
    let (corr, bal, sm) = (data[0], data[1], data[2]);
    // ---- goniometer
    let size = r.height() - 20.0;
    let g = Rect::from_center_size(Pos2::new(r.left() + 10.0 + size / 2.0, r.center().y), Vec2::splat(size));
    let c = g.center();
    let rad = size / 2.0;
    p.circle_stroke(c, rad, Stroke::new(1.0_f32, Color32::from_white_alpha(25)));
    p.circle_stroke(c, rad * 0.5, Stroke::new(1.0_f32, Color32::from_white_alpha(12)));
    // Axes: M vertical, S horizontal, L / R on the diagonals.
    let axis = Stroke::new(1.0_f32, Color32::from_white_alpha(22));
    p.line_segment([Pos2::new(c.x, g.top()), Pos2::new(c.x, g.bottom())], axis);
    p.line_segment([Pos2::new(g.left(), c.y), Pos2::new(g.right(), c.y)], axis);
    let d = rad * std::f32::consts::FRAC_1_SQRT_2;
    p.line_segment([c + Vec2::new(-d, -d), c + Vec2::new(d, d)], axis);
    p.line_segment([c + Vec2::new(d, -d), c + Vec2::new(-d, d)], axis);
    let lab = |pos: Pos2, t: &str| p.text(pos, egui::Align2::CENTER_CENTER, t, egui::FontId::proportional(9.5), Color32::from_white_alpha(120));
    lab(Pos2::new(c.x, g.top() + 7.0), "M");
    lab(Pos2::new(g.left() + 7.0, c.y), "+S");
    lab(Pos2::new(g.right() - 7.0, c.y), "−S");
    lab(c + Vec2::new(-d - 6.0, -d - 6.0), "L");
    lab(c + Vec2::new(d + 6.0, -d - 6.0), "R");
    let pts: Vec<Pos2> = data[3..]
        .chunks(2)
        .map(|lr| {
            let (l, rr) = (lr[0], lr[1]);
            // Rotate 45°: mid up, side sideways; soft-limit so loud signals stay in the circle.
            let m = (l + rr) * std::f32::consts::FRAC_1_SQRT_2;
            let sd = (l - rr) * std::f32::consts::FRAC_1_SQRT_2;
            let k = 1.0 / (1.0 + (m * m + sd * sd).sqrt() * 0.3);
            Pos2::new(c.x - sd * k * rad * 0.9, c.y - m * k * rad * 0.9)
        })
        .collect();
    let p2 = p.with_clip_rect(g.expand(2.0));
    let n = pts.len().max(1);
    for (i, w) in pts.windows(2).enumerate() {
        let a = 0.15 + 0.85 * i as f32 / n as f32;
        p2.line_segment([w[0], w[1]], Stroke::new(1.0_f32, col.gamma_multiply(a * 0.7)));
    }
    for q in pts.iter().step_by(3) {
        p2.circle_filled(*q, 1.2, Color32::WHITE.gamma_multiply(0.5));
    }
    // ---- correlation meter
    let x0 = g.right() + 24.0;
    let mr = Rect::from_min_max(Pos2::new(x0, r.top() + 34.0), Pos2::new(r.right() - 14.0, r.top() + 56.0));
    p.text(Pos2::new(mr.left(), mr.top() - 18.0), egui::Align2::LEFT_TOP, "PHASE CORRELATION", egui::FontId::proportional(9.5), theme::TEXT_DIM);
    {
        use egui::epaint::{Mesh, Vertex, WHITE_UV};
        let mut mesh = Mesh::default();
        let stops = [(0.0f32, Color32::from_rgb(0xf0, 0x52, 0x6a)), (0.5, Color32::from_rgb(0xf5, 0xc8, 0x42)), (1.0, Color32::from_rgb(0x5d, 0xe0, 0xa8))];
        for w in stops.windows(2) {
            let i = mesh.vertices.len() as u32;
            let xa = mr.left() + w[0].0 * mr.width();
            let xb = mr.left() + w[1].0 * mr.width();
            for (pos, cc) in [(Pos2::new(xa, mr.top()), w[0].1), (Pos2::new(xb, mr.top()), w[1].1), (Pos2::new(xb, mr.bottom()), w[1].1), (Pos2::new(xa, mr.bottom()), w[0].1)] {
                mesh.vertices.push(Vertex { pos, uv: WHITE_UV, color: cc.gamma_multiply(0.35) });
            }
            mesh.indices.extend_from_slice(&[i, i + 1, i + 2, i, i + 2, i + 3]);
        }
        p.add(Shape::mesh(mesh));
    }
    let cx = mr.left() + (corr + 1.0) / 2.0 * mr.width();
    p.rect_filled(Rect::from_center_size(Pos2::new(cx, mr.center().y), Vec2::new(4.0, mr.height() + 6.0)), 2.0, Color32::WHITE);
    for (v, t) in [(-1.0f32, "−1"), (0.0, "0"), (1.0, "+1")] {
        p.text(Pos2::new(mr.left() + (v + 1.0) / 2.0 * mr.width(), mr.bottom() + 3.0), egui::Align2::CENTER_TOP, t, egui::FontId::proportional(9.0), theme::TEXT_DIM);
    }
    let verdict = if corr < -0.1 {
        ("out of phase – mono will cancel", theme::REC)
    } else if corr < 0.3 {
        ("very wide", theme::WARN)
    } else if corr > 0.95 {
        ("mono", theme::TEXT_DIM)
    } else {
        ("healthy stereo", theme::OK)
    };
    p.text(Pos2::new(mr.right(), mr.top() - 18.0), egui::Align2::RIGHT_TOP, format!("{corr:+.2}  {}", verdict.0), egui::FontId::proportional(10.0), verdict.1);
    // ---- balance and width
    let br = Rect::from_min_max(Pos2::new(x0, mr.bottom() + 40.0), Pos2::new(r.right() - 14.0, mr.bottom() + 52.0));
    p.text(Pos2::new(br.left(), br.top() - 16.0), egui::Align2::LEFT_TOP, "BALANCE", egui::FontId::proportional(9.5), theme::TEXT_DIM);
    p.rect_filled(br, 6.0, Color32::from_white_alpha(12));
    let bx = br.center().x + bal * br.width() / 2.0;
    p.rect_filled(Rect::from_min_max(Pos2::new(br.center().x.min(bx), br.top()), Pos2::new(br.center().x.max(bx), br.bottom())), 6.0, col);
    p.line_segment([Pos2::new(br.center().x, br.top() - 2.0), Pos2::new(br.center().x, br.bottom() + 2.0)], Stroke::new(1.0_f32, Color32::WHITE));
    p.text(Pos2::new(br.left(), br.bottom() + 3.0), egui::Align2::LEFT_TOP, "L", egui::FontId::proportional(9.0), theme::TEXT_DIM);
    p.text(Pos2::new(br.right(), br.bottom() + 3.0), egui::Align2::RIGHT_TOP, "R", egui::FontId::proportional(9.0), theme::TEXT_DIM);
    p.text(
        Pos2::new(x0, r.bottom() - 10.0),
        egui::Align2::LEFT_BOTTOM,
        format!("width {:.0}% · side/mid {:.0}% · mono below {:.0} Hz", h.v("width") * 100.0, sm * 100.0, h.v("mono_below")),
        egui::FontId::proportional(10.5),
        theme::TEXT,
    );
}

/// Aurum Tune, laid out like a modern vocal tuner: the detected note large with its cents offset, a
/// needle meter (input deviation in white, the applied correction as a coloured arc), a scrolling pitch
/// trace over the note lanes (input white, corrected output coloured) and the key's notes underneath.
fn tune(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    use dragon_engine::effects::pitch::{KEYS, TUNE_SCALES};
    super::fx_mod::screen(p, r, 8.0);
    let key = h.v("key").round() as usize % 12;
    let sc = TUNE_SCALES.get(h.v("scale").round() as usize).map(|s| s.1).unwrap_or(TUNE_SCALES[0].1);
    let in_scale = |n: usize| sc.contains(&(((n + 12 - key) % 12) as u8));
    // Live values (or a gentle demo wobble in the gallery).
    let (det, target, out) = match h.tele {
        [d, t, o, _, ..] if *d >= 0.0 => (Some(*d), *t, *o),
        [_, ..] => (None, -1.0, -1.0),
        [] => {
            let d = 57.0 + 0.35 * (h.t() * 1.7).sin() + 0.1 * (h.t() * 6.0).sin();
            let t = dragon_engine::effects::pitch::nearest_in_scale(d, key as u8, sc);
            (Some(d), t, t + (d - t) * 0.15)
        }
    };
    // History of (input, output) for the trace, per insert.
    let hist_id = h.key.with("tune.hist");
    let mut hist: std::collections::VecDeque<(f32, f32)> = p.ctx().data(|d| d.get_temp(hist_id)).unwrap_or_default();
    if h.time.is_some() {
        hist.push_back(det.map(|d| (d, out)).unwrap_or((-1.0, -1.0)));
        while hist.len() > 240 {
            hist.pop_front();
        }
        p.ctx().data_mut(|d| d.insert_temp(hist_id, hist.clone()));
    }
    // ---- left: note + needle meter
    let left = Rect::from_min_max(r.min + Vec2::new(10.0, 10.0), Pos2::new(r.left() + r.width() * 0.36, r.bottom() - 46.0));
    let (name, cents) = match det {
        Some(d) => {
            let n = d.round();
            (format!("{}{}", KEYS[(n as i32).rem_euclid(12) as usize], (n as i32) / 12 - 1), (d - n) * 100.0)
        }
        None => ("–".into(), 0.0),
    };
    p.text(Pos2::new(left.center().x, left.top()), egui::Align2::CENTER_TOP, &name, egui::FontId::proportional(34.0), if det.is_some() { Color32::WHITE } else { theme::TEXT_DIM });
    p.text(Pos2::new(left.center().x, left.top() + 40.0), egui::Align2::CENTER_TOP, if det.is_some() { format!("{cents:+.0} ct") } else { "no pitch".into() }, egui::FontId::monospace(12.0), theme::TEXT_DIM);
    let c = Pos2::new(left.center().x, left.bottom() - 4.0);
    let rad = (left.width() * 0.45).min(left.height() - 66.0).max(30.0);
    let ang = |ct: f32| std::f32::consts::PI * (1.0 + (ct.clamp(-50.0, 50.0) + 50.0) / 100.0);
    let arc = |a0: f32, a1: f32, rr: f32| -> Vec<Pos2> { (0..=40).map(|k| a0 + (a1 - a0) * k as f32 / 40.0).map(|a| Pos2::new(c.x + rr * a.cos(), c.y + rr * a.sin())).collect() };
    p.add(Shape::line(arc(ang(-50.0), ang(50.0), rad), Stroke::new(6.0_f32, Color32::from_white_alpha(14))));
    for ct in [-50.0f32, -25.0, 0.0, 25.0, 50.0] {
        let a = ang(ct);
        p.line_segment([Pos2::new(c.x + (rad - 9.0) * a.cos(), c.y + (rad - 9.0) * a.sin()), Pos2::new(c.x + (rad + 4.0) * a.cos(), c.y + (rad + 4.0) * a.sin())], Stroke::new(1.0_f32, Color32::from_white_alpha(70)));
    }
    if let Some(d) = det {
        // Correction applied: arc from the input deviation to where the output lands.
        let in_ct = (d - target) * 100.0;
        let out_ct = (out - target) * 100.0;
        p.add(Shape::line(arc(ang(in_ct.min(out_ct)), ang(in_ct.max(out_ct)), rad), Stroke::new(6.0_f32, col)));
        let a = ang(in_ct);
        p.line_segment([c, Pos2::new(c.x + (rad - 4.0) * a.cos(), c.y + (rad - 4.0) * a.sin())], Stroke::new(2.0_f32, Color32::WHITE));
        p.circle_filled(c, 4.0, Color32::WHITE);
    }
    p.text(Pos2::new(c.x - rad, c.y + 2.0), egui::Align2::CENTER_TOP, "−50", egui::FontId::proportional(8.5), theme::TEXT_DIM);
    p.text(Pos2::new(c.x + rad, c.y + 2.0), egui::Align2::CENTER_TOP, "+50", egui::FontId::proportional(8.5), theme::TEXT_DIM);
    // ---- right: pitch trace over note lanes
    let tr = Rect::from_min_max(Pos2::new(left.right() + 16.0, r.top() + 10.0), Pos2::new(r.right() - 10.0, r.bottom() - 46.0));
    super::fx_mod::inset(p, tr, 4.0);
    let mid = det.or_else(|| hist.iter().rev().find(|x| x.0 >= 0.0).map(|x| x.0)).unwrap_or(60.0).round();
    let lo = mid - 6.5;
    let y_of = |m: f32| tr.bottom() - ((m - lo) / 13.0) * tr.height();
    for n in (lo.ceil() as i32)..=((lo + 13.0) as i32) {
        let y = y_of(n as f32);
        let ok = in_scale(n.rem_euclid(12) as usize);
        p.line_segment([Pos2::new(tr.left(), y), Pos2::new(tr.right(), y)], Stroke::new(1.0_f32, if ok { col.gamma_multiply(0.25) } else { Color32::from_white_alpha(8) }));
        if ok {
            p.text(Pos2::new(tr.left() + 3.0, y), egui::Align2::LEFT_CENTER, KEYS[n.rem_euclid(12) as usize], egui::FontId::proportional(8.5), col.gamma_multiply(0.7));
        }
    }
    let n = hist.len().max(1);
    let px = |i: usize| tr.right() - (n - 1 - i) as f32 * (tr.width() / 240.0);
    let p2 = p.with_clip_rect(tr);
    for (which, color, w) in [(0usize, Color32::from_white_alpha(150), 1.2_f32), (1, col, 2.0)] {
        let mut seg: Vec<Pos2> = Vec::new();
        for (i, v) in hist.iter().enumerate() {
            let m = if which == 0 { v.0 } else { v.1 };
            if m < 0.0 {
                if seg.len() > 1 {
                    p2.add(Shape::line(std::mem::take(&mut seg), Stroke::new(w, color)));
                }
                seg.clear();
                continue;
            }
            seg.push(Pos2::new(px(i), y_of(m)));
        }
        if seg.len() > 1 {
            p2.add(Shape::line(seg, Stroke::new(w, color)));
        }
    }
    // ---- bottom: the key's notes, the target lit
    let kr = Rect::from_min_max(Pos2::new(r.left() + 10.0, r.bottom() - 38.0), Pos2::new(r.right() - 10.0, r.bottom() - 8.0));
    let kw = kr.width() / 12.0;
    let tgt = if target >= 0.0 { Some((target.round() as i32).rem_euclid(12) as usize) } else { None };
    // Heat map: every detected note warms its key; the heat fades over a few seconds.
    let heat_id = h.key.with("tune.heat");
    let mut heat: [f32; 12] = p.ctx().data(|d| d.get_temp(heat_id)).unwrap_or([0.0; 12]);
    if h.time.is_some() {
        for v in heat.iter_mut() {
            *v *= 0.985;
        }
        if let Some(d) = det {
            let pc = (d.round() as i32).rem_euclid(12) as usize;
            heat[pc] = (heat[pc] + 0.06).min(1.0);
        }
        p.ctx().data_mut(|d| d.insert_temp(heat_id, heat));
    }
    for (i, nm) in KEYS.iter().enumerate() {
        let b = Rect::from_min_size(Pos2::new(kr.left() + i as f32 * kw + 1.0, kr.top()), Vec2::new(kw - 2.0, kr.height()));
        let on = in_scale(i);
        let lit = tgt == Some(i);
        // Heat glow (orange) under the key.
        if heat[i] > 0.02 {
            let hc = Color32::from_rgb(0xf0, 0x8a, 0x4b).gamma_multiply(heat[i].sqrt() * 0.9);
            p.rect_filled(b.expand(2.0), 6.0, hc);
        }
        p.rect_filled(
            b,
            5.0,
            if lit {
                col
            } else if on {
                col.gamma_multiply(0.22)
            } else {
                Color32::from_white_alpha(6)
            },
        );
        p.text(
            b.center(),
            egui::Align2::CENTER_CENTER,
            *nm,
            egui::FontId::proportional(10.0),
            if lit {
                Color32::BLACK
            } else if on {
                Color32::WHITE
            } else {
                theme::TEXT_DIM
            },
        );
    }
    p.text(
        r.right_top() + Vec2::new(-12.0, 8.0),
        egui::Align2::RIGHT_TOP,
        format!("{} {} · retune {:.0} ms", KEYS[key], TUNE_SCALES.get(h.v("scale").round() as usize).map(|s| s.0).unwrap_or(""), h.v("speed")),
        egui::FontId::proportional(10.0),
        theme::TEXT_DIM,
    );
}

// ---------------------------------------------------------------------------------------------- orbit

/// Ouroboros's display: (stereo field, the seven shape buttons, the three mode buttons).
fn orbit_layout(r: Rect) -> (Rect, Vec<Rect>, Vec<Rect>) {
    let bar = 30.0;
    let field = Rect::from_min_max(r.min + Vec2::new(10.0, 8.0), Pos2::new(r.right() - 10.0, r.bottom() - bar - 10.0));
    let y = r.bottom() - bar - 4.0;
    let shapes = (0..7).map(|k| Rect::from_min_size(Pos2::new(r.left() + 10.0 + k as f32 * 38.0, y), Vec2::new(34.0, bar - 4.0))).collect();
    let modes = (0..3).rev().enumerate().map(|(j, k)| (k, Rect::from_min_size(Pos2::new(r.right() - 10.0 - (j as f32 + 1.0) * 62.0, y), Vec2::new(58.0, bar - 4.0)))).fold(vec![Rect::NOTHING; 3], |mut v, (k, b)| {
        v[k] = b;
        v
    });
    (field, shapes, modes)
}

fn orbit(p: &egui::Painter, r: Rect, h: &Hero, _col: Color32) {
    use super::fx_mod::{art, glow, halo, stars, window, CREAM, OUROBOROS};
    use dragon_engine::effects::panner::{shape_value, ORBIT_MODES};
    let amber = Color32::from_rgb(0xff, 0x9a, 0x3c);
    let col = if h.enabled { amber } else { theme::TEXT_DIM };
    p.rect_filled(r, 6.0, Color32::from_rgb(0x14, 0x0c, 0x0c));
    let (field, shapes, modes) = orbit_layout(r);
    window(p, field);
    let (width, center) = (h.v("width") / 100.0, h.v("center") / 100.0);
    let shape = h.v("shape").round() as usize;
    let mode = h.v("mode").round() as usize;
    // The listener sits in the middle of the serpent's ring; positions -1 … 1 map to its upper half.
    let ring = (field.height() * 0.5 - 6.0).min(field.width() * 0.46).max(20.0);
    let ear = field.center();
    let rad = ring * 0.74;
    let at = |pos: f32, rr: f32| {
        let a = PI + (pos.clamp(-1.0, 1.0) + 1.0) * 0.5 * PI;
        Pos2::new(ear.x + rr * a.cos(), ear.y + rr * a.sin())
    };
    // The ouroboros, slowly circling.
    let fp = p.with_clip_rect(field.shrink(2.0));
    stars(&fp, field.shrink(4.0), h.t(), 90);
    if let Some(t) = art(p.ctx(), "fx-ouroboros", OUROBOROS) {
        halo(&fp, ear, ring * 1.35, amber, 0.16);
        super::analog::rotated(&fp, &t, ear, ring, h.t() * 0.12, Color32::WHITE);
    }
    for k in 1..=3 {
        let rr = rad * k as f32 / 3.0;
        let pts: Vec<Pos2> = (0..=48).map(|j| at(-1.0 + 2.0 * j as f32 / 48.0, rr)).collect();
        fp.add(Shape::line(pts, Stroke::new(1.0_f32, CREAM.gamma_multiply(if k == 3 { 0.35 } else { 0.14 }))));
    }
    fp.line_segment([at(-1.0, rad), at(1.0, rad)], Stroke::new(1.0_f32, CREAM.gamma_multiply(0.14)));
    for (pos, t) in [(-1.0, "L"), (0.0, "C"), (1.0, "R")] {
        fp.text(at(pos, rad + 9.0), egui::Align2::CENTER_CENTER, t, egui::FontId::proportional(10.0), CREAM.gamma_multiply(0.8));
    }
    // The range the motion covers (a wedge of lamplight).
    let (lo, hi) = ((center - width).max(-1.0), (center + width).min(1.0));
    let mut wedge = vec![ear];
    wedge.extend((0..=32).map(|j| at(lo + (hi - lo) * j as f32 / 32.0, rad)));
    fp.add(Shape::convex_polygon(wedge, glow(col, 0.12), Stroke::NONE));
    // The shape's path over one cycle, as a trail of sparks.
    for j in 0..40 {
        let ph = j as f32 / 40.0;
        let v = (center + width * shape_value(shape, ph, 7)).clamp(-1.0, 1.0);
        fp.circle_filled(at(v, rad * (0.3 + 0.62 * ph)), 1.6, col.gamma_multiply(0.55));
    }
    // The sound: live position, or a demonstration of the motion when there is no audio.
    let (pos, flash) = match h.tele {
        [a, b, ..] => (*a, *b),
        _ => ((center + width * shape_value(shape, h.t() * 0.5, (h.t() * 0.5) as i64)).clamp(-1.0, 1.0), 0.0),
    };
    let ball = at(pos, rad * 0.8);
    fp.line_segment([ear, ball], Stroke::new(1.0_f32, col.gamma_multiply(0.35)));
    if flash > 0.02 {
        fp.circle_stroke(ball, 10.0 + 14.0 * (1.0 - flash), Stroke::new(2.0_f32, col.gamma_multiply(flash)));
    }
    // An ember: its glow, then the hot core.
    halo(&fp, ball, 26.0, col, 0.6);
    fp.circle_filled(ball, 6.0, col);
    fp.circle_filled(ball, 3.0, Color32::from_rgb(0xff, 0xe6, 0xb0));
    fp.circle_filled(ear, 5.0, Color32::from_rgb(0xb8, 0x96, 0x5a));
    fp.circle_stroke(ear, 5.0, Stroke::new(1.0_f32, Color32::from_rgb(0x5a, 0x44, 0x22)));
    p.text(field.left_top() + Vec2::new(8.0, 6.0), egui::Align2::LEFT_TOP, format!("CENTRE {:+.0}   WIDTH {:.0} %", center * 100.0, width * 100.0), egui::FontId::proportional(9.5), CREAM.gamma_multiply(0.8));
    // Hardware buttons: black caps with an amber lamp when chosen.
    let button = |b: Rect, on: bool| {
        p.rect_filled(b.translate(Vec2::new(0.0, 1.5)), 4.0, Color32::from_black_alpha(140));
        p.rect_filled(b, 4.0, Color32::from_rgb(0x1a, 0x17, 0x16));
        p.rect_stroke(b, 4.0, Stroke::new(1.0_f32, if on { col } else { Color32::from_white_alpha(28) }), egui::StrokeKind::Inside);
        if on {
            p.rect_filled(b.shrink(1.0), 3.0, glow(col, 0.16));
        }
    };
    for (k, b) in shapes.iter().enumerate() {
        let on = k == shape;
        button(*b, on);
        let g = b.shrink2(Vec2::new(5.0, 6.0));
        let pts: Vec<Pos2> = (0..=24)
            .map(|j| {
                let ph = j as f32 / 24.0;
                // Random shapes: show four cycles' worth of steps.
                let v = if k >= 5 { shape_value(k, (ph * 4.0).fract(), (ph * 4.0) as i64 + 3) } else { shape_value(k, ph, 0) };
                Pos2::new(g.left() + g.width() * ph, g.center().y - v * g.height() * 0.5)
            })
            .collect();
        p.add(Shape::line(pts, Stroke::new(1.5_f32, if on { Color32::from_rgb(0xff, 0xd8, 0x9a) } else { CREAM.gamma_multiply(0.55) })));
    }
    for (k, b) in modes.iter().enumerate() {
        let on = k == mode;
        button(*b, on);
        p.text(b.center(), egui::Align2::CENTER_CENTER, ORBIT_MODES[k].to_uppercase(), egui::FontId::proportional(10.0), if on { Color32::from_rgb(0xff, 0xd8, 0x9a) } else { CREAM.gamma_multiply(0.6) });
    }
}

/// The strip's noise-suppressor column: (fader track, whole section).
fn ns_fader(r: Rect) -> (Rect, Rect) {
    let sec = Rect::from_min_size(Pos2::new(r.left() + 10.0, r.top() + 10.0), Vec2::new(56.0, r.height() - 20.0));
    let track = Rect::from_center_size(Pos2::new(sec.center().x, sec.center().y + 4.0), Vec2::new(6.0, sec.height() - 64.0));
    (track, sec)
}

/// Class-A Strip: the one-fader noise suppressor, the preamp's drive with the transformer warming up, the EQ curve as set, and the opto
/// compressor's cell glowing with its gain reduction above a moving-coil GR meter.
fn class_a_strip(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    use dragon_engine::dsp::{Biquad, BiquadKind};
    super::fx_mod::screen(p, r, 8.0);
    p.rect_stroke(r.shrink(3.0), 6.0, Stroke::new(1.0_f32, Color32::from_white_alpha(30)), egui::StrokeKind::Inside);
    // ---- noise suppressor (one fader)
    let (track, nsr) = ns_fader(r);
    p.rect_filled(nsr, 6.0, Color32::from_black_alpha(120));
    p.text(Pos2::new(nsr.center().x, nsr.top() + 6.0), egui::Align2::CENTER_TOP, "NS", egui::FontId::proportional(11.0), Color32::from_rgb(0xff, 0xd2, 0x7a));
    let ns = h.v("ns");
    let red = h.tele.get(4).copied().unwrap_or(0.0);
    // Lamp: lights with the suppression happening right now.
    let lamp = Pos2::new(nsr.center().x, nsr.top() + 28.0);
    let lit = (red / 20.0).clamp(0.0, 1.0);
    p.circle_filled(lamp, 7.0, Color32::from_rgb(0xff, 0xd2, 0x7a).gamma_multiply(0.12 * lit));
    p.circle_filled(lamp, 4.0, Color32::from_rgb(0xff, 0xd2, 0x7a).gamma_multiply(0.2 + 0.8 * lit));
    p.rect_filled(track, 3.0, Color32::from_rgb(0x14, 0x17, 0x1c));
    let fill = Rect::from_min_max(Pos2::new(track.left(), track.bottom() - track.height() * ns / 100.0), track.max);
    p.rect_filled(fill, 3.0, Color32::from_rgb(0xff, 0xd2, 0x7a).gamma_multiply(0.7));
    for k in 0..=10 {
        let y = track.bottom() - track.height() * k as f32 / 10.0;
        let w = if k % 5 == 0 { 9.0 } else { 5.0 };
        p.line_segment([Pos2::new(track.right() + 3.0, y), Pos2::new(track.right() + 3.0 + w, y)], Stroke::new(1.0_f32, Color32::from_white_alpha(70)));
    }
    let cap = Rect::from_center_size(Pos2::new(track.center().x, track.bottom() - track.height() * ns / 100.0), Vec2::new(30.0, 13.0));
    p.rect_filled(cap, 3.0, Color32::from_gray(205));
    p.line_segment([Pos2::new(cap.left() + 3.0, cap.center().y), Pos2::new(cap.right() - 3.0, cap.center().y)], Stroke::new(1.5_f32, Color32::from_gray(40)));
    p.text(Pos2::new(nsr.center().x, nsr.bottom() - 6.0), egui::Align2::CENTER_BOTTOM, format!("{ns:.0} %"), egui::FontId::monospace(10.0), theme::TEXT);
    let r = Rect::from_min_max(Pos2::new(nsr.right(), r.top()), r.max);
    // Live values only: at rest (no audio) the meters rest too.
    let (pin, gr, pout) = match h.tele {
        [a, b, c, ..] => (*a, *b, *c),
        _ => (0.0, 0.0, 0.0),
    };
    let w3 = (r.width() - 40.0) / 3.0;
    let sec = |k: usize| Rect::from_min_size(Pos2::new(r.left() + 10.0 + k as f32 * (w3 + 10.0), r.top() + 10.0), Vec2::new(w3, r.height() - 20.0));
    // ---- preamp
    let pr = sec(0);
    p.rect_filled(pr, 6.0, Color32::from_black_alpha(120));
    p.text(pr.left_top() + Vec2::new(8.0, 6.0), egui::Align2::LEFT_TOP, "PREAMP", egui::FontId::proportional(10.0), Color32::from_rgb(0xe2, 0xc0, 0x76));
    let drive = h.v("drive");
    let c = Pos2::new(pr.center().x, pr.center().y + 4.0);
    let rad = (pr.width().min(pr.height()) * 0.32).max(20.0);
    let a0 = std::f32::consts::PI * 0.75;
    let sweep = std::f32::consts::PI * 1.5;
    let arc = |from: f32, to: f32, rr: f32| -> Vec<Pos2> { (0..=40).map(|k| from + (to - from) * k as f32 / 40.0).map(|x| Pos2::new(c.x + rr * x.cos(), c.y + rr * x.sin())).collect() };
    for k in 0..=10 {
        let a = a0 + sweep * k as f32 / 10.0;
        p.line_segment([Pos2::new(c.x + (rad + 4.0) * a.cos(), c.y + (rad + 4.0) * a.sin()), Pos2::new(c.x + (rad + 9.0) * a.cos(), c.y + (rad + 9.0) * a.sin())], Stroke::new(1.0_f32, Color32::from_white_alpha(120)));
    }
    let nd = drive / 30.0;
    p.add(Shape::line(arc(a0, a0 + sweep * nd, rad + 1.0), Stroke::new(3.0_f32, Color32::from_rgb(0xe2, 0xc0, 0x76))));
    p.circle_filled(c, rad - 4.0, Color32::from_rgb(0x2e, 0x24, 0x16));
    p.circle_stroke(c, rad - 4.0, Stroke::new(2.0_f32, Color32::from_rgb(0x8c, 0x6c, 0x30)));
    let a = a0 + sweep * nd;
    p.line_segment([c, Pos2::new(c.x + (rad - 6.0) * a.cos(), c.y + (rad - 6.0) * a.sin())], Stroke::new(2.5_f32, Color32::WHITE));
    // Transformer glow with level.
    let heat = (gain_to_db_ui(pin) + 30.0).clamp(0.0, 30.0) / 30.0 * (0.3 + nd);
    p.circle_filled(Pos2::new(pr.right() - 14.0, pr.top() + 12.0), 5.0, Color32::from_rgb(0xff, 0xa0, 0x40).gamma_multiply(0.2 + 0.8 * heat.min(1.0)));
    p.text(Pos2::new(pr.center().x, pr.bottom() - 6.0), egui::Align2::CENTER_BOTTOM, format!("drive {drive:+.0} dB"), egui::FontId::proportional(10.0), theme::TEXT);
    // ---- EQ curve
    let er = sec(1);
    p.rect_filled(er, 6.0, Color32::from_black_alpha(120));
    p.text(er.left_top() + Vec2::new(8.0, 6.0), egui::Align2::LEFT_TOP, if h.v("eq_on") > 0.5 { "EQ" } else { "EQ (out)" }, egui::FontId::proportional(10.0), Color32::from_rgb(0xe2, 0xc0, 0x76));
    let plot_r = er.shrink2(Vec2::new(8.0, 20.0)).translate(Vec2::new(0.0, 6.0));
    let rate = 48000.0;
    let hpf = [0.0f32, 50.0, 80.0, 160.0, 300.0][(h.v("hpf").round() as usize).min(4)];
    let lowf = [35.0f32, 60.0, 110.0, 220.0][(h.v("low_freq").round() as usize).min(3)];
    let midf = [360.0f32, 700.0, 1600.0, 3200.0, 4800.0, 7200.0][(h.v("mid_freq").round() as usize).min(5)];
    let mg = h.v("mid_gain");
    let mut fs = vec![
        Biquad::new(BiquadKind::LowShelf, lowf, 0.55, h.v("low_gain"), rate),
        Biquad::new(BiquadKind::Peak, midf, 1.0 + mg.abs() / 18.0 * 0.6, mg, rate),
        Biquad::new(BiquadKind::HighShelf, 12000.0, 0.6, h.v("high_gain"), rate),
    ];
    if hpf > 0.0 {
        fs.push(Biquad::new(BiquadKind::HighPass, hpf, 0.71, 0.0, rate));
        fs.push(Biquad::new(BiquadKind::HighPass, hpf * 0.8, 0.5, 0.0, rate));
    }
    let y_of = |db: f32| plot_r.center().y - db.clamp(-20.0, 20.0) / 20.0 * plot_r.height() * 0.5;
    p.line_segment([Pos2::new(plot_r.left(), plot_r.center().y), Pos2::new(plot_r.right(), plot_r.center().y)], Stroke::new(1.0_f32, Color32::from_white_alpha(30)));
    let pts: Vec<Pos2> = (0..=100).map(|k| plot_r.left() + plot_r.width() * k as f32 / 100.0).map(|x| Pos2::new(x, y_of(fs.iter().map(|b| b.response_db(freq_at(plot_r, x), rate)).sum()))).collect();
    filled_curve(p, &pts, plot_r.center().y, Color32::from_rgb(0xe2, 0xc0, 0x76));
    // ---- opto compressor
    let cr = sec(2);
    p.rect_filled(cr, 6.0, Color32::from_black_alpha(120));
    p.text(cr.left_top() + Vec2::new(8.0, 6.0), egui::Align2::LEFT_TOP, if h.v("comp_on") > 0.5 { "OPTO COMP" } else { "OPTO COMP (out)" }, egui::FontId::proportional(10.0), col);
    // The cell's lamp brightens with gain reduction.
    let lamp = Pos2::new(cr.right() - 18.0, cr.top() + 14.0);
    let gl = (gr / 12.0).clamp(0.0, 1.0);
    for k in (1..=4).rev() {
        p.circle_filled(lamp, 4.0 + k as f32 * 3.0, Color32::from_rgb(0xff, 0xe0, 0x90).gamma_multiply(0.08 * gl));
    }
    p.circle_filled(lamp, 4.5, Color32::from_rgb(0xff, 0xe0, 0x90).gamma_multiply(0.25 + 0.75 * gl));
    let vu = Rect::from_min_max(Pos2::new(cr.left() + 10.0, cr.top() + 24.0), Pos2::new(cr.right() - 10.0, cr.bottom() - 22.0));
    let (u, _) = super::analog::spring(p.ctx(), h.key.with("golem.gr"), 1.0 - (gr / 20.0).clamp(0.0, 1.0), 1.8, 0.75, h.time.is_some());
    super::fx_mod::gauge(p, vu, super::fx_mod::GaugeArt::Basalt, u, None, &[(0.0, "20"), (0.5, "10"), (0.7, "6"), (0.85, "3"), (1.0, "0")], 2.0, "dB GR", &format!("{gr:.1}"));
    p.text(Pos2::new(cr.center().x, cr.bottom() - 6.0), egui::Align2::CENTER_BOTTOM, format!("GR {gr:.1} dB · {:.1}:1 · out {:.0} dB", h.v("ratio"), gain_to_db_ui(pout)), egui::FontId::proportional(10.0), theme::TEXT);
}

fn gain_to_db_ui(x: f32) -> f32 {
    dragon_engine::dsp::gain_to_db(x.max(1e-6))
}

/// Drum Bus: a pad that flashes with the punch, in / out meters, the smack compressor's gain reduction and
/// the macro settings as rings around the pad.
fn drum_bus(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    use dragon_engine::effects::strips::{DRUM_COLOURS, HEQ_FREQS};
    super::fx_mod::screen(p, r, 8.0);
    // Live values only: at rest (no audio) the meters rest too.
    let (pin, pout, gr, rin, rout) = match h.tele {
        [a, b, c, d, e, ..] => (*a, *b, *c, *d, *e),
        _ => (0.0, 0.0, 0.0, 0.0, 0.0),
    };
    // Gain-staging meters: peak bar, RMS line and a target zone (−12 … −8 dB RMS) for gain staging.
    // In and out as phials of molten metal: the peak is the liquid, the RMS a gold line, the target
    // zone (−12 … −8 dB RMS) marked on the glass.
    let glass = super::fx_mod::art(p.ctx(), "fx-glass-tube", GLASS_TUBE);
    let meter = |x: f32, peak: f32, rms: f32, label: &str| {
        let mr = Rect::from_min_max(Pos2::new(x - 11.0, r.top() + 18.0), Pos2::new(x + 11.0, r.bottom() - 20.0));
        let bore = Rect::from_min_max(Pos2::new(mr.left() + 5.0, mr.top() + mr.height() * 0.09), Pos2::new(mr.right() - 5.0, mr.bottom() - mr.height() * 0.08));
        let y = |db: f32| bore.bottom() - ((db + 36.0) / 36.0).clamp(0.0, 1.0) * bore.height();
        let pk = gain_to_db_ui(peak);
        let (lvl, _) = super::analog::spring(p.ctx(), h.key.with(("forge.m", label)), pk.max(-36.0), 3.0, 0.9, h.time.is_some());
        if lvl > -35.9 {
            let top = y(lvl);
            let hot = if pk > -0.5 { Color32::from_rgb(0xff, 0x5a, 0x3a) } else { Color32::from_rgb(0xff, 0x8a, 0x2c) };
            p.rect_filled(Rect::from_min_max(Pos2::new(bore.left(), top), bore.max), 2.0, hot);
            p.rect_filled(Rect::from_min_max(Pos2::new(bore.left(), top), Pos2::new(bore.center().x - 1.0, bore.bottom())), 2.0, Color32::from_rgba_unmultiplied(0xff, 0xd8, 0x90, 70));
            super::fx_mod::halo(p, Pos2::new(x, top), 16.0, hot, 0.45);
        }
        p.rect_filled(Rect::from_min_max(Pos2::new(bore.right() + 1.0, y(-8.0)), Pos2::new(bore.right() + 4.0, y(-12.0))), 0.0, theme::OK.gamma_multiply(0.7));
        if let Some(t) = &glass {
            p.image(t.id(), mr, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        let rm = gain_to_db_ui(rms);
        if rm > -36.0 {
            p.line_segment([Pos2::new(bore.left() - 3.0, y(rm)), Pos2::new(bore.right() + 3.0, y(rm))], Stroke::new(2.0_f32, Color32::from_rgb(0xe2, 0xc0, 0x76)));
        }
        p.text(Pos2::new(x, mr.top() - 3.0), egui::Align2::CENTER_BOTTOM, label, egui::FontId::proportional(9.0), Color32::from_rgb(0xe2, 0xc0, 0x76));
        p.text(
            Pos2::new(x, mr.bottom() + 3.0),
            egui::Align2::CENTER_TOP,
            if rm <= -90.0 { "-".to_string() } else { format!("{rm:.0}") },
            egui::FontId::monospace(8.5),
            if (-12.0..=-8.0).contains(&rm) { theme::OK } else { theme::TEXT_DIM },
        );
    };
    meter(r.left() + 22.0, pin, rin, "IN");
    meter(r.right() - 22.0, pout, rout, "OUT");
    // Modules as tiles: Drive, Harmonics, Compression, Lift; then Harmonic EQ and Parallels as bars.
    let body = Rect::from_min_max(Pos2::new(r.left() + 46.0, r.top() + 8.0), Pos2::new(r.right() - 46.0, r.bottom() - 8.0));
    let top = Rect::from_min_max(body.min, Pos2::new(body.right(), body.top() + body.height() * 0.55));
    let tiles = [
        ("DRIVE", h.v("drive"), DRUM_COLOURS.get(h.v("colour").round() as usize).copied().unwrap_or("A").split(' ').next().unwrap_or("A").to_string()),
        ("HARMONICS", h.v("harmonics"), if h.v("focus") > 0.5 { "80 Hz".into() } else { "50 Hz".into() }),
        ("COMP", h.v("comp_mix"), format!("{} · {gr:.1} dB", if h.v("comp_mode") > 0.5 { "B" } else { "A" })),
        ("LIFT", h.v("lift"), String::new()),
    ];
    let tw = top.width() / 4.0;
    for (k, (name, v, sub)) in tiles.iter().enumerate() {
        let tr = Rect::from_min_size(Pos2::new(top.left() + k as f32 * tw + 3.0, top.top()), Vec2::new(tw - 6.0, top.height()));
        super::fx_mod::inset(p, tr, 6.0);
        let g = Rect::from_min_max(Pos2::new(tr.left() + 4.0, tr.top() + 14.0), Pos2::new(tr.right() - 4.0, tr.bottom() - 12.0));
        // The value's needle; on COMP a red needle shows the real gain reduction (0 … 20 dB).
        let second = if *name == "COMP" { Some(super::analog::spring(p.ctx(), h.key.with("forge.gr"), (gr / 20.0).clamp(0.0, 1.0), 1.8, 0.75, h.time.is_some()).0) } else { None };
        super::fx_mod::gauge(p, g, super::fx_mod::GaugeArt::Forged, v / 100.0, second, &[(0.0, "0"), (0.5, "50"), (1.0, "100")], 0.9, "", &format!("{v:.0}"));
        p.text(Pos2::new(tr.center().x, tr.top() + 3.0), egui::Align2::CENTER_TOP, *name, egui::FontId::proportional(9.0), Color32::from_rgb(0xe2, 0xc0, 0x76));
        if !sub.is_empty() {
            p.text(Pos2::new(tr.center().x, tr.bottom() - 2.0), egui::Align2::CENTER_BOTTOM, sub, egui::FontId::proportional(8.5), theme::TEXT_DIM);
        }
    }
    let bottom = Rect::from_min_max(Pos2::new(body.left(), top.bottom() + 8.0), body.max);
    let half = bottom.width() / 2.0;
    let bars = |x0: f32, title: &str, items: &[(String, f32, f32)]| {
        let area = Rect::from_min_max(Pos2::new(x0 + 3.0, bottom.top()), Pos2::new(x0 + half - 3.0, bottom.bottom()));
        super::fx_mod::inset(p, area, 6.0);
        p.text(Pos2::new(area.left() + 6.0, area.top() + 3.0), egui::Align2::LEFT_TOP, title, egui::FontId::proportional(9.0), theme::TEXT);
        let bw = (area.width() - 12.0) / items.len() as f32;
        for (k, (name, v, max)) in items.iter().enumerate() {
            let br = Rect::from_min_max(Pos2::new(area.left() + 6.0 + k as f32 * bw + 3.0, area.top() + 16.0), Pos2::new(area.left() + 6.0 + (k + 1) as f32 * bw - 3.0, area.bottom() - 14.0));
            super::fx_mod::inset(p, br, 3.0);
            let f = (v / max).clamp(0.0, 1.0);
            p.rect_filled(Rect::from_min_max(Pos2::new(br.left(), br.bottom() - br.height() * f), br.max), 3.0, col.gamma_multiply(0.5 + 0.5 * f));
            p.text(Pos2::new(br.center().x, area.bottom() - 2.0), egui::Align2::CENTER_BOTTOM, name, egui::FontId::proportional(8.0), theme::TEXT_DIM);
        }
    };
    let heq: Vec<(String, f32, f32)> = HEQ_FREQS.iter().enumerate().map(|(k, f)| (if *f >= 1000.0 { format!("{:.1}k", f / 1000.0) } else { format!("{f:.0}") }, h.v(&format!("heq{}", k + 1)), 10.0)).collect();
    bars(bottom.left(), "HARMONIC EQ", &heq);
    let pars: Vec<(String, f32, f32)> = [("SUB", "sub"), ("LOW", "low"), ("LOUD", "loud"), ("GRIT", "grit")].iter().map(|(n, id)| (n.to_string(), h.v(id), 100.0)).collect();
    bars(bottom.left() + half, "PARALLELS", &pars);
}

/// Alter: the pitch and formant shifts as two big vertical sliders around a voice "face" whose mouth (the
/// spectral envelope) moves with the formant and whose wave moves with the pitch; robot mode shows its note.
fn alter(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    super::fx_mod::screen(p, r, 10.0);
    let t = h.t();
    let pitch = h.v("pitch");
    let formant = h.v("formant");
    let mode = h.v("mode").round() as usize;
    let robot = mode == 2;
    let link = h.v("link") > 0.5;
    let slider = |x: f32, v: f32, label: &str, c: Color32| {
        let sr = Rect::from_min_max(Pos2::new(x - 10.0, r.top() + 26.0), Pos2::new(x + 10.0, r.bottom() - 24.0));
        p.rect_filled(sr, 10.0, Color32::from_white_alpha(10));
        let y = sr.center().y - v / 12.0 * sr.height() * 0.5;
        p.line_segment([Pos2::new(sr.left() - 4.0, sr.center().y), Pos2::new(sr.right() + 4.0, sr.center().y)], Stroke::new(1.0_f32, Color32::from_white_alpha(60)));
        p.rect_filled(Rect::from_min_max(Pos2::new(sr.left(), y.min(sr.center().y)), Pos2::new(sr.right(), y.max(sr.center().y))), 6.0, c.gamma_multiply(0.6));
        p.circle_filled(Pos2::new(x, y), 9.0, c);
        p.text(Pos2::new(x, sr.top() - 6.0), egui::Align2::CENTER_BOTTOM, label, egui::FontId::proportional(9.5), theme::TEXT_DIM);
        p.text(Pos2::new(x, sr.bottom() + 4.0), egui::Align2::CENTER_TOP, format!("{v:+.1}"), egui::FontId::monospace(10.0), theme::TEXT);
    };
    slider(r.left() + 34.0, pitch, "PITCH", col);
    slider(r.right() - 34.0, formant + if link { pitch } else { 0.0 }, if link { "FORMANT (LINK)" } else { "FORMANT" }, Color32::from_rgb(0x5d, 0xe0, 0xa8));
    // The "voice": a spectral envelope (mouth) shifted by the formant, a wave whose period follows the pitch.
    let face = Rect::from_min_max(Pos2::new(r.left() + 70.0, r.top() + 14.0), Pos2::new(r.right() - 70.0, r.bottom() - 14.0));
    super::fx_mod::inset(p, face, 8.0);
    let fshift = 2f32.powf((formant + if link { pitch } else { 0.0 }) / 12.0);
    let env = |f: f32| -> f32 { (1.0 / (1.0 + ((f / fshift - 700.0) / 260.0).powi(2))) + 0.6 / (1.0 + ((f / fshift - 1700.0) / 380.0).powi(2)) + 0.3 / (1.0 + ((f / fshift - 2800.0) / 500.0).powi(2)) };
    let spec = Rect::from_min_max(Pos2::new(face.left() + 10.0, face.center().y), face.max - Vec2::new(10.0, 10.0));
    let pts: Vec<Pos2> = (0..=120).map(|k| k as f32 / 120.0).map(|u| Pos2::new(spec.left() + u * spec.width(), spec.bottom() - env(100.0 + u * 4400.0).min(1.3) / 1.3 * spec.height())).collect();
    filled_curve(p, &pts, spec.bottom(), Color32::from_rgb(0x5d, 0xe0, 0xa8));
    let robot_midi = dragon_engine::effects::alter::ROBOT_BASE + pitch.round();
    let base_hz = if robot { 440.0 * 2f32.powf((robot_midi - 69.0) / 12.0) } else { 150.0 * 2f32.powf(pitch / 12.0) };
    let wave = Rect::from_min_max(face.min + Vec2::new(10.0, 10.0), Pos2::new(face.right() - 10.0, face.center().y - 6.0));
    let cycles = (base_hz / 150.0 * 4.0).clamp(1.0, 16.0);
    let wp: Vec<Pos2> = (0..=200)
        .map(|k| k as f32 / 200.0)
        .map(|u| {
            let ph = u * cycles * std::f32::consts::TAU + t * 3.0;
            let y = ph.sin() * 0.6 + (ph * 2.0).sin() * 0.25 * fshift.min(2.0) + (ph * 3.0).sin() * 0.12;
            Pos2::new(wave.left() + u * wave.width(), wave.center().y - y * wave.height() * 0.4)
        })
        .collect();
    p.add(Shape::line(wp, Stroke::new(2.0_f32, col)));
    let label = match mode {
        2 => format!("ROBOT · {} ({base_hz:.0} Hz)", dragon_engine::instruments::note_name(robot_midi.clamp(0.0, 127.0) as u8)),
        1 => format!("QUANTIZE · {pitch:+.0} st · formant {formant:+.1}{}", if link { " · LINK" } else { "" }),
        _ => format!("TRANSPOSE · {pitch:+.1} st · formant {formant:+.1}{}", if link { " · LINK" } else { "" }),
    };
    p.text(Pos2::new(face.center().x, face.top() + 4.0), egui::Align2::CENTER_TOP, label, egui::FontId::proportional(11.0), theme::TEXT);
    if let [hz, out, ..] = h.tele {
        if *hz > 0.0 {
            p.text(Pos2::new(face.right() - 6.0, face.bottom() - 4.0), egui::Align2::RIGHT_BOTTOM, format!("in {hz:.0} Hz > out {out:.0} Hz"), egui::FontId::monospace(9.5), theme::TEXT_DIM);
        }
    }
}

// ------------------------------------------------------------------------------------------------ multiband

/// The display's height: taller in wide windows (full screen), at most 27 % of the screen so the
/// controls keep their room, and short for the multiband (its band columns need the space).
pub(crate) fn hero_height(id: &str, base: f32, w: f32, avail_h: f32, screen_h: f32) -> f32 {
    let mut height = (base * (w / 520.0).clamp(1.0, 2.2)).min(avail_h.max(base));
    if id == "multiband" {
        height = height.min(250.0);
    }
    // The signal-chain units: the display is an overview; the stages below are where you work.
    if id == "dragon_particle" {
        // The stone is the instrument: give it room.
        height = height.clamp(250.0, 280.0);
    }
    if matches!(id, "class_a_strip" | "drum_bus") {
        height = height.clamp(220.0, 240.0);
    }
    if id == "clean_voice" {
        height = height.clamp(200.0, 220.0);
    }
    height.min((screen_h * 0.27).max(base))
}

/// Where the multiband display draws: the plot, its dB scale (0 dB at the top, −60 dB at the bottom) and
/// each band's frequency span.
struct MbGeom {
    plot: Rect,
    /// (left, right) x of each active band.
    spans: Vec<(f32, f32)>,
    /// x of each active crossover.
    xs: Vec<f32>,
}

impl MbGeom {
    fn new(r: Rect, h: &Hero) -> Self {
        let plot = Rect::from_min_max(Pos2::new(r.left() + 30.0, r.top() + 20.0), Pos2::new(r.right() - 8.0, r.bottom() - 34.0));
        let n = mb_count(h);
        let xs: Vec<f32> = ["x1", "x2", "x3"].iter().take(n - 1).map(|x| fx(plot, h.v(x))).collect();
        let mut edges = vec![plot.left()];
        edges.extend(xs.iter().copied());
        edges.push(plot.right());
        let spans = edges.windows(2).map(|w| (w[0], w[1].max(w[0]))).collect();
        MbGeom { plot, spans, xs }
    }
    fn y(&self, db: f32) -> f32 {
        self.plot.top() + (-db / 60.0).clamp(0.0, 1.0) * self.plot.height()
    }
    fn band_at(&self, x: f32) -> usize {
        self.spans.iter().position(|(_, b)| x < *b).unwrap_or(self.spans.len().saturating_sub(1))
    }
}

fn mb_count(h: &Hero) -> usize {
    (h.v("bands").round() as usize + 1).clamp(1, 4)
}

fn hz_label(f: f32) -> String {
    if f >= 1000.0 {
        let k = f / 1000.0;
        if (k - k.round()).abs() < 0.05 {
            format!("{:.0}k", k)
        } else {
            format!("{k:.1}k")
        }
    } else {
        format!("{f:.0}")
    }
}

/// Multiband dynamics as band columns: one coloured column per band across a log-frequency axis,
/// each with its live level, its threshold line and its gain change over the top; drag a crossover
/// sideways, a band up / down for its threshold.
fn multiband(p: &egui::Painter, r: Rect, h: &Hero, _col: Color32) {
    use super::fx_panel::BAND_COLORS;
    super::fx_mod::screen(p, r, 6.0);
    let g = MbGeom::new(r, h);
    let small = egui::FontId::proportional(9.0);
    let grid = Stroke::new(1.0_f32, Color32::from_white_alpha(12));
    for db in [-12.0f32, -24.0, -36.0, -48.0] {
        let y = g.y(db);
        p.line_segment([Pos2::new(g.plot.left(), y), Pos2::new(g.plot.right(), y)], grid);
        p.text(Pos2::new(g.plot.left() - 4.0, y), egui::Align2::RIGHT_CENTER, format!("{db:.0}"), small.clone(), theme::TEXT_DIM);
    }
    p.text(Pos2::new(g.plot.left() - 4.0, g.plot.top()), egui::Align2::RIGHT_CENTER, "0", small.clone(), theme::TEXT_DIM);
    for f in [50.0f32, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0] {
        let x = fx(g.plot, f);
        p.line_segment([Pos2::new(x, g.plot.top()), Pos2::new(x, g.plot.bottom())], grid);
        p.text(Pos2::new(x + 3.0, g.plot.bottom() - 2.0), egui::Align2::LEFT_BOTTOM, hz_label(f), small.clone(), Color32::from_white_alpha(60));
    }
    let live = h.tele.len() >= 8;
    let t = h.t();
    let soloed = (1..=g.spans.len()).any(|n| h.v(&format!("b{n}_solo")) > 0.5);
    for (k, &(a, b)) in g.spans.iter().enumerate() {
        let n = k + 1;
        let c = BAND_COLORS[k % BAND_COLORS.len()];
        let off = h.v(&format!("b{n}_bypass")) > 0.5 || (soloed && h.v(&format!("b{n}_solo")) < 0.5) || !h.enabled;
        let c = if off { Color32::from_gray(90) } else { c };
        let col_r = Rect::from_x_y_ranges(a + 1.0..=b - 1.0, g.plot.y_range());
        p.rect_filled(col_r, 2.0, c.gamma_multiply(0.07));
        // The band's level, as a bar in the middle of its column.
        let lvl = if live { h.tele[k] } else { -34.0 + 9.0 * (t * (1.3 + 0.4 * k as f32) + k as f32).sin() };
        let bw = ((b - a) * 0.34).clamp(4.0, 26.0);
        let bar = Rect::from_x_y_ranges(col_r.center().x - bw / 2.0..=col_r.center().x + bw / 2.0, g.y(lvl.max(-60.0))..=g.plot.bottom());
        p.rect_filled(bar, 2.0, c.gamma_multiply(0.45));
        // The threshold, a line across the column with a handle.
        let thr = h.v(&format!("b{n}_thr"));
        let ty = g.y(thr);
        p.line_segment([Pos2::new(col_r.left() + 2.0, ty), Pos2::new(col_r.right() - 2.0, ty)], Stroke::new(1.6_f32, c));
        p.circle_filled(Pos2::new(col_r.center().x, ty), 4.5, c);
        p.circle_stroke(Pos2::new(col_r.center().x, ty), 4.5, Stroke::new(1.0_f32, Color32::from_black_alpha(160)));
        // The gain change hangs from the top (down = reduction, up arrow = upward gain).
        let chg = if live {
            h.tele[4 + k]
        } else if off {
            0.0
        } else {
            -((lvl - thr).max(0.0) * (1.0 - 1.0 / h.v(&format!("b{n}_ratio")).max(1.0))).min(h.v(&format!("b{n}_range")))
        };
        if chg.abs() > 0.05 {
            let len = (chg.abs() / 30.0).clamp(0.0, 1.0) * g.plot.height() * 0.6;
            let gr = Rect::from_x_y_ranges(col_r.center().x - bw / 2.0..=col_r.center().x + bw / 2.0, g.plot.top()..=g.plot.top() + len);
            p.rect_filled(gr, 2.0, if chg < 0.0 { Color32::from_rgb(0xff, 0x55, 0x48) } else { Color32::from_rgb(0x5c, 0xe0, 0x8a) }.gamma_multiply(0.8));
        }
        p.text(Pos2::new(col_r.center().x, r.top() + 4.0), egui::Align2::CENTER_TOP, format!("{chg:+.1}"), egui::FontId::monospace(10.0), if chg.abs() > 0.05 { Color32::WHITE } else { theme::TEXT_DIM });
        // Name, mode and frequency range under the column.
        let mode = ["COMP", "EXP", "UP COMP", "UP EXP"].get(h.v(&format!("b{n}_mode")).round() as usize).copied().unwrap_or("COMP");
        let lo = freq_at(g.plot, a);
        let hi = freq_at(g.plot, b);
        let label = if b - a > 70.0 { format!("BAND {n} · {mode}") } else { format!("{n}") };
        p.text(Pos2::new(col_r.center().x, r.bottom() - 15.0), egui::Align2::CENTER_BOTTOM, label, egui::FontId::proportional(10.0), c);
        if b - a > 60.0 {
            p.text(Pos2::new(col_r.center().x, r.bottom() - 3.0), egui::Align2::CENTER_BOTTOM, format!("{} – {} Hz", hz_label(lo), hz_label(hi)), small.clone(), theme::TEXT_DIM);
        }
    }
    // The crossovers, with a grip at the top.
    for (j, &x) in g.xs.iter().enumerate() {
        p.line_segment([Pos2::new(x, g.plot.top()), Pos2::new(x, g.plot.bottom())], Stroke::new(1.5_f32, Color32::from_white_alpha(150)));
        let grip = Rect::from_center_size(Pos2::new(x, g.plot.bottom() - 8.0), Vec2::new(8.0, 16.0));
        p.rect_filled(grip, 2.0, Color32::from_gray(210));
        let f = h.v(["x1", "x2", "x3"][j]);
        p.text(Pos2::new(x + 4.0, g.plot.top() + 2.0), egui::Align2::LEFT_TOP, format!("{} Hz", hz_label(f)), small.clone(), Color32::from_white_alpha(160));
    }
}

/// What a drag on the multiband display moves.
#[derive(Clone, Copy)]
enum MbDrag {
    Crossover(usize),
    Threshold(usize),
}

fn multiband_interact(ui: &Ui, r: Rect, resp: &egui::Response, h: &Hero, pos: Pos2, out: &mut Vec<(String, f32)>) {
    let g = MbGeom::new(r, h);
    let key = h.key.with("mb.drag");
    let near = |x: f32| g.xs.iter().position(|cx| (cx - x).abs() <= 7.0);
    let pick = |at: Pos2| match near(at.x) {
        Some(j) => MbDrag::Crossover(j),
        None => MbDrag::Threshold(g.band_at(at.x)),
    };
    if r.contains(pos) {
        let icon = if near(pos.x).is_some() { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::ResizeVertical };
        ui.ctx().set_cursor_icon(icon);
    }
    let mut target: Option<MbDrag> = ui.data(|d| d.get_temp(key));
    if resp.drag_started() || (resp.dragged() && target.is_none()) {
        let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
        target = Some(pick(origin));
        ui.data_mut(|d| d.insert_temp(key, target));
    }
    if resp.dragged() {
        match target {
            Some(MbDrag::Crossover(j)) => {
                let ids = ["x1", "x2", "x3"];
                let prm = h.def.params.iter().find(|p| p.id == ids[j]);
                let (mut lo, mut hi) = prm.map(|p| (p.min, p.max)).unwrap_or((20.0, 20000.0));
                // Keep the crossovers in order, a little apart.
                if j > 0 {
                    lo = lo.max(h.v(ids[j - 1]) * 1.25);
                }
                if j + 1 < g.xs.len() {
                    hi = hi.min(h.v(ids[j + 1]) / 1.25);
                }
                let f = freq_at(g.plot, pos.x).clamp(lo, hi.max(lo));
                out.push((ids[j].into(), f.round()));
            }
            Some(MbDrag::Threshold(k)) => {
                let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
                let id = format!("b{}_thr", k + 1);
                let db = (h.v(&id) - resp.drag_delta().y * fine * 60.0 / g.plot.height()).clamp(-60.0, 0.0);
                out.push((id, (db * 10.0).round() / 10.0));
            }
            None => {}
        }
    }
    if resp.drag_stopped() {
        ui.data_mut(|d| d.remove::<Option<MbDrag>>(key));
    }
    if resp.double_clicked() {
        match pick(pos) {
            MbDrag::Crossover(j) => {
                let id = ["x1", "x2", "x3"][j];
                if let Some(prm) = h.def.params.iter().find(|p| p.id == id) {
                    out.push((id.into(), prm.default));
                }
            }
            MbDrag::Threshold(k) => {
                let id = format!("b{}_thr", k + 1);
                if let Some(prm) = h.def.params.iter().find(|p| p.id == id) {
                    out.push((id, prm.default));
                }
            }
        }
    }
}

/// A band's meter for the multiband panel's columns: its level and its gain change, side by side.
pub fn band_meter(ui: &mut Ui, tele: &[f32], band: usize, color: Color32) {
    let w = ui.available_width().max(60.0);
    let (r, _) = ui.allocate_exact_size(Vec2::new(w, 30.0), egui::Sense::hover());
    let p = ui.painter_at(r);
    p.rect_filled(r, 4.0, Color32::from_rgb(0x0e, 0x10, 0x14));
    let live = tele.len() >= 8;
    let lvl = if live { tele[band] } else { -120.0 };
    let chg = if live { tele[4 + band] } else { 0.0 };
    let small = egui::FontId::monospace(9.0);
    let row = |y: f32, name: &str, frac: f32, from_right: bool, c: Color32, text: String| {
        let track = Rect::from_min_max(Pos2::new(r.left() + 22.0, y - 4.0), Pos2::new(r.right() - 44.0, y + 4.0));
        p.rect_filled(track, 2.0, Color32::from_white_alpha(10));
        let len = track.width() * frac.clamp(0.0, 1.0);
        let fill = if from_right { Rect::from_min_max(Pos2::new(track.right() - len, track.top()), track.right_bottom()) } else { Rect::from_min_max(track.left_top(), Pos2::new(track.left() + len, track.bottom())) };
        p.rect_filled(fill, 2.0, c);
        p.text(Pos2::new(r.left() + 4.0, y), egui::Align2::LEFT_CENTER, name, small.clone(), theme::TEXT_DIM);
        p.text(Pos2::new(r.right() - 4.0, y), egui::Align2::RIGHT_CENTER, text, small.clone(), theme::TEXT);
    };
    row(r.top() + 9.0, "IN", (lvl + 60.0) / 60.0, false, color.gamma_multiply(0.8), if lvl > -100.0 { format!("{lvl:.0}") } else { "-∞".into() });
    let gc = if chg < 0.0 { Color32::from_rgb(0xff, 0x55, 0x48) } else { Color32::from_rgb(0x5c, 0xe0, 0x8a) };
    row(r.bottom() - 9.0, "GR", chg.abs() / 24.0, chg < 0.0, gc, format!("{chg:+.1}"));
}

// ------------------------------------------------------------------------------------------------ clipper

/// Where the clipper's faceplate puts things: the input meter, the screen, the output meter.
struct ClipGeom {
    face: Rect,
    input: Rect,
    screen: Rect,
    output: Rect,
}

fn clip_geom(r: Rect) -> ClipGeom {
    let face = r.shrink(2.0);
    let input = Rect::from_min_max(Pos2::new(face.left() + 12.0, face.top() + 42.0), Pos2::new(face.left() + 40.0, face.bottom() - 12.0));
    let output = Rect::from_min_max(Pos2::new(face.right() - 40.0, face.top() + 42.0), Pos2::new(face.right() - 12.0, face.bottom() - 12.0));
    let screen = Rect::from_min_max(Pos2::new(input.right() + 14.0, face.top() + 42.0), Pos2::new(output.left() - 14.0, face.bottom() - 12.0));
    ClipGeom { face, input, screen, output }
}

/// The Soft Clipper's faceplate: brushed gold with engraved labels, input and output meters, a clip LED,
/// and a screen with the transfer curve and a sine pushed through it.
fn clipper(p: &egui::Painter, r: Rect, h: &Hero, _col: Color32) {
    use dragon_engine::effects::clipper::curve;
    use egui::epaint::{Mesh, Vertex, WHITE_UV};
    let g = clip_geom(r);
    // Brushed gold: a vertical gradient with fine horizontal grain.
    let mut mesh = Mesh::default();
    let (top, bottom) = (Color32::from_rgb(0xe4, 0xc2, 0x74), Color32::from_rgb(0xa8, 0x82, 0x3c));
    for (pos, c) in [(g.face.left_top(), top), (g.face.right_top(), top), (g.face.right_bottom(), bottom), (g.face.left_bottom(), bottom)] {
        mesh.vertices.push(Vertex { pos, uv: WHITE_UV, color: c });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    p.add(Shape::mesh(mesh));
    let mut y = g.face.top() + 2.0;
    let mut k = 0u32;
    while y < g.face.bottom() {
        let a = 2 + (k.wrapping_mul(2654435761) >> 29) as u8;
        p.line_segment([Pos2::new(g.face.left(), y), Pos2::new(g.face.right(), y)], Stroke::new(1.0_f32, Color32::from_white_alpha(a)));
        y += 3.0;
        k += 1;
    }
    p.rect_stroke(g.face, 6.0, Stroke::new(1.5_f32, Color32::from_rgb(0x7a, 0x5a, 0x22)), egui::StrokeKind::Inside);
    let ink = Color32::from_rgb(0x2a, 0x1e, 0x0c);
    p.text(Pos2::new(g.face.left() + 12.0, g.face.top() + 8.0), egui::Align2::LEFT_TOP, "SOFT CLIPPER", egui::FontId::proportional(13.0), ink);
    let (t_db, c_db) = (h.v("threshold"), h.v("ceiling"));
    let shape = h.v("shape").round() as usize;
    let os = ["1×", "2×", "4×"].get(h.v("oversampling").round() as usize).copied().unwrap_or("1×");
    p.text(
        Pos2::new(g.screen.center().x, g.face.top() + 9.0),
        egui::Align2::CENTER_TOP,
        format!("{} · THR {:.1} dB · CEIL {:.1} dB · {os}", dragon_engine::effects::clipper::CLIP_SHAPES.get(shape).copied().unwrap_or("Soft").to_uppercase(), t_db.min(c_db), c_db),
        egui::FontId::monospace(10.0),
        ink,
    );
    // Clip LED.
    let live = h.tele.len() >= 3;
    let clip_db = if live { h.tele[1] } else { 0.0 };
    let led = Pos2::new(g.face.right() - 22.0, g.face.top() + 15.0);
    let on = clip_db > 0.05 && h.enabled;
    if on {
        p.circle_filled(led, 9.0, Color32::from_rgba_unmultiplied(255, 60, 40, 70));
    }
    p.circle_filled(led, 5.0, if on { Color32::from_rgb(255, 70, 50) } else { Color32::from_rgb(0x55, 0x22, 0x18) });
    p.text(led - Vec2::new(10.0, 0.0), egui::Align2::RIGHT_CENTER, if on { format!("CLIP −{clip_db:.1}") } else { "CLIP".into() }, egui::FontId::monospace(10.0), ink);
    // Meters: input (with the threshold and ceiling marks) and output.
    let to_y = |m: Rect, db: f32| m.bottom() - ((db + 48.0) / 54.0).clamp(0.0, 1.0) * m.height();
    for (m, db, name) in [(g.input, if live { h.tele[0] } else { -120.0 }, "IN"), (g.output, if live { h.tele[2] } else { -120.0 }, "OUT")] {
        p.rect_filled(m, 3.0, Color32::from_rgb(0x14, 0x11, 0x0c));
        let fill = Rect::from_min_max(Pos2::new(m.left() + 4.0, to_y(m, db)), Pos2::new(m.right() - 4.0, m.bottom() - 2.0));
        if fill.height() > 0.5 {
            p.rect_filled(fill, 2.0, if db > -0.1 { Color32::from_rgb(255, 80, 60) } else { Color32::from_rgb(0xf0, 0xc8, 0x60) });
        }
        p.text(Pos2::new(m.center().x, m.top() - 3.0), egui::Align2::CENTER_BOTTOM, name, egui::FontId::monospace(9.0), ink);
    }
    for (db, c) in [(t_db.min(c_db), Color32::from_rgb(0xf0, 0xc8, 0x60)), (c_db, Color32::from_rgb(255, 90, 70))] {
        let y = to_y(g.input, db);
        p.line_segment([Pos2::new(g.input.left() - 4.0, y), Pos2::new(g.input.right() + 4.0, y)], Stroke::new(1.5_f32, c));
    }
    // The screen: transfer curve on the left, a driven sine on the right.
    p.rect_filled(g.screen, 4.0, Color32::from_rgb(0x10, 0x0e, 0x0a));
    p.rect_stroke(g.screen, 4.0, Stroke::new(1.0_f32, Color32::from_rgb(0x5a, 0x42, 0x18)), egui::StrokeKind::Inside);
    let amber = Color32::from_rgb(0xff, 0xc8, 0x5a);
    let (t, c) = (dragon_engine::dsp::db_to_gain(t_db.min(c_db)), dragon_engine::dsp::db_to_gain(c_db));
    let drive = dragon_engine::dsp::db_to_gain(h.v("input"));
    let sq_side = g.screen.height().min(g.screen.width() * 0.42) - 12.0;
    let sq = Rect::from_min_size(Pos2::new(g.screen.left() + 8.0, g.screen.center().y - sq_side / 2.0), Vec2::splat(sq_side.max(10.0)));
    let grid = Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 200, 90, 22));
    p.line_segment([sq.left_center(), sq.right_center()], grid);
    p.line_segment([sq.center_top(), sq.center_bottom()], grid);
    p.extend(Shape::dashed_line(&[sq.left_bottom(), sq.right_top()], grid, 3.0, 3.0));
    for lvl in [t, c] {
        let y = sq.center().y - lvl / 1.5 * sq.height() / 2.0;
        p.extend(Shape::dashed_line(&[Pos2::new(sq.left(), y), Pos2::new(sq.right(), y)], Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 120, 80, 60)), 2.0, 3.0));
    }
    let pts: Vec<Pos2> = (0..=96)
        .map(|k| {
            let x = -1.5 + 3.0 * k as f32 / 96.0;
            let y = curve(x, t, c, shape);
            Pos2::new(sq.center().x + x / 1.5 * sq.width() / 2.0, sq.center().y - y / 1.5 * sq.height() / 2.0)
        })
        .collect();
    p.add(Shape::line(pts, Stroke::new(2.0_f32, amber)));
    let wave = Rect::from_min_max(Pos2::new(sq.right() + 14.0, g.screen.top() + 8.0), Pos2::new(g.screen.right() - 8.0, g.screen.bottom() - 8.0));
    if wave.width() > 30.0 {
        let phase = h.t() * 1.5;
        let span = (drive * 1.0).max(c) * 1.05;
        let (mut dry, mut wet) = (Vec::new(), Vec::new());
        for k in 0..=160 {
            let u = k as f32 / 160.0;
            let x = drive * 0.9 * (std::f32::consts::TAU * 2.0 * u + phase).sin();
            let px = wave.left() + u * wave.width();
            dry.push(Pos2::new(px, wave.center().y - x / span * wave.height() / 2.0));
            wet.push(Pos2::new(px, wave.center().y - curve(x, t, c, shape) / span * wave.height() / 2.0));
        }
        for lvl in [c, -c] {
            let y = wave.center().y - lvl / span * wave.height() / 2.0;
            p.line_segment([Pos2::new(wave.left(), y), Pos2::new(wave.right(), y)], Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 90, 70, 70)));
        }
        p.add(Shape::line(dry, Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 200, 90, 60))));
        p.add(Shape::line(wet, Stroke::new(2.0_f32, amber)));
    }
}

/// Clean Voice: a valve that glows while the tube stage is in, two moving-coil meters for the leveller and
/// the vari-mu gain reduction, and gain-reduction bars for the de-esser, the four multiband faders and the
/// resonance suppressor.
fn clean_voice(p: &egui::Painter, r: Rect, h: &Hero, _col: Color32) {
    use super::analog::{spring, GNOSTIC_GOLD};
    use super::fx_mod::{art, glow, halo};
    super::fx_mod::screen(p, r, 8.0);
    let ctx = p.ctx().clone();
    let motion = h.time.is_some();
    let t = h.t();
    // Live values only: with no audio running everything rests (no reduction, silence).
    let tele: Vec<f32> = if h.tele.len() >= 11 { h.tele.to_vec() } else { vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, if h.v("tube") > 0.5 { 1.0 } else { 0.0 }, -120.0, -120.0] };
    let tube_on = tele[8] > 0.5;
    let (input, output) = (tele[9], tele[10]);
    let starlight = Color32::from_rgb(0xb4, 0xc4, 0xff);
    let amber = Color32::from_rgb(0xff, 0x8a, 0x3c);
    let label_plate = |rect: Rect, text: &str| {
        if let Some(tx) = art(&ctx, "fx-brass-plate", BRASS_PLATE) {
            p.image(tx.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        p.text(rect.center() + Vec2::new(0.0, 1.0), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional((rect.height() * 0.42).clamp(7.0, 10.0)), Color32::from_rgb(0x2a, 0x1c, 0x08));
    };
    let inner = r.shrink2(Vec2::new(12.0, 10.0));
    let plate_h = 18.0;
    let body_h = inner.height() - plate_h - 6.0;
    // ---- the valve: its heaters glow with the signal driving it (the real input level).
    let tube_rect = {
        let h_t = body_h;
        let w_t = h_t * 98.0 / 300.0;
        Rect::from_min_size(Pos2::new(inner.left() + 6.0, inner.top()), Vec2::new(w_t, h_t))
    };
    let drive = if tube_on { (0.35 + 0.65 * ((input + 40.0) / 40.0).clamp(0.0, 1.0)).clamp(0.0, 1.0) } else { 0.0 };
    let (heat, mv0) = spring(&ctx, h.key.with("cv.heat"), drive, 0.7, 1.0, motion);
    let flick = 1.0 + 0.05 * (t * 17.0).sin() * (t * 5.3).sin();
    halo(p, Pos2::new(tube_rect.center().x, tube_rect.top() + tube_rect.height() * 0.38), tube_rect.width() * 1.4, amber, 0.35 * heat * flick);
    if let Some(tx) = art(&ctx, "fx-power-tube", POWER_TUBE) {
        p.image(tx.id(), tube_rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        // The heaters between the plates, then their light through the glass.
        for dx in [-0.11, 0.11] {
            let c = Pos2::new(tube_rect.center().x + tube_rect.width() * dx, tube_rect.top() + tube_rect.height() * 0.40);
            p.rect_filled(Rect::from_center_size(c, Vec2::new(tube_rect.width() * 0.05, tube_rect.height() * 0.26)), 1.0, glow(amber, 0.9 * heat * flick));
            halo(p, c, tube_rect.width() * 0.3, amber, 0.4 * heat * flick);
        }
    }
    label_plate(Rect::from_center_size(Pos2::new(tube_rect.center().x, inner.bottom() - plate_h / 2.0), Vec2::new(tube_rect.width().max(64.0), plate_h)), if tube_on { "VALVE IN" } else { "VALVE OUT" });
    // ---- the two gauges: leveller and vari-mu gain reduction, 0 … 20 dB.
    let gs = body_h;
    let mut x = tube_rect.right() + 16.0;
    let mut moving = mv0;
    for (k, (name, gr)) in [("LEVELER", tele[0]), ("VARI-MU", tele[1])].into_iter().enumerate() {
        let gr_rect = Rect::from_min_size(Pos2::new(x, inner.top()), Vec2::splat(gs));
        let c = gr_rect.center();
        if let Some(tx) = art(&ctx, "fx-gauge", GAUGE) {
            p.image(tx.id(), gr_rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        let dial = gs * 0.5 * 0.76;
        halo(&p.with_clip_rect(gr_rect), c + Vec2::new(0.0, dial * 0.5), dial * 1.2, starlight, 0.10 + 0.12 * (gr / 20.0).clamp(0.0, 1.0));
        // Scale: 0 at the right, 20 dB at the left, over a 100° arc above the pivot.
        let pivot = c + Vec2::new(0.0, dial * 0.42);
        let rr = dial * 0.95;
        let ang = |db: f32| -PI / 2.0 + 0.95 - 1.9 * (db.clamp(0.0, 20.0) / 20.0);
        for db in [0.0f32, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0, 12.0, 15.0, 20.0] {
            let a = ang(db);
            let major = matches!(db as i32, 0 | 3 | 6 | 10 | 20);
            let col = if db >= 10.0 { Color32::from_rgb(0xff, 0x6a, 0x4a) } else { GNOSTIC_GOLD };
            let o = Pos2::new(pivot.x + rr * a.cos(), pivot.y + rr * a.sin());
            let i = Pos2::new(pivot.x + (rr - if major { 8.0 } else { 4.0 }) * a.cos(), pivot.y + (rr - if major { 8.0 } else { 4.0 }) * a.sin());
            p.line_segment([i, o], Stroke::new(if major { 1.6_f32 } else { 1.0_f32 }, col));
            if major {
                p.text(Pos2::new(pivot.x + (rr - 16.0) * a.cos(), pivot.y + (rr - 16.0) * a.sin()), egui::Align2::CENTER_CENTER, format!("{}", db as i32), egui::FontId::proportional(8.5), col);
            }
        }
        p.text(Pos2::new(c.x, c.y - dial * 0.05), egui::Align2::CENTER_CENTER, "dB GR", egui::FontId::proportional(8.0), starlight.gamma_multiply(0.7));
        // The needle: a real meter movement (VU-like ballistics).
        let (pos, mv) = spring(&ctx, h.key.with(("cv.needle", k)), gr.clamp(0.0, 22.0), 1.8, 0.75, motion);
        moving |= mv;
        let a = ang(pos);
        let tip = Pos2::new(pivot.x + (rr - 2.0) * a.cos(), pivot.y + (rr - 2.0) * a.sin());
        p.line_segment([pivot + Vec2::new(1.0, 2.0), tip + Vec2::new(1.0, 2.0)], Stroke::new(1.8_f32, Color32::from_black_alpha(120)));
        p.line_segment([pivot, tip], Stroke::new(1.6_f32, Color32::from_rgb(0xff, 0xe4, 0xa8)));
        p.circle_filled(pivot, 4.5, GNOSTIC_GOLD);
        p.circle_filled(pivot, 2.0, Color32::from_rgb(0x5a, 0x40, 0x18));
        // The reading.
        p.text(Pos2::new(c.x, pivot.y + dial * 0.28), egui::Align2::CENTER_CENTER, format!("{gr:.1}"), egui::FontId::monospace(10.0), starlight);
        label_plate(Rect::from_center_size(Pos2::new(c.x, inner.bottom() - plate_h / 2.0), Vec2::new(gs * 0.62, plate_h)), name);
        x = gr_rect.right() + 14.0;
    }
    // ---- the reduction of the other stages: phials filled by how much each takes (0 … 12 dB).
    let io_w = 120.0;
    let vials = [("DE-ESS", tele[2]), ("MB1 LOW", tele[3]), ("MB1 HIGH", tele[4]), ("MB2 LOW", tele[5]), ("MB2 HIGH", tele[6]), ("TAME", tele[7])];
    let area = Rect::from_min_max(Pos2::new(x + 4.0, inner.top()), Pos2::new(inner.right() - io_w - 12.0, inner.bottom()));
    let slot = area.width() / vials.len() as f32;
    let vh = body_h - 14.0;
    let glass = art(&ctx, "fx-glass-tube", GLASS_TUBE);
    for (k, (name, gr)) in vials.iter().enumerate() {
        let cx = area.left() + slot * (k as f32 + 0.5);
        let vw = (vh * 90.0 / 443.0).max(16.0);
        let vr = Rect::from_center_size(Pos2::new(cx, inner.top() + 14.0 + vh / 2.0), Vec2::new(vw, vh));
        let bore = Rect::from_min_max(Pos2::new(vr.left() + vw * 0.24, vr.top() + vh * 0.09), Pos2::new(vr.right() - vw * 0.24, vr.bottom() - vh * 0.08));
        let (lvl, mv) = spring(&ctx, h.key.with(("cv.vial", k)), (gr / 12.0).clamp(0.0, 1.0), 1.4, 0.9, motion);
        moving |= mv;
        let top = bore.bottom() - bore.height() * lvl.clamp(0.0, 1.0);
        if lvl > 0.003 {
            let liquid = Rect::from_min_max(Pos2::new(bore.left(), top), bore.max);
            p.rect_filled(liquid, 2.0, Color32::from_rgb(0x3a, 0x52, 0xd8));
            p.rect_filled(Rect::from_min_max(liquid.min, Pos2::new(liquid.center().x, liquid.bottom())), 2.0, Color32::from_rgba_unmultiplied(0xb4, 0xc4, 0xff, 60));
            halo(p, Pos2::new(cx, top), vw * 1.3, starlight, 0.25 + 0.4 * lvl);
            p.line_segment([Pos2::new(bore.left(), top), Pos2::new(bore.right(), top)], Stroke::new(1.5_f32, Color32::from_rgb(0xe0, 0xe8, 0xff)));
        }
        if let Some(tx) = &glass {
            p.image(tx.id(), vr, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        p.text(Pos2::new(cx, inner.top() + 6.0), egui::Align2::CENTER_CENTER, format!("{gr:.1}"), egui::FontId::monospace(9.0), if *gr > 0.05 { starlight } else { starlight.gamma_multiply(0.5) });
        label_plate(Rect::from_center_size(Pos2::new(cx, inner.bottom() - plate_h / 2.0), Vec2::new((slot - 8.0).min(76.0), plate_h)), name);
    }
    // ---- in and out: the real levels through the unit.
    let io = Rect::from_min_max(Pos2::new(inner.right() - io_w, inner.top()), inner.max);
    for (k, (name, db)) in [("INPUT", input), ("OUTPUT", output)].into_iter().enumerate() {
        let row = Rect::from_min_size(Pos2::new(io.left(), io.top() + k as f32 * (body_h / 2.0)), Vec2::new(io_w, body_h / 2.0 - 6.0));
        super::fx_mod::inset(p, row, 5.0);
        p.text(row.left_top() + Vec2::new(8.0, 6.0), egui::Align2::LEFT_TOP, name, egui::FontId::proportional(8.5), GNOSTIC_GOLD.gamma_multiply(0.85));
        let shown = if db <= -119.0 { "silent".to_string() } else { format!("{db:.1}") };
        p.text(Pos2::new(row.right() - 8.0, row.center().y + 4.0), egui::Align2::RIGHT_CENTER, shown, egui::FontId::monospace(16.0), starlight);
        p.text(Pos2::new(row.right() - 8.0, row.bottom() - 4.0), egui::Align2::RIGHT_BOTTOM, "dBFS", egui::FontId::proportional(7.5), starlight.gamma_multiply(0.6));
        let fill = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
        let bar = Rect::from_min_size(Pos2::new(row.left() + 8.0, row.bottom() - 7.0), Vec2::new((row.width() - 60.0) * fill, 3.0));
        p.rect_filled(bar, 1.0, if db > -1.0 { Color32::from_rgb(0xff, 0x6a, 0x4a) } else { starlight });
    }
    label_plate(Rect::from_center_size(Pos2::new(io.center().x, inner.bottom() - plate_h / 2.0), Vec2::new(io_w * 0.8, plate_h)), "LEVELS");
    if moving {
        ctx.request_repaint();
    }
}

/// Philosopher's Stone: one big glowing dial (drag or scroll it) ringed by orbiting particles that grow in
/// number and energy with the amount, the stage name inside, input / output meters at the sides and the
/// gain reduction and auto-gain readouts.
fn particle(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    use dragon_engine::effects::DragonParticle;
    // Deep background with a faint radial glow.
    super::fx_mod::screen(p, r, 10.0);
    let a = h.v("amount");
    let c = r.center();
    let rad = (r.height() * 0.5 - 22.0).max(30.0);
    let ember = Color32::from_rgb(0xf0, 0x6a, 0x3c);
    let gold = Color32::from_rgb(0xf5, 0xc8, 0x42);
    for k in (1..=6).rev() {
        p.circle_filled(c, rad + k as f32 * 6.0 * (0.4 + a), ember.gamma_multiply(0.025 * a));
    }
    // Orbiting particles.
    let t = h.t();
    let count = (24.0 + a * 96.0) as usize;
    for i in 0..count {
        let fi = i as f32;
        let orbit = rad + 10.0 + (fi * 0.618).fract() * (18.0 + a * 26.0);
        let speed = 0.2 + (fi * 0.37).fract() * 0.6 + a * 0.6;
        let ang = fi * 2.399 + t * speed * if i % 2 == 0 { 1.0 } else { -0.7 };
        let pos = Pos2::new(c.x + ang.cos() * orbit, c.y + ang.sin() * orbit * 0.92);
        let tw = 0.5 + 0.5 * (t * 3.0 + fi).sin();
        let cc = if i % 3 == 0 { gold } else { ember };
        p.circle_filled(pos, 0.8 + 1.6 * tw * (0.4 + a), cc.gamma_multiply(0.35 + 0.65 * tw * (0.3 + 0.7 * a)));
    }
    // The sigil turns behind the stone.
    super::fx_mod::sigil(p, r, t, Color32::from_rgb(0xff, 0xd2, 0x7a));
    // The stone itself (3D): turn it to work it. One full turn is 100 %; it glows as it wakes.
    super::fx_mod::stone(p, c, rad * 0.86, a, a, t);
    let a0 = std::f32::consts::PI * 0.75;
    let sweep = std::f32::consts::PI * 1.5;
    let ring = rad + 4.0;
    let arc = |from: f32, to: f32| -> Vec<Pos2> { (0..=64).map(|k| from + (to - from) * k as f32 / 64.0).map(|x| Pos2::new(c.x + ring * x.cos(), c.y + ring * x.sin())).collect() };
    p.add(Shape::line(arc(a0, a0 + sweep), Stroke::new(2.0_f32, Color32::from_rgba_unmultiplied(0xe2, 0xc0, 0x76, 50))));
    if a > 0.002 {
        p.add(Shape::line(arc(a0, a0 + sweep * a), Stroke::new(2.5_f32, ember)));
    }
    p.text(Pos2::new(c.x, c.y + rad + 4.0), egui::Align2::CENTER_TOP, format!("{:.0}  ·  {}", a * 100.0, DragonParticle::mode_name(a).to_uppercase()), egui::FontId::proportional(11.0), gold);
    // Side meters: input (left) and output (right), RMS on a −48 … 0 dB scale.
    // Live values only: at rest (no audio) the meters rest too.
    let (vin, vout, gr, ag) = match h.tele {
        [i, o, g, a2, ..] => (*i, *o, *g, *a2),
        _ => (0.0, 0.0, 0.0, 0.0),
    };
    let meter = |x: f32, v: f32, label: &str| {
        let mr = Rect::from_min_max(Pos2::new(x - 7.0, r.top() + 26.0), Pos2::new(x + 7.0, r.bottom() - 26.0));
        p.rect_filled(mr, 4.0, Color32::from_white_alpha(10));
        let db = dragon_engine::dsp::gain_to_db(v.max(1e-5));
        let f = ((db + 48.0) / 48.0).clamp(0.0, 1.0);
        let fill = Rect::from_min_max(Pos2::new(mr.left(), mr.bottom() - mr.height() * f), mr.max);
        p.rect_filled(fill, 4.0, if db > -6.0 { theme::WARN } else { gold.gamma_multiply(0.85) });
        p.text(Pos2::new(x, mr.top() - 4.0), egui::Align2::CENTER_BOTTOM, label, egui::FontId::proportional(9.0), theme::TEXT_DIM);
        p.text(Pos2::new(x, mr.bottom() + 4.0), egui::Align2::CENTER_TOP, if db < -90.0 { "-∞".to_string() } else { format!("{db:.0}") }, egui::FontId::monospace(9.0), theme::TEXT);
    };
    meter(r.left() + 26.0, vin, "IN");
    meter(r.right() - 26.0, vout, "OUT");
    let chars = ["Clean", "Warm", "Bright", "Punchy"];
    p.text(
        r.left_top() + Vec2::new(48.0, 8.0),
        egui::Align2::LEFT_TOP,
        format!("{} · punch {:.0}% · width {:.0}% · clip {:.0}%", chars.get(h.v("character").round() as usize).unwrap_or(&""), h.v("punch") * 100.0, h.v("width") * 100.0, h.v("clip") * 100.0),
        egui::FontId::proportional(10.0),
        theme::TEXT_DIM,
    );
    p.text(
        r.right_top() + Vec2::new(-48.0, 8.0),
        egui::Align2::RIGHT_TOP,
        format!("GR {gr:.1} dB · ceiling {:.1} dBTP{}", h.v("ceiling"), if h.v("auto_gain") > 0.5 { format!(" · auto gain {ag:+.1} dB") } else { String::new() }),
        egui::FontId::proportional(10.0),
        if gr > 0.1 { theme::WARN } else { theme::TEXT_DIM },
    );
    let _ = col;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every stock effect gets a hero display that draws without panicking, enabled and bypassed.
    #[test]
    fn every_stock_effect_has_a_display() {
        let ctx = egui::Context::default();
        for def in effects::registry() {
            let params = def.defaults();
            for enabled in [true, false] {
                let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(500.0, 400.0))), ..Default::default() };
                let out = ctx.run(raw, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let (edits, dragging) = hero(ui, &Hero { def, params: &params, gr: 3.0, enabled, color: theme::ACCENT, time: Some(1.25), tele: &[], key: egui::Id::new(def.id) });
                        assert!(edits.is_empty() && !dragging);
                    });
                });
                assert!(out.shapes.len() > 3, "{} draws something", def.id);
            }
        }
    }

    /// Every effect's display is a control: dragging on it changes a parameter (tried from a grid of
    /// starting points; the EQ's node dragging has its own test).
    #[test]
    fn every_display_responds_to_a_drag() {
        let mut dead = Vec::new();
        for def in effects::registry().iter().filter(|d| d.id != "eq" && !super::super::fx_jade::own_face(d.id) && !super::super::fx_ghost::is_ghost(d.id)) {
            let params = def.defaults();
            let mut moved = false;
            'points: for gy in [0.5f32, 0.3, 0.7] {
                for gx in [0.5f32, 0.05, 0.12, 0.3, 0.7, 0.88, 0.95] {
                    let ctx = egui::Context::default();
                    let mut area = Rect::NOTHING;
                    let mut got: Vec<(String, f32)> = Vec::new();
                    let frame = |events: Vec<egui::Event>, area: &mut Rect, got: &mut Vec<(String, f32)>| {
                        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(640.0, 520.0))), events, ..Default::default() };
                        let _ = ctx.run(raw, |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                *area = ui.available_rect_before_wrap();
                                let (e, _) = hero(ui, &Hero { def, params: &params, gr: 0.0, enabled: true, color: theme::ACCENT, time: None, tele: &[], key: egui::Id::new(def.id) });
                                got.extend(e);
                            });
                        });
                    };
                    frame(vec![], &mut area, &mut got);
                    let w = area.width().max(260.0);
                    let from = Pos2::new(area.left() + w * gx, area.top() + 120.0 * gy * 1.4);
                    frame(vec![egui::Event::PointerMoved(from)], &mut area, &mut got);
                    frame(vec![egui::Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }], &mut area, &mut got);
                    for k in 1..=5 {
                        frame(vec![egui::Event::PointerMoved(from + egui::vec2(6.0 * k as f32, -8.0 * k as f32))], &mut area, &mut got);
                    }
                    frame(vec![egui::Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }], &mut area, &mut got);
                    if !got.is_empty() {
                        moved = true;
                        break 'points;
                    }
                }
            }
            if !moved {
                dead.push(def.id);
            }
        }
        assert!(dead.is_empty(), "displays that ignore the mouse: {dead:?}");
    }

    /// Ouroboros: a click on a shape or mode button selects it; dragging the field right moves the centre.
    #[test]
    fn orbit_display_is_a_control() {
        let def = effects::find("orbit").unwrap();
        let params = def.defaults();
        let ctx = egui::Context::default();
        let mut area = Rect::NOTHING;
        let mut got: Vec<(String, f32)> = Vec::new();
        let frame = |events: Vec<egui::Event>, area: &mut Rect, got: &mut Vec<(String, f32)>| {
            let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(700.0, 400.0))), events, ..Default::default() };
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    *area = ui.available_rect_before_wrap();
                    let (e, _) = hero(ui, &Hero { def, params: &params, gr: 0.0, enabled: true, color: theme::ACCENT, time: None, tele: &[], key: egui::Id::new("orbit") });
                    got.extend(e);
                });
            });
        };
        frame(vec![], &mut area, &mut got);
        let w = area.width().max(260.0);
        let height = hero_height("orbit", 220.0, w, area.height(), 400.0);
        let (field, shapes, modes) = orbit_layout(Rect::from_min_size(area.min, egui::vec2(w, height)));
        let click = |at: Pos2, area: &mut Rect, got: &mut Vec<(String, f32)>| {
            frame(vec![egui::Event::PointerMoved(at)], area, got);
            frame(vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }], area, got);
            frame(vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }], area, got);
            frame(vec![], area, got);
        };
        click(shapes[5].center(), &mut area, &mut got);
        assert!(got.contains(&("shape".into(), 5.0)), "{got:?}");
        click(modes[2].center(), &mut area, &mut got);
        assert!(got.contains(&("mode".into(), 2.0)), "{got:?}");
        got.clear();
        let from = field.center();
        frame(vec![egui::Event::PointerMoved(from)], &mut area, &mut got);
        frame(vec![egui::Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }], &mut area, &mut got);
        for k in 1..=5 {
            frame(vec![egui::Event::PointerMoved(from + egui::vec2(16.0 * k as f32, 0.0))], &mut area, &mut got);
        }
        frame(vec![egui::Event::PointerButton { pos: from + egui::vec2(80.0, 0.0), button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }], &mut area, &mut got);
        // The display reports relative moves (params stay fixed here): 16 px of a ~660 px field = ~4.8 %.
        assert!(got.iter().filter(|(id, _)| id == "center").count() >= 4 && got.iter().all(|(_, v)| *v > 3.0), "{got:?}");
    }

    /// The Class-A strip's NS fader: dragging the cap to three quarters of the track sets ~75 %.
    #[test]
    fn strip_noise_suppressor_fader_drags() {
        let def = effects::find("class_a_strip").unwrap();
        let params = def.defaults();
        let ctx = egui::Context::default();
        let mut area = Rect::NOTHING;
        let mut got: Vec<(String, f32)> = Vec::new();
        let frame = |events: Vec<egui::Event>, area: &mut Rect, got: &mut Vec<(String, f32)>| {
            let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(700.0, 400.0))), events, ..Default::default() };
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    *area = ui.available_rect_before_wrap();
                    let (e, _) = hero(ui, &Hero { def, params: &params, gr: 0.0, enabled: true, color: theme::ACCENT, time: None, tele: &[], key: egui::Id::new("strip") });
                    got.extend(e);
                });
            });
        };
        frame(vec![], &mut area, &mut got);
        let w = area.width().max(260.0);
        let height = hero_height("class_a_strip", 200.0, w, area.height(), 400.0);
        let (track, _) = ns_fader(Rect::from_min_size(area.min, egui::vec2(w, height)));
        let from = Pos2::new(track.center().x, track.bottom());
        let to = Pos2::new(track.center().x, track.bottom() - track.height() * 0.75);
        frame(vec![egui::Event::PointerMoved(from)], &mut area, &mut got);
        frame(vec![egui::Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }], &mut area, &mut got);
        for k in 1..=6 {
            frame(vec![egui::Event::PointerMoved(from + (to - from) * (k as f32 / 6.0))], &mut area, &mut got);
        }
        frame(vec![egui::Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }], &mut area, &mut got);
        let last = got.iter().rev().find(|(id, _)| id == "ns").map(|x| x.1);
        assert!(last.is_some_and(|v| (v - 75.0).abs() <= 2.0), "ns = {last:?}");
    }

    /// Dragging an EQ node moves that band's frequency and gain.
    #[test]
    fn dragging_an_eq_node_edits_its_band() {
        let def = effects::find("eq").unwrap();
        let mut params = def.defaults();
        params.insert("b3_on".into(), 1.0);
        params.insert("b3_freq".into(), 1000.0);
        params.insert("b3_gain".into(), 0.0);
        let ctx = egui::Context::default();
        let mut area = Rect::NOTHING;
        let mut got: Vec<(String, f32)> = Vec::new();
        let frame = |events: Vec<egui::Event>, params: &BTreeMap<String, f32>, area: &mut Rect, got: &mut Vec<(String, f32)>| {
            let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(500.0, 400.0))), events, ..Default::default() };
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    *area = ui.available_rect_before_wrap();
                    let (e, _) = hero(ui, &Hero { def, params, gr: 0.0, enabled: true, color: theme::ACCENT, time: None, tele: &[], key: egui::Id::new("eq") });
                    got.extend(e);
                });
            });
        };
        frame(vec![], &params, &mut area, &mut got);
        let w = area.width().max(260.0);
        let r = plot(Rect::from_min_size(area.min, egui::vec2(w, 170.0)));
        let node = Pos2::new(fx(r, 1000.0), r.center().y);
        let to = Pos2::new(fx(r, 4000.0), r.center().y - r.height() * 0.5 * 0.5);
        frame(vec![egui::Event::PointerMoved(node)], &params, &mut area, &mut got);
        frame(vec![egui::Event::PointerButton { pos: node, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }], &params, &mut area, &mut got);
        for k in 1..=6 {
            frame(vec![egui::Event::PointerMoved(node + (to - node) * (k as f32 / 6.0))], &params, &mut area, &mut got);
        }
        frame(vec![egui::Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }], &params, &mut area, &mut got);
        let last = |k: &str| got.iter().rev().find(|e| e.0 == k).map(|e| e.1).unwrap_or_else(|| panic!("{k} edited: {got:?}"));
        assert!((last("b3_freq") / 4000.0 - 1.0).abs() < 0.05, "{}", last("b3_freq"));
        assert!((last("b3_gain") - 9.0).abs() < 0.3, "{}", last("b3_gain"));
    }

    /// Double-clicking empty EQ space adds a band there; Alt+clicking a node removes it.
    #[test]
    fn eq_bands_are_added_and_removed_by_clicking() {
        let def = effects::find("eq").unwrap();
        let params = def.defaults();
        let ctx = egui::Context::default();
        let mut area = Rect::NOTHING;
        let mut got: Vec<(String, f32)> = Vec::new();
        let mut frame = |events: Vec<egui::Event>, mods: egui::Modifiers, got: &mut Vec<(String, f32)>| {
            let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(500.0, 400.0))), events, modifiers: mods, ..Default::default() };
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    area = ui.available_rect_before_wrap();
                    let (e, _) = hero(ui, &Hero { def, params: &params, gr: 0.0, enabled: true, color: theme::ACCENT, time: None, tele: &[], key: egui::Id::new("eq") });
                    got.extend(e);
                });
            });
            area
        };
        let a = frame(vec![], Default::default(), &mut got);
        let r = plot(Rect::from_min_size(a.min, egui::vec2(a.width().max(260.0), 170.0)));
        let click = |at: Pos2, mods: egui::Modifiers| -> Vec<egui::Event> {
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: mods },
                egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: mods },
            ]
        };
        // Between the default bands (100, 300, 1k, 3k, 8k Hz): 550 Hz, +6 dB.
        let at = Pos2::new(fx(r, 550.0), r.center().y - r.height() * 0.5 / 3.0);
        frame(click(at, Default::default()), Default::default(), &mut got);
        frame(click(at, Default::default()), Default::default(), &mut got);
        let last = |got: &Vec<(String, f32)>, k: &str| got.iter().rev().find(|e| e.0 == k).map(|e| e.1);
        assert_eq!(last(&got, "b1_on"), Some(1.0), "first free band switched on: {got:?}");
        assert_eq!(last(&got, "b1_type"), Some(0.0), "a bell in the middle");
        assert!((last(&got, "b1_freq").unwrap() / 550.0 - 1.0).abs() < 0.05);
        assert!((last(&got, "b1_gain").unwrap() - 6.0).abs() < 0.3);
        // Alt+click the 1 kHz node.
        got.clear();
        let node = Pos2::new(fx(r, 1000.0), r.center().y);
        let alt = egui::Modifiers { alt: true, ..Default::default() };
        frame(vec![egui::Event::PointerMoved(Pos2::new(5.0, 5.0))], Default::default(), &mut got);
        frame(click(node, alt), alt, &mut got);
        assert_eq!(last(&got, "b4_on"), Some(0.0), "{got:?}");
    }
}
