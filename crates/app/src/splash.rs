//! Splash screen. Plays the animated logo generated with Higgsfield AI (`assets/splash.gif`) when it is
//! present next to the executable or in the source tree; otherwise draws an animated logo procedurally.

use crate::theme;
use egui::{Color32, Pos2, Stroke, TextureHandle, Vec2};
use std::time::{Duration, Instant};

const SHOW_FOR: f32 = 2.6;
const FADE: f32 = 0.5;

pub struct Splash {
    frames: Vec<(TextureHandle, Duration)>,
    total: Duration,
    start: Instant,
    reduced_motion: bool,
}

fn gif_candidates() -> Vec<std::path::PathBuf> {
    let mut v = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            v.push(d.join("assets").join("splash.gif"));
        }
    }
    v.push(std::path::PathBuf::from("assets/splash.gif"));
    v.push(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join("splash.gif"));
    v
}

fn load_gif(ctx: &egui::Context) -> Vec<(TextureHandle, Duration)> {
    use image::AnimationDecoder;
    for p in gif_candidates() {
        let Ok(f) = std::fs::File::open(&p) else { continue };
        let Ok(dec) = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(f)) else { continue };
        let Ok(frames) = dec.into_frames().collect_frames() else { continue };
        let mut out = Vec::new();
        for (i, fr) in frames.into_iter().enumerate().take(240) {
            let (num, den) = fr.delay().numer_denom_ms();
            let ms = num.checked_div(den).unwrap_or(80).max(20);
            let buf = fr.into_buffer();
            let img = egui::ColorImage::from_rgba_unmultiplied([buf.width() as usize, buf.height() as usize], buf.as_raw());
            out.push((ctx.load_texture(format!("splash{i}"), img, egui::TextureOptions::LINEAR), Duration::from_millis(ms as u64)));
        }
        if !out.is_empty() {
            tracing::info!("splash animation: {} ({} frames)", p.display(), out.len());
            return out;
        }
    }
    Vec::new()
}

impl Splash {
    pub fn new(ctx: &egui::Context, reduced_motion: bool) -> Self {
        let frames = load_gif(ctx);
        let total = frames.iter().map(|f| f.1).sum();
        Splash { frames, total, start: Instant::now(), reduced_motion }
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
        painter.rect_filled(screen, 0.0, theme::BG.gamma_multiply(alpha));
        let c = screen.center() - Vec2::new(0.0, 30.0);
        let size = screen.width().min(screen.height()) * 0.36;
        if !self.frames.is_empty() {
            let mut at = if self.reduced_motion { Duration::ZERO } else { Duration::from_secs_f32(t) };
            if !self.total.is_zero() {
                at = Duration::from_nanos((at.as_nanos() % self.total.as_nanos()) as u64);
            }
            let mut idx = 0;
            for (i, (_, d)) in self.frames.iter().enumerate() {
                if at < *d {
                    idx = i;
                    break;
                }
                at -= *d;
            }
            let tex = &self.frames[idx].0;
            let rect = egui::Rect::from_center_size(c, Vec2::splat(size));
            painter.image(tex.id(), rect, egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE.gamma_multiply(alpha));
        } else {
            draw_logo(&painter, c, size * 0.5, if self.reduced_motion { 0.0 } else { t }, alpha);
        }
        let ty = c.y + size * 0.62;
        painter.text(Pos2::new(c.x, ty), egui::Align2::CENTER_TOP, "DRAGON DAW", egui::FontId::proportional(30.0), Color32::WHITE.gamma_multiply(alpha));
        painter.text(Pos2::new(c.x, ty + 38.0), egui::Align2::CENTER_TOP, format!("record · make beats · mix · master   v{}", env!("CARGO_PKG_VERSION")), egui::FontId::proportional(13.0), theme::TEXT_DIM.gamma_multiply(alpha));
        ctx.request_repaint();
        true
    }
}

fn hexagon(c: Pos2, r: f32, rot: f32) -> Vec<Pos2> {
    (0..6).map(|i| {
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

/// The small brand mark in the menu bar.
pub fn logo(ui: &mut egui::Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.add(egui::Shape::convex_polygon(hexagon(rect.center(), size * 0.48, 0.0), theme::ACCENT, Stroke::new(1.0_f32, theme::ACCENT_2)));
    let c = rect.center();
    let w = size * 0.28;
    p.line_segment([Pos2::new(c.x - w, c.y), Pos2::new(c.x - w * 0.4, c.y - w * 0.8)], Stroke::new(1.5_f32, Color32::WHITE));
    p.line_segment([Pos2::new(c.x - w * 0.4, c.y - w * 0.8), Pos2::new(c.x + w * 0.2, c.y + w * 0.8)], Stroke::new(1.5_f32, Color32::WHITE));
    p.line_segment([Pos2::new(c.x + w * 0.2, c.y + w * 0.8), Pos2::new(c.x + w, c.y)], Stroke::new(1.5_f32, Color32::WHITE));
}
