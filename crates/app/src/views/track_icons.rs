//! Track icons, drawn as vector shapes (they never depend on a font): a small set for drums, bass,
//! instruments, vocals, effects and buses. A mixer track without a chosen icon shows the one its name
//! suggests ("Kick" → kick drum, "Vox 09" → microphone).

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2};
use std::f32::consts::{PI, TAU};

/// (key, label) of every icon, in picker order.
pub const ICONS: &[(&str, &str)] = &[
    ("kick", "Kick"),
    ("snare", "Snare"),
    ("hat", "Hi-hat / cymbal"),
    ("clap", "Clap"),
    ("perc", "Percussion"),
    ("bass", "Bass"),
    ("sub", "808 / sub"),
    ("synth", "Synth"),
    ("keys", "Keys / piano"),
    ("lead", "Lead"),
    ("pad", "Pad"),
    ("vocal", "Vocal"),
    ("guitar", "Guitar"),
    ("fx", "FX"),
    ("reverb", "Reverb / delay"),
    ("sfx", "SFX / riser"),
    ("bus", "Bus / group"),
    ("master", "Master"),
];

/// The icon a track name suggests (empty when nothing fits).
pub fn guess(name: &str) -> &'static str {
    let n = name.to_lowercase();
    let has = |keys: &[&str]| keys.iter().any(|k| n.contains(k));
    if has(&["master"]) {
        "master"
    } else if has(&["kick", "bd"]) {
        "kick"
    } else if has(&["snare", "rim"]) {
        "snare"
    } else if has(&["hat", "hh", "cymbal", "crash", "ride", "open"]) {
        "hat"
    } else if has(&["clap", "snap"]) {
        "clap"
    } else if has(&["perc", "shaker", "tamb", "conga", "bongo", "tom"]) {
        "perc"
    } else if has(&["808", "sub"]) {
        "sub"
    } else if has(&["bass"]) {
        "bass"
    } else if has(&["vox", "vocal", "voice", "adlib", "rap", "hook", "verse"]) {
        "vocal"
    } else if has(&["guitar", "gtr"]) {
        "guitar"
    } else if has(&["piano", "keys", "rhodes", "organ", "epiano"]) {
        "keys"
    } else if has(&["pad", "string", "choir", "atmo"]) {
        "pad"
    } else if has(&["lead", "pluck", "arp", "bell", "melody"]) {
        "lead"
    } else if has(&["synth"]) {
        "synth"
    } else if has(&["reverb", "verb", "delay", "echo"]) {
        "reverb"
    } else if has(&["sfx", "riser", "fx", "impact", "sweep", "noise", "uplifter", "downlifter"]) {
        "sfx"
    } else if has(&["bus", "group", "drums", "beat"]) {
        "bus"
    } else {
        ""
    }
}

/// Draws icon `key` in `r`: the Higgsfield-made gold icon, dimmed when `col` is a dim colour (e.g. not
/// selected); the vector drawing below if the icon set cannot be loaded. An unknown key draws nothing.
pub fn draw(p: &Painter, r: Rect, key: &str, col: Color32) {
    if let Some(k) = ICONS.iter().position(|(k, _)| *k == key) {
        let bright = (col.r().max(col.g()).max(col.b()) as f32 / 255.0).clamp(0.35, 1.0);
        let g = (255.0 * bright) as u8;
        if crate::brand::paint_icon(p, r, k, Color32::from_rgb(g, g, g)) {
            return;
        }
    }
    draw_vector(p, r, key, col);
}

