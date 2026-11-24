//! The face of **Draconic Ghost** (resonance suppressor), in its dark-teal dragon skin (art made with
//! Higgsfield): the skull mark and name with the engine, resolution and Soft / Hard switches and the
//! latency; Delta, Bypass, L/R – M/S and the stereo link on the left; the big display in the middle – the
//! input spectrum, the reduction (the white "ghost" hanging from the top), the six focus nodes (drag:
//! frequency and sensitivity, wheel: width, double-click: flat) and the low / high cut; IN / GR / OUT
//! meters on the right; the seven knobs with claw pointers; sidechain, band solo and the cut range at the
//! bottom.

use dragon_engine::effects::ghost::{band_hz, cut_window, focus_db, Node, ENGINES, NB, RESOLUTIONS};
use dragon_engine::effects::EffectDef;
use egui::{Color32, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use std::collections::BTreeMap;

const W: f32 = 1080.0;
const H: f32 = 610.0;

const BG: Color32 = Color32::from_rgb(0x0b, 0x1a, 0x1c);
const PANEL: Color32 = Color32::from_rgb(0x11, 0x26, 0x28);
const TEAL: Color32 = Color32::from_rgb(0x7f, 0xff, 0xe6);
const TEAL2: Color32 = Color32::from_rgb(0x40, 0xe0, 0xd0);
const GHOST: Color32 = Color32::from_rgb(0xe8, 0xff, 0xfb);
const DIM: Color32 = Color32::from_rgb(0x6f, 0x9c, 0x98);
const WARN: Color32 = Color32::from_rgb(0xff, 0x6b, 0x6b);

const LOGO: &[u8] = include_bytes!("../../assets/fx/ghost/logo.png");
const KNOB: &[u8] = include_bytes!("../../assets/fx/ghost/knob.png");
const CLAW: &[u8] = include_bytes!("../../assets/fx/ghost/claw.png");
const WING: &[u8] = include_bytes!("../../assets/fx/ghost/wing.png");
const SCALES: &[u8] = include_bytes!("../../assets/fx/ghost/scales.jpg");

pub fn is_ghost(id: &str) -> bool {
    id == "ghost"
}

/// The editor window's width for this face.
pub fn window_width() -> f32 {
    W + 32.0
}

fn tex(ctx: &egui::Context, name: &'static str, bytes: &[u8]) -> Option<egui::TextureHandle> {
    super::guitar_ui::texture(ctx, name, bytes)
}

fn full_uv() -> Rect {
    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0))
}

/// Drawing in design coordinates (W × H), scaled to the space there is.
struct Face<'a> {
    ui: &'a mut Ui,
    p: egui::Painter,
    o: Pos2,
    s: f32,
    def: &'static EffectDef,
    params: &'a BTreeMap<String, f32>,
    out: Vec<(String, f32)>,
    salt: egui::Id,
}

