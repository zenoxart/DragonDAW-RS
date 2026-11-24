//! The analog hardware look of the stock mixing effects: every effect is drawn as a rack unit, with a
//! faceplate in its own finish (photographed materials generated for Dragon DAW), rack ears with
//! screws, an engraved nameplate, a pilot lamp, a valve window on the valve units and a pair of moving-
//! coil VU meters whose needles swing with the audio (spring ballistics, drawn in code each frame).
//!
//! Only the look lives here: every control, display and gesture of an effect is unchanged
//! ([`super::fx_panel`] lays the controls out, [`super::fx_ui`] draws the displays).

use super::fx_panel::{KnobStyle, Skin};
use egui::{Color32, Mesh, Pos2, Rect, Shape, Stroke, TextureHandle, Vec2};
use std::f32::consts::PI;

/// A faceplate finish.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Finish {
    /// Brushed aluminium (EQ, filter, utilities): black ink, gunmetal knobs.
    Aluminium,
    /// Black anodised (dynamics): white ink, aluminium knobs.
    Black,
    /// Cream valve enamel (saturation, shape-shifting): black ink, chicken-head knobs.
    Cream,
    /// Brushed gold (the soft clipper).
    Gold,
    /// Oxblood crackle (modulation): cream knobs.
    Oxblood,
    /// Black cave rock with amethyst veins (Dragon's Lair).
    Amethyst,
    /// Wet slate with teal mineral streaks (Echo Cavern).
    Slate,
    /// Dark copper (crushers): gunmetal knobs.
    Copper,
    /// Carved basalt with runes (Golem Strip): the gnostic units, gold-engraved, brass knobs.
    Basalt,
    /// Blackened forged iron with ember cracks (Dragonforge).
    Forged,
    /// Lapis lazuli with gold stars (Purify).
    Lapis,
    /// Black obsidian with a gold diagram (Philosopher's Stone).
    Obsidian,
    /// Polished cinnabar, vermilion with dark veins (Cinnabar, the synth): cream knobs, like the modulation units.
    Cinnabar,
    /// Dark dragon-scale hide with gold edges (Wyrmdrum, the drum pads).
    Wyrmhide,
    /// Jade-green enamel with fine scales (Jadefang, the character EQ): black knurled knobs.
    Jade,
}

impl Finish {
    /// The average colour of the finish (behind the texture, and its fallback).
    pub fn base(self) -> Color32 {
        match self {
            Finish::Aluminium => Color32::from_rgb(0xb4, 0xb6, 0xb8),
            Finish::Black => Color32::from_rgb(0x1c, 0x1d, 0x20),
            Finish::Cream => Color32::from_rgb(0xd9, 0xcc, 0xae),
            Finish::Gold => Color32::from_rgb(0xa8, 0x86, 0x48),
            Finish::Oxblood => Color32::from_rgb(0x5a, 0x1c, 0x1e),
            Finish::Amethyst => Color32::from_rgb(0x22, 0x1e, 0x26),
            Finish::Slate => Color32::from_rgb(0x1c, 0x24, 0x26),
            Finish::Copper => Color32::from_rgb(0x4a, 0x2e, 0x24),
            Finish::Basalt => Color32::from_rgb(0x2a, 0x28, 0x26),
            Finish::Forged => Color32::from_rgb(0x34, 0x32, 0x33),
            Finish::Lapis => Color32::from_rgb(0x1a, 0x1e, 0x6a),
            Finish::Obsidian => Color32::from_rgb(0x0a, 0x0a, 0x0c),
            Finish::Cinnabar => Color32::from_rgb(0x46, 0x0e, 0x0c),
            Finish::Wyrmhide => Color32::from_rgb(0x2a, 0x2a, 0x1c),
            Finish::Jade => Color32::from_rgb(0x1e, 0x4e, 0x2a),
        }
    }

    /// Light finishes are printed in black, dark ones in white.
    pub fn light(self) -> bool {
        matches!(self, Finish::Aluminium | Finish::Cream | Finish::Gold)
    }

    pub fn ink(self) -> Color32 {
        match self {
            Finish::Aluminium => Color32::from_rgb(0x14, 0x15, 0x18),
            Finish::Cream => Color32::from_rgb(0x2a, 0x1e, 0x14),
            Finish::Gold => Color32::from_rgb(0x24, 0x18, 0x08),
            Finish::Oxblood | Finish::Cinnabar => Color32::from_rgb(0xf1, 0xe4, 0xc8),
            Finish::Wyrmhide => Color32::from_rgb(0xec, 0xd6, 0x9a),
            Finish::Jade => Color32::from_rgb(0xf4, 0xf6, 0xf0),
            Finish::Amethyst => Color32::from_rgb(0xe6, 0xdc, 0xff),
            Finish::Slate => Color32::from_rgb(0xd6, 0xf2, 0xec),
            Finish::Copper => Color32::from_rgb(0xf0, 0xc8, 0x98),
            Finish::Black => Color32::from_rgb(0xee, 0xee, 0xea),
            Finish::Basalt | Finish::Forged | Finish::Lapis | Finish::Obsidian => GNOSTIC_GOLD,
        }
    }

    pub fn ink_dim(self) -> Color32 {
        let i = self.ink();
        if self.light() {
            Color32::from_rgba_unmultiplied(i.r(), i.g(), i.b(), 190)
        } else {
            Color32::from_rgba_unmultiplied(i.r(), i.g(), i.b(), 165)
        }
    }

