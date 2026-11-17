//! Command palette, toasts, crash recovery and the about box.

use crate::state::State;
use crate::theme;
use egui::RichText;
use redhive_client_framework::widgets::{severity_color, severity_icon};
use redhive_contracts::ui::Severity;

pub fn palette(ctx: &egui::Context, st: &mut State) {
    let Some(mut q) = st.ui.palette.take() else { return };
    let mut keep = true;
    let mut run = None;
    egui::Modal::new(egui::Id::new("dragon.palette")).show(ctx, |ui| {
        ui.set_width(520.0);
        let r = ui.add(egui::TextEdit::singleline(&mut q).hint_text("Type a command…").desired_width(f32::INFINITY));
        r.request_focus();
        let hits = st.ctx.commands.search(&q, &st.ctx.permissions, |k| k.to_string());
        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
        egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
            for (i, h) in hits.iter().enumerate().take(30) {
                let sc = st.ctx.shortcuts.text(&h.command_id, false).unwrap_or_default();
                let text = format!("{}   {}", h.label, if h.path.is_empty() { String::new() } else { format!("({})", h.path) });
                let resp = ui.add(egui::Button::new(text).shortcut_text(sc).fill(if i == 0 { theme::PANEL_2 } else { egui::Color32::TRANSPARENT }).min_size(egui::vec2(500.0, 22.0)));
                if resp.clicked() || (i == 0 && enter) {
                    run = Some(h.command_id.clone());
                }
            }
        });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            keep = false;
        }
    });
    if let Some(c) = run {
        st.ctx.run(&c, serde_json::Value::Null);
        keep = false;
    }
    if keep {
        st.ui.palette = Some(q);
    }
}

pub fn toasts(ctx: &egui::Context, st: &mut State) {
    let now = std::time::Instant::now();
    st.ctx.toasts.retain(|t| t.until > now);
    if st.ctx.toasts.is_empty() {
        return;
    }
    egui::Area::new(egui::Id::new("dragon.toasts")).anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -30.0)).order(egui::Order::Foreground).show(ctx, |ui| {
        let mut dismiss = None;
        for (i, t) in st.ctx.toasts.iter().enumerate().rev().take(5) {
            egui::Frame::popup(ui.style()).stroke(egui::Stroke::new(1.0_f32, severity_color(t.severity))).show(ui, |ui| {
                ui.set_max_width(380.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(severity_icon(t.severity)).color(severity_color(t.severity)));
                    ui.label(&t.text);
                    if ui.small_button("🗙").clicked() {
                        dismiss = Some(i);
                    }
                });
            });
        }
        if let Some(i) = dismiss {
            st.ctx.toasts.remove(i);
        }
    });
    ctx.request_repaint_after(std::time::Duration::from_millis(250));
}

pub fn recovery(ctx: &egui::Context, st: &mut State) {
    let Some(list) = st.ui.recovery.clone() else { return };
    let mut close = false;
    egui::Modal::new(egui::Id::new("dragon.recovery")).show(ctx, |ui| {
        ui.heading(if st.crashed_last_time { "Dragon DAW did not close properly" } else { "Autosaves" });
        ui.label("Open an autosaved version of your work:");
        if list.is_empty() {
            ui.label(RichText::new("No autosaves.").color(theme::TEXT_DIM));
        }
        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
            for (p, t) in list.iter().take(20) {
                let age = t.elapsed().map(|d| d.as_secs()).unwrap_or(0);
                let ago = if age < 3600 {
                    format!("{} min ago", age / 60)
                } else if age < 86400 {
                    format!("{} h ago", age / 3600)
                } else {
                    format!("{} days ago", age / 86400)
                };
                if ui.button(format!("{}  ·  {ago}", p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default())).clicked() {
                    match st.autosave.load(p) {
                        Ok(proj) => {
                            st.replace_session(crate::session::Session::new(proj, None));
                            st.session.history.touch();
                            st.toast("Recovered – save it with Save as", Severity::Success);
                        }
                        Err(e) => st.toast(format!("Cannot load autosave: {e}"), Severity::Error),
                    }
                    close = true;
                }
            }
        });
        if ui.button("Close").clicked() {
            close = true;
        }
    });
    if close {
        st.ui.recovery = None;
        st.crashed_last_time = false;
    }
}

pub fn about(ctx: &egui::Context, st: &mut State) {
    if !st.ui.about {
        return;
    }
    let mut open = true;
    egui::Window::new("About Dragon DAW").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        if let Some(tex) = crate::brand::logo(ctx) {
            ui.add(egui::Image::new(&tex).fit_to_exact_size(egui::vec2(220.0, 220.0)).corner_radius(10.0));
        }
        ui.heading(format!("Dragon DAW {}", env!("CARGO_PKG_VERSION")));
        ui.label("Record, make beats, mix and master – fully offline.");
        ui.label("Built on the Red Hive desktop framework (egui).");
        ui.label(RichText::new("Copyright © ZenoxArt · Apache License 2.0").small());
        ui.label(RichText::new("Logo and startup sound from the original Dragon DAW.").small().color(theme::TEXT_DIM));
        ctx.request_repaint();
    });
    st.ui.about = open;
}
