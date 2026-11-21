//! The displays of the modulation units (Coven, Quicksilver, Moonphase, Wingbeat), drawn as analog
//! instruments to match their rack faceplates: on the left a lit window with the unit's emblem moving
//! with the LFO (three candles swaying as the chorus voices, a mercury column rising and falling with the
//! flanger's delay, the moon running through its phases with the phaser, a pair of dragon wings beating
//! with the tremolo), on the right a phosphor oscilloscope with what the effect does to the sound.
//!
//! Artwork generated with Higgsfield; the motion is drawn here, each frame. With reduced motion the
//! displays stand still and show the settings.

use super::fx_ui::Hero;
use egui::{Color32, Mesh, Pos2, Rect, Shape, Stroke, TextureHandle, Vec2};
use std::f32::consts::{PI, TAU};

const SIGIL: &[u8] = include_bytes!("../../assets/fx/sigil.jpg");
const CRT: &[u8] = include_bytes!("../../assets/fx/crt.jpg");
const MOON: &[u8] = include_bytes!("../../assets/fx/moon.png");
const WING: &[u8] = include_bytes!("../../assets/fx/wing.png");
const GLASS: &[u8] = include_bytes!("../../assets/fx/glass_tube.png");
const CANDLE: &[u8] = include_bytes!("../../assets/fx/candle.png");
const FLAME: &[u8] = include_bytes!("../../assets/fx/flame.png");
pub(super) const OUROBOROS: &[u8] = include_bytes!("../../assets/fx/ouroboros.png");

/// Phosphor green.
pub(super) const PHOSPHOR: Color32 = Color32::from_rgb(0x8c, 0xff, 0xa8);
/// The cream printing of the oxblood units.
pub(super) const CREAM: Color32 = Color32::from_rgb(0xf1, 0xe4, 0xc8);
const AMBER: Color32 = Color32::from_rgb(0xff, 0x9a, 0x3c);

pub(super) fn art(ctx: &egui::Context, name: &'static str, bytes: &[u8]) -> Option<TextureHandle> {
    super::guitar_ui::texture(ctx, name, bytes)
}

/// A colour that adds light (premultiplied, zero alpha: additive blending).
pub(super) fn glow(c: Color32, k: f32) -> Color32 {
    let k = k.clamp(0.0, 1.0);
    Color32::from_rgba_premultiplied((c.r() as f32 * k) as u8, (c.g() as f32 * k) as u8, (c.b() as f32 * k) as u8, 0)
}

/// A soft round glow of light: `k` at the centre fading to nothing at radius `r`.
pub(super) fn halo(p: &egui::Painter, c: Pos2, r: f32, color: Color32, k: f32) {
    if k <= 0.003 || r <= 0.5 {
        return;
    }
    let mut m = Mesh::default();
    let rings = [(0.0, 1.0), (0.25, 0.72), (0.5, 0.38), (0.75, 0.13), (1.0, 0.0)];
    let seg = 40u32;
    m.colored_vertex(c, glow(color, k));
    for (rr, a) in rings.iter().skip(1) {
        for j in 0..seg {
            let ang = TAU * j as f32 / seg as f32;
            m.colored_vertex(Pos2::new(c.x + r * rr * ang.cos(), c.y + r * rr * ang.sin()), glow(color, k * a));
        }
    }
    for j in 0..seg {
        m.add_triangle(0, 1 + j, 1 + (j + 1) % seg);
    }
    for ring in 0..(rings.len() as u32 - 2) {
        let (a0, b0) = (1 + ring * seg, 1 + (ring + 1) * seg);
        for j in 0..seg {
            let j1 = (j + 1) % seg;
            m.add_triangle(a0 + j, b0 + j, b0 + j1);
            m.add_triangle(a0 + j, b0 + j1, a0 + j1);
        }
    }
    p.add(Shape::mesh(m));
}

/// Draws texture `t` as the quad with corners `c` (top-left, top-right, bottom-right, bottom-left).
pub(super) fn quad(p: &egui::Painter, t: &TextureHandle, c: [Pos2; 4], tint: Color32) {
    let mut m = Mesh::with_texture(t.id());
    let uvs = [Pos2::new(0.0, 0.0), Pos2::new(1.0, 0.0), Pos2::new(1.0, 1.0), Pos2::new(0.0, 1.0)];
    for (pos, uv) in c.into_iter().zip(uvs) {
        m.vertices.push(egui::epaint::Vertex { pos, uv, color: tint });
    }
    m.indices.extend([0, 1, 2, 0, 2, 3]);
    p.add(Shape::mesh(m));
}

thread_local! {
    /// The phosphor of the screen being drawn (set per unit by the display).
    static SCREEN: std::cell::Cell<Color32> = const { std::cell::Cell::new(PHOSPHOR) };
}