    /// The colour of the unit's lit parts (value arcs, LEDs, selected buttons), where the finish has its
    /// own; otherwise the category colour.
    pub fn accent(self) -> Option<Color32> {
        match self {
            Finish::Oxblood => Some(Color32::from_rgb(0xf2, 0xa6, 0x4a)),
            Finish::Cinnabar => Some(Color32::from_rgb(0xff, 0x8a, 0x52)),
            Finish::Wyrmhide => Some(Color32::from_rgb(0xff, 0xb4, 0x4a)),
            Finish::Jade => Some(Color32::from_rgb(0xff, 0xd8, 0x80)),
            Finish::Amethyst => Some(Color32::from_rgb(0xb4, 0x7c, 0xff)),
            Finish::Slate => Some(Color32::from_rgb(0x52, 0xf0, 0xd8)),
            f if f.gnostic() => Some(GNOSTIC_GOLD),
            _ => None,
        }
    }

    /// The heavy, esoteric units: stone, iron, lapis and obsidian, inlaid with gold.
    pub fn gnostic(self) -> bool {
        matches!(self, Finish::Basalt | Finish::Forged | Finish::Lapis | Finish::Obsidian)
    }

    /// The alchemical motto engraved on a gnostic unit.
    pub fn motto(self) -> Option<&'static str> {
        match self {
            Finish::Basalt => Some("EMETH  ·  TRUTH GIVES THE CLAY ITS LIFE"),
            Finish::Forged => Some("IGNE NATURA RENOVATUR INTEGRA"),
            Finish::Lapis => Some("LUX IN TENEBRIS LUCET"),
            Finish::Obsidian => Some("SOLVE ET COAGULA  ·  V.I.T.R.I.O.L."),
            _ => None,
        }
    }

    pub fn knob(self) -> KnobStyle {
        match self {
            Finish::Aluminium | Finish::Gold | Finish::Copper => KnobStyle::Ring,
            Finish::Black | Finish::Amethyst | Finish::Slate => KnobStyle::Metal,
            Finish::Cream => KnobStyle::Chicken,
            Finish::Oxblood | Finish::Cinnabar => KnobStyle::Cream,
            Finish::Wyrmhide => KnobStyle::Metal,
            Finish::Jade => KnobStyle::Black,
            Finish::Basalt | Finish::Forged | Finish::Lapis | Finish::Obsidian => KnobStyle::Brass,
        }
    }

    /// The generated faceplate photograph, where there is one.
    fn plate(self) -> Option<(&'static str, &'static [u8])> {
        plates::plate(self)
    }
}

/// The gold of the gnostic units' engraving.
pub const GNOSTIC_GOLD: Color32 = Color32::from_rgb(0xe2, 0xc0, 0x76);

/// The finish of a stock effect.
pub fn finish_of(id: &str) -> Finish {
    match id {
        "eq" | "filter" | "utility" | "imager" | "tune" => Finish::Aluminium,
        "compressor" | "limiter" | "gate" | "deesser" | "transient" | "multiband" => Finish::Black,
        "class_a_strip" => Finish::Basalt,
        "drum_bus" => Finish::Forged,
        "clean_voice" => Finish::Lapis,
        "dragon_particle" => Finish::Obsidian,
        "saturator" | "multisat" | "alter" => Finish::Cream,
        "clipper" => Finish::Gold,
        "chorus" | "flanger" | "phaser" | "tremolo" | "orbit" => Finish::Oxblood,
        "reverb" => Finish::Amethyst,
        "delay" | "ghost" => Finish::Slate,
        "cinnabar" => Finish::Cinnabar,
        "wyrmdrum" => Finish::Wyrmhide,
        "jadefang" => Finish::Jade,
        _ => Finish::Copper,
    }
}

/// The panel skin of a finish: groups engraved into the face, printed in its ink.
pub fn skin(f: Finish) -> Skin {
    let section = if f.light() { Color32::from_black_alpha(22) } else { Color32::from_black_alpha(70) };
    Skin { section, knob: f.knob(), ink: f.ink(), ink_dim: f.ink_dim() }
}

/// Valve units get a glowing tube in their nameplate.
pub fn has_valve(id: &str) -> bool {
    matches!(id, "saturator" | "multisat" | "alter" | "class_a_strip" | "drum_bus")
}

/// Units whose left meter shows gain reduction (the right one shows the output).
pub fn meters_reduction(id: &str) -> bool {
    matches!(id, "compressor" | "limiter" | "gate" | "deesser" | "multiband" | "clipper" | "class_a_strip" | "drum_bus" | "transient")
}

// ------------------------------------------------------------------------------------------ textures

mod plates {
    use super::Finish;
    /// Faceplate photographs (Higgsfield), one per finish.
    pub fn plate(f: Finish) -> Option<(&'static str, &'static [u8])> {
        Some(match f {
            Finish::Aluminium => ("fx-plate-aluminium", include_bytes!("../../assets/fx/plate_aluminium.jpg")),
            Finish::Black => ("fx-plate-black", include_bytes!("../../assets/fx/plate_black.jpg")),
            Finish::Cream => ("fx-plate-cream", include_bytes!("../../assets/fx/plate_cream.jpg")),
            Finish::Gold => ("fx-plate-gold", include_bytes!("../../assets/fx/plate_gold.jpg")),
            Finish::Oxblood => ("fx-plate-oxblood", include_bytes!("../../assets/fx/plate_oxblood.jpg")),
            Finish::Amethyst => ("fx-plate-amethyst", include_bytes!("../../assets/fx/plate_amethyst.jpg")),
            Finish::Slate => ("fx-plate-slate", include_bytes!("../../assets/fx/plate_slate.jpg")),
            Finish::Copper => ("fx-plate-copper", include_bytes!("../../assets/fx/plate_copper.jpg")),
            Finish::Basalt => ("fx-plate-basalt", include_bytes!("../../assets/fx/plate_basalt.jpg")),
            Finish::Forged => ("fx-plate-forged", include_bytes!("../../assets/fx/plate_forged.jpg")),
            Finish::Lapis => ("fx-plate-lapis", include_bytes!("../../assets/fx/plate_lapis.jpg")),
            Finish::Obsidian => ("fx-plate-obsidian", include_bytes!("../../assets/fx/plate_obsidian.jpg")),
            Finish::Cinnabar => ("fx-plate-cinnabar", include_bytes!("../../assets/fx/plate_cinnabar.jpg")),
            Finish::Wyrmhide => ("fx-plate-wyrmhide", include_bytes!("../../assets/fx/plate_wyrmhide.jpg")),
            Finish::Jade => ("fx-plate-jade", include_bytes!("../../assets/fx/plate_jade.jpg")),
        })
    }
}

