//! Crucible (saturator): an alchemist's cauldron. Three jars of ingredients (the parallel tube
//! saturations: warm 6SN7, hot 12AX7, crisp EF86) pour into the brew as far as their amounts go, the
//! liquid takes their blended colour and boils with the signal driving it (the real input level), the
//! fire burns with the drive, and the brew is drawn off through a copper pipe into a phial that fills
//! with the mix. Everything is a control: drag a jar up / down for its amount, the cauldron for the
//! drive, the phial for the mix; double-click resets. A small amber screen shows the brew's curve.
//! Artwork generated with Higgsfield; the liquid, streams, steam and fire are drawn here.

use super::fx_mod::{art, glow, halo};
use super::fx_ui::Hero;
use dragon_engine::dsp::{db_to_gain, gain_to_db};
use egui::{Color32, Pos2, Rect, Shape, Stroke, Ui, Vec2};
use std::f32::consts::{PI, TAU};

const WORKSHOP: &[u8] = include_bytes!("../../assets/fx/workshop.jpg");
const CAULDRON: &[u8] = include_bytes!("../../assets/fx/cauldron.png");
const PHIAL: &[u8] = include_bytes!("../../assets/fx/phial.png");
const JARS: [(&str, &[u8]); 3] =
    [("fx-jar-amber", include_bytes!("../../assets/fx/jar_amber.png")), ("fx-jar-crimson", include_bytes!("../../assets/fx/jar_crimson.png")), ("fx-jar-emerald", include_bytes!("../../assets/fx/jar_emerald.png"))];
const ING_IDS: [&str; 3] = ["ing_warm", "ing_hot", "ing_crisp"];
const ING_COL: [Color32; 3] = [Color32::from_rgb(0xff, 0xae, 0x2a), Color32::from_rgb(0xff, 0x2e, 0x3c), Color32::from_rgb(0x4c, 0xf0, 0x5a)];
/// The colour of the plain character path (before any ingredient).
const BASE_COL: Color32 = Color32::from_rgb(0xff, 0x7a, 0x30);
const CREAM: Color32 = Color32::from_rgb(0xf1, 0xe4, 0xc8);
const COPPER: Color32 = Color32::from_rgb(0xb8, 0x6a, 0x38);

/// Where everything sits.
pub struct Layout {
    pub jars: [Rect; 3],
    pub cauldron: Rect,
    pub phial: Rect,
    pub curve: Rect,
}

pub fn layout(r: Rect) -> Layout {
    let inner = r.shrink(8.0);
    let h = inner.height();
    let w = inner.width();
    let jar_h = (h * 0.62).min(150.0);
    let jar_w = jar_h * 0.52;
    let zone = Rect::from_min_size(inner.min, Vec2::new(w * 0.33, h));
    let gap = (zone.width() - 3.0 * jar_w) / 4.0;
    let jars = std::array::from_fn(|k| Rect::from_min_size(Pos2::new(zone.left() + gap + k as f32 * (jar_w + gap), inner.top() + 14.0), Vec2::new(jar_w, jar_h)));
    let ch = h * 0.86;
    let cw = ch * 390.0 / 372.0;
    let cauldron = Rect::from_center_size(Pos2::new(inner.left() + w * 0.50, inner.bottom() - ch / 2.0 - 2.0), Vec2::new(cw, ch));
    let ph = h * 0.78;
    let pw = ph * 110.0 / 216.0;
    let phial = Rect::from_min_size(Pos2::new(inner.left() + w * 0.69, inner.top() + h * 0.08), Vec2::new(pw, ph));
    let curve = Rect::from_min_max(Pos2::new(inner.left() + w * 0.80, inner.top() + 10.0), Pos2::new(inner.right(), inner.bottom() - 26.0));
    Layout { jars, cauldron, phial, curve }
}

