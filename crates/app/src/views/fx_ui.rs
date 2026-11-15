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
    /// Identifies the insert (keeps per-insert display history such as Dragon Tune's pitch trace).
    pub key: egui::Id,
}

impl Hero<'_> {
    fn v(&self, id: &str) -> f32 {
        effects::value(self.def, self.params, id)
    }
    fn t(&self) -> f32 {
        self.time.unwrap_or(0.0)
    }
}

/// Log-frequency x position (20 Hz … 20 kHz) inside `r`.
fn fx(r: Rect, f: f32) -> f32 {
    r.left() + r.width() * ((f.max(20.0) / 20.0).log10() / 3.0).clamp(0.0, 1.0)
}

fn freq_at(r: Rect, x: f32) -> f32 {
    20.0 * 1000f32.powf(((x - r.left()) / r.width()).clamp(0.0, 1.0))
}

/// A filled curve: a glowing line over a vertical gradient that fades towards `base_y` (the look of the
/// reference sheet).
fn filled_curve(p: &egui::Painter, pts: &[Pos2], base_y: f32, col: Color32) {
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
fn plot(r: Rect) -> Rect {
    Rect::from_min_max(r.min + Vec2::new(32.0, 10.0), r.max - Vec2::new(10.0, 18.0))
}

/// Labels down the left edge: `(value, text)` placed with `y_of`.
fn y_axis(p: &egui::Painter, pr: Rect, marks: &[(f32, &str)], y_of: impl Fn(f32) -> f32) {
    for (v, t) in marks {
        let y = y_of(*v);
        p.line_segment([Pos2::new(pr.left(), y), Pos2::new(pr.right(), y)], Stroke::new(1.0_f32, Color32::from_white_alpha(if *v == 0.0 { 28 } else { 10 })));
        p.text(Pos2::new(pr.left() - 5.0, y), egui::Align2::RIGHT_CENTER, *t, egui::FontId::proportional(9.0), Color32::from_white_alpha(110));
    }
}

/// Labels along the bottom edge: `(value, text)` placed with `x_of`.
fn x_axis(p: &egui::Painter, pr: Rect, marks: &[(f32, String)], x_of: impl Fn(f32) -> f32) {
    for (v, t) in marks {
        let x = x_of(*v);
        p.line_segment([Pos2::new(x, pr.top()), Pos2::new(x, pr.bottom())], Stroke::new(1.0_f32, Color32::from_white_alpha(10)));
        p.text(Pos2::new(x, pr.bottom() + 3.0), egui::Align2::CENTER_TOP, t, egui::FontId::proportional(9.0), Color32::from_white_alpha(110));
    }
}

fn grid(p: &egui::Painter, r: Rect, log_freq: bool) {
    p.rect_filled(r, 6.0, Color32::from_rgb(0x12, 0x14, 0x18));
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
        "tune" | "clean_voice" | "class_a_strip" => 230.0,
        "drum_bus" | "alter" => 200.0,
        "dragon_particle" => 240.0,
        _ => 170.0,
    };
    // Wide windows (full screen) get a taller display in proportion.
    let height = (base * (w / 520.0).clamp(1.0, 2.2)).min(ui.available_height().max(base));
    let (r, resp) = ui.allocate_exact_size(Vec2::new(w, height), egui::Sense::click_and_drag());
    let edits = interact(ui, r, &resp, h);
    let p = ui.painter_at(r);
    let col = if h.enabled { h.color } else { theme::TEXT_DIM };
    match h.def.id {
        "eq" => eq(&p, r, h, col),
        "filter" => filter(&p, r, h, col),
        "compressor" | "limiter" | "gate" | "deesser" | "multiband" => dynamics(&p, r, h, col),
        "transient" => transient(&p, r, h, col),
        "delay" => delay(&p, r, h, col),
        "reverb" => reverb(&p, r, h, col),
        "saturator" | "bitcrusher" => shaper(&p, r, h, col),
        "chorus" | "flanger" | "phaser" | "tremolo" => lfo(&p, r, h, col),
        "imager" => phase_meter(&p, r, h, col),
        "utility" => stereo(&p, r, h, col),
        "tune" => tune(&p, r, h, col),
        "dragon_particle" => particle(&p, r, h, col),
        "clean_voice" => clean_voice(&p, r, h, col),
        "class_a_strip" => class_a_strip(&p, r, h, col),
        "drum_bus" => drum_bus(&p, r, h, col),
        "alter" => alter(&p, r, h, col),
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
    p.rect_stroke(r, 6.0, Stroke::new(1.0_f32, col.gamma_multiply(0.4)), egui::StrokeKind::Inside);
    (edits, resp.dragged())
}

/// Square area of the transfer-curve displays (dynamics, shaper), with room for axis labels.
fn square(r: Rect) -> Rect {
    Rect::from_min_size(r.min + Vec2::new(34.0, 10.0), Vec2::splat(r.height() - 32.0))
}

fn interact(ui: &Ui, r: Rect, resp: &egui::Response, h: &Hero) -> Vec<(String, f32)> {
    let mut out = Vec::new();
    let Some(pos) = resp.interact_pointer_pos().or_else(|| resp.hover_pos()) else { return out };
    let grab = resp.id.with("grab");
    match h.def.id {
        "eq" => {
            let r = plot(r);
            let range = 18.0;
            let y_of = |db: f32| r.center().y - db.clamp(-range, range) / range * r.height() * 0.5;
            let node = |b: usize| Pos2::new(fx(r, h.v(&format!("b{b}_freq"))), y_of(h.v(&format!("b{b}_gain"))));
            let near = |at: Pos2| (1..=7).filter(|b| h.v(&format!("b{b}_on")) > 0.5).min_by(|a, b| node(*a).distance(at).total_cmp(&node(*b).distance(at))).filter(|b| node(*b).distance(at) < 16.0);
            let nearest = near(pos);
            if resp.drag_started() {
                let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
                ui.data_mut(|d| d.insert_temp(grab, near(origin).unwrap_or(0)));
            }
            let held: usize = ui.data(|d| d.get_temp(grab)).unwrap_or(0);
            if resp.dragged() && held > 0 {
                out.push((format!("b{held}_freq"), freq_at(r, pos.x).clamp(20.0, 20_000.0)));
                let db = ((r.center().y - pos.y) / (r.height() * 0.5) * range).clamp(-range, range);
                out.push((format!("b{held}_gain"), (db * 10.0).round() / 10.0));
            }
            if let Some(b) = nearest {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                let s = ui.input(|i| i.smooth_scroll_delta.y);
                if s != 0.0 {
                    out.push((format!("b{b}_q"), (h.v(&format!("b{b}_q")) * (1.0 + s / 300.0)).clamp(0.1, 18.0)));
                }
                // Alt+click removes the band (FabFilter style).
                if resp.clicked() && ui.input(|i| i.modifiers.alt) {
                    out.push((format!("b{b}_on"), 0.0));
                }
                if !resp.dragged() {
                    let ty = EQ_TYPES.get(h.v(&format!("b{b}_type")).round() as usize).copied().unwrap_or("Bell");
                    egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), resp.id.with("eq.tip"), |ui| {
                        ui.label(format!("Band {b} · {ty}\n{} · {:+.1} dB · Q {:.2}", hz_text(h.v(&format!("b{b}_freq"))), h.v(&format!("b{b}_gain")), h.v(&format!("b{b}_q"))));
                        ui.label(egui::RichText::new("drag · wheel = Q · right-click = shape · Alt+click = remove").size(10.0).color(theme::TEXT_DIM));
                    });
                }
            } else if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            }
            // Double-click on empty space adds a band there: a cut near the edges, a bell elsewhere.
            if resp.double_clicked() && nearest.is_none() && r.contains(pos) {
                if let Some(b) = (1..=7).find(|b| h.v(&format!("b{b}_on")) < 0.5) {
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
                    out.extend([(format!("b{b}_on"), 1.0), (format!("b{b}_type"), ty), (format!("b{b}_freq"), f), (format!("b{b}_gain"), (db * 10.0).round() / 10.0), (format!("b{b}_q"), 0.9)]);
                }
            }
            // Right-click on a band: shape, Q presets, gain reset and removal.
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
        "dragon_particle" => {
            let c = r.center();
            let rad = (r.height() * 0.5 - 22.0).max(30.0);
            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
            if resp.dragged() && origin.distance(c) < rad + 10.0 {
                let d = -resp.drag_delta().y / 220.0;
                out.push(("amount".into(), (h.v("amount") + d).clamp(0.0, 1.0)));
            }
            if pos.distance(c) < rad + 10.0 {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                let s = ui.input(|i| i.smooth_scroll_delta.y);
                if s != 0.0 {
                    out.push(("amount".into(), (h.v("amount") + s / 800.0).clamp(0.0, 1.0)));
                }
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
        _ => {}
    }
    out
}

// ---------------------------------------------------------------------------------------------- EQ

const EQ_TYPES: [&str; 7] = ["Bell", "Low shelf", "High shelf", "Low pass", "High pass", "Band pass", "Notch"];

fn hz_text(f: f32) -> String {
    if f >= 1000.0 {
        format!("{:.2} kHz", f / 1000.0)
    } else {
        format!("{f:.0} Hz")
    }
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
    let bands: Vec<(usize, Biquad, f32, f32)> = (1..=7)
        .filter(|b| h.v(&format!("b{b}_on")) > 0.5)
        .map(|b| {
            let f = h.v(&format!("b{b}_freq"));
            let g = h.v(&format!("b{b}_gain"));
            let q = h.v(&format!("b{b}_q"));
            (b - 1, Biquad::new(BiquadKind::from_index(h.v(&format!("b{b}_type"))), f, q, g, RATE), f, g)
        })
        .collect();
    let out = h.v("output");
    let p = p.with_clip_rect(pr.expand(8.0).intersect(r));
    spectrum(&p, pr, h.tele);
    // Each band's own shape, faint, then the sum.
    for (k, bq, _, _) in &bands {
        let pts: Vec<Pos2> = (0..=120).map(|i| pr.left() + pr.width() * i as f32 / 120.0).map(|x| Pos2::new(x, y_of(bq.response_db(freq_at(pr, x), RATE)))).collect();
        p.add(Shape::line(pts, Stroke::new(1.0_f32, BAND_COLORS[*k].gamma_multiply(0.4))));
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
        p.circle_filled(c, 10.0 * pulse, BAND_COLORS[*k].gamma_multiply(0.22));
        p.circle_filled(c, 6.5, BAND_COLORS[*k]);
        p.circle_stroke(c, 6.5, Stroke::new(1.5_f32, Color32::from_white_alpha(200)));
        p.text(c, egui::Align2::CENTER_CENTER, format!("{}", k + 1), egui::FontId::proportional(8.5), Color32::BLACK);
    }
}

fn filter(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    grid(p, r, true);
    let pr = plot(r);
    let kind = match h.v("type").round() as i32 {
        0 => BiquadKind::LowPass,
        1 => BiquadKind::HighPass,
        _ => BiquadKind::BandPass,
    };
    // The cutoff wobbles with the LFO like the sound does.
    let lfo = (h.t() * TAU * h.v("lfo_rate")).sin() * h.v("lfo_depth");
    let cutoff = (h.v("cutoff") * 2f32.powf(lfo)).clamp(20.0, 20_000.0);
    let q = 0.707 + h.v("resonance") * 12.0;
    let bq = Biquad::new(kind, cutoff, q, 0.0, RATE);
    let (top, bottom) = (12.0, -24.0);
    let y_of = |db: f32| pr.top() + (top - db.clamp(bottom, top)) / (top - bottom) * pr.height();
    y_axis(p, pr, &[(12.0, "+12"), (6.0, "+6"), (0.0, "0"), (-6.0, "-6"), (-12.0, "-12"), (-18.0, "-18"), (-24.0, "-24")], y_of);
    let p = p.with_clip_rect(pr.expand(10.0).intersect(r));
    let pts: Vec<Pos2> = (0..=180).map(|i| pr.left() + pr.width() * i as f32 / 180.0).map(|x| Pos2::new(x, y_of(bq.response_db(freq_at(pr, x), RATE)))).collect();
    filled_curve(&p, &pts, pr.bottom(), col);
    // The cutoff node: a ring with a glow, like the reference.
    let c = Pos2::new(fx(pr, cutoff), y_of(bq.response_db(cutoff, RATE)));
    p.circle_filled(c, 11.0, col.gamma_multiply(0.25));
    p.circle_filled(c, 6.0, theme::BG);
    p.circle_stroke(c, 6.0, Stroke::new(2.0_f32, Color32::WHITE));
}

// --------------------------------------------------------------------------------------- dynamics

fn dynamics(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    p.rect_filled(r, 6.0, Color32::from_rgb(0x12, 0x14, 0x18));
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
        "gate" => (h.v("threshold"), 1.0 / 8.0, 0.0, 0.0),
        "multiband" => (h.v("mid_threshold"), h.v("mid_ratio"), 6.0, h.v("mid_gain")),
        "deesser" => (h.v("threshold"), h.v("ratio"), 0.0, 0.0),
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
    let x_in = if h.gr > 0.05 { (thr + h.gr * ratio / (ratio - 1.0).max(0.01)).min(0.0) } else { thr - 6.0 + 3.0 * (h.t() * 1.3).sin() };
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
    let history = h.def.id == "compressor";
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
        "limiter" => format!("ceiling {:.1} dB · +{:.1} dB drive", h.v("ceiling"), h.v("gain")),
        "gate" => format!("range {:.0} dB · hold {:.0} ms", h.v("range"), h.v("hold")),
        "deesser" => format!("listening at {:.1} kHz", h.v("freq") / 1000.0),
        "multiband" => format!("split {:.0} Hz / {:.1} kHz", h.v("low_mid"), h.v("mid_high") / 1000.0),
        _ => format!("{:.1}:1 · attack {:.0} ms · release {:.0} ms", ratio, h.v("attack"), h.v("release")),
    };
    p.text(Pos2::new(m.left(), m.bottom()), egui::Align2::LEFT_BOTTOM, caption, egui::FontId::proportional(10.5), theme::TEXT);
}

