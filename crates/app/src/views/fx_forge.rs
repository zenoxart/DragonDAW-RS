//! Dragonforge's face: a dragon's smithy. Every control and the sound are those of the drum bus; only the
//! look is its own (art generated with Higgsfield, the moving parts drawn here):
//!
//! - left: a carved dragon head over the hearth – it breathes fire into the coals, harder the more you
//!   drive; the iron DRIVE wheel with INPUT and HIGH-PASS beside it; the WARM / TAPE / CRUNCH lever;
//! - middle: a strip of red-hot iron that shows the drums as they come in (with the input's gain-staging
//!   target), the anvil (sparks on every hit) and HARMONICS with its 50 / 80 Hz switch;
//! - right: COMPRESSION as bellows that squeeze with the gain reduction (MIX, GLUE / PUNCH, LIFT), the
//!   HARMONIC EQ as tongs in glowing slots, PARALLELS and the brass OUTPUT knob;
//! - around it: glowing rune borders, golden claws in the corners, the engraved name and two VU meters.
//!
//! Knob gestures are the effects' own: drag (Shift = fine), wheel, double-click or Ctrl+click resets.

use super::fx_panel::{self, KnobStyle};
use dragon_engine::effects::strips::{DRUM_COLOURS, HEQ_FREQS};
use dragon_engine::effects::{EffectDef, ParamInfo};
use egui::{Color32, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use std::collections::BTreeMap;
use std::f32::consts::PI;

/// The design canvas (scaled to the window).
const W: f32 = 980.0;
const H: f32 = 650.0;

const GOLD: Color32 = Color32::from_rgb(0xe8, 0xc0, 0x6a);
const GOLD_DIM: Color32 = Color32::from_rgb(0xb8, 0x94, 0x52);
const EMBER: Color32 = Color32::from_rgb(0xff, 0x8a, 0x2c);

const HEARTH: &[u8] = include_bytes!("../../assets/fx/forge/hearth.png");
const ANVIL: &[u8] = include_bytes!("../../assets/fx/forge/anvil.png");
const BELLOWS: &[u8] = include_bytes!("../../assets/fx/forge/bellows.png");
const WHEEL: &[u8] = include_bytes!("../../assets/fx/forge/wheel.png");
const TONGS: &[u8] = include_bytes!("../../assets/fx/forge/tongs.png");
const RUNES: &[u8] = include_bytes!("../../assets/fx/forge/runes.jpg");
const CLAW: &[u8] = include_bytes!("../../assets/fx/forge/claw.png");
const DRAGON: &[u8] = include_bytes!("../../assets/fx/forge/dragon.png");
const FLAME: &[u8] = include_bytes!("../../assets/fx/flame.png");
const IRON: &[u8] = include_bytes!("../../assets/fx/plate_forged.jpg");

/// Effects drawn with this face.
pub fn is_forge(id: &str) -> bool {
    id == "drum_bus"
}

/// The editor window's width.
pub fn window_width() -> f32 {
    W + 2.0 * (14.0 + super::analog::EAR) + 14.0
}

fn tex(ctx: &egui::Context, name: &'static str, bytes: &[u8]) -> Option<egui::TextureHandle> {
    super::guitar_ui::texture(ctx, name, bytes)
}

fn full_uv() -> Rect {
    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0))
}

/// The image `t` fitted into `r` keeping its aspect (centred).
fn fit(t: &egui::TextureHandle, r: Rect) -> Rect {
    let [w, h] = t.size();
    let a = w as f32 / h.max(1) as f32;
    let (fw, fh) = if r.width() / r.height() > a { (r.height() * a, r.height()) } else { (r.width(), r.width() / a) };
    Rect::from_center_size(r.center(), Vec2::new(fw, fh))
}

/// Live values the face reads (the effect's telemetry, the unit's output level and the time).
pub struct Live<'a> {
    pub tele: &'a [f32],
    pub level: (f32, f32),
    pub time: Option<f32>,
    pub enabled: bool,
}

struct Face<'a> {
    ui: &'a mut Ui,
    p: egui::Painter,
    o: Pos2,
    s: f32,
    def: &'a EffectDef,
    params: &'a BTreeMap<String, f32>,
    salt: egui::Id,
    out: Vec<(String, f32)>,
}