const KNOB_METAL: &[u8] = include_bytes!("../../assets/fx/knob_metal.png");
const KNOB_CHICKEN: &[u8] = include_bytes!("../../assets/fx/knob_chicken.png");
const KNOB_CREAM: &[u8] = include_bytes!("../../assets/fx/knob_cream.png");
const KNOB_GUNMETAL: &[u8] = include_bytes!("../../assets/fx/knob_gunmetal.png");
const KNOB_BLACK: &[u8] = include_bytes!("../../assets/fx/knob_black.png");
const SCREW: &[u8] = include_bytes!("../../assets/fx/screw.png");
const LAMP: &[u8] = include_bytes!("../../assets/fx/lamp.png");
const TUBE: &[u8] = include_bytes!("../../assets/fx/tube.png");
const CORNER: &[u8] = include_bytes!("../../assets/fx/corner.png");
const VU_FACE: &[u8] = include_bytes!("../../assets/fx/vu_face.jpg");

/// A colour that adds light (premultiplied with zero alpha: additive blending).
fn light(c: Color32, k: f32) -> Color32 {
    let k = k.clamp(0.0, 1.0);
    Color32::from_rgba_premultiplied((c.r() as f32 * k) as u8, (c.g() as f32 * k) as u8, (c.b() as f32 * k) as u8, 0)
}

fn tex(ctx: &egui::Context, name: &'static str, bytes: &[u8]) -> Option<TextureHandle> {
    super::guitar_ui::texture(ctx, name, bytes)
}

/// The faceplate photograph of a finish, as a texture.
pub fn plate_texture(ctx: &egui::Context, f: Finish) -> Option<TextureHandle> {
    let (name, bytes) = f.plate()?;
    tex(ctx, name, bytes)
}

/// The photographed cap of a knob style (pointer straight up).
pub fn knob_cap(ctx: &egui::Context, style: KnobStyle) -> Option<TextureHandle> {
    match style {
        KnobStyle::Metal | KnobStyle::Brass => tex(ctx, "fx-knob-metal", KNOB_METAL),
        KnobStyle::Chicken => tex(ctx, "fx-knob-chicken", KNOB_CHICKEN),
        KnobStyle::Cream => tex(ctx, "fx-knob-cream", KNOB_CREAM),
        KnobStyle::Ring => tex(ctx, "fx-knob-gunmetal", KNOB_GUNMETAL),
        KnobStyle::Black => tex(ctx, "fx-knob-black", KNOB_BLACK),
    }
}

/// Draws texture `t` as a square of half-size `r` around `c`, turned by `rot` radians.
pub fn rotated(p: &egui::Painter, t: &TextureHandle, c: Pos2, r: f32, rot: f32, tint: Color32) {
    p.add(rotated_shape(t, c, r, rot, tint));
}

fn rotated_shape(t: &TextureHandle, c: Pos2, r: f32, rot: f32, tint: Color32) -> Shape {
    let mut m = Mesh::with_texture(t.id());
    let (s, co) = rot.sin_cos();
    let corner = |x: f32, y: f32| Pos2::new(c.x + (x * co - y * s) * r, c.y + (x * s + y * co) * r);
    for (pos, uv) in [(corner(-1.0, -1.0), Pos2::new(0.0, 0.0)), (corner(1.0, -1.0), Pos2::new(1.0, 0.0)), (corner(1.0, 1.0), Pos2::new(1.0, 1.0)), (corner(-1.0, 1.0), Pos2::new(0.0, 1.0))] {
        m.vertices.push(egui::epaint::Vertex { pos, uv, color: tint });
    }
    m.indices.extend([0, 1, 2, 0, 2, 3]);
    Shape::mesh(m)
}

// ------------------------------------------------------------------------------------------- the unit

/// Width of a rack ear.
pub const EAR: f32 = 26.0;

fn shade(c: Color32, f: f32) -> Color32 {
    let m = |x: u8| ((x as f32 * f).round().clamp(0.0, 255.0)) as u8;
    Color32::from_rgb(m(c.r()), m(c.g()), m(c.b()))
}

