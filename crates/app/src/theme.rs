//! The Dragon look: charcoal panels, ember-red accent, high-contrast meters.

use dragon_engine::model::Rgb;
use egui::{Color32, Stroke, Visuals};

pub const BG: Color32 = Color32::from_rgb(0x16, 0x18, 0x1c);
pub const PANEL: Color32 = Color32::from_rgb(0x1e, 0x21, 0x26);
pub const PANEL_2: Color32 = Color32::from_rgb(0x26, 0x2a, 0x30);
pub const GRID: Color32 = Color32::from_rgb(0x2e, 0x33, 0x3a);
pub const GRID_BAR: Color32 = Color32::from_rgb(0x44, 0x4b, 0x55);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x8a, 0x93, 0x9e);
pub const ACCENT: Color32 = Color32::from_rgb(0xe8, 0x4a, 0x2c);
pub const ACCENT_2: Color32 = Color32::from_rgb(0xff, 0x9a, 0x3c);
pub const PLAYHEAD: Color32 = Color32::from_rgb(0x5c, 0xe1, 0xe6);
pub const REC: Color32 = Color32::from_rgb(0xff, 0x33, 0x44);
pub const OK: Color32 = Color32::from_rgb(0x55, 0xd6, 0x7a);
pub const WARN: Color32 = Color32::from_rgb(0xff, 0xc8, 0x3c);

pub fn apply(ctx: &egui::Context) {
    let mut v = Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.extreme_bg_color = BG;
    v.faint_bg_color = PANEL_2;
    v.selection.bg_fill = ACCENT.linear_multiply(0.55);
    v.selection.stroke = Stroke::new(1.0_f32, ACCENT_2);
    v.hyperlink_color = ACCENT_2;
    v.widgets.active.bg_fill = ACCENT;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT_2);
    v.window_corner_radius = 6.into();
    ctx.set_visuals(v);
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(6.0, 4.0);
    style.spacing.button_padding = egui::vec2(6.0, 2.0);
    ctx.set_style(style);
}

pub fn rgb(c: Rgb) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}

/// Meter colour for a linear peak value.
pub fn meter_color(v: f32) -> Color32 {
    if v >= 1.0 {
        REC
    } else if v > 0.7 {
        WARN
    } else {
        OK
    }
}