/// Pro-C style scrolling history: input level (grey), output level (colour) and gain reduction
/// (orange, hanging from the top), on one dB scale. `static_gr` gives the curve for the gallery demo.
fn level_history(p: &egui::Painter, g: Rect, h: &Hero, col: Color32, static_out: impl Fn(f32) -> f32) {
    const LEN: usize = 240;
    p.rect_filled(g, 3.0, Color32::from_rgb(0x16, 0x19, 0x1f));
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
    use dragon_engine::effects::dynamics::{ts_shape, TS_SHAPES};
    p.rect_filled(r, 6.0, Color32::from_rgb(0x10, 0x12, 0x16));
    let att = [h.v("attack") / 100.0, h.v("att_dur") / 1000.0, h.v("att_shape").round()];
    let sus = [h.v("sustain") / 100.0, h.v("sus_dur") / 1000.0, h.v("sus_shape").round()];
    let weights = |t: f32| -> (f32, f32) {
        let (ad, sd) = (att[1].max(0.001), sus[1].max(0.01));
        let wa = if t < ad { ts_shape(att[2] as usize, t / ad) } else { 0.0 };
        let s0 = ad * 0.6;
        let ws = if t >= s0 && t < s0 + sd { ((t - s0) / 0.005).min(1.0) * ts_shape(sus[2] as usize, (t - s0) / sd) } else { 0.0 };
        (wa, ws)
    };
    // ---- top: scrolling level (grey) and gain change (attack orange-ish, sustain colour)
    let top = Rect::from_min_max(r.min + Vec2::new(10.0, 10.0), Pos2::new(r.right() - 10.0, r.top() + r.height() * 0.55));
    p.rect_filled(top, 4.0, Color32::from_rgb(0x16, 0x19, 0x1f));
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
        p.rect_filled(pr, 4.0, Color32::from_rgb(0x16, 0x19, 0x1f));
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
            format!("{:+.0}% · {:.0} ms · {}", amt * 100.0, dur * 1000.0, TS_SHAPES.get(shape as usize).unwrap_or(&"")),
            egui::FontId::proportional(9.5),
            theme::TEXT,
        );
    }
}

