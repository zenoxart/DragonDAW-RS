//! User settings of the shell (roaming `Settings.json`).

use redhive_client_framework::context::Paths;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Theme {
    #[default]
    Dark,
    Light,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellSettings {
    pub language: String,
    pub theme: Theme,
    pub scale: f32,
    pub os_toasts: bool,
    pub reduced_motion: bool,
}

impl Default for ShellSettings {
    fn default() -> Self {
        let lang = std::env::var("LANG").ok().filter(|l| l.starts_with("de")).map(|_| "de".to_string()).unwrap_or_else(|| "en".into());
        ShellSettings { language: lang, theme: Theme::Dark, scale: 1.0, os_toasts: true, reduced_motion: false }
    }
}

impl ShellSettings {
    pub fn load(paths: &Paths) -> Self {
        paths.load_json("Settings.json").unwrap_or_default()
    }

    pub fn save(&self, paths: &Paths) {
        paths.save_json("Settings.json", self);
    }

    /// Applies theme, accent colour and zoom.
    pub fn apply(&self, ctx: &eframe::egui::Context) {
        use eframe::egui;
        let dark = match self.theme {
            Theme::Dark => true,
            Theme::Light => false,
            Theme::System => ctx.system_theme().map(|t| t == egui::Theme::Dark).unwrap_or(true),
        };
        let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        let accent = egui::Color32::from_rgb(198, 40, 40);
        v.selection.bg_fill = accent;
        v.selection.stroke = egui::Stroke::new(1.0_f32, egui::Color32::WHITE);
        v.hyperlink_color = if dark { egui::Color32::from_rgb(239, 154, 154) } else { egui::Color32::from_rgb(183, 28, 28) };
        v.widgets.active.bg_fill = accent;
        v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, accent.gamma_multiply(0.8));
        v.window_corner_radius = egui::CornerRadius::same(8);
        ctx.set_visuals(v);
        ctx.set_zoom_factor(self.scale.clamp(0.6, 2.5));
        ctx.style_mut(|s| {
            s.animation_time = if self.reduced_motion { 0.0 } else { 0.083 };
            s.spacing.item_spacing = egui::vec2(8.0, 6.0);
            s.spacing.button_padding = egui::vec2(8.0, 4.0);
        });
    }
}