/// Shapes of the faceplate of finish `f` filling `r`: the photographed plate (or the plain finish),
/// its bevel, and rack ears with slotted holes and screws on both sides.
pub fn faceplate(ctx: &egui::Context, r: Rect, f: Finish) -> Vec<Shape> {
    let mut out = Vec::new();
    let body = Rect::from_min_max(Pos2::new(r.left() + EAR, r.top()), Pos2::new(r.right() - EAR, r.bottom()));
    // Ears: a darker bent strip of the same metal on each side.
    let ear_col = shade(f.base(), if f.light() { 0.72 } else { 0.85 });
    for (er, outer) in [(Rect::from_min_max(r.left_top(), Pos2::new(body.left() + 2.0, r.bottom())), true), (Rect::from_min_max(Pos2::new(body.right() - 2.0, r.top()), r.right_bottom()), false)] {
        out.push(Shape::rect_filled(er, egui::CornerRadius::same(3), ear_col));
        if let Some((name, bytes)) = f.plate() {
            if let Some(t) = tex(ctx, name, bytes) {
                out.push(Shape::image(t.id(), er, Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(0.05, 1.0)), Color32::from_gray(185)));
            }
        }
        // The bend line and the slotted mounting holes.
        let x = if outer { er.right() - 3.0 } else { er.left() + 3.0 };
        out.push(Shape::line_segment([Pos2::new(x, er.top() + 2.0), Pos2::new(x, er.bottom() - 2.0)], Stroke::new(1.0_f32, Color32::from_black_alpha(90))));
        let cx = er.center().x + if outer { -1.5 } else { 1.5 };
        let holes = if er.height() > 320.0 { vec![er.top() + 28.0, er.center().y, er.bottom() - 28.0] } else { vec![er.top() + 28.0, er.bottom() - 28.0] };
        for y in holes {
            let slot = Rect::from_center_size(Pos2::new(cx, y), Vec2::new(11.0, 20.0));
            out.push(Shape::rect_filled(slot, egui::CornerRadius::same(6), Color32::from_rgb(0x07, 0x07, 0x08)));
            out.push(Shape::rect_stroke(slot, egui::CornerRadius::same(6), Stroke::new(1.0_f32, Color32::from_white_alpha(28)), egui::StrokeKind::Outside));
        }
    }
    // The plate.
    out.push(Shape::rect_filled(body, egui::CornerRadius::same(2), f.base()));
    if let Some((name, bytes)) = f.plate() {
        if let Some(t) = tex(ctx, name, bytes) {
            // Cover: the photograph keeps its aspect and is cropped to the plate.
            let ts = t.size_vec2();
            let (pa, ta) = (body.width() / body.height(), ts.x / ts.y);
            let uv = if pa > ta {
                let h = ta / pa;
                Rect::from_min_max(Pos2::new(0.0, 0.5 - h / 2.0), Pos2::new(1.0, 0.5 + h / 2.0))
            } else {
                let w = pa / ta;
                Rect::from_min_max(Pos2::new(0.5 - w / 2.0, 0.0), Pos2::new(0.5 + w / 2.0, 1.0))
            };
            out.push(Shape::image(t.id(), body, uv, Color32::WHITE));
            // Calm the grain of the brushed finishes so the printing stays the strongest thing on them.
            let b = f.base();
            out.push(Shape::rect_filled(body, egui::CornerRadius::same(2), Color32::from_rgba_unmultiplied(b.r(), b.g(), b.b(), if f.light() { 80 } else { 60 })));
        }
    } else {
        // Without the photograph: a fine brushed grain.
        let mut y = body.top() + 1.0;
        let mut k = 0u32;
        while y < body.bottom() {
            k = k.wrapping_mul(1103515245).wrapping_add(12345);
            let a = ((k >> 16) % 7) as u8;
            let c = if k & 1 == 0 { Color32::from_white_alpha(a) } else { Color32::from_black_alpha(a) };
            out.push(Shape::line_segment([Pos2::new(body.left(), y), Pos2::new(body.right(), y)], Stroke::new(1.0_f32, c)));
            y += 2.0;
        }
    }
    // The gnostic units: a heavy vignette, a double gold inlay and filigree in the corners.
    if f.gnostic() {
        for k in 0..10 {
            out.push(Shape::rect_stroke(body.shrink(k as f32 * 3.0), egui::CornerRadius::same(2), Stroke::new(3.0_f32, Color32::from_black_alpha(70 - k * 7)), egui::StrokeKind::Inside));
        }
        let gold = |a: u8| Color32::from_rgba_unmultiplied(GNOSTIC_GOLD.r(), GNOSTIC_GOLD.g(), GNOSTIC_GOLD.b(), a);
        out.push(Shape::rect_stroke(body.shrink(7.0), egui::CornerRadius::same(3), Stroke::new(1.5_f32, gold(150)), egui::StrokeKind::Inside));
        out.push(Shape::rect_stroke(body.shrink(11.0), egui::CornerRadius::same(3), Stroke::new(0.8_f32, gold(90)), egui::StrokeKind::Inside));
        if let Some(t) = tex(ctx, "fx-corner", CORNER) {
            let s = 58.0_f32.min(body.height() * 0.2);
            let at = |x: f32, y: f32| Rect::from_min_size(Pos2::new(x, y), Vec2::splat(s));
            let (l, rr, tp, b) = (body.left() + 3.0, body.right() - 3.0 - s, body.top() + 3.0, body.bottom() - 3.0 - s);
            for (rect, u0, v0, u1, v1) in [(at(l, tp), 0.0, 0.0, 1.0, 1.0), (at(rr, tp), 1.0, 0.0, 0.0, 1.0), (at(l, b), 0.0, 1.0, 1.0, 0.0), (at(rr, b), 1.0, 1.0, 0.0, 0.0)] {
                out.push(Shape::image(t.id(), rect, Rect::from_min_max(Pos2::new(u0, v0), Pos2::new(u1, v1)), Color32::WHITE));
            }
        }
    }
    // Bevel: lit top edge, shadowed bottom edge.
    out.push(Shape::line_segment([body.left_top() + Vec2::new(1.0, 0.5), body.right_top() + Vec2::new(-1.0, 0.5)], Stroke::new(1.5_f32, Color32::from_white_alpha(if f.light() { 120 } else { 45 }))));
    out.push(Shape::line_segment([body.left_bottom() + Vec2::new(1.0, -0.5), body.right_bottom() + Vec2::new(-1.0, -0.5)], Stroke::new(2.0_f32, Color32::from_black_alpha(140))));
    out.push(Shape::rect_stroke(body, egui::CornerRadius::same(2), Stroke::new(1.0_f32, Color32::from_black_alpha(160)), egui::StrokeKind::Inside));
    // Screws in the ears.
    for (x, ys) in [(r.left() + EAR / 2.0 - 1.5, [r.top() + 28.0, r.bottom() - 28.0]), (r.right() - EAR / 2.0 + 1.5, [r.top() + 28.0, r.bottom() - 28.0])] {
        for (k, y) in ys.into_iter().enumerate() {
            let c = Pos2::new(x, y);
            let turn = 0.4 + k as f32 * 0.9 + (x * 0.013).fract() * 2.0;
            match tex(ctx, "fx-screw", SCREW) {
                Some(t) => {
                    out.push(Shape::circle_filled(c + Vec2::new(0.8, 1.4), 7.0, Color32::from_black_alpha(110)));
                    out.push(rotated_shape(&t, c, 7.5, turn, Color32::WHITE));
                }
                None => screw_shapes(&mut out, c, 5.0, turn),
            }
        }
    }
    out
}