impl Face<'_> {
    fn pt(&self, x: f32, y: f32) -> Pos2 {
        self.o + Vec2::new(x, y) * self.s
    }
    fn rect(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(self.pt(x0, y0), self.pt(x1, y1))
    }
    fn v(&self, id: &str) -> f32 {
        dragon_engine::effects::value(self.def, self.params, id)
    }
    fn prm(&self, id: &str) -> &'static ParamInfo {
        let def: &'static EffectDef = dragon_engine::effects::find("drum_bus").expect("drum bus");
        def.param(id).expect("drum bus parameter")
    }
    fn font(&self, size: f32) -> egui::FontId {
        egui::FontId::proportional(size * self.s)
    }
    /// A compartment: a sunken iron plate with rivets.
    fn panel(&self, r: Rect) {
        let p = &self.p;
        p.rect_filled(r.translate(Vec2::new(0.0, 3.0 * self.s)), 6.0, Color32::from_black_alpha(150));
        match tex(self.ui.ctx(), "forge-iron", IRON) {
            Some(t) => {
                let uv = Rect::from_min_size(Pos2::new((r.left() * 0.0013).fract(), 0.0), Vec2::new(r.width() / 900.0, r.height() / 600.0));
                p.image(t.id(), r, uv, Color32::from_gray(120));
            }
            None => {
                p.rect_filled(r, 6.0, Color32::from_rgb(0x24, 0x22, 0x22));
            }
        }
        p.rect_stroke(r, 6.0, Stroke::new(2.0_f32, Color32::from_black_alpha(220)), egui::StrokeKind::Inside);
        p.rect_stroke(r.shrink(2.0), 5.0, Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(0xe8, 0xc0, 0x6a, 40)), egui::StrokeKind::Inside);
        for c in [r.left_top() + Vec2::new(7.0, 7.0), r.right_top() + Vec2::new(-7.0, 7.0), r.left_bottom() + Vec2::new(7.0, -7.0), r.right_bottom() + Vec2::new(-7.0, -7.0)] {
            p.circle_filled(c, 2.6 * self.s.max(0.8), Color32::from_rgb(0x3a, 0x34, 0x30));
            p.circle_filled(c - Vec2::new(0.6, 0.6), 1.4 * self.s.max(0.8), Color32::from_rgb(0x8a, 0x7a, 0x66));
        }
    }
    /// A title badge: a dark plate with a gold border and engraved gold letters.
    fn badge(&self, cx: f32, cy: f32, text: &str, size: f32) {
        let g = self.p.layout_no_wrap(text.to_string(), self.font(size), GOLD);
        let half = g.size().x / 2.0 + 14.0 * self.s;
        let c = self.pt(cx, cy);
        let r = Rect::from_center_size(c, Vec2::new(half * 2.0, (size + 12.0) * self.s));
        self.p.rect_filled(r, 4.0, Color32::from_rgb(0x1c, 0x16, 0x12));
        self.p.rect_stroke(r, 4.0, Stroke::new(1.5_f32, GOLD_DIM), egui::StrokeKind::Inside);
        self.p.text(c + Vec2::new(0.0, 1.0), egui::Align2::CENTER_CENTER, text, self.font(size), Color32::from_black_alpha(200));
        self.p.text(c, egui::Align2::CENTER_CENTER, text, self.font(size), GOLD);
    }
    fn label(&self, x: f32, y: f32, text: &str, size: f32) {
        self.p.text(self.pt(x, y), egui::Align2::CENTER_CENTER, text, self.font(size), GOLD);
    }
    /// A knob with the effects' gestures.
    fn knob(&mut self, id: &str, x: f32, y: f32, rad: f32, style: KnobStyle) {
        let prm = self.prm(id);
        let v = self.v(id);
        let c = self.pt(x, y);
        let r = rad * self.s;
        let rect = Rect::from_center_size(c, Vec2::splat(r * 2.0 + 10.0));
        let mut resp = self.ui.interact(rect, self.salt.with(id), Sense::click_and_drag());
        let n = fx_panel::knob_input(self.ui, &mut resp, prm.normalize(v), prm.normalize(prm.default), if r > 30.0 { 280.0 } else { 200.0 }, prm);
        fx_panel::draw_knob(&self.p, c, r, n, style, EMBER, resp.hovered() || resp.dragged());
        if resp.changed() {
            self.out.push((id.to_string(), prm.denormalize(n)));
        }
        let resp = resp.on_hover_text(format!("{} {} – drag, Shift = fine, double-click resets, right-click for the menu", prm.name, prm.format(prm.denormalize(n))));
        if let Some(x) = crate::docs::knob_menu(&resp, self.def, prm, prm.denormalize(n)) {
            self.out.push((id.to_string(), x));
        }
    }
    /// A brass toggle between two choices (a choice parameter with two options).
    fn toggle(&mut self, id: &str, x: f32, y: f32, left: &str, right: &str) {
        let v = self.v(id).round() as i32;
        let c = self.pt(x, y);
        let track = Rect::from_center_size(c, Vec2::new(34.0, 14.0) * self.s);
        let resp = self.ui.interact(track.expand(6.0), self.salt.with(id), Sense::click());
        self.p.rect_filled(track, 7.0 * self.s, Color32::from_rgb(0x12, 0x0e, 0x0c));
        self.p.rect_stroke(track, 7.0 * self.s, Stroke::new(1.0_f32, GOLD_DIM), egui::StrokeKind::Inside);
        let kx = if v == 0 { track.left() + 7.0 * self.s } else { track.right() - 7.0 * self.s };
        self.p.circle_filled(Pos2::new(kx, c.y), 6.5 * self.s, Color32::from_rgb(0xc8, 0x98, 0x48));
        self.p.circle_filled(Pos2::new(kx - 1.5, c.y - 1.5), 3.0 * self.s, Color32::from_rgb(0xf8, 0xdc, 0x98));
        let on = |k: i32| if v == k { GOLD } else { GOLD_DIM.gamma_multiply(0.6) };
        self.p.text(Pos2::new(track.left() - 8.0 * self.s, c.y), egui::Align2::RIGHT_CENTER, left, self.font(11.0), on(0));
        self.p.text(Pos2::new(track.right() + 8.0 * self.s, c.y), egui::Align2::LEFT_CENTER, right, self.font(11.0), on(1));
        let prm = self.prm(id);
        if resp.on_hover_text(format!("{} – click to switch", prm.name)).clicked() {
            self.out.push((id.to_string(), if v == 0 { 1.0 } else { 0.0 }));
        }
    }
}