// ------------------------------------------------------------------------------------ time based

fn delay(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    grid(p, r, false);
    let pr = plot(r);
    let fb = h.v("feedback");
    let ping = h.v("mode").round() as i32 == 1;
    let time = h.v("time").max(1.0);
    // Show about eight repeats (at least half a second).
    let span_ms: f32 = (time * 8.0).max(500.0);
    let x_of = |ms: f32| pr.left() + 6.0 + ms / span_ms * (pr.width() - 12.0);
    let y_of = |db: f32| pr.top() + (-db.clamp(-30.0, 0.0) / 30.0) * pr.height();
    y_axis(p, pr, &[(0.0, "0 dB"), (-6.0, "-6"), (-12.0, "-12"), (-18.0, "-18"), (-24.0, "-24"), (-30.0, "-30")], y_of);
    let marks: Vec<(f32, String)> = (0..=4).map(|k| span_ms * k as f32 / 4.0).map(|ms| (ms, if ms >= 1000.0 { format!("{:.1} s", ms / 1000.0) } else { format!("{ms:.0} ms") })).collect();
    x_axis(p, pr, &marks, x_of);
    // Lollipop taps (dry hit in orange, repeats in the effect colour), one lights as the echo passes.
    let travel = (h.t() * 1000.0) % (span_ms + 300.0);
    let mut level = 1.0f32;
    let mut k = 0;
    while k < 48 && level > 0.03 {
        let t = time * k as f32;
        if t > span_ms {
            break;
        }
        let x = x_of(t);
        let db = dragon_engine::dsp::gain_to_db(level.max(1e-4));
        let y = y_of(db);
        let lit = (travel - t).abs() < 50.0;
        let c = if k == 0 { Color32::from_rgb(0xf0, 0x8a, 0x4b) } else { col.gamma_multiply(0.45 + 0.55 * level) };
        let side = if ping && k % 2 == 1 { 1.5 } else { 0.0 };
        p.line_segment([Pos2::new(x + side, pr.bottom()), Pos2::new(x + side, y)], Stroke::new(if lit { 4.0_f32 } else { 2.5 }, c.gamma_multiply(0.75)));
        p.circle_filled(Pos2::new(x + side, y), if lit { 8.0 } else { 6.0 }, c.gamma_multiply(0.25));
        p.circle_filled(Pos2::new(x + side, y), if lit { 4.5 } else { 3.5 }, if lit { Color32::WHITE } else { c });
        level *= fb.max(0.0);
        k += 1;
        if fb <= 0.0 {
            break;
        }
    }
    p.text(
        pr.right_top() + Vec2::new(-4.0, 2.0),
        egui::Align2::RIGHT_TOP,
        format!("{:.0} ms · {:.0}% feedback{}", time, fb * 100.0, if ping { " · ping-pong" } else { "" }),
        egui::FontId::proportional(10.5),
        theme::TEXT,
    );
}