fn screw_shapes(out: &mut Vec<Shape>, c: Pos2, r: f32, turn: f32) {
    out.push(Shape::circle_filled(c + Vec2::new(0.6, 1.0), r + 0.6, Color32::from_black_alpha(120)));
    out.push(Shape::circle_filled(c, r, Color32::from_gray(160)));
    out.push(Shape::circle_filled(c - Vec2::new(r * 0.25, r * 0.3), r * 0.55, Color32::from_gray(198)));
    out.push(Shape::circle_stroke(c, r, Stroke::new(0.8_f32, Color32::from_gray(70))));
    for k in 0..2 {
        let a = turn + k as f32 * PI / 2.0;
        let d = Vec2::new(a.cos(), a.sin()) * r * 0.62;
        out.push(Shape::line_segment([c - d, c + d], Stroke::new(1.4_f32, Color32::from_gray(50))));
    }
}

/// Text engraved into a plate: the ink with a lit edge below (light plates) or a dark one (dark plates).
pub fn engraved(p: &egui::Painter, at: Pos2, align: egui::Align2, text: &str, font: egui::FontId, f: Finish) {
    let edge = if f.light() { Color32::from_white_alpha(110) } else { Color32::from_black_alpha(170) };
    p.text(at + Vec2::new(0.0, 1.0), align, text, font.clone(), edge);
    p.text(at, align, text, font, f.ink());
}

// --------------------------------------------------------------------------------------- animations

/// A needle (or glow) that moves like a damped spring towards its target; `None` motion = jump.
/// Returns the position and whether it is still moving.
pub fn spring(ctx: &egui::Context, id: egui::Id, target: f32, hz: f32, damping: f32, motion: bool) -> (f32, bool) {
    if !motion {
        return (target, false);
    }
    let dt = ctx.input(|i| i.stable_dt).clamp(0.0, 0.05);
    let (mut x, mut v): (f32, f32) = ctx.data(|d| d.get_temp(id)).unwrap_or((target, 0.0));
    let w = 2.0 * PI * hz;
    // Semi-implicit Euler in small steps (stable for these rates).
    let steps = ((dt / 0.004).ceil() as usize).max(1);
    let h = dt / steps as f32;
    for _ in 0..steps {
        let a = w * w * (target - x) - 2.0 * damping * w * v;
        v += a * h;
        x += v * h;
    }
    ctx.data_mut(|d| d.insert_temp(id, (x, v)));
    (x, (x - target).abs() > 0.002 || v.abs() > 0.01)
}