/// The phosphor colour of a finish's screens: green on the metal units, amber on the valve, console,
/// gold and copper units, ice blue on the space units, pale gold (lapis: starlight) on the gnostic ones.
pub fn phosphor(f: super::analog::Finish) -> Color32 {
    use super::analog::Finish::*;
    match f {
        Cream | Gold | Copper => Color32::from_rgb(0xff, 0xb8, 0x4a),
        Basalt | Forged | Obsidian => Color32::from_rgb(0xff, 0xd2, 0x7a),
        Lapis => Color32::from_rgb(0xb4, 0xc4, 0xff),
        Verdigris => Color32::from_rgb(0x84, 0xe4, 0xff),
        _ => PHOSPHOR,
    }
}

pub fn set_screen(c: Color32) {
    SCREEN.with(|s| s.set(c));
}

/// The background of a display: a phosphor screen in the unit's colour (dark glass, a faint glow in the
/// middle, a vignette at the edges).
pub fn screen(p: &egui::Painter, r: Rect, round: f32) {
    let c = SCREEN.with(|s| s.get());
    let base = Color32::from_rgb((c.r() as f32 * 0.05) as u8 + 4, (c.g() as f32 * 0.05) as u8 + 4, (c.b() as f32 * 0.05) as u8 + 4);
    p.rect_filled(r, round, base);
    if c == PHOSPHOR {
        if let Some(t) = art(p.ctx(), "fx-crt", CRT) {
            p.image(t.id(), r, Rect::from_min_max(Pos2::new(0.03, 0.04), Pos2::new(0.97, 0.96)), Color32::from_gray(200));
        }
    } else {
        let cp = p.with_clip_rect(r);
        halo(&cp, r.center(), r.width().max(r.height()) * 0.6, c, 0.07);
    }
    for k in 0..6 {
        p.rect_stroke(r.shrink(k as f32 * 2.0), round, Stroke::new(2.0_f32, Color32::from_black_alpha(60 - k * 10)), egui::StrokeKind::Inside);
    }
}

/// The gnostic units' sigil, turning slowly in `r` as light of colour `c` (additive: only its gold
/// lines show).
pub fn sigil(p: &egui::Painter, r: Rect, t: f32, c: Color32) {
    let Some(tex) = art(p.ctx(), "fx-sigil", SIGIL) else { return };
    let k = 0.16 + 0.05 * (t * 0.6).sin();
    let tint = Color32::from_rgb((c.r() as f32 * 0.55 + 115.0).min(255.0) as u8, (c.g() as f32 * 0.55 + 115.0).min(255.0) as u8, (c.b() as f32 * 0.55 + 115.0).min(255.0) as u8);
    let rad = r.height() * 0.62;
    let pc = p.with_clip_rect(r);
    super::analog::rotated(&pc, &tex, r.center(), rad, t * 0.035, glow(tint, k));
}

/// A recessed panel on a screen (a graph inside a display).
pub fn inset(p: &egui::Painter, r: Rect, round: f32) {
    let c = SCREEN.with(|s| s.get());
    p.rect_filled(r, round, Color32::from_black_alpha(110));
    p.rect_stroke(r, round, Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 34)), egui::StrokeKind::Inside);
}

/// A recessed window in the display (behind the emblem): black velvet with a soft inner shadow.
pub(super) fn window(p: &egui::Painter, r: Rect) {
    p.rect_filled(r, 6.0, Color32::from_rgb(0x0b, 0x08, 0x08));
    for k in 0..5 {
        p.rect_stroke(r.shrink(k as f32 * 2.0), 6.0, Stroke::new(2.0_f32, Color32::from_black_alpha(70 - k * 13)), egui::StrokeKind::Inside);
    }
    p.rect_stroke(r, 6.0, Stroke::new(1.0_f32, Color32::from_white_alpha(22)), egui::StrokeKind::Outside);
}

/// The oscilloscope: phosphor glass with its graticule. Returns the plotting area.
pub(super) fn crt(p: &egui::Painter, r: Rect) -> Rect {
    p.rect_filled(r.expand(2.0), 8.0, Color32::from_rgb(0x05, 0x06, 0x05));
    p.rect_filled(r, 7.0, Color32::from_rgb(0x06, 0x14, 0x0a));
    if let Some(t) = art(p.ctx(), "fx-crt", CRT) {
        p.image(t.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    }
    let g = r.shrink2(Vec2::new(14.0, 12.0));
    let line = Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(0x8c, 0xff, 0xa8, 26));
    for k in 0..=10 {
        let x = g.left() + g.width() * k as f32 / 10.0;
        p.line_segment([Pos2::new(x, g.top()), Pos2::new(x, g.bottom())], line);
    }
    for k in 0..=8 {
        let y = g.top() + g.height() * k as f32 / 8.0;
        p.line_segment([Pos2::new(g.left(), y), Pos2::new(g.right(), y)], line);
    }
    // Minor ticks on the centre axes.
    let tick = Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(0x8c, 0xff, 0xa8, 46));
    for k in 0..=50 {
        let x = g.left() + g.width() * k as f32 / 50.0;
        p.line_segment([Pos2::new(x, g.center().y - 2.5), Pos2::new(x, g.center().y + 2.5)], tick);
    }
    for k in 0..=40 {
        let y = g.top() + g.height() * k as f32 / 40.0;
        p.line_segment([Pos2::new(g.center().x - 2.5, y), Pos2::new(g.center().x + 2.5, y)], tick);
    }
    g
}

