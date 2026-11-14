//! The sign-in screen.

use eframe::egui;
use redhive_client_framework::context::AppContext;
use redhive_client_framework::runtime::{poll, Promise};
use redhive_client_framework::security::{self, ConnectionSettings, Session};
use redhive_monads::{Error, Res};

pub struct LoginState {
    pub server: String,
    pub user: String,
    pub password: String,
    pub remember: bool,
    pub busy: Option<Promise<Res<Session>>>,
    pub error: Option<Error>,
}

impl LoginState {
    pub fn new(conn: &ConnectionSettings) -> Self {
        LoginState { server: conn.server_url.clone(), user: conn.remembered_user.clone().unwrap_or_default(), password: String::new(), remember: conn.remember, busy: None, error: None }
    }

    /// Draws the screen; returns the session once signed in.
    pub fn ui(&mut self, ctx: &egui::Context, app: &mut AppContext) -> Option<Session> {
        if let Some(r) = poll(&mut self.busy) {
            match r {
                Ok(s) => {
                    self.password.clear();
                    return Some(s);
                }
                Err(e) => self.error = Some(e),
            }
        }
        let mut result: Option<Option<Session>> = None;
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space((ui.available_height() * 0.18).max(20.0));
                redhive_client_framework::widgets::hexagon(ui, 56.0, egui::Color32::from_rgb(198, 40, 40));
                ui.heading(egui::RichText::new(app.t("shell:login.title")).size(24.0));
                ui.add_space(12.0);
                let width = ui.available_width().min(360.0);
                ui.allocate_ui(egui::vec2(width, 300.0), |ui| {
                    egui::Grid::new("login").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                        ui.label(app.t("shell:login.server"));
                        ui.add(egui::TextEdit::singleline(&mut self.server).desired_width(240.0));
                        ui.end_row();
                        ui.label(app.t("shell:login.user"));
                        let u = ui.add(egui::TextEdit::singleline(&mut self.user).desired_width(240.0));
                        if self.user.is_empty() && self.busy.is_none() && self.error.is_none() {
                            u.request_focus();
                        }
                        ui.end_row();
                        ui.label(app.t("shell:login.password"));
                        let p = ui.add(egui::TextEdit::singleline(&mut self.password).password(true).desired_width(240.0));
                        if !self.user.is_empty() && self.password.is_empty() && self.busy.is_none() && !p.has_focus() && self.error.is_none() {
                            p.request_focus();
                        }
                        let enter = p.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        ui.end_row();
                        ui.label("");
                        ui.checkbox(&mut self.remember, app.t("shell:login.remember"));
                        ui.end_row();
                        ui.label("");
                        let busy = self.busy.is_some();
                        ui.horizontal(|ui| {
                            let label = if busy { app.t("shell:login.busy") } else { app.t("shell:login.submit") };
                            if (ui.add_enabled(!busy, egui::Button::new(label).min_size(egui::vec2(120.0, 28.0))).clicked() || enter) && !busy {
                                self.error = None;
                                app.api.set_base(self.server.trim());
                                let api = app.api.clone();
                                let (u, pw) = (self.user.trim().to_string(), self.password.clone());
                                self.busy = Some(Promise::spawn(async move { security::login(&api, &u, &pw).await }));
                            }
                            if busy {
                                ui.spinner();
                            }
                        });
                        ui.end_row();
                    });
                });
                if let Some(e) = &self.error {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(app.lang.error(e)).color(egui::Color32::from_rgb(229, 57, 53)));
                }
                ui.add_space(16.0);
                ui.label(egui::RichText::new(app.t("shell:login.hint")).small().weak());
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add_space((ui.available_width() - 160.0).max(0.0) / 2.0);
                    for (code, name) in redhive_client_framework::i18n::LANGUAGES.iter().take(2) {
                        if ui.selectable_label(app.lang.language() == *code, *name).clicked() {
                            app.lang.set_language(code);
                            result = Some(None);
                        }
                    }
                });
            });
        });
        let _ = result;
        None
    }
}
