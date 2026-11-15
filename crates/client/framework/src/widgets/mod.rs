//! The UI kit used by the host and by rendered view packs (Golden Hive's `UiKit`, `DataList`, `InputForm`,
//! `SimpleChart`).

use crate::context::AppContext;
use redhive_contracts::ui::Severity;

/// A button with icon, translated label and a rich tooltip (label + shortcut, description, disabled reason).
pub fn command_button(ui: &mut egui::Ui, ctx: &AppContext, icon: &str, label_key: &str, command_id: Option<&str>, enabled: bool) -> egui::Response {
    let label = ctx.icons.label(icon, &ctx.t(label_key));
    let resp = ui.add_enabled(enabled, egui::Button::new(label));
    let mut tip = ctx.t(label_key);
    if let Some(sc) = command_id.and_then(|c| ctx.shortcuts.text(c, ctx.lang.language().starts_with("de"))) {
        tip = format!("{tip}   {sc}");
    }
    let desc_key = format!("{label_key}.description");
    if ctx.lang.has(&desc_key) {
        tip.push('\n');
        tip.push_str(&ctx.t(&desc_key));
    }
    resp.on_hover_text(tip)
}

/// Colour of a status value.
pub fn status_color(value: &str, dark: bool) -> egui::Color32 {
    let v = value.to_ascii_lowercase();
    let (r, g, b) = match v.as_str() {
        "approved" | "paid" | "active" | "completed" | "success" | "true" | "complete" | "autoMigrated" | "automigrated" | "compatible" | "proposed" | "confirmed" => (46, 125, 50),
        "submitted" | "waitingforapproval" | "waitingforinput" | "pilot" | "broad" | "running" | "info" | "draft" | "lowconfidence" => (21, 101, 192),
        "rejected" | "failed" | "broken" | "error" | "cancelled" | "false" | "blocked" | "invalid" => (198, 40, 40),
        "archived" | "retired" | "paused" | "held" | "needsreview" | "reviewrequired" | "warning" | "retrying" => (230, 126, 34),
        _ => (120, 120, 120),
    };
    if dark {
        egui::Color32::from_rgb(r, g, b)
    } else {
        egui::Color32::from_rgb((r as f32 * 0.85) as u8, (g as f32 * 0.85) as u8, (b as f32 * 0.85) as u8)
    }
}

pub fn badge(ui: &mut egui::Ui, text: &str, raw_value: &str) -> egui::Response {
    let color = status_color(raw_value, ui.visuals().dark_mode);
    let galley = ui.painter().layout_no_wrap(text.to_string(), egui::FontId::proportional(12.0), egui::Color32::WHITE);
    let size = galley.size() + egui::vec2(12.0, 4.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect_filled(rect, 8.0, color);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, egui::Color32::WHITE);
    resp
}

pub fn severity_color(s: Severity) -> egui::Color32 {
    match s {
        Severity::Info => egui::Color32::from_rgb(21, 101, 192),
        Severity::Success => egui::Color32::from_rgb(46, 125, 50),
        Severity::Warning => egui::Color32::from_rgb(230, 126, 34),
        Severity::Error => egui::Color32::from_rgb(198, 40, 40),
    }
}

pub fn severity_icon(s: Severity) -> &'static str {
    match s {
        Severity::Info => "ℹ",
        Severity::Success => "✔",
        Severity::Warning => "⚠",
        Severity::Error => "⛔",
    }
}

/// A card with a title (used by dashboards and the start page).
pub fn card<R>(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::group(ui.style())
        .corner_radius(6.0)
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            if !title.is_empty() {
                ui.label(egui::RichText::new(title).strong());
                ui.add_space(4.0);
            }
            add(ui)
        })
        .inner
}

/// An error box with the translated text.
pub fn error_box(ui: &mut egui::Ui, ctx: &AppContext, e: &redhive_monads::Error) {
    egui::Frame::new()
        .fill(egui::Color32::from_rgba_unmultiplied(198, 40, 40, 40))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(format!("⛔ {}", ctx.lang.error(e))).color(severity_color(Severity::Error)));
        })
        .response
        .on_hover_text(format!("{} ({:?})", e.code, e.kind));
}

/// A busy ring with a text.
pub fn busy(ui: &mut egui::Ui, ctx: &AppContext) {
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label(ctx.t("shell:common.loading"));
    });
}

/// The Red Hive logo: a filled hexagon (painted, so it does not depend on a font glyph).
pub fn hexagon(ui: &mut egui::Ui, size: f32, color: egui::Color32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let c = rect.center();
    let r = size * 0.5;
    let points: Vec<egui::Pos2> = (0..6)
        .map(|i| {
            let a = std::f32::consts::FRAC_PI_3 * i as f32 + std::f32::consts::FRAC_PI_6;
            egui::pos2(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect();
    ui.painter().add(egui::Shape::convex_polygon(points, color, egui::Stroke::NONE));
    response
}

/// Logo followed by the product name.
pub fn brand(ui: &mut egui::Ui, size: f32) {
    ui.horizontal(|ui| {
        hexagon(ui, size, egui::Color32::from_rgb(229, 57, 53));
        ui.label(egui::RichText::new("Red Hive").size(size).strong().color(egui::Color32::from_rgb(229, 57, 53)));
    });
}