/// The glass's reflection, drawn over the traces.
pub(super) fn crt_glass(p: &egui::Painter, r: Rect) {
    let mut m = Mesh::default();
    let (a, b) = (Color32::from_white_alpha(10), Color32::TRANSPARENT);
    let top = r.shrink(3.0);
    m.colored_vertex(top.left_top(), a);
    m.colored_vertex(top.right_top(), a);
    m.colored_vertex(Pos2::new(top.right(), top.top() + top.height() * 0.4), b);
    m.colored_vertex(Pos2::new(top.left(), top.top() + top.height() * 0.4), b);
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    p.add(Shape::mesh(m));
    p.rect_stroke(r, 7.0, Stroke::new(1.0_f32, Color32::from_white_alpha(30)), egui::StrokeKind::Inside);
}

/// A phosphor trace: the beam with its bloom.
pub(super) fn trace(p: &egui::Painter, pts: Vec<Pos2>, c: Color32, k: f32) {
    p.add(Shape::line(pts.clone(), Stroke::new(7.0_f32, glow(c, 0.10 * k))));
    p.add(Shape::line(pts.clone(), Stroke::new(3.5_f32, glow(c, 0.25 * k))));
    p.add(Shape::line(pts, Stroke::new(1.4_f32, c.gamma_multiply(k.clamp(0.0, 1.0)))));
}

/// Text in the phosphor of the scope.
pub(super) fn readout(p: &egui::Painter, at: Pos2, align: egui::Align2, text: &str, size: f32) {
    p.text(at, align, text, egui::FontId::monospace(size), PHOSPHOR.gamma_multiply(0.85));
}