impl Face<'_> {
    fn rect(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(Pos2::new(self.o.x + x0 * self.s, self.o.y + y0 * self.s), Pos2::new(self.o.x + x1 * self.s, self.o.y + y1 * self.s))
    }
    fn pt(&self, x: f32, y: f32) -> Pos2 {
        Pos2::new(self.o.x + x * self.s, self.o.y + y * self.s)
    }
    fn font(&self, size: f32) -> egui::FontId {
        egui::FontId::proportional(size * self.s)
    }
    /// A parameter's value (the last edit this frame wins).
    fn v(&self, id: &str) -> f32 {
        self.out.iter().rev().find(|e| e.0 == id).map(|e| e.1).or_else(|| self.params.get(id).copied()).or_else(|| self.def.param(id).map(|p| p.default)).unwrap_or(0.0)
    }
    fn set(&mut self, id: &str, v: f32) {
        self.out.push((id.to_string(), v));
    }
    fn panel(&self, r: Rect) {
        self.p.rect_filled(r, 10.0 * self.s, PANEL.gamma_multiply(0.92));
        self.p.rect_stroke(r, 10.0 * self.s, Stroke::new(1.2_f32, TEAL2.gamma_multiply(0.35)), egui::StrokeKind::Inside);
    }
    fn label(&self, at: Pos2, text: &str, size: f32, col: Color32, align: egui::Align2) {
        self.p.text(at, align, text, self.font(size), col);
    }
    /// A pill toggle (DELTA, BYPASS …).
    fn toggle(&mut self, x: f32, y: f32, id: &str, label: &str, tip: &str) {
        let on = self.v(id) >= 0.5;
        self.label(self.pt(x, y), label, 14.0, if on { TEAL } else { GHOST }, egui::Align2::LEFT_CENTER);
        let r = self.rect(x + 74.0, y - 11.0, x + 116.0, y + 11.0);
        let resp = self.ui.interact(r, self.salt.with(id), Sense::click()).on_hover_text(tip);
        self.p.rect_filled(r, 11.0 * self.s, if on { TEAL2.gamma_multiply(0.85) } else { Color32::from_rgb(0x2a, 0x3a, 0x3c) });
        let kx = if on { r.right() - 11.0 * self.s } else { r.left() + 11.0 * self.s };
        self.p.circle_filled(Pos2::new(kx, r.center().y), 8.5 * self.s, if on { Color32::WHITE } else { Color32::from_gray(170) });
        if resp.clicked() {
            self.set(id, if on { 0.0 } else { 1.0 });
        }
    }
    /// Segmented choice buttons for a choice parameter.
    fn segments(&mut self, x: f32, y: f32, w: f32, id: &str, labels: &[&str], tip: &str) {
        let cur = self.v(id).round() as usize;
        let n = labels.len() as f32;
        let r = self.rect(x, y, x + w, y + 30.0);
        self.p.rect_stroke(r, 7.0 * self.s, Stroke::new(1.2_f32, TEAL2.gamma_multiply(0.6)), egui::StrokeKind::Inside);
        for (k, l) in labels.iter().enumerate() {
            let sr = self.rect(x + w / n * k as f32, y, x + w / n * (k + 1) as f32, y + 30.0).shrink(2.0 * self.s);
            let resp = self.ui.interact(sr, self.salt.with((id, k)), Sense::click()).on_hover_text(tip);
            if k == cur {
                self.p.rect_filled(sr, 6.0 * self.s, TEAL2.gamma_multiply(0.85));
            } else if resp.hovered() {
                self.p.rect_filled(sr, 6.0 * self.s, TEAL2.gamma_multiply(0.15));
            }
            self.label(sr.center(), l, 13.0, if k == cur { Color32::from_rgb(0x05, 0x20, 0x20) } else { GHOST }, egui::Align2::CENTER_CENTER);
            if resp.clicked() {
                self.set(id, k as f32);
            }
        }
    }
    /// A horizontal slider (0 … 1 of the parameter's range) with an optional text under it.
    fn slider(&mut self, x: f32, y: f32, w: f32, id: &str, tip: &str) {
        let Some(pi) = self.def.param(id) else { return };
        let n = pi.normalize(self.v(id));
        let r = self.rect(x, y - 6.0, x + w, y + 6.0);
        let resp = self.ui.interact(r.expand(6.0 * self.s), self.salt.with(id), Sense::click_and_drag()).on_hover_text(tip);
        if let Some(x) = crate::docs::knob_menu(&resp, self.def, pi, self.v(id)) {
            self.set(id, x);
        }
        if let Some(pp) = resp.interact_pointer_pos().filter(|_| resp.dragged() || resp.clicked()) {
            let nn = ((pp.x - r.left()) / r.width()).clamp(0.0, 1.0);
            self.set(id, pi.denormalize(nn));
        }
        if resp.double_clicked() {
            self.set(id, pi.default);
        }
        let n = if resp.dragged() { pi.normalize(self.v(id)) } else { n };
        self.p.rect_filled(r, 6.0 * self.s, Color32::from_rgb(0x1c, 0x30, 0x32));
        self.p.rect_filled(Rect::from_min_max(r.min, Pos2::new(r.left() + r.width() * n, r.bottom())), 6.0 * self.s, TEAL2.gamma_multiply(0.85));
        self.p.circle_filled(Pos2::new(r.left() + r.width() * n, r.center().y), 10.0 * self.s, Color32::from_gray(230));
    }
    /// A knob: the art, a turquoise arc for the value, the claw pointer, the name and the value box.
    fn knob(&mut self, cx: f32, cy: f32, id: &str, name: &str) {
        let Some(pi) = self.def.param(id) else { return };
        let v = self.v(id);
        let rr = 52.0;
        let r = self.rect(cx - rr, cy - rr, cx + rr, cy + rr);
        let resp = self.ui.interact(r, self.salt.with(id), Sense::click_and_drag()).on_hover_text(format!("{name}: drag up / down (Shift: fine), double-click: default, right-click for the menu"));
        if let Some(x) = crate::docs::knob_menu(&resp, self.def, pi, v) {
            self.set(id, x);
        }
        let mut n = pi.normalize(v);
        if resp.dragged() {
            let fine = if self.ui.input(|i| i.modifiers.shift) { 0.2 } else { 1.0 };
            n = (n - resp.drag_delta().y / (220.0 * self.s) * fine).clamp(0.0, 1.0);
            self.set(id, pi.denormalize(n));
        }
        if resp.double_clicked() {
            n = pi.normalize(pi.default);
            self.set(id, pi.default);
        }
        let c = r.center();
        // Arc (−135° … +135°).
        let a0 = -135f32.to_radians();
        let ang = a0 + n * 270f32.to_radians();
        let arc = |p: &egui::Painter, from: f32, to: f32, rad: f32, col: Color32, w: f32| {
            let steps = 40;
            let pts: Vec<Pos2> = (0..=steps).map(|k| from + (to - from) * k as f32 / steps as f32).map(|a| c + Vec2::new(a.sin(), -a.cos()) * rad).collect();
            p.add(egui::Shape::line(pts, Stroke::new(w, col)));
        };
        arc(&self.p, a0, -a0, 49.0 * self.s, Color32::from_rgb(0x1e, 0x34, 0x36), 6.0 * self.s);
        arc(&self.p, a0, ang, 49.0 * self.s, TEAL, 6.0 * self.s);
        if let Some(t) = tex(self.ui.ctx(), "ghost.knob", KNOB) {
            self.p.image(t.id(), Rect::from_center_size(c, Vec2::splat(80.0 * self.s)), full_uv(), Color32::WHITE);
        }
        if let Some(t) = tex(self.ui.ctx(), "ghost.claw", CLAW) {
            // The claw from the knob's middle out to its rim, at its own proportions.
            let [tw, th] = t.size();
            let len = 40.0 * self.s;
            let wid = len * tw as f32 / th.max(1) as f32;
            pointer(&self.p, &t, c + Vec2::new(ang.sin(), -ang.cos()) * len * 0.42, wid * 0.5, len * 0.5, ang);
        }
        self.label(self.pt(cx, cy + 68.0), name, 17.0, GHOST, egui::Align2::CENTER_CENTER);
        let vr = self.rect(cx - 46.0, cy + 82.0, cx + 46.0, cy + 106.0);
        self.p.rect_stroke(vr, 7.0 * self.s, Stroke::new(1.2_f32, TEAL2.gamma_multiply(0.6)), egui::StrokeKind::Inside);
        let txt = match id {
            "depth" | "mix" | "link" => format!("{v:.0} %"),
            "sharpness" | "selectivity" => format!("{:.0} %", v * 10.0),
            "attack" => format!("{v:.1} ms"),
            "release" => format!("{v:.0} ms"),
            "trim" => format!("{v:+.1} dB"),
            _ => format!("{v:.1}"),
        };
        self.label(vr.center(), &txt, 13.5, GHOST, egui::Align2::CENTER_CENTER);
    }
}

