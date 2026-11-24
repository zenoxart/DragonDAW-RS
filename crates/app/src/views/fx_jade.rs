//! Jadefang's face, laid out like a classic channel-strip EQ module: on the left a black anodised panel
//! with the name, a recessed column of backlit voicing keys and the big white INPUT knob (OUTPUT below
//! it); on the right, on the jade plate, one row per band – a large GAIN knob with its printed scale and
//! a smaller FREQ knob with its range, the band's name under it: AIR, HF, HMF, LMF, LF, SUB. No graph:
//! you set it by ear, like the hardware.
//!
//! Knob gestures are the effects' own: drag (Shift = fine), wheel, double-click or Ctrl+click resets.

use super::fx_panel::{self, KnobStyle};
use dragon_engine::effects::jadefang::{AIR_FREQS, MODELS};
use dragon_engine::effects::{EffectDef, ParamInfo};
use egui::{Color32, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use std::collections::BTreeMap;
use std::f32::consts::PI;

const LEFT_W: f32 = 236.0;
const ROW_H: f32 = 84.0;
/// The black panel, a gap, then the rows (gain, freq with its name under it).
const MODULE_W: f32 = LEFT_W + 24.0 + 270.0;

/// The editor window's width for the module (faceplate ears and margins included).
pub fn window_width() -> f32 {
    MODULE_W + 2.0 * (14.0 + super::analog::EAR) + 14.0
}
const WHITE: Color32 = Color32::from_rgb(0xf2, 0xf4, 0xee);
const DIM: Color32 = Color32::from_rgb(0xc8, 0xd8, 0xc8);

/// A band row: (name, gain param, freq param, freq range labels).
type Row = (&'static str, &'static str, Option<&'static str>, (&'static str, &'static str));

/// Effects drawn with their own face instead of a display and the designed panel.
pub fn own_face(id: &str) -> bool {
    id == "jadefang"
}

/// The band rows.
const ROWS: [Row; 6] = [
    ("AIR", "air", Some("air_freq"), ("2.5k", "40k")),
    ("HF", "hf_gain", Some("hf_freq"), ("4k", "16k")),
    ("HMF", "hmf_gain", Some("hmf_freq"), ("1k", "8k")),
    ("LMF", "lmf_gain", Some("lmf_freq"), ("200", "2k")),
    ("LF", "lf_gain", Some("lf_freq"), ("40", "400")),
    ("SUB", "sub", None, ("", "")),
];

/// The height the face needs.
pub fn height() -> f32 {
    ROWS.len() as f32 * ROW_H + 10.0
}

fn value(def: &EffectDef, params: &BTreeMap<String, f32>, id: &str) -> f32 {
    dragon_engine::effects::value(def, params, id)
}

/// A knob at `c`: gestures, drawing, a hover hint. Returns the new value when it changed.
#[allow(clippy::too_many_arguments)]
fn knob(ui: &mut Ui, salt: egui::Id, prm: &ParamInfo, v: f32, c: Pos2, rad: f32, style: KnobStyle, accent: Color32) -> Option<f32> {
    let rect = Rect::from_center_size(c, Vec2::splat(rad * 2.0 + 10.0));
    let mut r = ui.interact(rect, salt.with(prm.id), Sense::click_and_drag());
    let n = fx_panel::knob_input(ui, &mut r, prm.normalize(v), prm.normalize(prm.default), if rad > 30.0 { 260.0 } else { 200.0 }, prm);
    let nv = prm.denormalize(n);
    let active = r.hovered() || r.dragged();
    fx_panel::draw_knob(ui.painter(), c, rad, n, style, accent, active);
    let changed = r.changed();
    let r = r.on_hover_text(format!("{} {} – drag, Shift = fine, double-click resets, right-click for the menu", prm.name, prm.format(nv)));
    if let Some(x) = dragon_engine::effects::find("jadefang").and_then(|d| crate::docs::knob_menu(&r, d, prm, nv)) {
        return Some(x);
    }
    changed.then_some(nv)
}

/// Text at the start / end of a knob's sweep (its printed scale).
fn ends(p: &egui::Painter, c: Pos2, rad: f32, lo: &str, hi: &str, size: f32, col: Color32) {
    let a0 = PI * 0.75;
    let a1 = a0 + PI * 1.5;
    let at = |a: f32| c + Vec2::new(a.cos(), a.sin()) * (rad + 22.0);
    p.text(at(a0) + Vec2::new(-2.0, 4.0), egui::Align2::CENTER_CENTER, lo, egui::FontId::proportional(size), col);
    p.text(at(a1) + Vec2::new(2.0, 4.0), egui::Align2::CENTER_CENTER, hi, egui::FontId::proportional(size), col);
}

/// Draws the face and returns the changes (parameter, new value).
pub fn body(ui: &mut Ui, def: &EffectDef, params: &BTreeMap<String, f32>, accent: Color32, salt: egui::Id) -> Vec<(String, f32)> {
    let w = ui.available_width().max(MODULE_W);
    let (full, _) = ui.allocate_exact_size(Vec2::new(w, height()), Sense::hover());
    // The module is as wide as its controls, centred on the plate.
    let rect = Rect::from_min_size(Pos2::new(full.center().x - MODULE_W / 2.0, full.top()), Vec2::new(MODULE_W, full.height()));
    let p = ui.painter().clone();
    fx_panel::set_ink(WHITE, DIM);
    let mut out = Vec::new();
    let prm = |id: &str| def.param(id).expect("jadefang parameter");

    // ---- the black panel
    let left = Rect::from_min_size(rect.min, Vec2::new(LEFT_W, rect.height()));
    p.rect_filled(left.translate(Vec2::new(0.0, 3.0)), 6.0, Color32::from_black_alpha(120));
    match super::analog::plate_texture(ui.ctx(), super::analog::Finish::Black) {
        Some(t) => {
            p.image(t.id(), left, Rect::from_min_max(Pos2::ZERO, Pos2::new(0.35, 1.0)), Color32::from_gray(120));
        }
        None => {
            p.rect_filled(left, 6.0, Color32::from_rgb(0x16, 0x17, 0x19));
        }
    }
    p.rect_stroke(left, 6.0, Stroke::new(1.5_f32, Color32::from_black_alpha(200)), egui::StrokeKind::Inside);
    p.line_segment([left.left_top() + Vec2::new(6.0, 1.0), left.right_top() + Vec2::new(-6.0, 1.0)], Stroke::new(1.0_f32, Color32::from_white_alpha(40)));
    // Name.
    let cx = left.center().x;
    p.text(Pos2::new(cx, left.top() + 30.0), egui::Align2::CENTER_CENTER, "JADEFANG", egui::FontId::proportional(30.0), WHITE);
    p.line_segment([Pos2::new(cx - 70.0, left.top() + 52.0), Pos2::new(cx + 70.0, left.top() + 52.0)], Stroke::new(1.5_f32, WHITE));
    p.text(Pos2::new(cx, left.top() + 66.0), egui::Align2::CENTER_CENTER, "CHARACTER EQ", egui::FontId::proportional(15.0), WHITE);
    // The keys, in a recessed well.
    let model = value(def, params, "model").round().max(0.0) as usize;
    let key_h = 27.0;
    let gap = 6.0;
    let well = Rect::from_min_size(Pos2::new(left.left() + 18.0, left.top() + 86.0), Vec2::new(LEFT_W - 36.0, MODELS.len() as f32 * (key_h + gap) - gap + 22.0));
    p.rect_filled(well, 6.0, Color32::from_rgb(0x0b, 0x0c, 0x0d));
    p.rect_stroke(well, 6.0, Stroke::new(2.0_f32, Color32::from_black_alpha(230)), egui::StrokeKind::Inside);
    p.line_segment([well.left_bottom() + Vec2::new(6.0, 0.0), well.right_bottom() + Vec2::new(-6.0, 0.0)], Stroke::new(1.0_f32, Color32::from_white_alpha(30)));
    for (k, name) in MODELS.iter().enumerate() {
        let kr = Rect::from_min_size(Pos2::new(well.left() + 12.0, well.top() + 11.0 + k as f32 * (key_h + gap)), Vec2::new(well.width() - 24.0, key_h));
        let resp = ui.interact(kr, salt.with(("key", k)), Sense::click());
        let lit = k == model;
        if lit {
            super::fx_mod::halo(&p, kr.center(), kr.width() * 0.55, Color32::from_rgb(0xff, 0xfa, 0xe8), 0.4);
        }
        p.rect_filled(kr.translate(Vec2::new(0.0, 2.0)), 4.0, Color32::from_black_alpha(200));
        let face = if lit {
            Color32::from_rgb(0xfa, 0xfa, 0xf6)
        } else if resp.hovered() {
            Color32::from_rgb(0xb6, 0xb8, 0xba)
        } else {
            Color32::from_rgb(0x9c, 0x9e, 0xa0)
        };
        p.rect_filled(kr, 4.0, face);
        p.line_segment([kr.left_top() + Vec2::new(3.0, 1.0), kr.right_top() + Vec2::new(-3.0, 1.0)], Stroke::new(1.0_f32, Color32::from_white_alpha(if lit { 230 } else { 120 })));
        p.rect_stroke(kr, 4.0, Stroke::new(1.0_f32, Color32::from_black_alpha(150)), egui::StrokeKind::Inside);
        p.text(kr.center(), egui::Align2::CENTER_CENTER, name.to_uppercase(), egui::FontId::proportional(14.0), Color32::from_rgb(0x1c, 0x1c, 0x1e));
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.on_hover_text(voicing_hint(k)).clicked() && !lit {
            out.push(("model".to_string(), k as f32));
        }
    }
    // INPUT, big and white, with its scale.
    let in_c = Pos2::new(cx, well.bottom() + 62.0);
    let in_rad = 34.0;
    for (n, label) in [(0.0, "-24"), (0.25, "-12"), (0.5, "0"), (0.75, "+12"), (1.0, "+24")] {
        let a = PI * 0.75 + PI * 1.5 * n;
        p.text(in_c + Vec2::new(a.cos(), a.sin()) * (in_rad + 20.0), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(12.0), WHITE);
    }
    let v = value(def, params, "input");
    {
        let ip = prm("input");
        let kr = Rect::from_center_size(in_c, Vec2::splat(in_rad * 2.0 + 10.0));
        let mut r = ui.interact(kr, salt.with("input"), Sense::click_and_drag());
        let n = fx_panel::knob_input(ui, &mut r, ip.normalize(v), ip.normalize(ip.default), 300.0, ip);
        white_knob(&p, in_c, in_rad, n);
        if r.changed() {
            out.push(("input".into(), ip.denormalize(n)));
        }
        r.on_hover_text(format!("Input {} – drives the voicing (the output follows it back); drag, Shift = fine, double-click resets", ip.format(ip.denormalize(n))));
    }
    p.text(Pos2::new(cx, in_c.y + in_rad + 20.0), egui::Align2::CENTER_CENTER, "INPUT", egui::FontId::proportional(15.0), WHITE);
    // OUTPUT, small.
    let out_c = Pos2::new(cx, (in_c.y + in_rad + 62.0).min(left.bottom() - 44.0));
    let v = value(def, params, "output");
    if let Some(nv) = knob(ui, salt, prm("output"), v, out_c, 18.0, KnobStyle::Black, accent) {
        out.push(("output".into(), nv));
    }
    ends(&p, out_c, 18.0, "-12", "+12", 9.5, DIM);
    p.text(Pos2::new(cx, out_c.y + 40.0), egui::Align2::CENTER_CENTER, "OUTPUT", egui::FontId::proportional(11.0), WHITE);

    // ---- the band rows on the jade plate
    let x0 = left.right() + 24.0;
    for (k, (name, gain, freq, (lo, hi))) in ROWS.iter().enumerate() {
        let top = rect.top() + 6.0 + k as f32 * ROW_H;
        let gc = Pos2::new(x0 + 58.0, top + 34.0);
        let gp = prm(gain);
        let v = value(def, params, gain);
        if let Some(nv) = knob(ui, salt, gp, v, gc, 30.0, KnobStyle::Black, accent) {
            out.push(((*gain).to_string(), nv));
        }
        let (glo, ghi) = match *gain {
            "air" => ("0", "+15"),
            "sub" => ("-10", "+10"),
            _ => ("-12", "+12"),
        };
        ends(&p, gc, 30.0, glo, ghi, 10.5, DIM);
        p.text(Pos2::new(gc.x, gc.y + 42.0), egui::Align2::CENTER_CENTER, "GAIN", egui::FontId::proportional(12.0), WHITE);
        let fc = Pos2::new(x0 + 196.0, top + 28.0);
        match freq {
            Some(f) => {
                let fp = prm(f);
                let v = value(def, params, f);
                if let Some(nv) = knob(ui, salt, fp, v, fc, 22.0, KnobStyle::Black, accent) {
                    out.push(((*f).to_string(), if *f == "air_freq" { nv.round() } else { nv }));
                }
                ends(&p, fc, 22.0, lo, hi, 10.0, DIM);
                let shown = if *f == "air_freq" { AIR_FREQS.get(v.round().max(0.0) as usize).copied().unwrap_or("").to_string() } else { fp.format(v) };
                p.text(Pos2::new(fc.x, fc.y + 30.0), egui::Align2::CENTER_CENTER, "FREQ", egui::FontId::proportional(12.0), WHITE);
                let _ = shown;
            }
            None => {
                p.text(Pos2::new(fc.x, fc.y + 8.0), egui::Align2::CENTER_CENTER, "18 Hz", egui::FontId::proportional(11.0), DIM);
                p.text(Pos2::new(fc.x, fc.y + 22.0), egui::Align2::CENTER_CENTER, "very broad", egui::FontId::proportional(10.0), DIM);
            }
        }
        // The band's name, bold under its FREQ knob (as printed on the module).
        let name_pos = Pos2::new(fc.x, fc.y + 49.0);
        p.text(name_pos + Vec2::new(0.6, 0.0), egui::Align2::CENTER_CENTER, *name, egui::FontId::proportional(17.0), WHITE);
        p.text(name_pos, egui::Align2::CENTER_CENTER, *name, egui::FontId::proportional(17.0), WHITE);
        if k + 1 < ROWS.len() {
            let y = top + ROW_H - 3.0;
            p.line_segment([Pos2::new(x0 - 6.0, y), Pos2::new(rect.right() - 4.0, y)], Stroke::new(1.0_f32, Color32::from_black_alpha(70)));
        }
    }
    out
}

/// The big white INPUT knob: a scale of ticks, a white cap with soft shading and a black line.
fn white_knob(p: &egui::Painter, c: Pos2, rad: f32, n: f32) {
    let a0 = PI * 0.75;
    let sweep = PI * 1.5;
    let at = |a: f32, r: f32| c + Vec2::new(a.cos(), a.sin()) * r;
    for t in 0..=24 {
        let a = a0 + sweep * t as f32 / 24.0;
        let long = t % 6 == 0;
        p.line_segment([at(a, rad + 3.0), at(a, rad + if long { 11.0 } else { 7.0 })], Stroke::new(if long { 2.0_f32 } else { 1.2_f32 }, WHITE));
    }
    p.circle_filled(c + Vec2::new(1.5, 4.0), rad, Color32::from_black_alpha(110));
    // Knurled edge, then the cap brightening towards its top-left.
    p.circle_filled(c, rad, Color32::from_gray(200));
    for k in 0..36 {
        let a = k as f32 / 36.0 * std::f32::consts::TAU;
        p.line_segment([at(a, rad - 3.0), at(a, rad)], Stroke::new(1.0_f32, Color32::from_gray(160)));
    }
    for (k, g) in [(0.0, 222u8), (0.18, 232), (0.36, 240), (0.52, 246)] {
        let rr = (rad - 4.0) * (1.0 - k * 0.55);
        p.circle_filled(c - Vec2::new(rr * k * 0.25, rr * k * 0.3), rr, Color32::from_gray(g));
    }
    p.circle_stroke(c, rad - 4.0, Stroke::new(1.0_f32, Color32::from_gray(170)));
    let a = a0 + sweep * n.clamp(0.0, 1.0);
    p.line_segment([at(a, rad * 0.25), at(a, rad - 7.0)], Stroke::new(4.0_f32, Color32::from_rgb(0x14, 0x14, 0x16)));
}

fn voicing_hint(k: usize) -> &'static str {
    match k {
        0 => "Clean scale – transparent, constant Q, no colour",
        1 => "Hoard – console iron: broad bells, a transformer stage the input drives, weight at 50 Hz, a softer top",
        2 => "Ember – valve program EQ: shelves with the dip beside them, warm valve harmonics",
        3 => "Talon – punchy console: the more you boost or cut, the narrower the band; steeper shelves",
        4 => "Skyfire – the open one: extra-broad bands, a stronger Air band, clean",
        _ => "Ancient – vintage passive: broad and gentle, a soft top roll-off",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs one frame of the face with `events`; returns the changes.
    fn frame(ctx: &egui::Context, params: &BTreeMap<String, f32>, events: Vec<egui::Event>) -> Vec<(String, f32)> {
        let def = dragon_engine::effects::find("jadefang").unwrap();
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 800.0))), events, ..Default::default() };
        let mut got = Vec::new();
        let _ = ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| got = body(ui, def, params, Color32::GOLD, egui::Id::new("t")));
        });
        got
    }

    #[test]
    fn keys_pick_the_voicing_and_knobs_turn() {
        let ctx = egui::Context::default();
        let params = dragon_engine::effects::find("jadefang").unwrap().defaults();
        frame(&ctx, &params, vec![]);
        // Where the module sits (centred in the 900 px panel, 8 px margins).
        let full_left = 8.0;
        let full_w = 900.0 - 16.0;
        let left = full_left + full_w / 2.0 - MODULE_W / 2.0;
        let top = 8.0;
        // The third key (Ember).
        let key = Pos2::new(left + LEFT_W / 2.0, top + 86.0 + 11.0 + 2.0 * 33.0 + 13.0);
        frame(&ctx, &params, vec![egui::Event::PointerMoved(key)]);
        frame(&ctx, &params, vec![egui::Event::PointerButton { pos: key, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }]);
        let got = frame(&ctx, &params, vec![egui::Event::PointerButton { pos: key, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }]);
        assert!(got.contains(&("model".to_string(), 2.0)), "{got:?}");
        // Drag the HF gain knob (row 2) up.
        let knob = Pos2::new(left + LEFT_W + 24.0 + 58.0, top + 6.0 + ROW_H + 34.0);
        frame(&ctx, &params, vec![egui::Event::PointerMoved(knob)]);
        frame(&ctx, &params, vec![egui::Event::PointerButton { pos: knob, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }]);
        let mut all = Vec::new();
        for k in 1..=6 {
            all.extend(frame(&ctx, &params, vec![egui::Event::PointerMoved(knob - Vec2::new(0.0, 8.0 * k as f32))]));
        }
        frame(&ctx, &params, vec![egui::Event::PointerButton { pos: knob, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }]);
        assert!(all.iter().any(|(k, v)| k == "hf_gain" && *v > 0.5), "{all:?}");
    }
}