/// The LFO wave of a unit at phase `ph` (radians), −1 … 1.
fn wave(h: &Hero, ph: f32) -> f32 {
    if h.def.id == "tremolo" {
        match h.v("shape").round() as i32 {
            1 => 1.0 - 4.0 * ((ph / TAU).rem_euclid(1.0) - 0.5).abs(),
            2 => {
                if (ph / TAU).rem_euclid(1.0) < 0.5 {
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
}

fn log_x(g: Rect, f: f32) -> f32 {
    g.left() + g.width() * ((f.max(20.0) / 20.0).log10() / 3.0)
}

/// A frequency response (dB, −24 … +12) as a trace, with the frequency marks. Where the notches lie
/// closer than the beam can draw (a flanger's comb up high), each column shows the swing between its
/// peaks and notches as a phosphor smear under the peak line.
fn response(p: &egui::Painter, g: Rect, db: impl Fn(f32) -> f32, k: f32) {
    let y = |d: f32| g.top() + g.height() * (12.0 - d.clamp(-24.0, 12.0)) / 36.0;
    let cols = g.width().max(1.0) as usize;
    let f_at = |x: f32| 20.0 * 1000f32.powf(x / g.width());
    let mut top = Vec::with_capacity(cols + 1);
    for c in 0..=cols {
        let (f0, f1) = (f_at(c as f32), f_at(c as f32 + 1.0));
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for j in 0..=12 {
            let d = db(f0 + (f1 - f0) * j as f32 / 12.0);
            lo = lo.min(d);
            hi = hi.max(d);
        }
        let x = g.left() + c as f32;
        if hi - lo > 6.0 {
            p.line_segment([Pos2::new(x, y(hi)), Pos2::new(x, y(lo))], Stroke::new(1.0_f32, glow(PHOSPHOR, 0.16 * k)));
            top.push(Pos2::new(x, y(hi)));
        } else {
            top.push(Pos2::new(x, y(db((f0 * f1).sqrt()))));
        }
    }
    trace(p, top, PHOSPHOR, k);
    for (f, t) in [(100.0, "100"), (1000.0, "1k"), (10000.0, "10k")] {
        readout(p, Pos2::new(log_x(g, f), g.bottom() - 2.0), egui::Align2::CENTER_BOTTOM, t, 9.0);
    }
    readout(p, Pos2::new(g.left() + 2.0, y(0.0) - 1.0), egui::Align2::LEFT_BOTTOM, "0 dB", 9.0);
}

/// The display of a modulation unit (chorus, flanger, phaser, tremolo).
pub fn draw(p: &egui::Painter, r: Rect, h: &Hero) {
    p.rect_filled(r, 6.0, Color32::from_rgb(0x14, 0x0c, 0x0c));
    let rate = h.v("rate");
    let depth = h.v("depth");
    let t = h.t();
    let shift = t * rate * TAU;
    let lfo = wave(h, shift);
    let win_w = (r.height() * 1.15).min(r.width() * 0.36).max(90.0);
    let win = Rect::from_min_size(r.min + Vec2::new(8.0, 8.0), Vec2::new(win_w, r.height() - 16.0));
    let scope = Rect::from_min_max(Pos2::new(win.right() + 10.0, r.top() + 8.0), r.max - Vec2::new(8.0, 8.0));
    window(p, win);
    let wp = p.with_clip_rect(win.shrink(2.0));
    match h.def.id {
        "chorus" => candles(&wp, win, h, shift, depth, t),
        "flanger" => mercury(&wp, win, h, lfo, depth),
        "phaser" => moon(&wp, win, shift, depth, t),
        _ => wings(&wp, win, h, lfo, depth),
    }
    let g = crt(p, scope);
    let sp = p.with_clip_rect(scope.shrink(3.0));
    match h.def.id {
        "chorus" => {
            // Three voices, each delayed by its own sweep of the LFO.
            let mid = g.center().y;
            let amp = g.height() * 0.36 * depth.max(0.06);
            for (k, bright) in [(0, 1.0), (1, 0.7), (2, 0.5)] {
                let off = k as f32 * TAU / 3.0;
                let pts: Vec<Pos2> = (0..=160).map(|j| j as f32 / 160.0).map(|u| Pos2::new(g.left() + u * g.width(), mid - (u * TAU * 2.0 - shift - off).sin() * amp)).collect();
                trace(&sp, pts, PHOSPHOR, bright);
            }
        }
        "flanger" => {
            // Comb filter of the swept delay: notches walking up and down the spectrum.
            let base = h.v("delay").max(0.1) / 1000.0;
            let tau = base * (1.0 + 0.9 * depth * lfo).max(0.05);
            let fb = h.v("feedback");
            let resp = |f: f32| {
                let w = TAU * f * tau;
                // Feed-forward plus feedback comb: |1 + e^-jw| / |1 - fb e^-jw|, normalised.
                let (c, s) = (w.cos(), w.sin());
                let num = ((1.0 + c).powi(2) + s * s).sqrt() / 2.0;
                let den = ((1.0 - fb * c).powi(2) + (fb * s).powi(2)).sqrt();
                20.0 * (num / den.max(0.05)).max(1e-3).log10()
            };
            response(&sp, g, resp, 1.0);
        }
        "phaser" => {
            // All-pass stages against the dry sound: notches around the swept centre.
            let n = [2.0, 4.0, 6.0, 8.0, 12.0][(h.v("stages").round() as usize).min(4)];
            let fc = h.v("center") * 2f32.powf(2.0 * depth * lfo);
            let fb = h.v("feedback");
            let resp = |f: f32| {
                let phi = -2.0 * (f / fc).atan() * n;
                let num = (phi / 2.0).cos().abs();
                let den = 1.0 - fb * 0.8 * phi.cos();
                20.0 * (num / den.abs().max(0.1)).max(1e-3).log10()
            };
            response(&sp, g, resp, 1.0);
        }
        _ => {
            // A tone through the tremolo: the carrier and its envelope.
            let mid = g.center().y;
            let amp = g.height() * 0.42;
            let gain = |u: f32| 1.0 - depth * (0.5 - 0.5 * wave(h, u * TAU * 2.0 - shift));
            let carrier: Vec<Pos2> = (0..=600).map(|j| j as f32 / 600.0).map(|u| Pos2::new(g.left() + u * g.width(), mid - (u * 150.0 + t * 40.0).sin() * amp * gain(u))).collect();
            trace(&sp, carrier, PHOSPHOR, 0.55);
            for s in [-1.0, 1.0] {
                let env: Vec<Pos2> = (0..=160).map(|j| j as f32 / 160.0).map(|u| Pos2::new(g.left() + u * g.width(), mid - s * amp * gain(u))).collect();
                trace(&sp, env, PHOSPHOR, 1.0);
            }
        }
    }
    let extra = match h.def.id {
        "phaser" => format!("  {} STAGES", [2, 4, 6, 8, 12][(h.v("stages").round() as usize).min(4)]),
        "flanger" => format!("  FB {:+.0}%", h.v("feedback") * 100.0),
        "chorus" => format!("  {:.1} MS", h.v("delay")),
        "tremolo" if h.v("pan") > 0.5 => "  AUTO-PAN".into(),
        _ => String::new(),
    };
    readout(p, g.left_top() + Vec2::new(2.0, 1.0), egui::Align2::LEFT_TOP, &format!("{rate:.2} HZ  DEPTH {:.0}%{extra}", depth * 100.0), 10.0);
    crt_glass(p, scope);
}

/// Coven: three candles, their flames swaying as the three chorus voices.
fn candles(p: &egui::Painter, win: Rect, _h: &Hero, shift: f32, depth: f32, t: f32) {
    let ctx = p.ctx().clone();
    let (Some(body), Some(flame)) = (art(&ctx, "fx-candle", CANDLE), art(&ctx, "fx-flame", FLAME)) else { return };
    let bs = body.size_vec2();
    let fs = flame.size_vec2();
    let floor = win.bottom() - 8.0;
    let unit = win.width() / 3.4;
    for (k, (xf, hf)) in [(0.2, 0.52), (0.5, 0.64), (0.8, 0.46)].into_iter().enumerate() {
        let w = unit;
        let hgt = (w * bs.y / bs.x).min(win.height() * hf);
        let w = hgt * bs.x / bs.y;
        let cx = win.left() + win.width() * xf;
        let br = Rect::from_min_max(Pos2::new(cx - w / 2.0, floor - hgt), Pos2::new(cx + w / 2.0, floor));
        // Warm light on the velvet around each candle.
        let wick = Pos2::new(cx, br.top() + hgt * 0.02);
        let off = k as f32 * TAU / 3.0;
        let sway = (shift + off).sin() * depth;
        let flick = 1.0 + 0.08 * (t * 11.0 + k as f32 * 2.1).sin() * (t * 4.3 + k as f32).sin();
        halo(p, wick - Vec2::new(0.0, 8.0), hgt * 0.9, AMBER, 0.32 * flick);
        p.image(body.id(), br, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        // The flame, leaning with its voice and flickering.
        let fh = hgt * 0.42 * flick;
        let fw = fh * fs.x / fs.y;
        let lean = sway * fw * 0.9;
        let bottom = wick.y + fh * 0.12;
        let c = [Pos2::new(cx - fw / 2.0 + lean, bottom - fh), Pos2::new(cx + fw / 2.0 + lean, bottom - fh), Pos2::new(cx + fw / 2.0, bottom), Pos2::new(cx - fw / 2.0, bottom)];
        quad(p, &flame, c, glow(Color32::WHITE, 1.0));
        quad(p, &flame, c, glow(Color32::WHITE, 0.5));
    }
}

/// Quicksilver: a mercury column in a glass tube, rising and falling with the flanger's delay.
fn mercury(p: &egui::Painter, win: Rect, h: &Hero, lfo: f32, depth: f32) {
    let ctx = p.ctx().clone();
    let Some(glass) = art(&ctx, "fx-glass-tube", GLASS) else { return };
    let gs = glass.size_vec2();
    let th = win.height() - 16.0;
    let tw = th * gs.x / gs.y;
    let tr = Rect::from_center_size(Pos2::new(win.left() + win.width() * 0.38, win.center().y), Vec2::new(tw, th));
    // The bore inside the glass (between the brass caps).
    let bore = Rect::from_min_max(Pos2::new(tr.left() + tw * 0.22, tr.top() + th * 0.085), Pos2::new(tr.right() - tw * 0.22, tr.bottom() - th * 0.075));
    let level = (0.5 + 0.42 * lfo * depth).clamp(0.04, 0.96);
    let top = bore.bottom() - bore.height() * level;
    // The column: a metallic gradient across, a rounded meniscus on top.
    let cols = [70u8, 150, 235, 205, 120, 60];
    let mut m = Mesh::default();
    let n = cols.len() - 1;
    for (k, g) in cols.iter().enumerate() {
        let x = bore.left() + bore.width() * k as f32 / n as f32;
        let c = Color32::from_rgb(*g, *g, (*g as f32 * 1.04).min(255.0) as u8);
        m.colored_vertex(Pos2::new(x, top), c);
        m.colored_vertex(Pos2::new(x, bore.bottom()), c);
    }
    for k in 0..n as u32 {
        let i = k * 2;
        m.add_triangle(i, i + 1, i + 2);
        m.add_triangle(i + 1, i + 3, i + 2);
    }
    p.add(Shape::mesh(m));
    let rad = bore.width() / 2.0;
    let dome: Vec<Pos2> = (0..=16).map(|j| PI + PI * j as f32 / 16.0).map(|a| Pos2::new(bore.center().x + rad * a.cos(), top + rad * 0.55 * a.sin())).collect();
    p.add(Shape::convex_polygon(dome, Color32::from_gray(190), Stroke::NONE));
    p.circle_filled(Pos2::new(bore.center().x - rad * 0.3, top - rad * 0.15), rad * 0.22, Color32::from_white_alpha(200));
    p.image(glass.id(), tr, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    // The scale beside the tube: the delay, in ms.
    let base = h.v("delay");
    let sx = tr.right() + 6.0;
    for k in 0..=10 {
        let y = bore.bottom() - bore.height() * k as f32 / 10.0;
        let long = k % 5 == 0;
        p.line_segment([Pos2::new(sx, y), Pos2::new(sx + if long { 10.0 } else { 5.0 }, y)], Stroke::new(1.0_f32, CREAM.gamma_multiply(0.8)));
        if long {
            let ms = base * (1.0 + 0.9 * depth * (k as f32 / 5.0 - 1.0)).max(0.05);
            p.text(Pos2::new(sx + 13.0, y), egui::Align2::LEFT_CENTER, format!("{ms:.1}"), egui::FontId::proportional(9.0), CREAM.gamma_multiply(0.85));
        }
    }
    p.text(Pos2::new(sx + 13.0, bore.top() - 10.0), egui::Align2::LEFT_CENTER, "MS", egui::FontId::proportional(8.0), CREAM.gamma_multiply(0.7));
    // The pointer at the column's top.
    p.add(Shape::convex_polygon(vec![Pos2::new(sx - 1.0, top), Pos2::new(sx + 7.0, top - 4.0), Pos2::new(sx + 7.0, top + 4.0)], AMBER, Stroke::NONE));
}

/// Moonphase: the moon running through its phases, one cycle per sweep of the phaser.
fn moon(p: &egui::Painter, win: Rect, shift: f32, depth: f32, t: f32) {
    let ctx = p.ctx().clone();
    stars(p, win, t, 40);
    let rad = (win.width().min(win.height()) * 0.40).max(10.0);
    let c = win.center();
    halo(p, c, rad * 1.6, Color32::from_rgb(0xc8, 0xd4, 0xff), 0.22);
    if let Some(m) = art(&ctx, "fx-moon", MOON) {
        p.image(m.id(), Rect::from_center_size(c, Vec2::splat(rad * 2.0)), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    } else {
        p.circle_filled(c, rad, Color32::from_gray(170));
    }
    // The unlit side: phase 0 = new, 0.5 = full. Drawn row by row (a crescent is not convex).
    let ph = (shift / TAU).rem_euclid(1.0);
    let k = (TAU * ph).cos();
    let shade = Color32::from_black_alpha((120.0 + 120.0 * depth.clamp(0.0, 1.0)) as u8);
    let rows = (rad * 2.0) as i32;
    for j in 0..=rows {
        let y = -rad + j as f32 + 0.5;
        let w = (rad * rad - y * y).max(0.0).sqrt();
        let (a, b) = if ph < 0.5 { (-w, k * w) } else { (-k * w, w) };
        if b > a {
            p.line_segment([Pos2::new(c.x + a, c.y + y), Pos2::new(c.x + b, c.y + y)], Stroke::new(1.0_f32, shade));
        }
    }
    p.text(Pos2::new(win.center().x, win.bottom() - 6.0), egui::Align2::CENTER_BOTTOM, ["NEW", "WAXING", "FULL", "WANING"][((ph * 4.0 + 0.5) as usize) % 4], egui::FontId::proportional(8.5), CREAM.gamma_multiply(0.75));
}

/// Wingbeat: a pair of dragon wings beating with the tremolo; with auto-pan they beat in turn.
fn wings(p: &egui::Painter, win: Rect, h: &Hero, lfo: f32, depth: f32) {
    let ctx = p.ctx().clone();
    let Some(w) = art(&ctx, "fx-wing", WING) else { return };
    let root = Pos2::new(win.center().x, win.bottom() - win.height() * 0.22);
    let size = (win.width() * 0.48).min(win.height() * 0.85);
    let pan = h.v("pan") > 0.5;
    // The lamp between the wings: the tremolo's gain.
    let gain = 1.0 - depth * (0.5 - 0.5 * lfo);
    halo(p, root, size * 0.55, AMBER, 0.55 * gain);
    for (side, l) in [(1.0f32, lfo), (-1.0, if pan { -lfo } else { lfo })] {
        // Root at the image's lower-left corner; raise the tip with the LFO.
        let a = -side * (l * 0.38 * depth.max(0.05));
        let (sn, cs) = a.sin_cos();
        let pt = |u: f32, v: f32| {
            // (u, v) in the image (0 … 1), relative to the root at (0.04, 0.96); mirrored for the left.
            let x = (u - 0.04) * size * side;
            let y = (v - 0.96) * size;
            Pos2::new(root.x + x * cs - y * sn, root.y + x * sn + y * cs)
        };
        let corners = [pt(0.0, 0.0), pt(1.0, 0.0), pt(1.0, 1.0), pt(0.0, 1.0)];
        let lift = 0.75 + 0.25 * (0.5 + 0.5 * l);
        let g = (255.0 * lift) as u8;
        quad(p, &w, corners, Color32::from_rgb(g, g, g));
    }
    p.circle_filled(root, 5.0, Color32::from_rgb(0xff, 0xd8, 0x9a).gamma_multiply(0.4 + 0.6 * gain));
}

/// Twinkling stars over `r`.
pub(super) fn stars(p: &egui::Painter, r: Rect, t: f32, n: usize) {
    let mut s = 0x9e37_79b9u32;
    for _ in 0..n {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        let x = r.left() + (s % 1000) as f32 / 1000.0 * r.width();
        let y = r.top() + ((s >> 10) % 1000) as f32 / 1000.0 * r.height();
        let tw = 0.5 + 0.5 * (t * (1.0 + (s % 7) as f32 * 0.4) + (s % 13) as f32).sin();
        p.circle_filled(Pos2::new(x, y), 0.6 + (s % 3) as f32 * 0.35, Color32::from_white_alpha((40.0 + 90.0 * tw) as u8));
    }
}

// ------------------------------------------------------------------------------------------- emblems

/// How a unit's emblem moves.
#[derive(Clone, Copy, PartialEq)]
enum Motion {
    /// Sparkling glints over it.
    Shimmer,
    /// Bubbles rising through its liquid.
    Bubbles,
    /// Stretching wider with the signal.
    Spread,
    /// Trembling (a struck fork, a pulled lodestone, a twitching claw).
    Tremble,
    /// Glowing at points (an eye, a heart) with the signal.
    Eyes(&'static [(f32, f32)]),
    /// Lifting (the portcullis opens as the gate does).
    Rise,
    /// Turning slowly.
    Turn,
    /// Sparks and embers flying up.
    Sparks,
    /// Ripples spreading out.
    Ripples,
    /// Breathing in and out.
    Breathe,
}

/// What a unit's emblem follows: its output level or its gain reduction.
#[derive(Clone, Copy, PartialEq)]
enum Follows {
    Level,
    Reduction,
}

macro_rules! emblem_art {
    ($($id:literal),*) => {
        fn emblem_bytes(id: &str) -> Option<(&'static str, &'static [u8])> {
            match id {
                $($id => Some((concat!("fx-emblem-", $id), include_bytes!(concat!("../../assets/fx/emblems/", $id, ".png")))),)*
                _ => None,
            }
        }
    };
}
emblem_art!(
    "alter",
    "bitcrusher",
    "class_a_strip",
    "clean_voice",
    "clipper",
    "compressor",
    "deesser",
    "delay",
    "dragon_particle",
    "drum_bus",
    "eq",
    "filter",
    "gate",
    "imager",
    "limiter",
    "multiband",
    "multisat",
    "reverb",
    "saturator",
    "transient",
    "tune",
    "utility"
);

fn emblem_spec(id: &str) -> (Motion, Follows, Color32) {
    use Follows::*;
    use Motion::*;
    match id {
        "eq" => (Shimmer, Level, Color32::from_rgb(0xd8, 0xe8, 0xff)),
        "filter" => (Bubbles, Level, Color32::from_rgb(0x6c, 0xff, 0x7a)),
        "utility" => (Bubbles, Level, Color32::from_rgb(0x5a, 0x8c, 0xff)),
        "imager" => (Spread, Level, Color32::from_rgb(0xd8, 0xa0, 0x5a)),
        "tune" => (Tremble, Level, Color32::from_rgb(0xff, 0xd0, 0x60)),
        "compressor" => (Eyes(&[(0.63, 0.30)]), Reduction, Color32::from_rgb(0xff, 0x9a, 0x2c)),
        "limiter" => (Tremble, Reduction, Color32::from_rgb(0x9a, 0xb8, 0xff)),
        "gate" => (Rise, Level, Color32::from_rgb(0xff, 0xb0, 0x60)),
        "deesser" => (Eyes(&[(0.52, 0.17)]), Reduction, Color32::from_rgb(0x6c, 0xff, 0x9a)),
        "transient" => (Tremble, Level, Color32::from_rgb(0xff, 0xa0, 0x50)),
        "multiband" => (Turn, Level, Color32::from_rgb(0xff, 0xff, 0xff)),
        "class_a_strip" => (Eyes(&[(0.30, 0.47), (0.70, 0.47)]), Level, Color32::from_rgb(0xff, 0x8a, 0x2c)),
        "drum_bus" => (Sparks, Level, Color32::from_rgb(0xff, 0xa8, 0x3c)),
        "saturator" => (Sparks, Level, Color32::from_rgb(0xff, 0x8a, 0x2c)),
        "multisat" => (Eyes(&[(0.50, 0.55)]), Level, Color32::from_rgb(0xff, 0x7a, 0x2c)),
        "alter" => (Breathe, Level, Color32::from_rgb(0xd8, 0xa0, 0x5a)),
        "clipper" => (Shimmer, Level, Color32::from_rgb(0xff, 0xd8, 0x80)),
        "reverb" => (Ripples, Level, Color32::from_rgb(0xb0, 0x70, 0xff)),
        "delay" => (Ripples, Level, Color32::from_rgb(0x50, 0xff, 0xd8)),
        "bitcrusher" => (Turn, Level, Color32::from_rgb(0xff, 0x80, 0x40)),
        "dragon_particle" => (Breathe, Level, Color32::from_rgb(0xff, 0x30, 0x30)),
        "clean_voice" => (Ripples, Level, Color32::from_rgb(0xc8, 0xe8, 0xff)),
        _ => (Breathe, Level, AMBER),
    }
}

/// Whether a unit has an animated emblem in its nameplate.
pub fn has_emblem(id: &str) -> bool {
    emblem_bytes(id).is_some()
}

/// Whether the emblem follows gain reduction (otherwise the output level).
pub fn emblem_follows_reduction(id: &str) -> bool {
    emblem_spec(id).1 == Follows::Reduction
}

fn hash(a: u32) -> f32 {
    let mut x = a.wrapping_mul(0x9e37_79b9) ^ 0x85eb_ca6b;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    (x % 10_000) as f32 / 10_000.0
}

/// Draws unit `id`'s emblem in its lit window `r`, moving with `s` (0 … 1: the level or the gain
/// reduction it follows) at time `t`.
pub fn emblem(p: &egui::Painter, r: Rect, id: &str, s: f32, t: f32) {
    let Some((name, bytes)) = emblem_bytes(id) else { return };
    let Some(tex) = art(p.ctx(), name, bytes) else { return };
    let (motion, _, c) = emblem_spec(id);
    let s = s.clamp(0.0, 1.0);
    window(p, r);
    let p = p.with_clip_rect(r.shrink(2.0));
    let ts = tex.size_vec2();
    let inner = r.shrink(7.0);
    let k = (inner.width() / ts.x).min(inner.height() / ts.y);
    let mut size = ts * k;
    let mut center = inner.center();
    let mut rot = 0.0;
    halo(&p, center, r.width() * 0.6, c, 0.08 + 0.25 * s);
    match motion {
        Motion::Spread => size.x *= 0.85 + 0.25 * s,
        Motion::Breathe => size *= 0.94 + 0.06 * (t * 2.2).sin() * (0.3 + 0.7 * s) + 0.04 * s,
        Motion::Tremble => {
            rot = (t * 47.0).sin() * 0.06 * s;
            center.x += (t * 61.0).sin() * 1.5 * s;
        }
        Motion::Rise => center.y -= size.y * 0.55 * s,
        Motion::Turn => rot = t * (0.15 + 0.9 * s),
        _ => {}
    }
    let (sn, cs) = rot.sin_cos();
    let pt = |x: f32, y: f32| Pos2::new(center.x + x * cs - y * sn, center.y + x * sn + y * cs);
    let (hx, hy) = (size.x / 2.0, size.y / 2.0);
    quad(&p, &tex, [pt(-hx, -hy), pt(hx, -hy), pt(hx, hy), pt(-hx, hy)], Color32::from_gray((150.0 + 105.0 * (0.4 + 0.6 * s)) as u8));
    let img = Rect::from_center_size(center, size);
    match motion {
        Motion::Shimmer => {
            for i in 0..10u32 {
                let ph = t * (0.7 + hash(i) * 0.8) + hash(i + 50);
                let cyc = ph.floor() as u32;
                let f = ph.fract();
                let at = Pos2::new(img.left() + img.width() * (0.2 + 0.6 * hash(i * 7 + cyc)), img.top() + img.height() * (0.1 + 0.6 * hash(i * 13 + cyc)));
                let a = (f * PI).sin() * (0.25 + 0.75 * s);
                halo(&p, at, 6.0, c, 0.8 * a);
                p.line_segment([at - Vec2::new(4.0 * a, 0.0), at + Vec2::new(4.0 * a, 0.0)], Stroke::new(1.0_f32, glow(Color32::WHITE, a)));
                p.line_segment([at - Vec2::new(0.0, 4.0 * a), at + Vec2::new(0.0, 4.0 * a)], Stroke::new(1.0_f32, glow(Color32::WHITE, a)));
            }
        }
        Motion::Bubbles => {
            let liquid = Rect::from_min_max(Pos2::new(img.left() + img.width() * 0.3, img.top() + img.height() * 0.45), Pos2::new(img.right() - img.width() * 0.3, img.bottom() - img.height() * 0.12));
            let n = (3.0 + 9.0 * s) as u32;
            for i in 0..n {
                let ph = t * (0.4 + 0.5 * hash(i)) + hash(i + 9);
                let cyc = ph.floor() as u32;
                let f = ph.fract();
                let x = liquid.left() + liquid.width() * hash(i * 31 + cyc) + (f * 9.0 + i as f32).sin() * 1.5;
                let y = liquid.bottom() - liquid.height() * f;
                p.circle_stroke(Pos2::new(x, y), 1.0 + 1.5 * hash(i + 3), Stroke::new(1.0_f32, glow(c, 0.9 * (1.0 - f))));
            }
        }
        Motion::Eyes(points) => {
            for (u, v) in points {
                let at = Pos2::new(img.left() + img.width() * u, img.top() + img.height() * v);
                let flick = 1.0 + 0.06 * (t * 9.0).sin();
                halo(&p, at, 6.0 + 10.0 * s, c, (0.25 + 0.75 * s) * flick);
            }
        }
        Motion::Sparks => {
            let n = (2.0 + 16.0 * s) as u32;
            let from = Pos2::new(img.center().x, img.top() + img.height() * 0.25);
            for i in 0..n {
                let ph = t * (0.8 + 0.8 * hash(i)) + hash(i + 77);
                let cyc = ph.floor() as u32;
                let f = ph.fract();
                let dir = (hash(i * 17 + cyc) - 0.5) * 2.0;
                let at = Pos2::new(from.x + dir * r.width() * 0.45 * f, from.y - r.height() * 0.5 * f + r.height() * 0.4 * f * f);
                p.circle_filled(at, 1.2, glow(Color32::from_rgb(0xff, 0xd0, 0x80), 1.0 - f));
                halo(&p, at, 4.0, c, 0.5 * (1.0 - f));
            }
        }
        Motion::Ripples => {
            let at = Pos2::new(img.center().x, img.top() + img.height() * 0.62);
            for i in 0..3 {
                let f = (t * (0.35 + 0.5 * s) + i as f32 / 3.0).fract();
                let rr = Vec2::new(img.width() * 0.45 * f, img.height() * 0.14 * f);
                let pts: Vec<Pos2> = (0..=32).map(|j| j as f32 / 32.0 * TAU).map(|a| Pos2::new(at.x + rr.x * a.cos(), at.y + rr.y * a.sin())).collect();
                p.add(Shape::line(pts, Stroke::new(1.0_f32, glow(c, (1.0 - f) * (0.3 + 0.6 * s)))));
            }
        }
        Motion::Turn | Motion::Spread | Motion::Breathe | Motion::Tremble | Motion::Rise => {
            halo(&p, img.center(), img.width() * 0.3, c, 0.12 * s);
        }
    }
}
