//! Splash screen: the Dragon DAW logo (from the original Dragon DAW) rising out of a red glow.

use crate::theme;
use egui::{Color32, Pos2, Stroke, Vec2};
use std::time::Instant;

const SHOW_FOR: f32 = 2.6;
const FADE: f32 = 0.5;

pub struct Splash {
    start: Instant,
    reduced_motion: bool,
}

impl Splash {
    pub fn new(_ctx: &egui::Context, reduced_motion: bool) -> Self {
        Splash { start: Instant::now(), reduced_motion }
    }

    /// Draws the splash; returns `false` when it is finished.
    pub fn show(&mut self, ctx: &egui::Context) -> bool {
        let t = self.start.elapsed().as_secs_f32();
        let skip = ctx.input(|i| i.pointer.any_click() || i.key_pressed(egui::Key::Escape));
        if t > SHOW_FOR + FADE || skip {
            return false;
        }
        let alpha = if t > SHOW_FOR { 1.0 - (t - SHOW_FOR) / FADE } else { 1.0 };
        let screen = ctx.screen_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("dragon.splash")));
        painter.rect_filled(screen, 0.0, Color32::from_rgb(0x14, 0x0a, 0x08).gamma_multiply(alpha));
        let c = screen.center() - Vec2::new(0.0, 30.0);
        let size = screen.width().min(screen.height()) * 0.5;
        // Rise in: scale up from 85 % and fade in over the first half second.
        let rise = if self.reduced_motion { 1.0 } else { (t / 0.6).clamp(0.0, 1.0) };
        let ease = 1.0 - (1.0 - rise).powi(3);
        let glow = if self.reduced_motion { 0.6 } else { 0.55 + 0.25 * (t * 2.4).sin() };
        // A soft glow: many faint discs, the outer ones fainter.
        for k in (0..28).rev() {
            let q = k as f32 / 27.0;
            let rr = size * (0.3 + 0.55 * q);
            painter.circle_filled(c, rr, Color32::from_rgba_unmultiplied(200, 34, 16, (5.0 * glow * alpha * ease * (1.0 - q)) as u8));
        }
        let s = size * (0.85 + 0.15 * ease);
        let rect = egui::Rect::from_center_size(c, Vec2::splat(s));
        match crate::brand::logo(ctx) {
            Some(tex) => {
                painter.image(tex.id(), rect, egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE.gamma_multiply(alpha * ease));
            }
            None => draw_logo(&painter, c, s * 0.5, t, alpha * ease),
        }
        painter.text(
            Pos2::new(c.x, rect.bottom() + 8.0),
            egui::Align2::CENTER_TOP,
            format!("DAW  ·  record · make beats · mix · master   v{}", env!("CARGO_PKG_VERSION")),
            egui::FontId::proportional(14.0),
            theme::TEXT_DIM.gamma_multiply(alpha * ease),
        );
        ctx.request_repaint();
        true
    }
}

fn hexagon(c: Pos2, r: f32, rot: f32) -> Vec<Pos2> {
    (0..6)
        .map(|i| {
            let a = rot + i as f32 * std::f32::consts::TAU / 6.0;
            Pos2::new(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect()
}

/// Animated hexagon with a flame gradient and a pulsing waveform through it.
pub fn draw_logo(p: &egui::Painter, c: Pos2, r: f32, t: f32, alpha: f32) {
    let pulse = 1.0 + 0.04 * (t * 3.0).sin();
    for k in 0..4 {
        let rr = r * pulse * (1.0 - k as f32 * 0.08);
        let col = theme::ACCENT.lerp_to_gamma(theme::ACCENT_2, k as f32 / 3.0).gamma_multiply(alpha * (1.0 - k as f32 * 0.18));
        p.add(egui::Shape::closed_line(hexagon(c, rr, t * 0.2 + k as f32 * 0.06), Stroke::new(3.0 - k as f32 * 0.5, col)));
    }
    p.add(egui::Shape::convex_polygon(hexagon(c, r * 0.62 * pulse, t * 0.2), theme::ACCENT.gamma_multiply(0.18 * alpha), Stroke::NONE));
    let n = 64;
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| {
            let x = i as f32 / n as f32;
            let env = (x * std::f32::consts::PI).sin();
            let y = env * (x * 18.0 - t * 6.0).sin() * (0.35 + 0.25 * (t * 2.0).sin().abs());
            Pos2::new(c.x - r * 0.8 + x * r * 1.6, c.y + y * r * 0.5)
        })
        .collect();
    p.add(egui::Shape::line(pts, Stroke::new(2.5_f32, theme::PLAYHEAD.gamma_multiply(alpha))));
}

/// The small brand mark in the menu bar: the dragon's head.
pub fn logo(ui: &mut egui::Ui, size: f32) {
    match crate::brand::mark(ui.ctx()) {
        Some(tex) => {
            ui.add(egui::Image::new(&tex).fit_to_exact_size(Vec2::splat(size)).corner_radius(4.0));
        }
        None => {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
            draw_logo(&ui.painter_at(rect), rect.center(), size * 0.45, 0.0, 1.0);
        }
    }
}