/// A moving-coil VU meter in `r`: backlit face, scale −20 … +3, red zone, the needle at `pos` (0 = rest
/// at −20, 1 = +3) and the meter's caption. `glow` lights the face (0 = off).
pub fn vu(p: &egui::Painter, r: Rect, pos: f32, caption: &str, reduction: bool, glow: f32, night: bool) {
    // Bezel and glass.
    p.rect_filled(r.expand(3.0), egui::CornerRadius::same(5), Color32::from_rgb(0x0b, 0x0b, 0x0c));
    p.rect_stroke(r.expand(3.0), egui::CornerRadius::same(5), Stroke::new(1.0_f32, Color32::from_white_alpha(40)), egui::StrokeKind::Outside);
    let lit = 0.55 + 0.45 * glow.clamp(0.0, 1.0);
    let face = Color32::from_rgb((0xf2 as f32 * lit) as u8, (0xd9 as f32 * lit) as u8, (0x9c as f32 * lit) as u8);
    p.rect_filled(r, egui::CornerRadius::same(3), face);
    // The backlit paper of the dial (photographed), dimmer while the unit is off; the gnostic units'
    // meters are black glass lit gold from behind.
    if night {
        p.rect_filled(r, egui::CornerRadius::same(3), Color32::from_rgb(0x0e, 0x0b, 0x07));
        super::fx_mod::halo(&p.with_clip_rect(r), Pos2::new(r.center().x, r.bottom()), r.width() * 0.7, GNOSTIC_GOLD, 0.18 * lit);
    } else if let Some(t) = tex(p.ctx(), "fx-vu-face", VU_FACE) {
        let g = (lit * 255.0) as u8;
        p.add(Shape::image(t.id(), r, Rect::from_min_max(Pos2::new(0.04, 0.06), Pos2::new(0.96, 0.94)), Color32::from_rgb(g, g, g)));
    }
    // Warm vignette from the lamps behind the face.
    for k in 0..6 {
        let rr = r.shrink(k as f32 * 2.0);
        p.rect_stroke(rr, egui::CornerRadius::same(3), Stroke::new(2.0_f32, Color32::from_black_alpha(26 - k * 4)), egui::StrokeKind::Inside);
    }
    let ink = if night { GNOSTIC_GOLD } else { Color32::from_rgb(0x22, 0x1a, 0x12) };
    let red = if night { Color32::from_rgb(0xff, 0x6a, 0x3a) } else { Color32::from_rgb(0xb8, 0x22, 0x1c) };
    let pivot = Pos2::new(r.center().x, r.bottom() + r.height() * 0.3);
    let rad = r.height() * 1.22;
    let (a0, a1) = (-PI / 2.0 - 0.75, -PI / 2.0 + 0.75);
    let at = |t: f32, rr: f32| {
        let a = a0 + (a1 - a0) * t;
        Pos2::new(pivot.x + rr * a.cos(), pivot.y + rr * a.sin())
    };
    // Scale: dB marks mapped like a real VU (0 VU at 0.72 of the arc).
    let mark = |db: f32| -> f32 {
        // Power law of the voltage, as the meter's face is printed.
        let v = 10f32.powf(db / 20.0);
        let (lo, hi) = (10f32.powf(-1.0), 10f32.powf(3.0 / 20.0));
        ((v - lo) / (hi - lo)).clamp(0.0, 1.0)
    };
    let arc: Vec<Pos2> = (0..=40).map(|k| at(k as f32 / 40.0 * mark(0.0), rad * 0.78)).collect();
    p.add(Shape::line(arc, Stroke::new(1.2_f32, ink)));
    let red_arc: Vec<Pos2> = (0..=20).map(|k| at(mark(0.0) + (1.0 - mark(0.0)) * k as f32 / 20.0, rad * 0.78)).collect();
    p.add(Shape::line(red_arc, Stroke::new(3.0_f32, red)));
    let font = egui::FontId::proportional((r.height() * 0.13).clamp(7.0, 10.0));
    for (db, label) in [(-20.0, "20"), (-10.0, "10"), (-7.0, "7"), (-5.0, "5"), (-3.0, "3"), (-2.0, ""), (-1.0, ""), (0.0, "0"), (1.0, ""), (2.0, ""), (3.0, "3")] {
        let t = mark(db);
        let col = if db > 0.0 { red } else { ink };
        let long = !label.is_empty();
        p.line_segment([at(t, rad * 0.78), at(t, rad * if long { 0.86 } else { 0.83 })], Stroke::new(1.0_f32, col));
        if long {
            p.text(at(t, rad * 0.93), egui::Align2::CENTER_CENTER, label, font.clone(), col);
        }
    }
    p.text(Pos2::new(r.center().x, r.bottom() - r.height() * 0.3), egui::Align2::CENTER_CENTER, "VU", egui::FontId::proportional((r.height() * 0.2).clamp(9.0, 15.0)), ink);
    p.text(Pos2::new(r.left() + 5.0, r.bottom() - 4.0), egui::Align2::LEFT_BOTTOM, caption, egui::FontId::proportional(7.5), ink.gamma_multiply(0.8));
    if reduction {
        p.text(Pos2::new(r.right() - 5.0, r.bottom() - 4.0), egui::Align2::RIGHT_BOTTOM, "GR", egui::FontId::proportional(7.5), red);
    }
    // The needle and its shadow, clipped to the face.
    let tip = at(pos.clamp(-0.03, 1.04), rad * 0.9);
    let base = at(pos.clamp(-0.03, 1.04), rad * 0.18);
    let clip = p.with_clip_rect(r);
    clip.line_segment([base + Vec2::new(1.5, 2.0), tip + Vec2::new(1.5, 2.0)], Stroke::new(1.4_f32, Color32::from_black_alpha(50)));
    clip.line_segment([base, tip], Stroke::new(1.3_f32, if night { Color32::from_rgb(0xff, 0xe4, 0xa8) } else { Color32::from_rgb(0x14, 0x10, 0x0c) }));
    // The movement's cover at the bottom.
    let cover = Rect::from_min_max(Pos2::new(r.left(), r.bottom() - r.height() * 0.16), r.right_bottom());
    clip.rect_filled(cover, egui::CornerRadius { nw: 0, ne: 0, sw: 3, se: 3 }, Color32::from_rgb(0x1a, 0x17, 0x14));
    // Glass reflection.
    p.rect_filled(Rect::from_min_max(r.left_top(), Pos2::new(r.right(), r.top() + r.height() * 0.3)), egui::CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 }, Color32::from_white_alpha(18));
}

/// Where a level in dBFS sits on the VU scale (0 VU = −18 dBFS), 0 … 1.
pub fn vu_pos(dbfs: f32) -> f32 {
    let db = dbfs + 18.0;
    let v = 10f32.powf(db.clamp(-40.0, 6.0) / 20.0);
    let (lo, hi) = (10f32.powf(-1.0), 10f32.powf(3.0 / 20.0));
    (v - lo) / (hi - lo)
}

/// A gain reduction in dB on the VU scale: the needle falls from 0 VU as the unit compresses.
pub fn gr_pos(db: f32) -> f32 {
    let v = 10f32.powf(-db.max(0.0) / 20.0);
    let (lo, hi) = (10f32.powf(-1.0), 10f32.powf(3.0 / 20.0));
    (v - lo) / (hi - lo)
}

/// A pilot lamp (an amber jewel in a chrome bezel) at `c`: lit with `glow` 0 … 1.
pub fn lamp(p: &egui::Painter, c: Pos2, r: f32, color: Color32, glow: f32) {
    let g = glow.clamp(0.0, 1.0);
    match tex(p.ctx(), "fx-lamp", LAMP) {
        Some(t) => {
            let d = 0.35 + 0.65 * g;
            let tint = Color32::from_rgb((255.0 * d) as u8, (255.0 * d) as u8, (255.0 * d) as u8);
            p.circle_filled(c + Vec2::new(0.8, 1.5), r + 2.5, Color32::from_black_alpha(120));
            p.image(t.id(), Rect::from_center_size(c, Vec2::splat(2.0 * r + 5.0)), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), tint);
            // The filament behind the glass, and its light on the plate around.
            p.circle_filled(c, r * 0.75, light(color, 0.55 * g));
            p.circle_filled(c - Vec2::new(r * 0.15, r * 0.2), r * 0.35, light(Color32::from_rgb(0xff, 0xe0, 0xa0), 0.6 * g));
        }
        None => {
            p.circle_filled(c, r + 3.0, Color32::from_gray(40));
            let dark = shade(color, 0.22);
            let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * g) as u8;
            p.circle_filled(c, r, Color32::from_rgb(mix(dark.r(), color.r()), mix(dark.g(), color.g()), mix(dark.b(), color.b())));
        }
    }
    if g > 0.01 {
        for k in 0..5 {
            p.circle_filled(c, r + 4.0 + k as f32 * 3.0, color.gamma_multiply(0.045 * g));
        }
    }
}