/// An image rotated by `rot` (0 = pointing up) around `c`, `hw` × `hh` half sizes.
fn pointer(p: &egui::Painter, t: &egui::TextureHandle, c: Pos2, hw: f32, hh: f32, rot: f32) {
    let mut m = egui::Mesh::with_texture(t.id());
    let (sn, co) = rot.sin_cos();
    let corner = |x: f32, y: f32| Pos2::new(c.x + x * co - y * sn, c.y + x * sn + y * co);
    for (pos, uv) in [(corner(-hw, -hh), Pos2::new(0.0, 0.0)), (corner(hw, -hh), Pos2::new(1.0, 0.0)), (corner(hw, hh), Pos2::new(1.0, 1.0)), (corner(-hw, hh), Pos2::new(0.0, 1.0))] {
        m.vertices.push(egui::epaint::Vertex { pos, uv, color: Color32::WHITE });
    }
    m.indices.extend([0, 1, 2, 0, 2, 3]);
    p.add(egui::Shape::mesh(m));
}

/// The focus nodes from the parameters.
fn nodes(f: &Face) -> [Node; 6] {
    [(0, f.v("f1"), f.v("g1"), 0.7), (1, f.v("f2"), f.v("g2"), f.v("q2")), (1, f.v("f3"), f.v("g3"), f.v("q3")), (1, f.v("f4"), f.v("g4"), f.v("q4")), (1, f.v("f5"), f.v("g5"), f.v("q5")), (2, f.v("f6"), f.v("g6"), 0.7)]
}