fn reverb(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    grid(p, r, false);
    let pr = plot(r);
    let decay = h.v("decay");
    let pre = h.v("predelay");
    let size = h.v("size");
    let early = h.v("early");
    let late = h.v("late");
    let span = (decay * 1.1 + pre / 1000.0).clamp(0.5, 24.0);
    let x_of = |s: f32| pr.left() + s / span * pr.width();
    let y_of = |db: f32| pr.top() + (-db.clamp(-30.0, 0.0) / 30.0) * pr.height();
    y_axis(p, pr, &[(0.0, "0 dB"), (-6.0, "-6"), (-12.0, "-12"), (-18.0, "-18"), (-24.0, "-24"), (-30.0, "-30")], y_of);
    let marks: Vec<(f32, String)> = (0..=4).map(|k| span * k as f32 / 4.0).map(|s| (s, format!("{s:.1} s"))).collect();
    x_axis(p, pr, &marks, x_of);
    // The tail as a dense "impulse response": one thin bar per pixel column, its height a noisy sample
    // under the exponential envelope (in dB), shimmering slowly.
    let start = pre / 1000.0;
    let level = |s: f32| -> f32 {
        if s < start {
            0.0
        } else {
            (-6.9 * (s - start) / decay.max(0.1)).exp() * late.max(0.15)
        }
    };
    let mut env_pts = Vec::new();
    let mut x = pr.left();
    let mut seed = 0x9e37_79b9u32;
    while x < pr.right() {
        let s = (x - pr.left()) / pr.width() * span;
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let noise = ((seed % 1000) as f32 / 1000.0) * (0.75 + 0.25 * (h.t() * 3.0 + x * 0.3).sin());
        let a = level(s) * (0.35 + 0.65 * noise);
        if a > 0.0 {
            let y = y_of(dragon_engine::dsp::gain_to_db(a.max(1e-4)));
            let ty = y.max(pr.top());
            p.line_segment([Pos2::new(x, pr.bottom()), Pos2::new(x, ty)], Stroke::new(1.0_f32, col.gamma_multiply(0.7)));
        }
        if s >= start {
            env_pts.push(Pos2::new(x, y_of(dragon_engine::dsp::gain_to_db(level(s).max(1e-4)))));
        }
        x += 1.5;
    }
    if env_pts.len() > 1 {
        p.add(Shape::line(env_pts.clone(), Stroke::new(5.0_f32, col.gamma_multiply(0.15))));
        p.add(Shape::line(env_pts, Stroke::new(1.5_f32, col)));
    }
    // Early reflections: bright spikes after the pre-delay.
    for k in 0..8 {
        let s = start + (k as f32 + 1.0) * 0.008 * h.v("er_size") * (1.0 + size);
        let a = early * (1.0 - k as f32 / 9.0);
        if a > 0.01 {
            let y = y_of(dragon_engine::dsp::gain_to_db(a));
            p.line_segment([Pos2::new(x_of(s), pr.bottom()), Pos2::new(x_of(s), y)], Stroke::new(2.0_f32, Color32::from_rgb(0xf0, 0x8a, 0x4b)));
        }
    }
    let modes = match h.def.param("mode").map(|p| p.kind) {
        Some(effects::ParamKind::Choice(n)) => n,
        _ => &[],
    };
    let mode = modes.get(h.v("mode").round() as usize).copied().unwrap_or("");
    p.text(pr.right_top() + Vec2::new(-4.0, 2.0), egui::Align2::RIGHT_TOP, format!("{mode} · {decay:.1} s decay · {pre:.0} ms pre-delay"), egui::FontId::proportional(10.5), theme::TEXT);
}

// ---------------------------------------------------------------------------------- distortion