/// A glass valve in `r` whose heater glows with `glow` 0 … 1 (and flickers a little at `time`).
pub fn valve(p: &egui::Painter, r: Rect, glow: f32, time: f32) {
    let flick = 1.0 + 0.04 * (time * 23.0).sin() * (time * 7.3).sin();
    let g = (glow * flick).clamp(0.0, 1.2);
    // Window in the plate.
    p.rect_filled(r.expand(3.0), egui::CornerRadius::same(5), Color32::from_rgb(0x07, 0x06, 0x06));
    p.rect_stroke(r.expand(3.0), egui::CornerRadius::same(5), Stroke::new(1.0_f32, Color32::from_white_alpha(40)), egui::StrokeKind::Outside);
    let orange = Color32::from_rgb(0xff, 0x8a, 0x2c);
    let c = r.center();
    if let Some(t) = tex(p.ctx(), "fx-tube", TUBE) {
        // The photographed tube, its heaters glowing through the plates and the glass catching the light.
        let ts = t.size_vec2();
        let h = r.height() - 2.0;
        let tr = Rect::from_center_size(c, Vec2::new(h * ts.x / ts.y, h));
        let clip = p.with_clip_rect(r.expand(2.0));
        for k in 0..7 {
            clip.circle_filled(Pos2::new(c.x, tr.top() + tr.height() * 0.5), tr.width() * (0.3 + 0.12 * k as f32), light(orange, 0.06 * g));
        }
        clip.image(t.id(), tr, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        for (dx, k) in [(-0.17, 1.0), (0.17, 1.0)] {
            let hc = Pos2::new(c.x + tr.width() * dx, tr.top() + tr.height() * 0.5);
            clip.rect_filled(Rect::from_center_size(hc, Vec2::new(tr.width() * 0.07, tr.height() * 0.28)), egui::CornerRadius::same(1), light(orange, 0.85 * g * k));
            clip.circle_filled(hc, tr.width() * 0.16, light(orange, 0.22 * g));
        }
        return;
    }
    // Glow behind the glass.
    for k in 0..8 {
        p.circle_filled(c + Vec2::new(0.0, r.height() * 0.08), r.width() * (0.18 + 0.06 * k as f32), orange.gamma_multiply(0.035 * g));
    }
    // Glass envelope.
    let env = Rect::from_center_size(c, Vec2::new(r.width() * 0.62, r.height() * 0.9));
    p.rect_filled(env, egui::CornerRadius { nw: 120, ne: 120, sw: 6, se: 6 }, Color32::from_white_alpha(10));
    p.rect_stroke(env, egui::CornerRadius { nw: 120, ne: 120, sw: 6, se: 6 }, Stroke::new(1.0_f32, Color32::from_white_alpha(60)), egui::StrokeKind::Inside);
    // Plate, grid and the heater.
    let plate = Rect::from_center_size(c + Vec2::new(0.0, r.height() * 0.06), Vec2::new(env.width() * 0.62, env.height() * 0.46));
    p.rect_filled(plate, egui::CornerRadius::same(2), Color32::from_gray(52));
    p.rect_stroke(plate, egui::CornerRadius::same(2), Stroke::new(1.0_f32, Color32::from_gray(90)), egui::StrokeKind::Inside);
    let heat = Rect::from_center_size(plate.center(), Vec2::new(plate.width() * 0.22, plate.height() * 0.8));
    p.rect_filled(heat, egui::CornerRadius::same(1), orange.gamma_multiply((0.25 + 0.75 * g.min(1.0)).min(1.0)));
    p.circle_filled(heat.center(), heat.width() * 1.4, Color32::from_rgb(0xff, 0xc0, 0x60).gamma_multiply(0.25 * g.min(1.0)));
    // Getter on top and the pins.
    p.rect_filled(Rect::from_center_size(Pos2::new(c.x, env.top() + env.height() * 0.1), Vec2::new(env.width() * 0.4, 3.0)), egui::CornerRadius::same(1), Color32::from_rgb(0x9a, 0x9e, 0xa6));
    p.line_segment([Pos2::new(env.left() + 3.0, env.top() + 14.0), Pos2::new(env.left() + 3.0, env.bottom() - 8.0)], Stroke::new(1.5_f32, Color32::from_white_alpha(46)));
}

/// What the nameplate shows.
pub struct Plate<'a> {
    pub id: &'a str,
    /// The display name ("Prism (parametric EQ)": name and descriptor).
    pub name: &'a str,
    pub category: &'a str,
    pub enabled: bool,
    /// Gain reduction in dB.
    pub gr: f32,
    /// The channel's output peaks (linear).
    /// The unit's output RMS, left and right (linear).
    pub level: (f32, f32),
    /// Animation time (None = reduced motion: needles jump, nothing flickers).
    pub time: Option<f32>,
    pub key: egui::Id,
}