/// The ingredients' amounts and the brew's colour (the character path fills the rest).
fn brew(h: &Hero) -> ([f32; 3], Color32) {
    let a: [f32; 3] = std::array::from_fn(|k| h.v(ING_IDS[k]).clamp(0.0, 1.0));
    let total: f32 = a.iter().sum();
    let base = (1.0 - total).max(0.0);
    let n = (base + total).max(1e-6);
    let mix = |f: fn(Color32) -> u8| -> u8 { ((f(BASE_COL) as f32 * base + (0..3).map(|k| f(ING_COL[k]) as f32 * a[k]).sum::<f32>()) / n) as u8 };
    (a, Color32::from_rgb(mix(|c| c.r()), mix(|c| c.g()), mix(|c| c.b())))
}

/// The cauldron's mouth (the liquid's surface) as an ellipse: centre and radii.
fn mouth(c: Rect) -> (Pos2, Vec2) {
    (Pos2::new(c.left() + c.width() * 0.505, c.top() + c.height() * 0.13), Vec2::new(c.width() * 0.32, c.height() * 0.05))
}

fn ellipse(c: Pos2, rad: Vec2, n: usize) -> Vec<Pos2> {
    (0..n).map(|k| k as f32 / n as f32 * TAU).map(|a| Pos2::new(c.x + rad.x * a.cos(), c.y + rad.y * a.sin())).collect()
}

/// The brew's transfer curve at input `x` (what the amber screen draws).
fn brew_curve(h: &Hero, x: f32) -> f32 {
    let drive = db_to_gain(h.v("drive"));
    let tubes = dragon_engine::dsp::tube::TUBES;
    let (a, _) = brew(h);
    let total: f32 = a.iter().sum();
    let base = (1.0 - total).max(0.0);
    let d = x * drive;
    let mode = h.v("mode").round() as usize;
    let mut y = if base > 0.0 {
        base * if mode == 0 { dragon_engine::dsp::tube::tube_curve(&tubes[(h.v("tube").round() as usize).min(tubes.len() - 1)], h.v("bias"), d) } else { dragon_engine::effects::saturation_curve(mode, d) }
    } else {
        0.0
    };
    for (k, (_, model, extra)) in dragon_engine::effects::CRUCIBLE_INGREDIENTS.iter().enumerate() {
        if a[k] > 0.0 {
            let g = db_to_gain(*extra);
            y += dragon_engine::dsp::tube::tube_curve(&tubes[*model], h.v("bias"), d * g) / g.sqrt() * a[k];
        }
    }
    y / (base + total).max(1e-6) * db_to_gain(h.v("output"))
}