/// Draws the face and returns the changes.
pub fn body(ui: &mut Ui, def: &EffectDef, params: &BTreeMap<String, f32>, live: &Live, salt: egui::Id) -> Vec<(String, f32)> {
    let avail = ui.available_width().max(600.0);
    let s = (avail / W).min(1.15);
    let (full, _) = ui.allocate_exact_size(Vec2::new(avail, H * s), Sense::hover());
    let o = Pos2::new(full.center().x - W * s / 2.0, full.top());
    let p = ui.painter().clone();
    fx_panel::set_ink(GOLD, GOLD_DIM);
    let mut f = Face { ui, p, o, s, def, params, salt, out: Vec::new() };
    let ctx = f.ui.ctx().clone();
    let motion = live.time.is_some();
    let t = live.time.unwrap_or(0.0);
    let (pin, _pout, gr, rin) = match live.tele {
        [a, b, c, d, ..] => (*a, *b, *c, *d),
        _ => (0.0, 0.0, 0.0, 0.0),
    };
    let hit = live.tele.get(5).copied().unwrap_or(0.0) > 0.5;
    let db = |x: f32| 20.0 * x.max(1e-6).log10();
    let lvl = super::analog::vu_pos(db(live.level.0.max(live.level.1))).clamp(0.0, 1.0);

    // ---- frame: the iron, the rune borders, the claws
    let all = f.rect(0.0, 0.0, W, H);
    f.p.rect_filled(all, 8.0, Color32::from_rgb(0x14, 0x11, 0x10));
    if let Some(rt) = tex(&ctx, "forge-runes", RUNES) {
        for x in [4.0, W - 34.0] {
            let r = f.rect(x, 40.0, x + 30.0, H - 40.0);
            f.p.image(rt.id(), r, full_uv(), Color32::from_gray(230));
            // The runes glow with the level.
            super::fx_mod::halo(&f.p, r.center(), r.height() * 0.4, EMBER, 0.05 + 0.1 * lvl);
        }
    }

    // ---- top: name badge and VU meters
    {
        let r = f.rect(230.0, 10.0, 630.0, 88.0);
        f.p.rect_filled(r, 6.0, Color32::from_rgb(0x1a, 0x14, 0x10));
        f.p.rect_stroke(r, 6.0, Stroke::new(2.5_f32, GOLD_DIM), egui::StrokeKind::Inside);
        f.p.rect_stroke(r.shrink(5.0), 4.0, Stroke::new(1.0_f32, GOLD_DIM.gamma_multiply(0.6)), egui::StrokeKind::Inside);
        f.p.text(f.pt(430.0, 40.0) + Vec2::new(0.0, 2.0), egui::Align2::CENTER_CENTER, "DRAGONFORGE", f.font(38.0), Color32::from_black_alpha(220));
        f.p.text(f.pt(430.0, 40.0), egui::Align2::CENTER_CENTER, "DRAGONFORGE", f.font(38.0), GOLD);
        f.p.text(f.pt(430.0, 72.0), egui::Align2::CENTER_CENTER, "DRUM BUS PROCESSOR", f.font(13.0), GOLD_DIM);
        let (glow, _) = super::analog::spring(&ctx, salt.with("lamp"), if live.enabled { 1.0 } else { 0.0 }, 1.6, 1.0, motion);
        super::analog::lamp(&f.p, f.pt(258.0, 30.0), 6.0 * s, Color32::from_rgb(0xff, 0x9a, 0x3c), glow);
        for (k, (lv, cap)) in [(live.level.0, "L"), (live.level.1, "R")].into_iter().enumerate() {
            let target = if live.enabled { super::analog::vu_pos(db(lv)) } else { 0.0 };
            let (pos, _) = super::analog::spring(&ctx, salt.with(("vu", k)), target, 1.7, 0.72, motion);
            let x0 = 650.0 + k as f32 * 148.0;
            super::analog::vu(&f.p, f.rect(x0, 10.0, x0 + 140.0, 92.0), pos, cap, false, glow, true);
        }
    }

    // ---- left: hearth, drive
    let drive = f.v("drive") / 100.0;
    {
        let r = f.rect(40.0, 100.0, 272.0, 332.0);
        f.panel(r);
        if let Some(ht) = tex(&ctx, "forge-hearth", HEARTH) {
            let fr = fit(&ht, r.shrink(8.0 * s));
            f.p.image(ht.id(), fr, full_uv(), Color32::WHITE);
            // The fire inside the hearth: the more drive, the hotter it burns – more and taller flames that
            // fill the firebox, a colour going from deep red over orange to a yellow-white core, glowing
            // coals, embers rising and the dragon's eyes catching the heat. It flickers with the level.
            let (heat, _) = super::analog::spring(&ctx, salt.with("heat"), drive, 2.5, 0.9, motion);
            let pulse = 0.85 + 0.15 * lvl;
            let fx = |u: f32| fr.left() + fr.width() * u;
            let fy = |v: f32| fr.top() + fr.height() * v;
            // The firebox opening (in the art: 25 … 75 % across, 48 … 78 % down).
            let firebox = Rect::from_min_max(Pos2::new(fx(0.255), fy(0.49)), Pos2::new(fx(0.745), fy(0.785)));
            let fp = f.p.with_clip_rect(firebox);
            let base_y = fy(0.765);
            // Heat behind the flames.
            let back = Color32::from_rgb(0xff, (0x50 as f32 + 0x90 as f32 * heat) as u8, (0x10 as f32 + 0x50 as f32 * heat * heat) as u8);
            super::fx_mod::halo(&fp, Pos2::new(firebox.center().x, base_y), firebox.width() * (0.5 + 0.3 * heat), back, (0.25 + 0.55 * heat) * pulse);
            if let Some(fl) = tex(&ctx, "fx-flame", FLAME) {
                // Outer flames: from a few low tongues at rest to a wall of fire reaching the arch.
                let count = 5 + (heat * 6.0).round() as usize;
                for k in 0..count {
                    let u = if count > 1 { k as f32 / (count - 1) as f32 } else { 0.5 };
                    let x = firebox.left() + firebox.width() * (0.08 + 0.84 * u);
                    let ph = k as f32 * 1.37;
                    let flick = if motion { 0.78 + 0.22 * ((t * 8.3 + ph).sin() * 0.6 + (t * 13.1 + ph * 2.1).sin() * 0.4) } else { 1.0 };
                    let centre = 1.0 - (u - 0.5).abs() * 0.8;
                    let hh = firebox.height() * (0.28 + 0.72 * heat) * centre * flick * pulse;
                    let fw = hh * (0.5 + 0.1 * heat);
                    let sway = if motion { (t * 3.0 + ph).sin() * 2.0 * s } else { 0.0 };
                    let tint = Color32::from_rgb(0xff, (0x88 as f32 + 0x60 as f32 * heat) as u8, (0x50 as f32 + 0x70 as f32 * heat) as u8);
                    fp.image(fl.id(), Rect::from_min_max(Pos2::new(x - fw / 2.0 + sway, base_y - hh), Pos2::new(x + fw / 2.0 + sway, base_y)), full_uv(), tint);
                }
                // The white-hot core once the drive is up.
                if heat > 0.25 {
                    let core = ((heat - 0.25) / 0.75).clamp(0.0, 1.0);
                    for k in 0..3 {
                        let x = firebox.center().x + (k as f32 - 1.0) * firebox.width() * 0.16;
                        let flick = if motion { 0.85 + 0.15 * (t * 11.0 + k as f32 * 2.3).sin() } else { 1.0 };
                        let hh = firebox.height() * 0.55 * core * flick * pulse * if k == 1 { 1.0 } else { 0.75 };
                        let fw = hh * 0.45;
                        fp.image(fl.id(), Rect::from_min_max(Pos2::new(x - fw / 2.0, base_y - hh), Pos2::new(x + fw / 2.0, base_y)), full_uv(), Color32::from_rgba_unmultiplied(0xff, 0xf4, 0xd0, (200.0 * core) as u8));
                    }
                }
            }
            // The coal bed glows hotter.
            for k in 0..4 {
                let x = firebox.left() + firebox.width() * (0.15 + 0.233 * k as f32);
                let breathe = if motion { 0.8 + 0.2 * (t * 2.2 + k as f32 * 1.9).sin() } else { 1.0 };
                super::fx_mod::halo(&fp, Pos2::new(x, base_y), 14.0 * s, Color32::from_rgb(0xff, 0x9a, 0x30), (0.2 + 0.5 * heat) * breathe);
            }
            // Embers rising out of the fire.
            if motion {
                let n_emb = (4.0 + 14.0 * heat) as usize;
                for k in 0..n_emb {
                    let seed = (k as f32 * 0.618_034).fract();
                    let life = 1.2 + seed * 0.8;
                    let age = ((t + seed * 7.0) % life) / life;
                    let x = firebox.left() + firebox.width() * (0.12 + 0.76 * seed) + (t * 2.0 + k as f32).sin() * 4.0 * s;
                    let y = base_y - age * firebox.height() * (0.6 + 0.5 * heat);
                    let a = ((1.0 - age) * 255.0 * (0.4 + 0.6 * heat)) as u8;
                    f.p.circle_filled(Pos2::new(x, y), (1.0 + seed) * s, Color32::from_rgba_unmultiplied(0xff, 0xc8, 0x60, a));
                }
            }
            // The dragon's eyes catch the heat.
            for u in [0.405, 0.595] {
                super::fx_mod::halo(&f.p, Pos2::new(fx(u), fy(0.215)), 9.0 * s, Color32::from_rgb(0xff, 0x70, 0x20), 0.15 + 0.55 * heat * pulse);
            }
        }
        f.badge(156.0, 350.0, "DRIVE", 17.0);
        // The iron wheel.
        let c = f.pt(156.0, 448.0);
        let rad = 66.0 * s;
        let prm = f.prm("drive");
        let mut resp = f.ui.interact(Rect::from_center_size(c, Vec2::splat(rad * 2.0 + 12.0)), salt.with("drive"), Sense::click_and_drag());
        let n = fx_panel::knob_input(f.ui, &mut resp, prm.normalize(f.v("drive")), prm.normalize(prm.default), 320.0, prm);
        let a0 = PI * 0.75;
        for k in 0..=10 {
            let a = a0 + PI * 1.5 * k as f32 / 10.0;
            let lit = (k as f32) / 10.0 <= n + 1e-3;
            f.p.line_segment([c + Vec2::angled(a) * (rad + 4.0), c + Vec2::angled(a) * (rad + if k % 5 == 0 { 13.0 } else { 9.0 })], Stroke::new(2.0_f32, if lit { EMBER } else { GOLD_DIM.gamma_multiply(0.6) }));
            if k % 2 == 0 {
                f.p.text(c + Vec2::angled(a) * (rad + 24.0), egui::Align2::CENTER_CENTER, format!("{}", k * 10), f.font(11.0), GOLD);
            }
        }
        f.p.add(Shape::line((0..=40).map(|i| c + Vec2::angled(a0 + PI * 1.5 * n * i as f32 / 40.0) * (rad + 2.0)).collect(), Stroke::new(3.0_f32, EMBER)));
        super::fx_mod::halo(&f.p, c, rad * 1.1, EMBER, 0.08 + 0.25 * n);
        f.p.circle_filled(c + Vec2::new(2.0, 5.0), rad, Color32::from_black_alpha(140));
        if let Some(wt) = tex(&ctx, "forge-wheel", WHEEL) {
            super::analog::rotated(&f.p, &wt, c, rad, a0 + PI * 1.5 * n + PI / 2.0, Color32::WHITE);
        } else {
            fx_panel::draw_knob(&f.p, c, rad, n, KnobStyle::Brass, EMBER, true);
        }
        if resp.changed() {
            f.out.push(("drive".into(), prm.denormalize(n)));
        }
        resp.on_hover_text(format!("Drive {} – drag, Shift = fine, double-click resets", prm.format(prm.denormalize(n))));
        // Input and high-pass beside the wheel.
        f.knob("input", 64.0, 548.0, 16.0, KnobStyle::Black);
        f.label(64.0, 574.0, "INPUT", 10.5);
        f.knob("drive_hp", 248.0, 548.0, 16.0, KnobStyle::Black);
        f.label(248.0, 574.0, "HIGH-PASS", 10.5);
        // The colour lever.
        let lr = f.rect(56.0, 590.0, 256.0, 640.0);
        f.panel(lr);
        let colour = f.v("colour").round().clamp(0.0, 2.0) as usize;
        let track = f.rect(80.0, 624.0, 232.0, 630.0);
        f.p.rect_filled(track, 3.0, Color32::from_rgb(0x0c, 0x0a, 0x09));
        for (k, full_name) in DRUM_COLOURS.iter().enumerate().take(3) {
            let x = 80.0 + 76.0 * k as f32;
            let name = full_name.split(['(', ')']).nth(1).unwrap_or(full_name).to_uppercase();
            f.p.text(f.pt(x, 604.0), egui::Align2::CENTER_CENTER, name, f.font(11.0), if k == colour { GOLD } else { GOLD_DIM.gamma_multiply(0.6) });
            let hr = f.rect(x - 26.0, 594.0, x + 26.0, 640.0);
            let resp = f.ui.interact(hr, salt.with(("colour", k)), Sense::click());
            if resp.on_hover_text(format!("Drive colour: {full_name}")).clicked() && k != colour {
                f.out.push(("colour".into(), k as f32));
            }
        }
        let kx = 80.0 + 76.0 * colour as f32;
        let (lx, _) = super::analog::spring(&ctx, salt.with("lever"), kx, 4.0, 0.9, motion);
        f.p.circle_filled(f.pt(lx, 627.0) + Vec2::new(1.0, 2.0), 9.0 * s, Color32::from_black_alpha(160));
        f.p.circle_filled(f.pt(lx, 627.0), 9.0 * s, Color32::from_rgb(0xc8, 0x98, 0x48));
        f.p.circle_filled(f.pt(lx, 627.0) - Vec2::new(2.0, 2.0), 4.0 * s, Color32::from_rgb(0xf8, 0xdc, 0x98));
    }

    // ---- middle: red-hot iron display, anvil, harmonics
    {
        let r = f.rect(282.0, 100.0, 532.0, 262.0);
        f.panel(r);
        let screen = r.shrink(12.0 * s);
        f.p.rect_filled(screen, 4.0, Color32::from_rgb(0x1a, 0x08, 0x04));
        // The drums as they come in: a history of input peaks, drawn as glowing hot metal.
        // One column every 1/30 s holding the loudest block since the last one (frame-rate independent).
        let key = salt.with("forge.hist");
        let (mut hist, mut last, mut hold): (Vec<f32>, f32, f32) = ctx.data(|d| d.get_temp(key)).unwrap_or_else(|| (vec![0.0; 90], t, 0.0));
        if live.enabled {
            hold = hold.max(pin.min(1.5));
            let steps = if motion { ((t - last) * 30.0).floor().clamp(0.0, 90.0) as usize } else { 1 };
            for _ in 0..steps {
                hist.remove(0);
                hist.push(hold);
                hold = 0.0;
            }
            if steps > 0 {
                last = t;
            }
            ctx.data_mut(|d| d.insert_temp(key, (hist.clone(), last, hold)));
        }
        let mid = screen.top() + 46.0 * s;
        let hh = 34.0 * s;
        let step = screen.width() / (hist.len() - 1) as f32;
        // Drawn on a dB scale (−48 … 0 dBFS), so quiet drums still show.
        let amp = |v: f32| ((db(v) + 48.0) / 48.0).clamp(0.0, 1.0);
        let top: Vec<Pos2> = hist.iter().enumerate().map(|(i, v)| Pos2::new(screen.left() + i as f32 * step, mid - amp(*v) * hh)).collect();
        let bot: Vec<Pos2> = hist.iter().enumerate().map(|(i, v)| Pos2::new(screen.left() + i as f32 * step, mid + amp(*v) * hh)).collect();
        for (k, w) in [(0.18, 7.0_f32), (0.45, 3.5), (1.0, 1.6)] {
            let col = Color32::from_rgba_unmultiplied(0xff, (0x70 as f32 + 0x60 as f32 * k) as u8, 0x20, (255.0 * k) as u8);
            f.p.add(Shape::line(top.clone(), Stroke::new(w * s, col)));
            f.p.add(Shape::line(bot.clone(), Stroke::new(w * s, col)));
        }
        // The input's gain-staging target: the input RMS against the gold zone (−12 … −8 dB), with a scale
        // and the reading.
        let bar = Rect::from_min_max(Pos2::new(screen.left() + 10.0 * s, screen.top() + 100.0 * s), Pos2::new(screen.right() - 10.0 * s, screen.top() + 112.0 * s));
        let xdb = |d: f32| bar.left() + ((d + 36.0) / 36.0).clamp(0.0, 1.0) * bar.width();
        let rd = if live.enabled { db(rin) } else { -120.0 };
        let (shown, _) = super::analog::spring(&ctx, salt.with("forge.rms"), rd.max(-40.0), 6.0, 1.0, motion);
        let in_gold = (-12.0..=-8.0).contains(&rd);
        f.p.text(Pos2::new(bar.left(), bar.top() - 4.0 * s), egui::Align2::LEFT_BOTTOM, "INPUT  ·  aim for the gold", f.font(10.5), GOLD);
        let reading = if rd > -60.0 { format!("{rd:.1} dB") } else { "–".to_string() };
        let rcol = if in_gold {
            Color32::from_rgb(0x9a, 0xe0, 0x6a)
        } else if rd > -8.0 {
            Color32::from_rgb(0xff, 0x6a, 0x40)
        } else {
            GOLD_DIM
        };
        f.p.text(Pos2::new(bar.right(), bar.top() - 4.0 * s), egui::Align2::RIGHT_BOTTOM, reading, f.font(10.5), rcol);
        f.p.rect_filled(bar, 3.0, Color32::from_black_alpha(200));
        let zone = Rect::from_min_max(Pos2::new(xdb(-12.0), bar.top() - 2.0), Pos2::new(xdb(-8.0), bar.bottom() + 2.0));
        f.p.rect_filled(zone, 2.0, GOLD.gamma_multiply(if in_gold { 0.9 } else { 0.45 }));
        if shown > -36.0 {
            let fill = Rect::from_min_max(bar.left_top() + Vec2::new(0.0, 3.0 * s), Pos2::new(xdb(shown), bar.bottom() - 3.0 * s));
            f.p.rect_filled(fill, 2.0, if in_gold { Color32::from_rgb(0x9a, 0xe0, 0x6a) } else { EMBER });
            f.p.line_segment([Pos2::new(xdb(shown), bar.top() - 3.0), Pos2::new(xdb(shown), bar.bottom() + 3.0)], Stroke::new(2.0_f32, Color32::from_rgb(0xff, 0xe8, 0xb0)));
        }
        for (d, l) in [(-36.0, "-36"), (-24.0, "-24"), (-12.0, "-12"), (-8.0, "-8"), (0.0, "0")] {
            let x = xdb(d);
            f.p.line_segment([Pos2::new(x, bar.bottom() + 1.0), Pos2::new(x, bar.bottom() + 4.0 * s)], Stroke::new(1.0_f32, GOLD_DIM));
            let al = if d <= -36.0 {
                egui::Align2::LEFT_TOP
            } else if d >= 0.0 {
                egui::Align2::RIGHT_TOP
            } else {
                egui::Align2::CENTER_TOP
            };
            f.p.text(Pos2::new(x, bar.bottom() + 5.0 * s), al, l, f.font(9.0), GOLD_DIM);
        }
        // The anvil, sparks on every hit.
        let ar = f.rect(300.0, 266.0, 514.0, 356.0);
        if let Some(at) = tex(&ctx, "forge-anvil", ANVIL) {
            let fr = fit(&at, ar);
            f.p.image(at.id(), fr, full_uv(), Color32::WHITE);
            let spark_key = salt.with("sparks");
            let mut born: f32 = ctx.data(|d| d.get_temp(spark_key)).unwrap_or(-10.0);
            if hit && live.enabled && t - born > 0.12 {
                born = t;
                ctx.data_mut(|d| d.insert_temp(spark_key, born));
            }
            let age = t - born;
            if motion && age < 0.6 {
                let c = Pos2::new(fr.center().x, fr.top() + fr.height() * 0.2);
                for k in 0..14 {
                    let a = -PI * (0.15 + 0.7 * ((k as f32 * 0.618).fract()));
                    let sp = 60.0 + 50.0 * ((k as f32 * 0.37).fract());
                    let pos = c + Vec2::angled(a) * sp * age * s + Vec2::new(0.0, 90.0 * age * age * s);
                    f.p.circle_filled(pos, 1.6 * s, Color32::from_rgba_unmultiplied(0xff, 0xd0, 0x60, (255.0 * (1.0 - age / 0.6)) as u8));
                }
            }
        }
        let hr = f.rect(282.0, 362.0, 532.0, 640.0);
        f.panel(hr);
        f.badge(407.0, 384.0, "HARMONICS", 16.0);
        f.knob("harmonics", 407.0, 486.0, 46.0, KnobStyle::Black);
        f.label(407.0, 556.0, "AMOUNT", 12.0);
        f.toggle("focus", 407.0, 604.0, "50 Hz", "80 Hz");
    }

    // ---- right: compression, emblem, harmonic EQ, parallels, output
    {
        let cr = f.rect(542.0, 100.0, 852.0, 300.0);
        f.panel(cr);
        f.badge(697.0, 120.0, "COMPRESSION", 15.0);
        if let Some(bt) = tex(&ctx, "forge-bellows", BELLOWS) {
            let gr_n = (gr / 10.0).clamp(0.0, 1.0);
            let (sq, _) = super::analog::spring(&ctx, salt.with("bellows"), gr_n, 3.0, 0.8, motion);
            let base = fit(&bt, f.rect(562.0, 140.0, 832.0, 214.0));
            let squeezed = Rect::from_center_size(base.center(), Vec2::new(base.width() * (1.0 - 0.22 * sq), base.height() * (1.0 + 0.04 * sq)));
            f.p.image(bt.id(), squeezed, full_uv(), Color32::WHITE);
        }
        f.knob("comp_mix", 600.0, 248.0, 20.0, KnobStyle::Black);
        f.label(600.0, 284.0, "MIX", 11.0);
        f.toggle("comp_mode", 697.0, 250.0, "GLUE", "PUNCH");
        f.knob("lift", 794.0, 248.0, 20.0, KnobStyle::Black);
        f.label(794.0, 284.0, "LIFT", 11.0);
        let er = f.rect(860.0, 100.0, 944.0, 300.0);
        f.panel(er);
        if let Some(dt) = tex(&ctx, "forge-dragon", DRAGON) {
            f.p.image(dt.id(), fit(&dt, er.shrink(8.0 * s)), full_uv(), Color32::WHITE);
            super::fx_mod::halo(&f.p, er.center(), er.width() * 0.5, GOLD, 0.05 + 0.1 * lvl);
        }
        // Harmonic EQ: tongs in glowing slots. Each is a real fader in whole dB: click or drag anywhere in
        // its column and the tongs go to the pointer, the wheel steps by 1 dB, double-click resets.
        let qr = f.rect(542.0, 308.0, 802.0, 500.0);
        f.panel(qr);
        f.badge(672.0, 328.0, "HARMONIC EQ", 15.0);
        // The scale beside the first slot.
        for (d, l) in [(0.0, "0"), (5.0, "5"), (10.0, "10")] {
            f.p.text(f.pt(553.0, 448.0 - 9.0 * d), egui::Align2::CENTER_CENTER, l, f.font(9.0), GOLD_DIM);
        }
        for (k, hz) in HEQ_FREQS.iter().enumerate() {
            let id = format!("heq{}", k + 1);
            let x = 580.0 + 46.0 * k as f32;
            let prm = f.prm(&id);
            let v = f.v(&id).round().clamp(0.0, 10.0);
            // Slot: 0 dB at the bottom (y 448), 10 dB at the top (y 358) – 9 px per dB.
            let slot = f.rect(x - 5.0, 354.0, x + 5.0, 452.0);
            let ydb = |d: f32| f.o.y + (448.0 - 9.0 * d) * f.s;
            let col_r = f.rect(x - 22.0, 342.0, x + 22.0, 462.0);
            let resp = f.ui.interact(col_r, salt.with(id.as_str()), Sense::click_and_drag());
            let active = resp.hovered() || resp.dragged();
            let mut nv = v;
            if resp.is_pointer_button_down_on() || resp.clicked() || resp.dragged() {
                if let Some(pp) = resp.interact_pointer_pos() {
                    nv = ((f.o.y + 448.0 * f.s - pp.y) / (9.0 * f.s)).round().clamp(0.0, 10.0);
                }
            }
            if resp.hovered() {
                let wheel = f.ui.input(|i| i.raw_scroll_delta.y);
                if wheel != 0.0 {
                    nv = (v + wheel.signum()).clamp(0.0, 10.0);
                }
            }
            if resp.double_clicked() {
                nv = prm.default;
            }
            if nv != v {
                f.out.push((id.clone(), nv));
            }
            if active {
                f.p.rect_filled(col_r, 4.0, Color32::from_rgba_unmultiplied(0xff, 0x9a, 0x3c, 18));
                f.ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            }
            f.p.rect_filled(slot, 4.0, Color32::from_rgb(0x0a, 0x06, 0x05));
            f.p.rect_stroke(slot, 4.0, Stroke::new(1.0_f32, GOLD_DIM.gamma_multiply(0.5)), egui::StrokeKind::Outside);
            // A notch per dB.
            for d in 0..=10 {
                let y = ydb(d as f32);
                let w = if d % 5 == 0 { 9.0 } else { 6.0 } * f.s;
                f.p.line_segment([Pos2::new(slot.right() + 2.0, y), Pos2::new(slot.right() + 2.0 + w, y)], Stroke::new(1.0_f32, GOLD_DIM.gamma_multiply(if d as f32 <= nv { 1.0 } else { 0.5 })));
            }
            let hy = ydb(nv);
            if nv > 0.0 {
                let fill = Rect::from_min_max(Pos2::new(slot.left() + 2.0, hy), Pos2::new(slot.right() - 2.0, ydb(0.0)));
                f.p.rect_filled(fill, 3.0, Color32::from_rgb(0xff, (0x6a as f32 + 8.0 * nv) as u8, 0x20));
                super::fx_mod::halo(&f.p, Pos2::new(slot.center().x, hy), 12.0 * f.s, EMBER, 0.2 + 0.05 * nv);
            }
            if let Some(tt) = tex(&ctx, "forge-tongs", TONGS) {
                let hrr = fit(&tt, Rect::from_center_size(Pos2::new(slot.center().x, hy + 10.0 * f.s), Vec2::new(30.0, 42.0) * f.s));
                f.p.image(tt.id(), hrr, full_uv(), if active { Color32::WHITE } else { Color32::from_gray(225) });
            }
            let label = if *hz >= 1000.0 { format!("{}k", hz / 1000.0) } else { format!("{hz:.0}") };
            let lr = Rect::from_center_size(f.pt(x, 474.0), Vec2::new(36.0, 15.0) * f.s);
            f.p.rect_filled(lr, 3.0, Color32::from_rgb(0x1c, 0x16, 0x12));
            f.p.rect_stroke(lr, 3.0, Stroke::new(1.0_f32, GOLD_DIM), egui::StrokeKind::Inside);
            f.p.text(lr.center(), egui::Align2::CENTER_CENTER, label, f.font(10.0), GOLD);
            let vcol = if nv > 0.0 { Color32::from_rgb(0xff, 0xb0, 0x50) } else { GOLD_DIM };
            f.p.text(f.pt(x, 490.0), egui::Align2::CENTER_CENTER, format!("+{nv:.0} dB"), f.font(10.0), vcol);
            resp.on_hover_text(format!("{} – click or drag to set, wheel ±1 dB, double-click resets", prm.name));
        }
        // Parallels.
        let pr = f.rect(542.0, 508.0, 802.0, 640.0);
        f.panel(pr);
        f.badge(672.0, 528.0, "PARALLELS", 15.0);
        for (k, (id, name)) in [("sub", "SUB"), ("low", "LOW"), ("loud", "LOUD"), ("grit", "GRIT")].into_iter().enumerate() {
            let x = 578.0 + 62.0 * k as f32;
            f.knob(id, x, 580.0, 17.0, KnobStyle::Black);
            f.label(x, 616.0, name, 11.0);
        }
        // Output.
        let outr = f.rect(812.0, 308.0, 944.0, 640.0);
        f.panel(outr);
        f.badge(878.0, 330.0, "OUTPUT", 15.0);
        let prm = f.prm("output");
        for (v, l) in [(-24.0, "-24"), (-12.0, "-12"), (0.0, "0"), (12.0, "+12")] {
            let n = prm.normalize(v);
            let a = PI * 0.75 + PI * 1.5 * n;
            f.p.text(f.pt(878.0, 470.0) + Vec2::angled(a) * 58.0 * s, egui::Align2::CENTER_CENTER, l, f.font(10.5), GOLD);
        }
        f.knob("output", 878.0, 470.0, 38.0, KnobStyle::Brass);
    }

    // ---- golden claws in the corners
    if let Some(ct) = tex(&ctx, "forge-claw", CLAW) {
        let size = 58.0;
        for (x, y, flip_x, flip_y) in [(0.0, 0.0, false, false), (W - size, 0.0, true, false), (0.0, H - size, false, true), (W - size, H - size, true, true)] {
            let r = f.rect(x, y, x + size, y + size);
            let uv = Rect::from_min_max(Pos2::new(if flip_x { 1.0 } else { 0.0 }, if flip_y { 1.0 } else { 0.0 }), Pos2::new(if flip_x { 0.0 } else { 1.0 }, if flip_y { 0.0 } else { 1.0 }));
            f.p.image(ct.id(), r, uv, Color32::WHITE);
        }
    }
    if motion && live.enabled {
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
    }
    f.out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(ctx: &egui::Context, params: &BTreeMap<String, f32>, events: Vec<egui::Event>) -> Vec<(String, f32)> {
        let def = dragon_engine::effects::find("drum_bus").unwrap();
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(W + 16.0, H + 40.0))), events, ..Default::default() };
        let mut got = Vec::new();
        let _ = ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| got = body(ui, def, params, &Live { tele: &[], level: (0.0, 0.0), time: None, enabled: true }, egui::Id::new("t")));
        });
        got
    }

    fn click(ctx: &egui::Context, params: &BTreeMap<String, f32>, at: Pos2) -> Vec<(String, f32)> {
        frame(ctx, params, vec![egui::Event::PointerMoved(at)]);
        frame(ctx, params, vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }]);
        frame(ctx, params, vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }])
    }

    /// The forge's controls work: the colour lever, the GLUE / PUNCH switch and the drive wheel.
    #[test]
    fn lever_switch_and_wheel() {
        let ctx = egui::Context::default();
        let params = dragon_engine::effects::find("drum_bus").unwrap().defaults();
        frame(&ctx, &params, vec![]);
        // Canvas at scale 1, at the panel's margin (8 px).
        let at = |x: f32, y: f32| Pos2::new(8.0 + x, 8.0 + y);
        assert!(click(&ctx, &params, at(232.0, 615.0)).contains(&("colour".to_string(), 2.0)), "CRUNCH");
        assert!(click(&ctx, &params, at(697.0, 250.0)).contains(&("comp_mode".to_string(), 1.0)), "PUNCH");
        let wheel = at(156.0, 448.0);
        frame(&ctx, &params, vec![egui::Event::PointerMoved(wheel)]);
        frame(&ctx, &params, vec![egui::Event::PointerButton { pos: wheel, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }]);
        let mut all = Vec::new();
        for k in 1..=5 {
            all.extend(frame(&ctx, &params, vec![egui::Event::PointerMoved(wheel - Vec2::new(0.0, 10.0 * k as f32))]));
        }
        // (The test never feeds the value back, so every step moves it from the default.)
        assert!(all.iter().any(|(k, v)| k == "drive" && *v > 21.0), "{all:?}");
    }

    /// The harmonic EQ faders go to where they're clicked or dragged, in whole dB, and the
    /// harmonics focus starts on 80 Hz.
    #[test]
    fn harmonic_eq_faders_follow_the_pointer() {
        let ctx = egui::Context::default();
        let params = dragon_engine::effects::find("drum_bus").unwrap().defaults();
        assert_eq!(params.get("focus"), Some(&1.0), "80 Hz by default");
        frame(&ctx, &params, vec![]);
        let at = |x: f32, y: f32| Pos2::new(8.0 + x, 8.0 + y);
        // 3.5 kHz column (x 718), 6 dB = y 448 - 54.
        assert!(click(&ctx, &params, at(718.0, 394.0)).contains(&("heq4".to_string(), 6.0)), "click sets 6 dB");
        // Dragging the 60 Hz fader to the top gives 10 dB, step by step.
        let start = at(580.0, 446.0);
        frame(&ctx, &params, vec![egui::Event::PointerMoved(start)]);
        frame(&ctx, &params, vec![egui::Event::PointerButton { pos: start, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }]);
        let mut all = Vec::new();
        for k in 1..=12 {
            all.extend(frame(&ctx, &params, vec![egui::Event::PointerMoved(start - Vec2::new(0.0, 9.0 * k as f32))]));
        }
        assert!(all.contains(&("heq1".to_string(), 10.0)), "{all:?}");
    }
}