/// The unit's nameplate row: pilot lamp, the engraved name, the valve window on valve units and two VU
/// meters (gain reduction and output on dynamics units, left and right output otherwise).
pub fn nameplate(ui: &mut egui::Ui, pl: &Plate) {
    let f = finish_of(pl.id);
    let w = ui.available_width();
    let h = 72.0;
    let (r, _) = ui.allocate_exact_size(Vec2::new(w, h), egui::Sense::hover());
    let p = ui.painter_at(r.expand(6.0));
    let ctx = ui.ctx().clone();
    let motion = pl.time.is_some();
    let t = pl.time.unwrap_or(0.0);
    // `level` is the unit's output RMS (300 ms integration, measured after this insert): a VU's reading.
    let db = |x: f32| 20.0 * x.max(1e-6).log10();
    let (l, rr) = pl.level;
    // Lamp: warm amber when the unit is on; it fades in and out like a filament.
    let (glow, m0) = spring(&ctx, pl.key.with("lamp"), if pl.enabled { 1.0 } else { 0.0 }, 1.6, 1.0, motion);
    let lamp_c = Pos2::new(r.left() + 13.0, r.top() + 22.0);
    lamp(&p, lamp_c, 6.0, Color32::from_rgb(0xff, 0x9a, 0x3c), glow);
    engraved(&p, Pos2::new(lamp_c.x, r.top() + 44.0), egui::Align2::CENTER_TOP, "POWER", egui::FontId::proportional(7.0), f);
    // Name and descriptor.
    let (name, desc) = match pl.name.split_once(" (") {
        Some((n, d)) => (n, d.trim_end_matches(')')),
        None => (pl.name, ""),
    };
    let x = r.left() + 34.0;
    if f.gnostic() {
        // Gold that glows a little, breathing slowly, behind the name.
        let breath = 0.5 + 0.5 * (t * 0.8).sin();
        let nw = p.layout_no_wrap(name.to_uppercase(), egui::FontId::proportional(26.0), f.ink()).size().x;
        super::fx_mod::halo(&p, Pos2::new(x + nw / 2.0, r.top() + 22.0), nw * 0.6 + 20.0, GNOSTIC_GOLD, 0.05 + 0.05 * breath * glow);
    }
    let name_size = if f.gnostic() { 26.0 } else { 22.0 };
    engraved(&p, Pos2::new(x, r.top() + if f.gnostic() { 6.0 } else { 10.0 }), egui::Align2::LEFT_TOP, &name.to_uppercase(), egui::FontId::proportional(name_size), f);
    let sub = if desc.is_empty() { pl.category.to_uppercase() } else { format!("{}  ·  {}", desc.to_uppercase(), pl.category.to_uppercase()) };
    p.text(Pos2::new(x + 1.0, r.top() + 40.0), egui::Align2::LEFT_TOP, sub, egui::FontId::proportional(9.5), f.ink_dim());
    match f.motto() {
        Some(m) => {
            p.text(Pos2::new(x + 1.0, r.top() + 54.0), egui::Align2::LEFT_TOP, m, egui::FontId::proportional(8.5), GNOSTIC_GOLD.gamma_multiply(0.8));
        }
        None => {
            p.text(Pos2::new(x + 1.0, r.top() + 55.0), egui::Align2::LEFT_TOP, "DRAGON DAW  ·  ALCHEMY SERIES", egui::FontId::proportional(7.5), f.ink_dim().gamma_multiply(0.75));
        }
    }
    let mut moving = m0;
    // Meters on the right.
    let mw = 116.0;
    let gap = 12.0;
    let room = w - 300.0;
    let n_meters = if room >= 2.0 * mw + gap {
        2
    } else if room >= mw {
        1
    } else {
        0
    };
    let mut right = r.right() - 4.0;
    let red = meters_reduction(pl.id);
    let targets: [(f32, &str, bool); 2] = if red {
        [(if pl.enabled { gr_pos(pl.gr) } else { 0.0 }, "GAIN", true), (if pl.enabled { vu_pos(db(l.max(rr))) } else { 0.0 }, "OUTPUT", false)]
    } else {
        [(if pl.enabled { vu_pos(db(l)) } else { 0.0 }, "LEFT", false), (if pl.enabled { vu_pos(db(rr)) } else { 0.0 }, "RIGHT", false)]
    };
    for k in (0..n_meters).rev() {
        let (target, caption, is_gr) = targets[k + (2 - n_meters)];
        let mr = Rect::from_min_size(Pos2::new(right - mw, r.top() + 6.0), Vec2::new(mw, h - 12.0));
        // A VU movement: about 300 ms to settle, with a little overshoot.
        let (pos, mv) = spring(&ctx, pl.key.with(("vu", k)), target, 1.7, 0.72, motion);
        moving |= mv;
        vu(&p, mr, pos, caption, is_gr, glow, f.gnostic());
        right -= mw + gap;
    }
    // The unit's emblem, alive with what it does.
    if super::fx_mod::has_emblem(pl.id) && right - (h - 8.0) - 4.0 >= r.left() + 250.0 {
        let follow = if super::fx_mod::emblem_follows_reduction(pl.id) { (pl.gr / 12.0).clamp(0.0, 1.0) } else { vu_pos(db(l.max(rr))).clamp(0.0, 1.0) };
        let (sig, mv) = spring(&ctx, pl.key.with("emblem"), if pl.enabled { follow } else { 0.0 }, 1.2, 0.9, motion);
        moving |= mv;
        let side = h - 8.0;
        let er = Rect::from_min_size(Pos2::new(right - side - 4.0, r.top() + 4.0), Vec2::splat(side));
        super::fx_mod::emblem(&p, er, pl.id, sig, t);
        right -= side + 12.0;
    }
    // The valve, glowing with the signal it is driving.
    if has_valve(pl.id) && right - 44.0 >= r.left() + 250.0 {
        let drive = if pl.enabled { 0.55 + 0.45 * (vu_pos(db(l.max(rr))).clamp(0.0, 1.0)) } else { 0.0 };
        let (g, mv) = spring(&ctx, pl.key.with("valve"), drive, 0.6, 1.0, motion);
        moving |= mv;
        let vr = Rect::from_min_size(Pos2::new(right - 40.0, r.top() + 4.0), Vec2::new(40.0, h - 8.0));
        valve(&p, vr, g, t);
    }
    if moving || (motion && pl.enabled) {
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_effect_has_a_finish_and_readable_ink() {
        for def in dragon_engine::effects::registry() {
            let f = finish_of(def.id);
            let lum = |c: Color32| 0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32;
            assert!((lum(f.ink()) - lum(f.base())).abs() > 90.0, "{}: ink not readable on {f:?}", def.id);
        }
    }

    #[test]
    fn vu_scale() {
        assert!((vu_pos(-18.0) - gr_pos(0.0)).abs() < 1e-4);
        assert!(vu_pos(-60.0) <= 0.0 && vu_pos(-15.0) >= 0.99);
        assert!(gr_pos(6.0) < gr_pos(1.0));
    }
}