pub fn draw(p: &egui::Painter, r: Rect, h: &Hero) {
    let t = h.t();
    let ctx = p.ctx().clone();
    p.rect_filled(r, 6.0, Color32::from_rgb(0x0a, 0x06, 0x04));
    if let Some(tx) = art(&ctx, "fx-workshop", WORKSHOP) {
        let ts = tx.size_vec2();
        let (ra, ta) = (r.width() / r.height(), ts.x / ts.y);
        let uv = if ra > ta {
            let hh = ta / ra;
            Rect::from_min_max(Pos2::new(0.0, 0.5 - hh / 2.0), Pos2::new(1.0, 0.5 + hh / 2.0))
        } else {
            let ww = ra / ta;
            Rect::from_min_max(Pos2::new(0.5 - ww / 2.0, 0.0), Pos2::new(0.5 + ww / 2.0, 1.0))
        };
        p.image(tx.id(), r, uv, Color32::from_gray(200));
    }
    let l = layout(r);
    let (amounts, colour) = brew(h);
    let drive_db = h.v("drive");
    // Heat: how hard the signal drives the brew right now (the real input level times the drive); a
    // lit fire with no sound only simmers.
    let level = match h.tele {
        [i, ..] => *i,
        _ => 0.0,
    };
    let hot_db = gain_to_db(level.max(1e-6)) + drive_db;
    let heat_target = if h.enabled { (0.12 + ((hot_db + 30.0) / 36.0).clamp(0.0, 1.0) * 0.88).clamp(0.0, 1.0) } else { 0.0 };
    let (heat, moving) = super::analog::spring(&ctx, h.key.with("crucible.heat"), heat_target, 1.2, 1.0, h.time.is_some());
    let hold: usize = ctx.data(|d| d.get_temp(h.key.with("crucible.hot"))).unwrap_or(0);
    // ---- the fire under the cauldron (its size is the drive)
    let fire_k = ((drive_db / 36.0).clamp(0.0, 1.0) * 0.8 + 0.2) * if h.enabled { 1.0 } else { 0.15 };
    let base_y = l.cauldron.bottom() - l.cauldron.height() * 0.05;
    halo(p, Pos2::new(l.cauldron.center().x, base_y), l.cauldron.width() * 0.7, Color32::from_rgb(0xff, 0x6a, 0x1a), 0.45 * fire_k);
    for k in 0..14 {
        let u = k as f32 / 13.0 - 0.5;
        let flick = 0.7 + 0.3 * (t * (7.0 + k as f32) + k as f32 * 1.7).sin();
        let fh = l.cauldron.height() * 0.22 * fire_k * flick * (1.0 - u.abs() * 1.2).max(0.2);
        let x = l.cauldron.center().x + u * l.cauldron.width() * 0.55;
        let tip = Pos2::new(x + (t * 3.0 + k as f32).sin() * 3.0, base_y - fh);
        p.add(Shape::convex_polygon(vec![Pos2::new(x - 7.0, base_y), tip, Pos2::new(x + 7.0, base_y)], glow(Color32::from_rgb(0xff, 0x8a, 0x2a), 0.55), Stroke::NONE));
        p.add(Shape::convex_polygon(vec![Pos2::new(x - 3.0, base_y), Pos2::new(tip.x, base_y - fh * 0.55), Pos2::new(x + 3.0, base_y)], glow(Color32::from_rgb(0xff, 0xe0, 0x90), 0.6), Stroke::NONE));
    }
    // ---- the cauldron and its brew
    if let Some(tx) = art(&ctx, "fx-cauldron", CAULDRON) {
        let tint = if hold == 4 { Color32::WHITE } else { Color32::from_gray(225) };
        p.image(tx.id(), l.cauldron, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), tint);
    }
    let (mc, mr) = mouth(l.cauldron);
    let surface = ellipse(mc + Vec2::new(0.0, mr.y * 0.15), mr * 0.97, 48);
    p.add(Shape::convex_polygon(surface.clone(), colour.gamma_multiply(0.85), Stroke::NONE));
    halo(p, mc, mr.x * 1.1, colour, 0.25 + 0.4 * heat);
    // A brighter swirl of each ingredient on the surface.
    let lp = p.with_clip_rect(Rect::from_center_size(mc, mr * 2.0));
    for (k, a) in amounts.iter().enumerate() {
        if *a > 0.01 {
            let ang = t * 0.6 + k as f32 * TAU / 3.0;
            let at = mc + Vec2::new(ang.cos() * mr.x * 0.45, ang.sin() * mr.y * 0.45);
            halo(&lp, at, mr.x * 0.5 * a.sqrt(), ING_COL[k], 0.35 * a);
        }
    }
    // Bubbles: more and bigger the hotter it boils.
    let n = (3.0 + 26.0 * heat) as u32;
    for i in 0..n {
        let hsh = |x: u32| ((x.wrapping_mul(2_654_435_761) >> 8) % 10_000) as f32 / 10_000.0;
        let ph = t * (0.5 + 1.6 * heat) * (0.6 + 0.8 * hsh(i)) + hsh(i + 99);
        let cyc = ph.floor() as u32;
        let f = ph.fract();
        let ang = hsh(i * 7 + cyc) * TAU;
        let rr = hsh(i * 13 + cyc).sqrt() * 0.85;
        let at = mc + Vec2::new(ang.cos() * mr.x * rr, ang.sin() * mr.y * rr + mr.y * 0.1);
        let size = (2.0 + 5.0 * heat * hsh(i * 3)) * (f * PI).sin();
        if size > 0.3 {
            lp.circle_stroke(at, size, Stroke::new(1.2_f32, Color32::from_rgba_unmultiplied(255, 240, 210, (120.0 * (1.0 - f)) as u8)));
            lp.circle_filled(at - Vec2::new(size * 0.3, size * 0.3), size * 0.25, Color32::from_white_alpha(140));
        }
    }
    // Steam rising.
    for i in 0..(4.0 + 8.0 * heat) as u32 {
        let f = (t * 0.25 + i as f32 * 0.137).fract();
        let x = mc.x + ((i as f32 * 1.9).sin() * 0.6 + (t * 0.7 + i as f32).sin() * 0.25) * mr.x * (0.4 + f);
        let y = mc.y - f * l.cauldron.height() * 0.55;
        halo(p, Pos2::new(x, y), 10.0 + 22.0 * f, colour.linear_multiply(0.6), 0.12 * (1.0 - f) * (0.3 + heat));
    }
    // ---- the jars and their streams into the brew
    for k in 0..3 {
        let jr = l.jars[k];
        let a = amounts[k];
        if let Some(tx) = art(&ctx, JARS[k].0, JARS[k].1) {
            let lift = if hold == k + 1 { -3.0 } else { 0.0 };
            halo(p, jr.center(), jr.width() * (0.7 + 0.5 * a), ING_COL[k], 0.15 + 0.35 * a);
            p.image(tx.id(), jr.translate(Vec2::new(0.0, lift)), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::from_gray((150.0 + 105.0 * (0.35 + 0.65 * a)) as u8));
        }
        // Its amount: an arc under the jar, and the name.
        let c = Pos2::new(jr.center().x, jr.bottom() + 16.0);
        let arc = |to: f32| -> Vec<Pos2> { (0..=24).map(|j| PI * 0.85 + (PI * 1.3) * to * j as f32 / 24.0).map(|q| Pos2::new(c.x + 11.0 * q.cos(), c.y + 11.0 * q.sin())).collect() };
        p.add(Shape::line(arc(1.0), Stroke::new(2.0_f32, Color32::from_white_alpha(30))));
        if a > 0.002 {
            p.add(Shape::line(arc(a), Stroke::new(2.5_f32, ING_COL[k])));
        }
        p.text(c, egui::Align2::CENTER_CENTER, format!("{:.0}", a * 100.0), egui::FontId::monospace(8.5), CREAM);
        let names = ["WARM TUBE", "HOT TRIODE", "CRISP PENTODE"];
        p.text(Pos2::new(c.x, c.y + 16.0), egui::Align2::CENTER_TOP, names[k], egui::FontId::proportional(9.0), if hold == k + 1 { Color32::WHITE } else { CREAM.gamma_multiply(0.85) });
        if a > 0.002 {
            // The pour: from the spout, an arc down into the cauldron, as thick as the amount.
            let from = Pos2::new(jr.right() - jr.width() * 0.08, jr.top() + jr.height() * 0.12);
            let to = Pos2::new(mc.x - mr.x * 0.55 + k as f32 * mr.x * 0.4, mc.y + mr.y * 0.2);
            let ctrl = Pos2::new((from.x + to.x) / 2.0, from.y - 30.0);
            let pts: Vec<Pos2> = (0..=32)
                .map(|j| j as f32 / 32.0)
                .map(|u| Pos2::new((1.0 - u).powi(2) * from.x + 2.0 * (1.0 - u) * u * ctrl.x + u * u * to.x, (1.0 - u).powi(2) * from.y + 2.0 * (1.0 - u) * u * ctrl.y + u * u * to.y))
                .collect();
            let wdt = 1.5 + 5.0 * a;
            p.add(Shape::line(pts.clone(), Stroke::new(wdt + 6.0, glow(ING_COL[k], 0.25))));
            p.add(Shape::line(pts.clone(), Stroke::new(wdt, ING_COL[k])));
            // Grains flowing along it.
            for g in 0..6 {
                let u = ((t * (0.6 + a) + g as f32 / 6.0).fract() * 32.0) as usize;
                if let Some(q) = pts.get(u) {
                    p.circle_filled(*q, wdt * 0.45, Color32::from_white_alpha(160));
                }
            }
        }
    }
    // ---- the pipe and the phial (the mix)
    let mix = h.v("mix").clamp(0.0, 1.0);
    let pr = l.phial;
    let out_pt = Pos2::new(l.cauldron.right() - l.cauldron.width() * 0.1, l.cauldron.top() + l.cauldron.height() * 0.32);
    let in_pt = Pos2::new(pr.left() + pr.width() * 0.2, pr.top() + pr.height() * 0.8);
    let pipe = vec![out_pt, Pos2::new((out_pt.x + in_pt.x) / 2.0, out_pt.y), Pos2::new((out_pt.x + in_pt.x) / 2.0, in_pt.y), in_pt];
    p.add(Shape::line(pipe.clone(), Stroke::new(7.0_f32, Color32::from_rgb(0x4a, 0x26, 0x12))));
    p.add(Shape::line(pipe.clone(), Stroke::new(5.0_f32, COPPER)));
    p.add(Shape::line(pipe.iter().map(|q| *q - Vec2::new(0.0, 1.5)).collect(), Stroke::new(1.5_f32, Color32::from_rgb(0xf0, 0xb0, 0x80))));
    if mix > 0.002 {
        for g in 0..5 {
            let u = (t * 0.5 + g as f32 / 5.0).fract();
            let seg = (u * 3.0).floor() as usize;
            let (a0, b0) = (pipe[seg.min(2)], pipe[(seg + 1).min(3)]);
            let f = u * 3.0 - seg as f32;
            p.circle_filled(a0 + (b0 - a0) * f, 1.6, colour);
        }
    }
    // Liquid in the phial: the bulb fills first, then the neck.
    let bulb_c = Pos2::new(pr.left() + pr.width() * 0.5, pr.top() + pr.height() * 0.75);
    let bulb_r = pr.width() * 0.46;
    let neck = Rect::from_min_max(Pos2::new(pr.center().x - pr.width() * 0.08, pr.top() + pr.height() * 0.14), Pos2::new(pr.center().x + pr.width() * 0.08, bulb_c.y - bulb_r * 0.7));
    let top_y = {
        let bulb_top = bulb_c.y - bulb_r;
        let bulb_bot = bulb_c.y + bulb_r;
        let split = 0.78;
        if mix <= split {
            bulb_bot - (bulb_bot - bulb_top) * (mix / split)
        } else {
            neck.bottom() - (neck.bottom() - neck.top()) * ((mix - split) / (1.0 - split))
        }
    };
    if mix > 0.002 {
        let below = Rect::from_min_max(Pos2::new(pr.left(), top_y), pr.max);
        let lp = p.with_clip_rect(below);
        lp.circle_filled(bulb_c, bulb_r * 0.96, colour.gamma_multiply(0.9));
        lp.rect_filled(neck, 2.0, colour.gamma_multiply(0.9));
        halo(p, bulb_c, bulb_r * 1.6, colour, 0.2 + 0.35 * mix);
        // A gentle surface glint.
        lp.line_segment([Pos2::new(bulb_c.x - bulb_r * 0.6, top_y + 1.0), Pos2::new(bulb_c.x + bulb_r * 0.6, top_y + 1.0)], Stroke::new(1.2_f32, Color32::from_white_alpha(120)));
    }
    if let Some(tx) = art(&ctx, "fx-phial", PHIAL) {
        p.image(tx.id(), pr, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), if hold == 5 { Color32::WHITE } else { Color32::from_gray(230) });
    }
    p.text(Pos2::new(pr.center().x, pr.bottom() + 4.0), egui::Align2::CENTER_TOP, format!("MIX {:.0} %", mix * 100.0), egui::FontId::proportional(10.0), if hold == 5 { Color32::WHITE } else { CREAM });
    // ---- the brew's curve, on a small amber screen
    let cr = l.curve;
    super::fx_mod::screen(p, cr, 6.0);
    let g = cr.shrink(8.0);
    for k in 0..=4 {
        let x = g.left() + g.width() * k as f32 / 4.0;
        let y = g.top() + g.height() * k as f32 / 4.0;
        p.line_segment([Pos2::new(x, g.top()), Pos2::new(x, g.bottom())], Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(0xff, 0xb8, 0x4a, 22)));
        p.line_segment([Pos2::new(g.left(), y), Pos2::new(g.right(), y)], Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(0xff, 0xb8, 0x4a, 22)));
    }
    let pts: Vec<Pos2> = (0..=80).map(|k| -1.0 + 2.0 * k as f32 / 80.0).map(|x| Pos2::new(g.center().x + x * g.width() * 0.5, g.center().y - brew_curve(h, x).clamp(-1.0, 1.0) * g.height() * 0.5)).collect();
    super::fx_mod::trace(p, pts, Color32::from_rgb(0xff, 0xb8, 0x4a), 1.0);
    p.text(cr.left_top() + Vec2::new(8.0, 5.0), egui::Align2::LEFT_TOP, "THE BREW", egui::FontId::proportional(9.0), Color32::from_rgb(0xff, 0xd8, 0x9a));
    super::fx_mod::crt_glass(p, cr);
    // Live: in and out (real peaks).
    if let [i, o, ..] = h.tele {
        let fmt = |v: f32| if v < 1e-5 { "silent".to_string() } else { format!("{:.1} dB", gain_to_db(v)) };
        p.text(Pos2::new(cr.center().x, cr.bottom() + 4.0), egui::Align2::CENTER_TOP, format!("IN {}  OUT {}", fmt(*i), fmt(*o)), egui::FontId::proportional(9.0), CREAM.gamma_multiply(0.85));
    }
    p.text(r.left_top() + Vec2::new(12.0, 6.0), egui::Align2::LEFT_TOP, format!("DRIVE {:.1} dB  ·  FIRE {:.0} %", drive_db, fire_k * 100.0), egui::FontId::proportional(10.0), CREAM);
    if moving || h.time.is_some() {
        ctx.request_repaint();
    }
}