fn fmt_hz(hz: f32) -> String {
    if hz >= 1000.0 {
        format!("{:.1} kHz", hz / 1000.0).replace(".0 ", " ")
    } else {
        format!("{hz:.0} Hz")
    }
}

/// Draws the face; returns the parameter edits.
pub fn body(ui: &mut Ui, def: &'static EffectDef, params: &BTreeMap<String, f32>, live: &super::fx_forge::Live, salt: egui::Id) -> Vec<(String, f32)> {
    let avail = ui.available_width().max(640.0);
    let s = (avail / W).min(1.2);
    let (r, _) = ui.allocate_exact_size(Vec2::new(W * s, H * s), Sense::hover());
    let p = ui.painter_at(r);
    let mut f = Face { ui, p, o: r.min, s, def, params, out: Vec::new(), salt };
    let ctx = f.ui.ctx().clone();

    // Background: deep teal with a faint scale pattern, engraved wings in the header corners.
    f.p.rect_filled(r, 12.0 * s, BG);
    if let Some(t) = tex(&ctx, "ghost.scales", SCALES) {
        let tile = 160.0 * s;
        let (nx, ny) = ((r.width() / tile).ceil() as i32, (r.height() / tile).ceil() as i32);
        for j in 0..ny {
            for i in 0..nx {
                let tr = Rect::from_min_size(r.min + Vec2::new(i as f32 * tile, j as f32 * tile), Vec2::splat(tile));
                f.p.image(t.id(), tr, full_uv(), Color32::from_white_alpha(46));
            }
        }
    }
    if let Some(t) = tex(&ctx, "ghost.wing", WING) {
        // Faint, in the outer corners only (past the switches and the name).
        let wr = f.rect(W - 120.0, 4.0, W - 6.0, 80.0);
        f.p.image(t.id(), wr, full_uv(), Color32::from_white_alpha(55));
        let wl = f.rect(-30.0, 66.0, 70.0, 88.0);
        let _ = wl;
    }

    // ---- header
    if let Some(t) = tex(&ctx, "ghost.logo", LOGO) {
        f.p.image(t.id(), f.rect(22.0, 8.0, 76.0, 70.0), full_uv(), Color32::WHITE);
    }
    f.label(f.pt(86.0, 30.0), "DRACONIC GHOST", 32.0, TEAL, egui::Align2::LEFT_CENTER);
    f.label(f.pt(88.0, 58.0), "DYNAMIC RESONANCE SUPPRESSOR  ·  SPECTRAL CLEANER", 10.5, DIM, egui::Align2::LEFT_CENTER);
    f.segments(470.0, 22.0, 210.0, "engine", ENGINES, "Engine: Zero (no latency), Low (1.3 ms lookahead – the cut starts before the transient), Precise (STFT, 21 ms)");
    f.segments(696.0, 22.0, 132.0, "resolution", RESOLUTIONS, "Resolution: finer frequency analysis (more CPU), no extra latency");
    f.segments(844.0, 22.0, 120.0, "character", &["Soft", "Hard"], "Soft: transparent and musical · Hard: aggressive clean-up");
    let lat = live.tele.get(3).copied().unwrap_or(64.0);
    f.label(f.pt(1022.0, 37.0), &format!("{:.1} ms", lat / 48.0), 15.0, GHOST, egui::Align2::CENTER_CENTER);

    // ---- left column
    let lc = f.rect(14.0, 88.0, 150.0, 404.0);
    f.panel(lc);
    f.toggle(28.0, 120.0, "delta", "DELTA", "Delta: listen to only what is removed");
    f.toggle(28.0, 166.0, "bypass", "BYPASS", "Bypass (a 10 ms crossfade, latency kept)");
    f.p.line_segment([f.pt(26.0, 196.0), f.pt(138.0, 196.0)], Stroke::new(1.0_f32, TEAL2.gamma_multiply(0.25)));
    f.segments(26.0, 210.0, 112.0, "stereo", &["L/R", "M/S"], "L/R: each side on its own · M/S: mid and side (clean only the side, e.g. a harsh reverb)");
    f.label(f.pt(82.0, 268.0), "STEREO LINK", 13.0, GHOST, egui::Align2::CENTER_CENTER);
    f.slider(30.0, 292.0, 104.0, "link", "Stereo link: 100 % keeps the image stable, 0 % cleans each side on its own");
    f.label(f.pt(82.0, 312.0), &format!("{:.0} %", f.v("link")), 11.0, DIM, egui::Align2::CENTER_CENTER);
    if f.v("stereo") >= 0.5 {
        f.label(f.pt(82.0, 344.0), "M  ·  BALANCE  ·  S", 11.0, GHOST, egui::Align2::CENTER_CENTER);
        f.slider(30.0, 366.0, 104.0, "ms_balance", "Where the cleaning goes: left = mid only, right = side only");
    }

    // ---- the display
    let dr = f.rect(160.0, 88.0, 900.0, 404.0);
    f.panel(dr);
    let plot = f.rect(204.0, 100.0, 888.0, 380.0);
    let x_of = |hz: f32| plot.left() + ((hz / 20.0).log2() / 10.0).clamp(0.0, 1.0) * plot.width();
    let hz_of = |x: f32| 20.0 * 2f32.powf(((x - plot.left()) / plot.width()).clamp(0.0, 1.0) * 10.0);
    let y_of = |db: f32| plot.top() + (-db / 60.0).clamp(0.0, 1.0) * plot.height();
    let pp = f.p.with_clip_rect(plot.expand(2.0));
    for db in (0..=60).step_by(6) {
        let y = y_of(-(db as f32));
        pp.line_segment([Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)], Stroke::new(1.0_f32, Color32::from_white_alpha(10)));
        f.label(Pos2::new(plot.left() - 6.0 * s, y), &format!("{} dB", -db), 10.0, DIM, egui::Align2::RIGHT_CENTER);
    }
    for hz in [20.0f32, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0, 20000.0] {
        let x = x_of(hz);
        pp.line_segment([Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())], Stroke::new(1.0_f32, Color32::from_white_alpha(10)));
        f.label(Pos2::new(x, plot.bottom() + 12.0 * s), &fmt_hz(hz), 10.0, GHOST, egui::Align2::CENTER_CENTER);
    }
    let bands = |k: usize| -> f32 { live.tele.get(4 + k).copied().unwrap_or(-120.0) };
    let reds = |k: usize| -> f32 { live.tele.get(4 + NB + k).copied().unwrap_or(0.0) };
    // Input spectrum: a filled curve (levels shown from −60 to 0 dBFS, +12 dB tilt-free).
    let pts: Vec<Pos2> = (0..NB).map(|k| Pos2::new(x_of(band_hz(k)), y_of(bands(k).clamp(-60.0, 0.0)))).collect();
    if live.enabled && pts.iter().any(|q| q.y < plot.bottom() - 1.0) {
        for w in pts.windows(2) {
            let quad = vec![w[0], w[1], Pos2::new(w[1].x, plot.bottom()), Pos2::new(w[0].x, plot.bottom())];
            pp.add(egui::Shape::convex_polygon(quad, TEAL2.gamma_multiply(0.38), Stroke::NONE));
        }
        pp.add(egui::Shape::line(pts.clone(), Stroke::new(1.5_f32, TEAL2)));
    }
    // Low / high cut: the outside is shaded.
    let (lo, hi) = (f.v("low_cut"), f.v("high_cut"));
    pp.rect_filled(Rect::from_min_max(plot.min, Pos2::new(x_of(lo), plot.bottom())), 0.0, Color32::from_black_alpha(110));
    pp.rect_filled(Rect::from_min_max(Pos2::new(x_of(hi), plot.top()), plot.max), 0.0, Color32::from_black_alpha(110));
    // The reduction: the ghost hanging from the top (0 dB line down by the cut, 1 dB = 1/60 of the height × 2).
    let ghost: Vec<Pos2> = (0..NB).map(|k| Pos2::new(x_of(band_hz(k)), plot.top() + reds(k).clamp(0.0, 30.0) * plot.height() / 30.0 * 0.55)).collect();
    pp.add(egui::Shape::line(ghost.iter().map(|q| *q + Vec2::new(0.0, 1.0)).collect(), Stroke::new(5.0_f32, GHOST.gamma_multiply(0.15))));
    pp.add(egui::Shape::line(ghost, Stroke::new(2.0_f32, GHOST)));
    // Focus line: the sensitivity curve around −24 dB (±12 dB = ±1/5 of the height), with its nodes.
    let base = y_of(-24.0);
    let gy = |g: f32| base - g / 12.0 * plot.height() * 0.2;
    let nd = nodes(&f);
    let curve: Vec<Pos2> = (0..=160).map(|k| plot.left() + plot.width() * k as f32 / 160.0).map(|x| Pos2::new(x, gy(focus_db(&nd, hz_of(x)).clamp(-12.0, 12.0)))).collect();
    pp.add(egui::Shape::line(curve, Stroke::new(2.0_f32, TEAL)));
    let cutw: Vec<Pos2> = (0..=160).map(|k| plot.left() + plot.width() * k as f32 / 160.0).map(|x| Pos2::new(x, plot.bottom() - cut_window(lo, hi, hz_of(x)) * 4.0 * s)).collect();
    pp.add(egui::Shape::line(cutw, Stroke::new(1.0_f32, TEAL2.gamma_multiply(0.5))));
    let solo = f.v("solo").round() as usize;
    for (k, (_, hz, g, q)) in nd.iter().enumerate() {
        let (fid, gid) = (format!("f{}", k + 1), format!("g{}", k + 1));
        let c = Pos2::new(x_of(*hz), gy(*g));
        let nr = Rect::from_center_size(c, Vec2::splat(26.0 * s));
        let resp = f.ui.interact(nr, salt.with(("node", k)), Sense::click_and_drag()).on_hover_text(format!(
            "Focus {}: {} · {:+.1} dB{} – drag to move, {}double-click: flat, Alt+click: listen to this band",
            k + 1,
            fmt_hz(*hz),
            g,
            if k == 0 || k == 5 { String::new() } else { format!(" · Q {q:.2}") },
            if k == 0 || k == 5 { "" } else { "wheel: width, " }
        ));
        if resp.dragged() {
            if let Some(pp2) = resp.interact_pointer_pos() {
                if let (Some(pf), Some(pg)) = (def.param(&fid), def.param(&gid)) {
                    f.set(&fid, hz_of(pp2.x).clamp(pf.min, pf.max));
                    f.set(&gid, ((base - pp2.y) / (plot.height() * 0.2) * 12.0).clamp(pg.min, pg.max));
                }
            }
        }
        if resp.double_clicked() {
            f.set(&gid, 0.0);
        }
        if resp.clicked() && f.ui.input(|i| i.modifiers.alt) {
            f.set("solo", if solo == k + 1 { 0.0 } else { (k + 1) as f32 });
        }
        if resp.hovered() && (1..=4).contains(&k) {
            let wheel = f.ui.input(|i| i.raw_scroll_delta.y);
            if wheel != 0.0 {
                let qid = format!("q{}", k + 1);
                f.set(&qid, (q * if wheel > 0.0 { 1.15 } else { 1.0 / 1.15 }).clamp(0.3, 8.0));
            }
        }
        let hot = resp.hovered() || resp.dragged() || solo == k + 1;
        if hot {
            pp.circle_stroke(c, 18.0 * s, Stroke::new(1.5_f32, TEAL.gamma_multiply(0.6)));
        }
        pp.circle_filled(c, 9.0 * s, if hot { Color32::WHITE } else { TEAL.gamma_multiply(0.9) });
        pp.circle_stroke(c, 9.0 * s, Stroke::new(1.5_f32, BG));
    }
    // Cut handles on the bottom edge.
    for (id, hz, label) in [("low_cut", lo, "LOW CUT"), ("high_cut", hi, "HIGH CUT")] {
        let x = x_of(hz);
        let hr = Rect::from_center_size(Pos2::new(x, plot.bottom() - 10.0 * s), Vec2::new(16.0 * s, 26.0 * s));
        let resp = f.ui.interact(hr, salt.with(id), Sense::click_and_drag()).on_hover_text(format!("{label}: {} – only between the cuts is cleaned (drag)", fmt_hz(hz)));
        if resp.dragged() {
            if let (Some(pp2), Some(pi)) = (resp.interact_pointer_pos(), def.param(id)) {
                f.set(id, hz_of(pp2.x).clamp(pi.min, pi.max));
            }
        }
        pp.line_segment([Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())], Stroke::new(1.0_f32, TEAL2.gamma_multiply(0.5)));
        pp.rect_filled(hr.shrink2(Vec2::new(3.0 * s, 0.0)), 3.0 * s, if resp.hovered() || resp.dragged() { Color32::WHITE } else { TEAL2 });
    }
    // Legend.
    let lg = f.rect(744.0, 104.0, 884.0, 150.0);
    f.p.rect_filled(lg, 6.0 * s, BG.gamma_multiply(0.85));
    f.p.rect_filled(f.rect(752.0, 113.0, 768.0, 125.0), 2.0, TEAL2.gamma_multiply(0.7));
    f.label(f.pt(776.0, 119.0), "Input spectrum", 11.5, GHOST, egui::Align2::LEFT_CENTER);
    f.p.line_segment([f.pt(752.0, 140.0), f.pt(768.0, 140.0)], Stroke::new(2.0_f32, GHOST));
    f.label(f.pt(776.0, 140.0), "Reduction", 11.5, GHOST, egui::Align2::LEFT_CENTER);

    // ---- meters
    let mr = f.rect(910.0, 88.0, 1066.0, 404.0);
    f.panel(mr);
    let (inl, outl, gr) = (live.tele.first().copied().unwrap_or(-120.0), live.tele.get(1).copied().unwrap_or(-120.0), live.tele.get(2).copied().unwrap_or(0.0));
    let (inl, outl, gr) = if live.enabled { (inl, outl, gr) } else { (-120.0, -120.0, 0.0) };
    for (k, (label, v, from_top)) in [("IN", inl, false), ("GR", gr, true), ("OUT", outl, false)].into_iter().enumerate() {
        let x = 950.0 + k as f32 * 44.0;
        f.label(f.pt(x, 110.0), label, 14.0, GHOST, egui::Align2::CENTER_CENTER);
        let m = f.rect(x - 11.0, 124.0, x + 11.0, 388.0);
        f.p.rect_filled(m, 6.0 * s, Color32::from_rgb(0x05, 0x10, 0x11));
        f.p.rect_stroke(m, 6.0 * s, Stroke::new(1.0_f32, TEAL2.gamma_multiply(0.3)), egui::StrokeKind::Inside);
        // Segments: 26 of them; IN / OUT −60 … 0 dBFS from the bottom, GR 0 … 30 dB from the top.
        let lit = if from_top { (v / 30.0 * 26.0).round() as i32 } else { ((v + 60.0) / 60.0 * 26.0).round() as i32 };
        for i in 0..26 {
            let y0 = m.top() + 4.0 * s + i as f32 * (m.height() - 8.0 * s) / 26.0;
            let seg = Rect::from_min_max(Pos2::new(m.left() + 4.0 * s, y0 + 1.0), Pos2::new(m.right() - 4.0 * s, y0 + (m.height() - 8.0 * s) / 26.0 - 1.5));
            let on = if from_top { i < lit } else { 25 - i < lit };
            let col = if from_top {
                GHOST
            } else if i < 2 {
                WARN
            } else {
                TEAL2
            };
            f.p.rect_filled(seg, 1.0, if on { col } else { col.gamma_multiply(0.08) });
        }
    }
    for (k, db) in [0, -12, -24, -36, -48, -60].into_iter().enumerate() {
        let y = 128.0 + k as f32 * 256.0 / 5.0;
        f.label(f.pt(925.0, y), &db.to_string(), 9.0, DIM, egui::Align2::RIGHT_CENTER);
    }

    // ---- knobs
    let kr = f.rect(14.0, 414.0, 1066.0, 552.0);
    f.panel(kr);
    for (k, (id, name)) in [("depth", "DEPTH"), ("sharpness", "SHARPNESS"), ("selectivity", "SELECTIVITY"), ("attack", "ATTACK"), ("release", "RELEASE"), ("mix", "MIX"), ("trim", "TRIM")].into_iter().enumerate() {
        f.knob(110.0 + k as f32 * 143.0, 456.0, id, name);
    }

    // ---- bottom bar
    let br = f.rect(14.0, 560.0, 1066.0, 604.0);
    f.panel(br);
    f.toggle(30.0, 582.0, "sidechain", "SIDECHAIN", "Sidechain: listen to the key input (set it on the insert) – cut this track wherever the key is resonant");
    f.label(f.pt(214.0, 582.0), "BAND SOLO", 13.0, GHOST, egui::Align2::LEFT_CENTER);
    for k in 0..6 {
        let br = f.rect(304.0 + k as f32 * 36.0, 568.0, 334.0 + k as f32 * 36.0, 596.0);
        let resp = f.ui.interact(br, salt.with(("solo", k)), Sense::click()).on_hover_text(format!("Listen to focus band {} only (click again: off)", k + 1));
        let on = solo == k + 1;
        f.p.rect_filled(br, 6.0 * s, if on { TEAL2 } else { Color32::from_rgb(0x1c, 0x30, 0x32) });
        f.p.rect_stroke(br, 6.0 * s, Stroke::new(1.0_f32, TEAL2.gamma_multiply(0.5)), egui::StrokeKind::Inside);
        f.label(br.center(), &(k + 1).to_string(), 13.0, if on { BG } else { GHOST }, egui::Align2::CENTER_CENTER);
        if resp.clicked() {
            f.set("solo", if on { 0.0 } else { (k + 1) as f32 });
        }
    }
    f.label(f.pt(560.0, 582.0), &format!("LOW CUT  {}", fmt_hz(lo)), 13.0, GHOST, egui::Align2::LEFT_CENTER);
    // The cut range on one line (log 20 Hz … 20 kHz), two handles.
    let line = f.rect(690.0, 576.0, 920.0, 588.0);
    let lx = |hz: f32| line.left() + ((hz / 20.0).log2() / 10.0).clamp(0.0, 1.0) * line.width();
    f.p.rect_filled(line, 6.0 * s, Color32::from_rgb(0x1c, 0x30, 0x32));
    f.p.rect_filled(Rect::from_min_max(Pos2::new(lx(lo), line.top()), Pos2::new(lx(hi), line.bottom())), 6.0 * s, TEAL2.gamma_multiply(0.85));
    for (id, hz) in [("low_cut", lo), ("high_cut", hi)] {
        let c = Pos2::new(lx(hz), line.center().y);
        let resp = f.ui.interact(Rect::from_center_size(c, Vec2::splat(22.0 * s)), salt.with(("range", id)), Sense::drag());
        if resp.dragged() {
            if let (Some(pp2), Some(pi)) = (resp.interact_pointer_pos(), def.param(id)) {
                let hz2 = 20.0 * 2f32.powf(((pp2.x - line.left()) / line.width()).clamp(0.0, 1.0) * 10.0);
                f.set(id, hz2.clamp(pi.min, pi.max));
            }
        }
        f.p.circle_filled(c, 10.0 * s, Color32::from_gray(230));
    }
    f.label(f.pt(1052.0, 582.0), &format!("HIGH CUT  {}", fmt_hz(hi)), 13.0, GHOST, egui::Align2::RIGHT_CENTER);
    if live.time.is_some() && live.enabled {
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
    }
    f.out
}