fn shaper(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    p.rect_filled(r, 6.0, Color32::from_rgb(0x12, 0x14, 0x18));
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
    // A dot rides the curve with the test sine.
    let xin = (h.t() * 2.0).sin() * 0.9;
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

fn lfo(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    grid(p, r, false);
    let rate = h.v("rate");
    let depth = h.v("depth");
    let id = h.def.id;
    let wave = |ph: f32| -> f32 {
        if id == "tremolo" {
            match h.v("shape").round() as i32 {
                1 => 1.0 - 4.0 * ((ph / TAU).fract() - 0.5).abs(),
                2 => {
                    if (ph / TAU).fract() < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                }
                _ => ph.sin(),
            }
        } else {
            ph.sin()
        }
    };
    let mid = r.center().y;
    let amp = r.height() * 0.4 * depth.max(0.05);
    let shift = h.t() * rate * TAU;
    let cycles = 2.0;
    let pts: Vec<Pos2> = (0..=200).map(|k| k as f32 / 200.0).map(|u| Pos2::new(r.left() + 8.0 + u * (r.width() - 16.0), mid - wave(u * TAU * cycles - shift) * amp)).collect();
    filled_curve(p, &pts, mid, col);
    // A "voice" dot that bobs with the LFO at the left; for chorus / flanger echoes trail behind it.
    let dot = Pos2::new(r.left() + 30.0, mid - wave(-shift) * amp);
    let trail = match id {
        "chorus" => 3,
        "flanger" | "phaser" => (2.0 + h.v("feedback").abs() * 6.0) as i32,
        _ => 0,
    };
    for k in 1..=trail {
        let d = Pos2::new(dot.x + k as f32 * 9.0, mid - wave(-shift - k as f32 * 0.35) * amp);
        p.circle_filled(d, 5.0 - k as f32 * 0.4, col.gamma_multiply(0.6 - k as f32 * 0.06));
    }
    p.circle_filled(dot, 7.0, Color32::WHITE);
    let extra = match id {
        "phaser" => match h.def.param("stages").map(|p| p.kind) {
            Some(effects::ParamKind::Choice(n)) => format!(" · {} stages", n.get(h.v("stages").round() as usize).copied().unwrap_or("")),
            _ => String::new(),
        },
        "flanger" => format!(" · {:+.0}% feedback", h.v("feedback") * 100.0),
        "tremolo" if h.v("pan") > 0.5 => " · auto-pan".into(),
        _ => String::new(),
    };
    p.text(r.right_top() + Vec2::new(-8.0, 6.0), egui::Align2::RIGHT_TOP, format!("{rate:.2} Hz · {:.0}% depth{extra}", depth * 100.0), egui::FontId::proportional(10.5), theme::TEXT);
}

// --------------------------------------------------------------------------------------- utility

fn stereo(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    p.rect_filled(r, 6.0, Color32::from_rgb(0x12, 0x14, 0x18));
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
    p.rect_filled(r, 6.0, Color32::from_rgb(0x10, 0x12, 0x16));
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

/// Dragon Tune, laid out like a modern vocal tuner: the detected note large with its cents offset, a
/// needle meter (input deviation in white, the applied correction as a coloured arc), a scrolling pitch
/// trace over the note lanes (input white, corrected output coloured) and the key's notes underneath.
fn tune(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    use dragon_engine::effects::pitch::{KEYS, TUNE_SCALES};
    p.rect_filled(r, 8.0, Color32::from_rgb(0x10, 0x12, 0x16));
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
    p.rect_filled(tr, 4.0, Color32::from_rgb(0x16, 0x19, 0x1f));
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
    for (i, nm) in KEYS.iter().enumerate() {
        let b = Rect::from_min_size(Pos2::new(kr.left() + i as f32 * kw + 1.0, kr.top()), Vec2::new(kw - 2.0, kr.height()));
        let on = in_scale(i);
        let lit = tgt == Some(i);
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

/// Class-A Strip: the preamp's drive with the transformer warming up, the EQ curve as set, and the opto
/// compressor's cell glowing with its gain reduction above a moving-coil GR meter.
fn class_a_strip(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    use dragon_engine::dsp::{Biquad, BiquadKind};
    p.rect_filled(r, 8.0, Color32::from_rgb(0x3a, 0x44, 0x52));
    p.rect_stroke(r.shrink(3.0), 6.0, Stroke::new(1.0_f32, Color32::from_white_alpha(30)), egui::StrokeKind::Inside);
    let t = h.t();
    let (pin, gr, pout) = match h.tele {
        [a, b, c, ..] => (*a, *b, *c),
        _ => {
            let w = (t * 1.6).sin().abs();
            (0.4 + 0.3 * w, 2.0 + 6.0 * w, 0.5)
        }
    };
    let w3 = (r.width() - 40.0) / 3.0;
    let sec = |k: usize| Rect::from_min_size(Pos2::new(r.left() + 10.0 + k as f32 * (w3 + 10.0), r.top() + 10.0), Vec2::new(w3, r.height() - 20.0));
    // ---- preamp
    let pr = sec(0);
    p.rect_filled(pr, 6.0, Color32::from_rgb(0x2b, 0x32, 0x3d));
    p.text(pr.left_top() + Vec2::new(8.0, 6.0), egui::Align2::LEFT_TOP, "PREAMP", egui::FontId::proportional(10.0), Color32::from_rgb(0xe8, 0x5a, 0x4f));
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
    p.add(Shape::line(arc(a0, a0 + sweep * nd, rad + 1.0), Stroke::new(3.0_f32, Color32::from_rgb(0xe8, 0x5a, 0x4f))));
    p.circle_filled(c, rad - 4.0, Color32::from_rgb(0xc8, 0x3a, 0x30));
    p.circle_stroke(c, rad - 4.0, Stroke::new(2.0_f32, Color32::from_rgb(0x8a, 0x24, 0x1d)));
    let a = a0 + sweep * nd;
    p.line_segment([c, Pos2::new(c.x + (rad - 6.0) * a.cos(), c.y + (rad - 6.0) * a.sin())], Stroke::new(2.5_f32, Color32::WHITE));
    // Transformer glow with level.
    let heat = (gain_to_db_ui(pin) + 30.0).clamp(0.0, 30.0) / 30.0 * (0.3 + nd);
    p.circle_filled(Pos2::new(pr.right() - 14.0, pr.top() + 12.0), 5.0, Color32::from_rgb(0xff, 0xa0, 0x40).gamma_multiply(0.2 + 0.8 * heat.min(1.0)));
    p.text(Pos2::new(pr.center().x, pr.bottom() - 6.0), egui::Align2::CENTER_BOTTOM, format!("drive {drive:+.0} dB"), egui::FontId::proportional(10.0), theme::TEXT);
    // ---- EQ curve
    let er = sec(1);
    p.rect_filled(er, 6.0, Color32::from_rgb(0x2b, 0x32, 0x3d));
    p.text(er.left_top() + Vec2::new(8.0, 6.0), egui::Align2::LEFT_TOP, if h.v("eq_on") > 0.5 { "EQ" } else { "EQ (out)" }, egui::FontId::proportional(10.0), Color32::from_rgb(0x6c, 0xb4, 0xee));
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
    filled_curve(p, &pts, plot_r.center().y, Color32::from_rgb(0x6c, 0xb4, 0xee));
    // ---- opto compressor
    let cr = sec(2);
    p.rect_filled(cr, 6.0, Color32::from_rgb(0x2b, 0x32, 0x3d));
    p.text(cr.left_top() + Vec2::new(8.0, 6.0), egui::Align2::LEFT_TOP, if h.v("comp_on") > 0.5 { "OPTO COMP" } else { "OPTO COMP (out)" }, egui::FontId::proportional(10.0), col);
    // The cell's lamp brightens with gain reduction.
    let lamp = Pos2::new(cr.right() - 18.0, cr.top() + 14.0);
    let gl = (gr / 12.0).clamp(0.0, 1.0);
    for k in (1..=4).rev() {
        p.circle_filled(lamp, 4.0 + k as f32 * 3.0, Color32::from_rgb(0xff, 0xe0, 0x90).gamma_multiply(0.08 * gl));
    }
    p.circle_filled(lamp, 4.5, Color32::from_rgb(0xff, 0xe0, 0x90).gamma_multiply(0.25 + 0.75 * gl));
    let vu = Rect::from_min_max(Pos2::new(cr.left() + 10.0, cr.top() + 30.0), Pos2::new(cr.right() - 10.0, cr.bottom() - 26.0));
    p.rect_filled(vu, 5.0, Color32::from_rgb(0xf2, 0xe6, 0xc4));
    let pivot = Pos2::new(vu.center().x, vu.bottom() - 6.0);
    let rr = (vu.height() - 12.0).min(vu.width() * 0.55);
    for db in [0.0f32, 2.0, 4.0, 6.0, 10.0, 15.0, 20.0] {
        let a = -std::f32::consts::FRAC_PI_2 + 0.75 - 1.5 * (db / 20.0);
        p.line_segment(
            [Pos2::new(pivot.x + (rr - 6.0) * a.cos(), pivot.y + (rr - 6.0) * a.sin()), Pos2::new(pivot.x + rr * a.cos(), pivot.y + rr * a.sin())],
            Stroke::new(1.5_f32, if db >= 10.0 { Color32::from_rgb(0xb0, 0x30, 0x30) } else { Color32::from_gray(40) }),
        );
    }
    let a = -std::f32::consts::FRAC_PI_2 + 0.75 - 1.5 * (gr.clamp(0.0, 20.0) / 20.0);
    p.line_segment([pivot, Pos2::new(pivot.x + (rr - 3.0) * a.cos(), pivot.y + (rr - 3.0) * a.sin())], Stroke::new(1.5_f32, Color32::from_gray(25)));
    p.text(Pos2::new(cr.center().x, cr.bottom() - 6.0), egui::Align2::CENTER_BOTTOM, format!("GR {gr:.1} dB · {:.1}:1 · out {:.0} dB", h.v("ratio"), gain_to_db_ui(pout)), egui::FontId::proportional(10.0), theme::TEXT);
}

fn gain_to_db_ui(x: f32) -> f32 {
    dragon_engine::dsp::gain_to_db(x.max(1e-6))
}

/// Drum Bus: a pad that flashes with the punch, in / out meters, the smack compressor's gain reduction and
/// the macro settings as rings around the pad.
fn drum_bus(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    p.rect_filled(r, 8.0, Color32::from_rgb(0x12, 0x10, 0x10));
    let t = h.t();
    let (pin, pout, gr, hit) = match h.tele {
        [a, b, c, d, ..] => (*a, *b, *c, *d),
        _ => {
            let ph = (t * 2.0).fract();
            ((-ph * 6.0).exp() * 0.8, (-ph * 4.0).exp() * 0.9, 6.0 * (-ph * 3.0).exp(), (-ph * 12.0).exp())
        }
    };
    let c = r.center() + Vec2::new(0.0, 6.0);
    let rad = r.height() * 0.24;
    // Macro rings: punch, smack, drive, clip, width – each an arc around the pad.
    let rings = [("PUNCH", h.v("punch")), ("SMACK", h.v("smack")), ("DRIVE", h.v("drive")), ("CLIP", h.v("clip")), ("WIDTH", h.v("width"))];
    for (k, (name, v)) in rings.iter().enumerate() {
        let rr = rad + 12.0 + k as f32 * 7.0;
        let a0 = -std::f32::consts::FRAC_PI_2;
        let pts: Vec<Pos2> = (0..=48).map(|i| a0 + std::f32::consts::TAU * (v / 100.0) * i as f32 / 48.0).map(|a| Pos2::new(c.x + rr * a.cos(), c.y + rr * a.sin())).collect();
        p.circle_stroke(c, rr, Stroke::new(3.0_f32, Color32::from_white_alpha(10)));
        if pts.len() > 1 {
            p.add(Shape::line(pts, Stroke::new(3.0_f32, col.gamma_multiply(0.45 + 0.11 * k as f32))));
        }
        let _ = name;
    }
    // The pad flashes on hits.
    p.circle_filled(c, rad, Color32::from_rgb(0x2a, 0x22, 0x22));
    p.circle_filled(c, rad * (0.6 + 0.3 * hit.min(1.0)), col.gamma_multiply(0.25 + 0.6 * hit.min(1.0)));
    p.circle_stroke(c, rad, Stroke::new(2.0_f32, col));
    p.text(c, egui::Align2::CENTER_CENTER, "DRUMS", egui::FontId::proportional(14.0), Color32::WHITE);
    // Meters.
    let meter = |x: f32, v: f32, label: &str| {
        let mr = Rect::from_min_max(Pos2::new(x - 8.0, r.top() + 24.0), Pos2::new(x + 8.0, r.bottom() - 24.0));
        p.rect_filled(mr, 4.0, Color32::from_white_alpha(10));
        let f = ((gain_to_db_ui(v) + 36.0) / 36.0).clamp(0.0, 1.0);
        p.rect_filled(Rect::from_min_max(Pos2::new(mr.left(), mr.bottom() - mr.height() * f), mr.max), 4.0, if f > 0.95 { theme::REC } else { col });
        p.text(Pos2::new(x, mr.top() - 4.0), egui::Align2::CENTER_BOTTOM, label, egui::FontId::proportional(9.0), theme::TEXT_DIM);
    };
    meter(r.left() + 24.0, pin, "IN");
    meter(r.right() - 24.0, pout, "OUT");
    p.text(Pos2::new(r.left() + 46.0, r.top() + 8.0), egui::Align2::LEFT_TOP, format!("punch {:.0}% · smack {:.0}% (GR {gr:.1} dB)", h.v("punch"), h.v("smack")), egui::FontId::proportional(10.0), theme::TEXT);
    p.text(
        Pos2::new(r.right() - 46.0, r.top() + 8.0),
        egui::Align2::RIGHT_TOP,
        format!("low {:+.1} · body {:+.1} · crack {:+.1} · air {:+.1} dB", h.v("low"), h.v("body"), h.v("crack"), h.v("air")),
        egui::FontId::proportional(10.0),
        theme::TEXT_DIM,
    );
    p.text(Pos2::new(r.left() + 46.0, r.bottom() - 8.0), egui::Align2::LEFT_BOTTOM, "rings: punch · smack · drive · clip · width", egui::FontId::proportional(9.0), theme::TEXT_DIM);
}

/// Alter: the pitch and formant shifts as two big vertical sliders around a voice "face" whose mouth (the
/// spectral envelope) moves with the formant and whose wave moves with the pitch; robot mode shows its note.
fn alter(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    p.rect_filled(r, 10.0, Color32::from_rgb(0x14, 0x18, 0x22));
    let t = h.t();
    let pitch = h.v("pitch");
    let formant = h.v("formant");
    let robot = h.v("mode") > 0.5;
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
    slider(r.left() + 34.0, if robot { 0.0 } else { pitch }, "PITCH", col);
    slider(r.right() - 34.0, formant, "FORMANT", Color32::from_rgb(0x5d, 0xe0, 0xa8));
    // The "voice": a spectral envelope (mouth) shifted by the formant, a wave whose period follows the pitch.
    let face = Rect::from_min_max(Pos2::new(r.left() + 70.0, r.top() + 14.0), Pos2::new(r.right() - 70.0, r.bottom() - 14.0));
    p.rect_filled(face, 8.0, Color32::from_rgb(0x1c, 0x22, 0x2e));
    let fshift = 2f32.powf(formant / 12.0);
    let env = |f: f32| -> f32 { (1.0 / (1.0 + ((f / fshift - 700.0) / 260.0).powi(2))) + 0.6 / (1.0 + ((f / fshift - 1700.0) / 380.0).powi(2)) + 0.3 / (1.0 + ((f / fshift - 2800.0) / 500.0).powi(2)) };
    let spec = Rect::from_min_max(Pos2::new(face.left() + 10.0, face.center().y), face.max - Vec2::new(10.0, 10.0));
    let pts: Vec<Pos2> = (0..=120).map(|k| k as f32 / 120.0).map(|u| Pos2::new(spec.left() + u * spec.width(), spec.bottom() - env(100.0 + u * 4400.0).min(1.3) / 1.3 * spec.height())).collect();
    filled_curve(p, &pts, spec.bottom(), Color32::from_rgb(0x5d, 0xe0, 0xa8));
    let base_hz = if robot {
        let midi = 12.0 * (h.v("robot_octave") + 1.0) + h.v("robot_note").round();
        440.0 * 2f32.powf((midi - 69.0) / 12.0)
    } else {
        150.0 * 2f32.powf(pitch / 12.0)
    };
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
    let label = if robot {
        format!("ROBOT · {}{} ({base_hz:.0} Hz)", dragon_engine::effects::alter::ALTER_NOTES[(h.v("robot_note").round() as usize).min(11)], h.v("robot_octave"))
    } else {
        format!("{pitch:+.1} st · formant {formant:+.1} st")
    };
    p.text(Pos2::new(face.center().x, face.top() + 4.0), egui::Align2::CENTER_TOP, label, egui::FontId::proportional(11.0), theme::TEXT);
    if let [hz, out, ..] = h.tele {
        if *hz > 0.0 {
            p.text(Pos2::new(face.right() - 6.0, face.bottom() - 4.0), egui::Align2::RIGHT_BOTTOM, format!("in {hz:.0} Hz > out {out:.0} Hz"), egui::FontId::monospace(9.5), theme::TEXT_DIM);
        }
    }
}

/// Clean Voice: a valve that glows while the tube stage is in, two moving-coil meters for the leveller and
/// the vari-mu gain reduction, and gain-reduction bars for the de-esser, the four multiband faders and the
/// resonance suppressor.
fn clean_voice(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    p.rect_filled(r, 8.0, Color32::from_rgb(0x14, 0x16, 0x1b));
    let t = h.t();
    let tele: Vec<f32> = if h.tele.len() >= 9 {
        h.tele.to_vec()
    } else {
        let w = (t * 2.0).sin().abs();
        vec![3.0 + 3.0 * w, 1.5 + 2.0 * w, 2.0 * w, 1.0, 1.5 * w, 0.8, 1.2, 2.5 * w, if h.v("tube") > 0.5 { 1.0 } else { 0.0 }]
    };
    let tube_on = tele[8] > 0.5;
    // ---- the valve
    let vx = r.left() + 54.0;
    let glass = Rect::from_center_size(Pos2::new(vx, r.center().y - 6.0), Vec2::new(54.0, r.height() - 60.0));
    let glow_id = h.key.with("cv.glow");
    let mut glow: f32 = p.ctx().data(|d| d.get_temp(glow_id)).unwrap_or(if tube_on { 1.0 } else { 0.0 });
    let target = if tube_on { 1.0 } else { 0.0 };
    glow += (target - glow) * if target > glow { 0.05 } else { 0.02 };
    p.ctx().data_mut(|d| d.insert_temp(glow_id, glow));
    let heat = Color32::from_rgb(0xff, 0x8a, 0x3c);
    for k in (1..=5).rev() {
        p.circle_filled(Pos2::new(vx, glass.center().y + 10.0), 14.0 + k as f32 * 6.0, heat.gamma_multiply(0.04 * glow));
    }
    p.rect_filled(glass, egui::CornerRadius { nw: 26, ne: 26, sw: 6, se: 6 }, Color32::from_rgba_unmultiplied(0x90, 0xa0, 0xb0, 22));
    p.rect_stroke(glass, egui::CornerRadius { nw: 26, ne: 26, sw: 6, se: 6 }, Stroke::new(1.0_f32, Color32::from_white_alpha(60)), egui::StrokeKind::Inside);
    // Plates and the heater filament.
    let plate = Rect::from_center_size(Pos2::new(vx, glass.center().y + 4.0), Vec2::new(30.0, glass.height() * 0.45));
    p.rect_filled(plate, 3.0, Color32::from_gray(48));
    p.rect_stroke(plate, 3.0, Stroke::new(1.0_f32, Color32::from_gray(90)), egui::StrokeKind::Inside);
    for k in 0..3 {
        let x = plate.left() + 8.0 + k as f32 * 7.0;
        p.line_segment([Pos2::new(x, plate.top() + 4.0), Pos2::new(x, plate.bottom() - 4.0)], Stroke::new(2.0_f32, heat.gamma_multiply(0.15 + 0.85 * glow * (0.85 + 0.15 * (t * 7.0 + k as f32).sin()))));
    }
    let base = Rect::from_min_max(Pos2::new(glass.left() - 4.0, glass.bottom()), Pos2::new(glass.right() + 4.0, glass.bottom() + 12.0));
    p.rect_filled(base, 3.0, Color32::from_gray(30));
    p.text(Pos2::new(vx, r.bottom() - 8.0), egui::Align2::CENTER_BOTTOM, if tube_on { "TUBE IN" } else { "TUBE OUT" }, egui::FontId::proportional(9.5), if tube_on { heat } else { theme::TEXT_DIM });
    // ---- two VU meters
    let vu = |rect: Rect, gr: f32, label: &str| {
        p.rect_filled(rect, 6.0, Color32::from_rgb(0xec, 0xe2, 0xc6));
        p.rect_stroke(rect, 6.0, Stroke::new(2.0_f32, Color32::from_gray(40)), egui::StrokeKind::Inside);
        let pivot = Pos2::new(rect.center().x, rect.bottom() - 8.0);
        let rad = rect.height() - 22.0;
        let ang = |db: f32| -std::f32::consts::FRAC_PI_2 - 0.9 * (db.clamp(0.0, 20.0) / 20.0) + 0.0;
        // Scale: 0 at the right, −20 at the left; red zone at the heavy end.
        for db in [0.0f32, 3.0, 6.0, 10.0, 15.0, 20.0] {
            let a = ang(db) + 0.45;
            let o = Pos2::new(pivot.x + rad * a.cos(), pivot.y + rad * a.sin());
            let i = Pos2::new(pivot.x + (rad - 7.0) * a.cos(), pivot.y + (rad - 7.0) * a.sin());
            p.line_segment([i, o], Stroke::new(1.5_f32, if db >= 10.0 { Color32::from_rgb(0xc0, 0x30, 0x30) } else { Color32::from_gray(40) }));
            p.text(Pos2::new(pivot.x + (rad + 7.0) * a.cos(), pivot.y + (rad + 7.0) * a.sin()), egui::Align2::CENTER_CENTER, format!("{}", -(db as i32)), egui::FontId::proportional(8.0), Color32::from_gray(50));
        }
        let a = ang(gr) + 0.45;
        p.line_segment([pivot, Pos2::new(pivot.x + (rad - 2.0) * a.cos(), pivot.y + (rad - 2.0) * a.sin())], Stroke::new(1.5_f32, Color32::from_rgb(0x20, 0x20, 0x20)));
        p.circle_filled(pivot, 4.0, Color32::from_gray(30));
        p.text(Pos2::new(rect.center().x, rect.bottom() - 18.0), egui::Align2::CENTER_BOTTOM, label, egui::FontId::proportional(9.0), Color32::from_gray(60));
    };
    let mw = ((r.width() - 120.0) * 0.28).clamp(90.0, 170.0);
    let m1 = Rect::from_min_size(Pos2::new(vx + 46.0, r.top() + 12.0), Vec2::new(mw, (r.height() - 24.0) * 0.62));
    let m2 = Rect::from_min_size(Pos2::new(m1.right() + 10.0, m1.top()), m1.size());
    vu(m1, tele[0], "COMP");
    vu(m2, tele[1], "VARI MU");
    // ---- GR bars
    let bars = [("DE-ESS", tele[2]), ("MB1 LOW", tele[3]), ("MB1 HIGH", tele[4]), ("MB2 LOW", tele[5]), ("MB2 HIGH", tele[6]), ("SOOTHE", tele[7])];
    let bx0 = m2.right() + 16.0;
    let bw = ((r.right() - 12.0 - bx0) / bars.len() as f32).max(18.0);
    for (k, (name, gr)) in bars.iter().enumerate() {
        let br = Rect::from_min_size(Pos2::new(bx0 + k as f32 * bw + 3.0, r.top() + 22.0), Vec2::new(bw - 10.0, r.height() - 66.0));
        p.rect_filled(br, 3.0, Color32::from_white_alpha(10));
        let f = (gr / 12.0).clamp(0.0, 1.0);
        p.rect_filled(Rect::from_min_max(br.min, Pos2::new(br.right(), br.top() + br.height() * f)), 3.0, col);
        p.text(Pos2::new(br.center().x, br.bottom() + 4.0), egui::Align2::CENTER_TOP, name.replace(' ', "\n"), egui::FontId::proportional(8.0), theme::TEXT_DIM);
        p.text(Pos2::new(br.center().x, br.top() - 3.0), egui::Align2::CENTER_BOTTOM, format!("{gr:.1}"), egui::FontId::monospace(8.5), theme::TEXT);
    }
    // Small readouts under the meters.
    p.text(Pos2::new(m1.left(), r.bottom() - 10.0), egui::Align2::LEFT_BOTTOM, format!("comp {:.0}% · vari mu {:.0}%", h.v("comp"), h.v("varimu")), egui::FontId::proportional(10.0), theme::TEXT);
}

/// Dragon Particle: one big glowing dial (drag or scroll it) ringed by orbiting particles that grow in
/// number and energy with the amount, the stage name inside, input / output meters at the sides and the
/// gain reduction and auto-gain readouts.
fn particle(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    use dragon_engine::effects::DragonParticle;
    // Deep background with a faint radial glow.
    p.rect_filled(r, 10.0, Color32::from_rgb(0x0c, 0x0d, 0x11));
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
    // The dial: track, value arc, glossy cap.
    let a0 = std::f32::consts::PI * 0.75;
    let sweep = std::f32::consts::PI * 1.5;
    let arc = |from: f32, to: f32, rr: f32| -> Vec<Pos2> { (0..=64).map(|k| from + (to - from) * k as f32 / 64.0).map(|x| Pos2::new(c.x + rr * x.cos(), c.y + rr * x.sin())).collect() };
    p.add(Shape::line(arc(a0, a0 + sweep, rad), Stroke::new(8.0_f32, Color32::from_white_alpha(14))));
    p.add(Shape::line(arc(a0, a0 + sweep * a, rad), Stroke::new(14.0_f32, ember.gamma_multiply(0.25))));
    p.add(Shape::line(arc(a0, a0 + sweep * a, rad), Stroke::new(6.0_f32, ember)));
    p.circle_filled(c, rad - 14.0, Color32::from_rgb(0x1a, 0x1c, 0x22));
    p.circle_stroke(c, rad - 14.0, Stroke::new(1.0_f32, Color32::from_white_alpha(30)));
    let ang = a0 + sweep * a;
    p.line_segment([Pos2::new(c.x + (rad - 30.0) * ang.cos(), c.y + (rad - 30.0) * ang.sin()), Pos2::new(c.x + (rad - 18.0) * ang.cos(), c.y + (rad - 18.0) * ang.sin())], Stroke::new(3.0_f32, Color32::WHITE));
    p.text(c - Vec2::new(0.0, 10.0), egui::Align2::CENTER_CENTER, format!("{:.0}", a * 100.0), egui::FontId::proportional(30.0), Color32::WHITE);
    p.text(c + Vec2::new(0.0, 16.0), egui::Align2::CENTER_CENTER, DragonParticle::mode_name(a).to_uppercase(), egui::FontId::proportional(10.0), ember);
    // Side meters: input (left) and output (right), RMS on a −48 … 0 dB scale.
    let (vin, vout, gr, ag) = match h.tele {
        [i, o, g, a2, ..] => (*i, *o, *g, *a2),
        _ => {
            let w = 0.25 + 0.05 * (t * 4.0).sin();
            (w, w * (1.0 + a * 1.5), a * 4.0, 0.0)
        }
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

/// The synth's display: the mixed oscillators as one scrolling waveform (left) and the amp envelope
/// with a playhead running through it (right).
pub fn synth_hero(ui: &mut Ui, s: &dragon_engine::model::SynthParams, color: Color32, time: Option<f32>) {
    use dragon_engine::model::Waveform;
    let w = ui.available_width().max(260.0);
    let (r, _) = ui.allocate_exact_size(Vec2::new(w, 120.0), egui::Sense::hover());
    let p = ui.painter_at(r);
    grid(&p, r, false);
    let t = time.unwrap_or(0.0);
    let split = r.left() + r.width() * 0.6;
    let wr = Rect::from_min_max(r.min + Vec2::new(8.0, 8.0), Pos2::new(split - 8.0, r.bottom() - 8.0));
    let osc = |wave: Waveform, ph: f32, pw: f32| -> f32 {
        let x = ph.rem_euclid(1.0);
        match wave {
            Waveform::Saw => 2.0 * x - 1.0,
            Waveform::Square => {
                if x < pw {
                    1.0
                } else {
                    -1.0
                }
            }
            Waveform::Triangle => 1.0 - 4.0 * (x - 0.5).abs(),
            Waveform::Sine => (x * TAU).sin(),
            Waveform::Noise => ((x * 9301.0 + 49297.0).sin() * 233280.0).fract() * 2.0 - 1.0,
        }
    };
    let total: f32 = s.osc.iter().map(|o| o.level).sum::<f32>().max(0.001);
    let pts: Vec<Pos2> = (0..=240)
        .map(|k| {
            let u = k as f32 / 240.0;
            let y: f32 = s.osc.iter().map(|o| o.level * osc(o.wave, (u * 2.0 + t * 0.25) * 2f32.powf(o.coarse as f32 / 12.0 + o.fine / 1200.0), o.pulse_width)).sum::<f32>() / total;
            Pos2::new(wr.left() + u * wr.width(), wr.center().y - y.clamp(-1.2, 1.2) * wr.height() * 0.42)
        })
        .collect();
    filled_curve(&p, &pts, wr.center().y, color);
    // Amp envelope.
    let er = Rect::from_min_max(Pos2::new(split + 8.0, r.top() + 18.0), r.max - Vec2::new(8.0, 8.0));
    let e = &s.amp_adsr;
    let (a, d, rl) = (e.attack.max(0.001), e.decay.max(0.001), e.release.max(0.001));
    let hold = 0.25 * (a + d + rl);
    let total_t = a + d + hold + rl;
    let x_of = |tt: f32| er.left() + tt / total_t * er.width();
    let y_of = |v: f32| er.bottom() - v * er.height();
    let env = vec![Pos2::new(x_of(0.0), y_of(0.0)), Pos2::new(x_of(a), y_of(1.0)), Pos2::new(x_of(a + d), y_of(e.sustain)), Pos2::new(x_of(a + d + hold), y_of(e.sustain)), Pos2::new(x_of(total_t), y_of(0.0))];
    filled_curve(&p, &env, er.bottom(), theme::ACCENT_2);
    for q in &env[1..4] {
        p.circle_filled(*q, 3.5, Color32::WHITE);
    }
    if time.is_some() {
        let ph = (t / (total_t + 0.5)).fract() * (total_t + 0.5);
        if ph <= total_t {
            p.line_segment([Pos2::new(x_of(ph), er.top()), Pos2::new(x_of(ph), er.bottom())], Stroke::new(1.0_f32, Color32::WHITE.gamma_multiply(0.6)));
        }
    }
    p.text(Pos2::new(er.left(), r.top() + 5.0), egui::Align2::LEFT_TOP, "AMP ENVELOPE", egui::FontId::proportional(9.0), theme::TEXT_DIM);
    p.rect_stroke(r, 6.0, Stroke::new(1.0_f32, color.gamma_multiply(0.4)), egui::StrokeKind::Inside);
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