/// Handles: 1–3 the jars (amounts), 4 the cauldron (drive), 5 the phial (mix).
pub fn interact(ui: &Ui, r: Rect, resp: &egui::Response, h: &Hero) -> Vec<(String, f32)> {
    let mut out = Vec::new();
    let Some(pos) = resp.interact_pointer_pos().or_else(|| resp.hover_pos()) else { return out };
    let l = layout(r);
    let pick = |at: Pos2| -> usize {
        for k in 0..3 {
            if l.jars[k].expand2(Vec2::new(6.0, 30.0)).contains(at) {
                return k + 1;
            }
        }
        if l.phial.expand(6.0).contains(at) {
            return 5;
        }
        if l.cauldron.contains(at) {
            return 4;
        }
        0
    };
    let key = h.key.with("crucible.held");
    if resp.drag_started() {
        let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
        ui.data_mut(|d| d.insert_temp(key, pick(origin)));
    }
    let held: usize = ui.data(|d| d.get_temp(key)).unwrap_or(0);
    let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
    let nudge = |id: &str, du: f32, out: &mut Vec<(String, f32)>| {
        if let Some(prm) = h.def.params.iter().find(|p| p.id == id) {
            out.push((id.to_string(), prm.denormalize((prm.normalize(h.v(id)) + du).clamp(0.0, 1.0))));
        }
    };
    if resp.dragged() {
        let dy = -resp.drag_delta().y * fine;
        match held {
            k @ 1..=3 => nudge(ING_IDS[k - 1], dy / 150.0, &mut out),
            4 => nudge("drive", dy / 200.0, &mut out),
            5 => {
                // The phial fills to where you hold it.
                let u = ((l.phial.bottom() - pos.y) / l.phial.height()).clamp(0.0, 1.0);
                out.push(("mix".into(), if fine < 1.0 { (h.v("mix") + dy / 1500.0).clamp(0.0, 1.0) } else { u }));
            }
            _ => {}
        }
    }
    if resp.drag_stopped() {
        ui.data_mut(|d| d.insert_temp(key, 0usize));
    }
    let under = if held > 0 { held } else { pick(pos) };
    ui.data_mut(|d| d.insert_temp(h.key.with("crucible.hot"), under));
    if resp.double_clicked() {
        let reset = |id: &str, out: &mut Vec<(String, f32)>| {
            if let Some(prm) = h.def.params.iter().find(|p| p.id == id) {
                out.push((id.to_string(), prm.default));
            }
        };
        match pick(pos) {
            k @ 1..=3 => reset(ING_IDS[k - 1], &mut out),
            4 => reset("drive", &mut out),
            5 => reset("mix", &mut out),
            _ => {}
        }
    }
    if under > 0 {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    out
}