/// The vector fallback of [`draw`].
fn draw_vector(p: &Painter, r: Rect, key: &str, col: Color32) {
    let s = r.width().min(r.height());
    let c = r.center();
    let at = |x: f32, y: f32| Pos2::new(c.x + (x - 0.5) * s, c.y + (y - 0.5) * s);
    let w = (s * 0.09).max(1.2);
    let line = Stroke::new(w, col);
    let poly = |pts: Vec<Pos2>| Shape::line(pts, line);
    let wave = |f: &dyn Fn(f32) -> f32, cycles: f32, amp: f32| -> Shape { poly((0..=32).map(|i| i as f32 / 32.0).map(|t| at(0.08 + 0.84 * t, 0.5 - amp * f(t * cycles))).collect()) };
    match key {
        "kick" => {
            p.circle_stroke(at(0.5, 0.5), s * 0.4, line);
            p.circle_filled(at(0.5, 0.5), s * 0.14, col);
        }
        "snare" => {
            p.add(Shape::ellipse_stroke(at(0.5, 0.42), Vec2::new(s * 0.38, s * 0.12), line));
            p.line_segment([at(0.12, 0.42), at(0.12, 0.75)], line);
            p.line_segment([at(0.88, 0.42), at(0.88, 0.75)], line);
            p.add(Shape::line((0..=16).map(|i| i as f32 / 16.0).map(|t| at(0.12 + 0.76 * t, 0.75 + 0.1 * (PI * t).sin())).collect(), line));
            p.line_segment([at(0.3, 0.08), at(0.62, 0.36)], line);
            p.line_segment([at(0.75, 0.08), at(0.45, 0.36)], line);
        }
        "hat" => {
            p.add(Shape::ellipse_stroke(at(0.5, 0.3), Vec2::new(s * 0.42, s * 0.08), line));
            p.add(Shape::ellipse_stroke(at(0.5, 0.42), Vec2::new(s * 0.42, s * 0.08), line));
            p.line_segment([at(0.5, 0.42), at(0.5, 0.92)], line);
            p.line_segment([at(0.3, 0.92), at(0.7, 0.92)], line);
        }
        "clap" => {
            p.circle_filled(at(0.5, 0.5), s * 0.12, col);
            for k in 0..8 {
                let a = k as f32 / 8.0 * TAU;
                p.line_segment([at(0.5 + 0.22 * a.cos(), 0.5 + 0.22 * a.sin()), at(0.5 + 0.42 * a.cos(), 0.5 + 0.42 * a.sin())], line);
            }
        }
        "perc" => {
            p.add(poly(vec![at(0.5, 0.12), at(0.88, 0.82), at(0.12, 0.82), at(0.42, 0.27)]));
            p.line_segment([at(0.62, 0.55), at(0.92, 0.25)], line);
        }
        "bass" => {
            p.add(wave(&|x| (TAU * x).sin(), 1.5, 0.28));
        }
        "sub" => {
            p.add(wave(&|x| (TAU * x).sin(), 0.75, 0.38));
            p.line_segment([at(0.08, 0.88), at(0.92, 0.88)], Stroke::new(w * 0.7, col.gamma_multiply(0.6)));
        }
        "synth" => {
            p.add(poly(vec![at(0.08, 0.7), at(0.08, 0.3), at(0.36, 0.3), at(0.36, 0.7), at(0.64, 0.7), at(0.64, 0.3), at(0.92, 0.3), at(0.92, 0.7)]));
        }
        "keys" => {
            let k = Rect::from_min_max(at(0.08, 0.2), at(0.92, 0.82));
            p.rect_stroke(k, 1.5, line, egui::StrokeKind::Inside);
            for i in 1..4 {
                let x = k.left() + k.width() * i as f32 / 4.0;
                p.line_segment([Pos2::new(x, k.top()), Pos2::new(x, k.bottom())], Stroke::new(w * 0.7, col));
            }
            for i in [1.0f32, 2.0, 3.0] {
                let x = k.left() + k.width() * i / 4.0;
                p.rect_filled(Rect::from_center_size(Pos2::new(x, k.top() + k.height() * 0.3), Vec2::new(k.width() * 0.11, k.height() * 0.58)), 0.5, col);
            }
        }
        "lead" => {
            p.add(poly(vec![at(0.08, 0.72), at(0.36, 0.28), at(0.36, 0.72), at(0.64, 0.28), at(0.64, 0.72), at(0.92, 0.28)]));
        }
        "pad" => {
            for (x, y, rr) in [(0.32, 0.56, 0.2), (0.52, 0.42, 0.24), (0.72, 0.58, 0.18)] {
                p.circle_stroke(at(x, y), s * rr, line);
            }
        }
        "vocal" => {
            let cap = Rect::from_min_max(at(0.36, 0.08), at(0.64, 0.56));
            p.rect_stroke(cap, s * 0.14, line, egui::StrokeKind::Inside);
            p.add(Shape::line((0..=16).map(|i| i as f32 / 16.0).map(|t| at(0.24 + 0.52 * t, 0.46 + 0.22 * (PI * t).sin())).collect(), line));
            p.line_segment([at(0.5, 0.68), at(0.5, 0.88)], line);
            p.line_segment([at(0.34, 0.9), at(0.66, 0.9)], line);
        }
        "guitar" => {
            p.circle_stroke(at(0.36, 0.66), s * 0.2, line);
            p.circle_stroke(at(0.5, 0.48), s * 0.14, line);
            p.line_segment([at(0.56, 0.42), at(0.9, 0.08)], line);
            p.circle_filled(at(0.36, 0.66), s * 0.06, col);
        }
        "fx" => {
            let star = |cx: f32, cy: f32, rr: f32| -> Vec<Pos2> {
                (0..8)
                    .map(|k| {
                        let a = k as f32 / 8.0 * TAU - PI / 2.0;
                        let d = if k % 2 == 0 { rr } else { rr * 0.32 };
                        at(cx + d * a.cos(), cy + d * a.sin())
                    })
                    .collect()
            };
            p.add(Shape::convex_polygon(star(0.42, 0.52, 0.36), Color32::TRANSPARENT, line));
            let mut st = star(0.42, 0.52, 0.36);
            st.push(st[0]);
            p.add(poly(st));
            p.circle_filled(at(0.8, 0.2), s * 0.07, col);
        }
        "reverb" => {
            p.circle_filled(at(0.2, 0.5), s * 0.08, col);
            for k in 1..=3 {
                let rr = 0.2 * k as f32;
                p.add(Shape::line((0..=12).map(|i| -0.9 + 1.8 * i as f32 / 12.0).map(|a| at(0.2 + rr * a.cos(), 0.5 + rr * a.sin())).collect(), Stroke::new(w, col.gamma_multiply(1.0 - 0.2 * k as f32))));
            }
        }
        "sfx" => {
            p.add(poly(vec![at(0.6, 0.06), at(0.3, 0.52), at(0.52, 0.52), at(0.4, 0.94), at(0.72, 0.42), at(0.5, 0.42), at(0.6, 0.06)]));
        }
        "bus" => {
            for y in [0.22f32, 0.5, 0.78] {
                p.line_segment([at(0.1, y), at(0.55, 0.5)], line);
            }
            p.line_segment([at(0.55, 0.5), at(0.9, 0.5)], Stroke::new(w * 1.4, col));
            p.add(poly(vec![at(0.76, 0.38), at(0.9, 0.5), at(0.76, 0.62)]));
        }
        "master" => {
            p.add(poly(vec![at(0.12, 0.74), at(0.12, 0.3), at(0.32, 0.5), at(0.5, 0.2), at(0.68, 0.5), at(0.88, 0.3), at(0.88, 0.74), at(0.12, 0.74)]));
            p.line_segment([at(0.12, 0.86), at(0.88, 0.86)], line);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_suggest_icons() {
        for (name, icon) in [("Kick", "kick"), ("27_Vox09", "vocal"), ("808 Glide", "sub"), ("Closed hat", "hat"), ("Reverb", "reverb"), ("Drum Bus", "bus"), ("Lead Pluck", "lead"), ("Track 3", "")] {
            assert_eq!(guess(name), icon, "{name}");
        }
        for (k, _) in ICONS {
            assert!(ICONS.iter().filter(|x| x.0 == *k).count() == 1);
        }
    }

    /// Every icon draws something (and nothing panics at small sizes).
    #[test]
    fn every_icon_draws() {
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let p = ctx.layer_painter(egui::LayerId::background());
            for (k, _) in ICONS {
                for size in [10.0, 16.0, 40.0] {
                    draw(&p, Rect::from_min_size(Pos2::ZERO, Vec2::splat(size)), k, Color32::WHITE);
                }
            }
        });
    }
}
