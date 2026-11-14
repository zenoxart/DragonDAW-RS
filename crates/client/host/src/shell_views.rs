//! Views provided by the shell itself: start page, navigation tree, notification center, output log,
//! module explorer (with rollouts and publishing), settings, shortcut editor and profile.

use crate::settings::Theme;
use crate::shell::Shell;
use base64::Engine;
use eframe::egui;
use redhive_client_framework::context::LogLevel;
use redhive_client_framework::runtime::{poll, Promise};
use redhive_client_framework::widgets;
use redhive_contracts::api::{AvatarDto, ChangePasswordRequest};
use redhive_contracts::modules::{AdvanceRolloutRequest, ModuleManifest, PublishRequest, PublishResult, RolloutInfo, RolloutStage};
use redhive_contracts::notifications::Severity;
use redhive_monads::{Error, Res};
use serde_json::{json, Value};

#[derive(Default)]
pub struct ShellViews {
    pub explorer_dirty: bool,
    catalog: Option<Vec<Value>>,
    rollouts: Option<Vec<RolloutInfo>>,
    catalog_load: Option<Promise<Res<(Vec<Value>, Vec<RolloutInfo>)>>>,
    action: Option<Promise<Res<String>>>,
    publish_text: String,
    publish_immediate: bool,
    output_filter: String,
    output_errors_only: bool,
    shortcut_filter: String,
    profile: ProfileState,
    explorer_error: Option<Error>,
}

#[derive(Default)]
struct ProfileState {
    current: String,
    new: String,
    repeat: String,
    picture_path: String,
    busy: Option<Promise<Res<String>>>,
    error: Option<Error>,
    avatar: Option<egui::TextureHandle>,
    avatar_load: Option<Promise<Res<Vec<u8>>>>,
    avatar_requested: bool,
}

fn args(pairs: &[(&str, Value)]) -> serde_json::Map<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

impl Shell {
    pub fn start_page(&mut self, ui: &mut egui::Ui) {
        let perms = self.app.session.as_ref().map(|s| s.profile.permissions.clone()).unwrap_or_default();
        let name = self.app.session.as_ref().map(|s| s.profile.display_name.clone()).unwrap_or_default();
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading(self.app.lang.t_args("shell:start.welcome", &args(&[("name", json!(name))])));
            ui.label(egui::RichText::new(self.app.t("shell:start.subtitle")).weak());
            ui.add_space(12.0);
            ui.label(egui::RichText::new(self.app.t("shell:start.modules")).strong());
            let homes: Vec<(String, String, String)> = self.app.navigation.homes(&perms).iter().map(|v| (v.key.clone(), self.app.t(&v.title), v.icon.clone())).collect();
            ui.horizontal_wrapped(|ui| {
                for (key, title, icon) in homes {
                    let glyph = self.app.icons.glyph(&icon);
                    let b = egui::Button::new(egui::RichText::new(format!("{glyph}\n{title}")).size(15.0)).min_size(egui::vec2(150.0, 70.0));
                    if ui.add(b).clicked() {
                        self.app.open(&key, None);
                    }
                }
            });
            ui.add_space(12.0);
            ui.columns(2, |cols| {
                widgets::card(&mut cols[0], &self.app.t("shell:start.tasks"), |ui| {
                    if self.workflows.inbox.items.is_empty() {
                        ui.label(egui::RichText::new(self.app.t("shell:start.noTasks")).weak());
                    }
                    for item in self.workflows.inbox.items.iter().take(8) {
                        let text = format!("{} · {}", item.workflow_name, item.task.title);
                        if ui.link(text).clicked() {
                            self.app.open("workflows.inbox", Some(item.instance_id.to_string()));
                        }
                    }
                });
                widgets::card(&mut cols[1], &self.app.t("shell:start.notifications"), |ui| {
                    if self.center.items.is_empty() {
                        ui.label(egui::RichText::new(self.app.t("shell:notifications.empty")).weak());
                    }
                    for item in self.center.items.iter().take(8) {
                        let e = &item.envelope;
                        let text = self.app.lang.t_args(&e.title, &e.text_args);
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(widgets::severity_icon(e.severity)).color(widgets::severity_color(e.severity)));
                            if ui.link(text).clicked() {
                                if let Some(t) = &e.navigation_target {
                                    self.app.open(t, e.navigation_parameter.clone());
                                }
                            }
                        });
                    }
                });
            });
        });
    }

    pub fn navigation_tree(&mut self, ui: &mut egui::Ui) {
        let perms = self.app.session.as_ref().map(|s| s.profile.permissions.clone()).unwrap_or_default();
        egui::ScrollArea::vertical().show(ui, |ui| {
            let mut owners: Vec<(String, String)> = self.modules.values().map(|m| (m.manifest.module_id.clone(), self.app.t(&m.manifest.display_name))).collect();
            owners.insert(0, ("shell".into(), self.app.t("shell:app.title")));
            for (owner, title) in owners {
                let views: Vec<(String, String, String)> = self.app.navigation.views_of(&owner, &perms).iter().map(|v| (v.key.clone(), self.app.t(&v.title), v.icon.clone())).collect();
                if views.is_empty() {
                    continue;
                }
                egui::CollapsingHeader::new(egui::RichText::new(title).strong()).id_salt(("nav", &owner)).default_open(owner != "shell").show(ui, |ui| {
                    for (key, t, icon) in views {
                        let open = self.dock.is_open(&key);
                        if ui.selectable_label(open, self.app.icons.label(&icon, &t)).clicked() {
                            self.app.open(&key, None);
                        }
                    }
                });
            }
        });
    }

    pub fn notifications_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button(self.app.t("shell:notifications.markRead")).clicked() {
                self.center.mark_all_read();
            }
            if ui.button(self.app.t("shell:notifications.clear")).clicked() {
                self.center.clear();
            }
        });
        ui.separator();
        if self.center.items.is_empty() {
            ui.label(egui::RichText::new(self.app.t("shell:notifications.empty")).weak());
            return;
        }
        let mut open = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for item in &self.center.items {
                let e = &item.envelope;
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(widgets::severity_icon(e.severity)).color(widgets::severity_color(e.severity)));
                    let mut text = egui::RichText::new(self.app.lang.t_args(&e.title, &e.text_args));
                    if !item.read {
                        text = text.strong();
                    }
                    let r = if e.navigation_target.is_some() { ui.link(text) } else { ui.label(text) };
                    if r.clicked() {
                        open = e.navigation_target.clone().map(|t| (t, e.navigation_parameter.clone()));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(e.created_at.with_timezone(&chrono::Local).format("%d.%m. %H:%M").to_string()).small().weak());
                        ui.label(egui::RichText::new(&e.event_type).small().weak());
                    });
                });
            }
        });
        if let Some((t, p)) = open {
            self.center.mark_all_read();
            self.app.open(&t, p);
        }
    }

    pub fn output_view(&mut self, ui: &mut egui::Ui) {
        let sv = &mut self.shell_views;
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut sv.output_filter).hint_text(self.app.t("shell:common.search")).desired_width(220.0));
            ui.checkbox(&mut sv.output_errors_only, "⚠");
            if ui.button(self.app.t("shell:common.copy")).clicked() {
                let text: String = self.app.log.entries.iter().map(|e| format!("{} {:?} [{}] {}\n", e.at.format("%H:%M:%S"), e.level, e.source, e.message)).collect();
                ui.ctx().copy_text(text);
            }
        });
        ui.separator();
        let filter = sv.output_filter.to_lowercase();
        let errors_only = sv.output_errors_only;
        egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
            for e in self.app.log.entries.iter() {
                if errors_only && !matches!(e.level, LogLevel::Warning | LogLevel::Error) {
                    continue;
                }
                if !filter.is_empty() && !e.message.to_lowercase().contains(&filter) && !e.source.to_lowercase().contains(&filter) {
                    continue;
                }
                let color = match e.level {
                    LogLevel::Error => egui::Color32::from_rgb(229, 57, 53),
                    LogLevel::Warning => egui::Color32::from_rgb(251, 140, 0),
                    _ => ui.visuals().text_color(),
                };
                ui.label(egui::RichText::new(format!("{}  [{}]  {}", e.at.format("%H:%M:%S"), e.source, e.message)).monospace().color(color));
            }
        });
    }

    fn load_explorer(&mut self) {
        let api = self.app.api.clone();
        let rollouts_allowed = self.app.has("updates.manage") || self.app.has("modules.manage");
        self.shell_views.catalog_load = Some(Promise::spawn(async move {
            let catalog: Vec<Value> = api.get("/api/modules").await?;
            let rollouts: Vec<RolloutInfo> = if rollouts_allowed { api.get("/api/updates/rollouts").await.unwrap_or_default() } else { vec![] };
            Ok((catalog, rollouts))
        }));
        self.shell_views.explorer_dirty = false;
    }

    pub fn module_explorer(&mut self, ui: &mut egui::Ui) {
        if (self.shell_views.catalog.is_none() && self.shell_views.catalog_load.is_none()) || self.shell_views.explorer_dirty {
            self.load_explorer();
        }
        if let Some(r) = poll(&mut self.shell_views.catalog_load) {
            match r {
                Ok((c, r)) => {
                    self.shell_views.catalog = Some(c);
                    self.shell_views.rollouts = Some(r);
                    self.shell_views.explorer_error = None;
                }
                Err(e) => self.shell_views.explorer_error = Some(e),
            }
        }
        if let Some(r) = poll(&mut self.shell_views.action) {
            match r {
                Ok(text) => {
                    self.app.toast(text, Severity::Success);
                    self.load_explorer();
                }
                Err(e) => self.app.toast_error(&e),
            }
        }
        ui.horizontal(|ui| {
            ui.heading(self.app.t("shell:modules.explorer"));
            if ui.button(self.app.icons.label("refresh", &self.app.t("shell:common.refresh"))).clicked() {
                self.load_explorer();
            }
            if ui.button(self.app.icons.label("refresh", &self.app.t("shell:cmd.reloadModules"))).clicked() {
                self.app.run("shell.reloadModules", Value::Null);
            }
            if self.shell_views.catalog_load.is_some() || self.shell_views.action.is_some() {
                ui.spinner();
            }
        });
        if let Some(e) = &self.shell_views.explorer_error {
            widgets::error_box(ui, &self.app, e);
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            // loaded in this client
            widgets::card(ui, &self.app.t("shell:modules.loaded"), |ui| {
                egui::Grid::new("loaded-modules").striped(true).num_columns(4).show(ui, |ui| {
                    for m in self.modules.values() {
                        ui.label(egui::RichText::new(self.app.icons.label(&m.manifest.icon, &self.app.t(&m.manifest.display_name))).strong());
                        ui.label(format!("{} {}", self.app.t("shell:modules.version"), m.manifest.version));
                        if m.development {
                            ui.label(egui::RichText::new(self.app.t("shell:modules.development")).color(egui::Color32::from_rgb(251, 140, 0)));
                        } else if m.signature_verified {
                            ui.label(egui::RichText::new(format!("✔ {}", self.app.t("shell:modules.verified"))).color(egui::Color32::from_rgb(67, 160, 71)));
                        } else {
                            ui.label("–");
                        }
                        match &m.newer_version {
                            Some(v) => ui.label(egui::RichText::new(self.app.lang.t_args("shell:modules.newer", &args(&[("version", json!(v))]))).small().weak()),
                            None => ui.label(""),
                        };
                        ui.end_row();
                    }
                    for (id, e) in &self.rejected {
                        ui.label(egui::RichText::new(id).strong().color(egui::Color32::from_rgb(229, 57, 53)));
                        ui.label(self.app.lang.error(e));
                        ui.label("");
                        ui.label("");
                        ui.end_row();
                    }
                });
            });
            // catalog
            if let Some(catalog) = self.shell_views.catalog.clone() {
                widgets::card(ui, &self.app.t("shell:menu.modules"), |ui| {
                    egui::Grid::new("catalog").striped(true).num_columns(4).show(ui, |ui| {
                        for entry in &catalog {
                            let id = entry.get("moduleId").and_then(Value::as_str).unwrap_or_default().to_string();
                            let version = entry.get("version").or_else(|| entry.pointer("/manifest/version")).and_then(Value::as_str).unwrap_or_default();
                            let enabled = entry.get("enabled").and_then(Value::as_bool).unwrap_or(true);
                            let name = entry.get("displayName").or_else(|| entry.pointer("/manifest/displayName")).and_then(Value::as_str).map(|k| self.app.t(k)).unwrap_or(id.clone());
                            ui.label(egui::RichText::new(&name).strong());
                            ui.label(egui::RichText::new(&id).small().weak());
                            ui.label(version);
                            if self.app.has("modules.manage") {
                                let mut on = enabled;
                                if ui.checkbox(&mut on, "").changed() {
                                    let api = self.app.api.clone();
                                    let text = format!("{id}: {}", if on { "✔" } else { "✖" });
                                    self.shell_views.action = Some(Promise::spawn(async move {
                                        api.put::<Value, _>(&format!("/api/modules/{id}/enabled"), &json!({ "enabled": on })).await.map(|_| text)
                                    }));
                                }
                            } else {
                                ui.label(if enabled { "✔" } else { "✖" });
                            }
                            ui.end_row();
                        }
                    });
                });
            }
            // rollouts
            if let Some(rollouts) = self.shell_views.rollouts.clone().filter(|r| !r.is_empty()) {
                widgets::card(ui, &self.app.t("shell:modules.rollouts"), |ui| {
                    for r in rollouts {
                        ui.horizontal(|ui| {
                            let color = match r.stage {
                                RolloutStage::Held => egui::Color32::from_rgb(229, 57, 53),
                                RolloutStage::Complete => egui::Color32::from_rgb(67, 160, 71),
                                RolloutStage::Cancelled => ui.visuals().weak_text_color(),
                                _ => egui::Color32::from_rgb(251, 140, 0),
                            };
                            ui.label(egui::RichText::new(format!("{:?}", r.stage)).color(color).strong());
                            ui.label(format!("{} {} ⏵ {}", r.module_id, r.from_version.clone().unwrap_or_else(|| "–".into()), r.to_version));
                            if r.broken_workflows > 0 {
                                let t = self.app.lang.t_args("shell:updates.held", &args(&[("module", json!(r.module_id)), ("count", json!(r.broken_workflows))]));
                                if ui.link(egui::RichText::new(t).small()).clicked() {
                                    self.app.open("workflows.impact", r.impact_report_id.map(|i| i.to_string()));
                                }
                            }
                            if let Some(h) = &r.hold_reason {
                                ui.label(egui::RichText::new(h).small().weak());
                            }
                            let active = matches!(r.stage, RolloutStage::Pilot | RolloutStage::Broad | RolloutStage::Held);
                            if active && self.app.has("updates.manage") {
                                let mut act = |label: String, path: &'static str, body: Value| {
                                    if ui.button(label).clicked() {
                                        let api = self.app.api.clone();
                                        let id = r.id;
                                        let done = format!("{} {}", r.module_id, r.to_version);
                                        self.shell_views.action = Some(Promise::spawn(async move { api.post::<Value, _>(&format!("/api/updates/rollouts/{id}/{path}"), &body).await.map(|_| done) }));
                                    }
                                };
                                act(self.app.t("shell:modules.advance"), "advance", json!(AdvanceRolloutRequest { force: false }));
                                if r.stage == RolloutStage::Held {
                                    act(self.app.t("shell:modules.force"), "advance", json!(AdvanceRolloutRequest { force: true }));
                                } else {
                                    act(self.app.t("shell:modules.hold"), "hold", json!({ "reason": "held by administrator" }));
                                }
                                act(self.app.t("shell:modules.cancel"), "cancel", json!({}));
                            }
                        });
                    }
                });
            }
            // publish
            if self.app.has("modules.manage") {
                widgets::card(ui, &self.app.t("shell:modules.publish"), |ui| {
                    ui.label(egui::RichText::new(self.app.t("shell:modules.publishHint")).weak());
                    ui.add(egui::TextEdit::multiline(&mut self.shell_views.publish_text).code_editor().desired_rows(8).desired_width(f32::INFINITY));
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.shell_views.publish_immediate, "immediate");
                        if ui.button(self.app.icons.label("upload", &self.app.t("shell:modules.publish"))).clicked() {
                            match serde_json::from_str::<ModuleManifest>(&self.shell_views.publish_text) {
                                Ok(manifest) => {
                                    let api = self.app.api.clone();
                                    let req = PublishRequest { manifest, channel: "stable".into(), immediate: self.shell_views.publish_immediate };
                                    let template = self.app.t("shell:modules.published");
                                    self.shell_views.action = Some(Promise::spawn(async move {
                                        let r: PublishResult = api.post("/api/modules", &req).await?;
                                        Ok(template.replace("{module}", &r.module_id).replace("{version}", &r.version))
                                    }));
                                }
                                Err(e) => self.app.toast_error(&Error::validation("manifest.invalid", "errors:validation").with_metadata("detail", e.to_string())),
                            }
                        }
                    });
                });
            }
        });
    }

    pub fn settings_view(&mut self, ui: &mut egui::Ui) {
        ui.heading(self.app.t("shell:settings.title"));
        ui.add_space(8.0);
        let before = format!("{:?}", (self.settings.theme, self.settings.scale, &self.settings.language, self.settings.os_toasts, self.settings.reduced_motion));
        egui::Grid::new("settings").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
            ui.label(self.app.t("shell:settings.language"));
            egui::ComboBox::from_id_salt("lang").selected_text(self.settings.language.clone()).show_ui(ui, |ui| {
                for (code, name) in redhive_client_framework::i18n::LANGUAGES {
                    ui.selectable_value(&mut self.settings.language, code.to_string(), *name);
                }
            });
            ui.end_row();
            ui.label(self.app.t("shell:settings.theme"));
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.settings.theme, Theme::Dark, self.app.t("shell:settings.themeDark"));
                ui.selectable_value(&mut self.settings.theme, Theme::Light, self.app.t("shell:settings.themeLight"));
                ui.selectable_value(&mut self.settings.theme, Theme::System, self.app.t("shell:settings.themeSystem"));
            });
            ui.end_row();
            ui.label(self.app.t("shell:settings.scale"));
            ui.add(egui::Slider::new(&mut self.settings.scale, 0.75..=2.0).step_by(0.05));
            ui.end_row();
            ui.label(self.app.t("shell:settings.osToasts"));
            ui.checkbox(&mut self.settings.os_toasts, "");
            ui.end_row();
            ui.label(self.app.t("shell:settings.reducedMotion"));
            ui.checkbox(&mut self.settings.reduced_motion, "");
            ui.end_row();
        });
        let after = format!("{:?}", (self.settings.theme, self.settings.scale, &self.settings.language, self.settings.os_toasts, self.settings.reduced_motion));
        if before != after {
            self.settings.save(&self.app.paths);
        }
        ui.add_space(12.0);
        ui.label(egui::RichText::new(format!("{} · {}", self.app.t(self.windows_size_key(ui)), self.app.api.base())).small().weak());
    }

    fn windows_size_key(&self, ui: &egui::Ui) -> &'static str {
        redhive_client_framework::windowing::SizeClass::of(ui.ctx().screen_rect().width()).text_key()
    }

    pub fn shortcuts_view(&mut self, ui: &mut egui::Ui) {
        let german = self.app.lang.language().starts_with("de");
        ui.horizontal(|ui| {
            ui.heading(self.app.t("shell:shortcuts.title"));
            ui.add(egui::TextEdit::singleline(&mut self.shell_views.shortcut_filter).hint_text(self.app.t("shell:common.search")).desired_width(200.0));
            ui.label(self.app.t("shell:shortcuts.preset"));
            let mut preset = self.app.shortcuts.profile.preset.clone();
            egui::ComboBox::from_id_salt("preset").selected_text(preset.clone()).show_ui(ui, |ui| {
                for p in ["default", "visualStudio", "emacs"] {
                    ui.selectable_value(&mut preset, p.to_string(), p);
                }
            });
            if preset != self.app.shortcuts.profile.preset {
                self.app.shortcuts.set_preset(&preset);
            }
            if ui.button(self.app.t("shell:shortcuts.resetAll")).clicked() {
                self.app.shortcuts.reset(None);
            }
            if ui.button(self.app.t("shell:shortcuts.export")).clicked() {
                ui.ctx().copy_text(serde_json::to_string_pretty(&self.app.shortcuts.profile).unwrap_or_default());
                let t = self.app.t("shell:common.copied");
                self.app.toast(t, Severity::Info);
            }
        });
        // a recorded gesture is applied here
        if let (Some(cmd), Some(g)) = (self.app.shortcuts.recording.clone(), self.app.shortcuts.recorded.take()) {
            self.app.shortcuts.recording = None;
            if redhive_client_framework::shortcuts::ShortcutManager::is_reserved(&g) {
                let t = self.app.t("shell:shortcuts.reserved");
                self.app.toast(t, Severity::Warning);
            } else {
                self.app.shortcuts.set_override(&cmd, Some(&g));
                self.app.shortcuts.rebuild(&self.app.commands);
            }
        }
        for c in &self.app.shortcuts.conflicts {
            let names: Vec<String> = c.commands.iter().map(|id| self.app.commands.get(id).map(|r| self.app.t(&r.descriptor.label.0)).unwrap_or(id.clone())).collect();
            ui.label(egui::RichText::new(format!("⚠ {}: {}", c.gesture, names.join(", "))).color(egui::Color32::from_rgb(251, 140, 0)));
        }
        ui.separator();
        let filter = self.shell_views.shortcut_filter.to_lowercase();
        let mut cmds: Vec<(String, String)> = self.app.commands.all().map(|c| (c.descriptor.id.clone(), self.app.t(&c.descriptor.label.0))).filter(|(id, l)| filter.is_empty() || l.to_lowercase().contains(&filter) || id.contains(&filter)).collect();
        cmds.sort_by(|a, b| a.1.cmp(&b.1));
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("shortcuts").striped(true).num_columns(4).show(ui, |ui| {
                ui.label(egui::RichText::new(self.app.t("shell:shortcuts.command")).strong());
                ui.label(egui::RichText::new(self.app.t("shell:shortcuts.gesture")).strong());
                ui.label("");
                ui.label("");
                ui.end_row();
                for (id, label) in cmds {
                    ui.label(&label).on_hover_text(&id);
                    let recording = self.app.shortcuts.recording.as_deref() == Some(id.as_str());
                    if recording {
                        ui.label(egui::RichText::new(self.app.t("shell:shortcuts.recording")).italics());
                    } else {
                        ui.label(egui::RichText::new(self.app.shortcuts.text(&id, german).unwrap_or_default()).monospace());
                    }
                    if ui.button(self.app.t("shell:shortcuts.record")).clicked() {
                        self.app.shortcuts.recording = Some(id.clone());
                        self.app.shortcuts.recorded = None;
                    }
                    if ui.button(self.app.t("shell:shortcuts.reset")).clicked() {
                        self.app.shortcuts.reset(Some(&id));
                    }
                    ui.end_row();
                }
            });
        });
    }

    pub fn profile_view(&mut self, ui: &mut egui::Ui) {
        let Some(profile) = self.app.session.as_ref().map(|s| s.profile.clone()) else { return };
        let ps = &mut self.shell_views.profile;
        if let Some(r) = poll(&mut ps.busy) {
            match r {
                Ok(t) => {
                    ps.error = None;
                    ps.current.clear();
                    ps.new.clear();
                    ps.repeat.clear();
                    ps.avatar_requested = false;
                    ps.avatar = None;
                    self.app.toast(t, Severity::Success);
                }
                Err(e) => ps.error = Some(e),
            }
        }
        if !ps.avatar_requested && profile.has_avatar {
            ps.avatar_requested = true;
            let api = self.app.api.clone();
            let id = profile.id;
            ps.avatar_load = Some(Promise::spawn(async move {
                let a: AvatarDto = api.get(&format!("/api/identity/users/{id}/avatar")).await?;
                base64::engine::general_purpose::STANDARD.decode(a.data_base64).map_err(|_| Error::unexpected("avatar.decode", "errors:unexpected"))
            }));
        }
        if let Some(Ok(bytes)) = poll(&mut ps.avatar_load) {
            if let Ok(img) = image::load_from_memory(&bytes) {
                let rgba = img.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let ci = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                ps.avatar = Some(ui.ctx().load_texture("avatar", ci, Default::default()));
            }
        }
        ui.heading(self.app.t("shell:profile.title"));
        ui.horizontal(|ui| {
            match &ps.avatar {
                Some(t) => {
                    ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(72.0, 72.0)).corner_radius(36.0));
                }
                None => {
                    let initials: String = profile.display_name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect();
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(72.0, 72.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 36.0, egui::Color32::from_rgb(198, 40, 40));
                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, initials, egui::FontId::proportional(26.0), egui::Color32::WHITE);
                }
            }
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(&profile.display_name).size(20.0).strong());
                ui.label(format!("{} · {} · {}", profile.user_name, profile.email, profile.tenant_name));
                ui.label(format!("{}: {}", self.app.t("shell:profile.roles"), profile.roles.join(", ")));
                ui.label(format!("{}: {}", self.app.t("shell:profile.limit"), widgets::format::number(profile.approval_limit, 2, self.app.lang.language().starts_with("de"))));
                if let Some(l) = profile.last_login_at {
                    ui.label(egui::RichText::new(format!("{}: {}", self.app.t("shell:profile.lastLogin"), l.with_timezone(&chrono::Local).format("%d.%m.%Y %H:%M"))).small().weak());
                }
            });
        });
        ui.add_space(10.0);
        if let Some(e) = &ps.error {
            widgets::error_box(ui, &self.app, e);
        }
        let busy = ps.busy.is_some();
        widgets::card(ui, &self.app.t("shell:profile.picture"), |ui| {
            ui.horizontal(|ui| {
                #[cfg(windows)]
                {
                    if ui.button(self.app.t("shell:profile.choosePicture")).clicked() {
                        if let Some(p) = rfd::FileDialog::new().add_filter("Image", &["png", "jpg", "jpeg"]).pick_file() {
                            ps.picture_path = p.display().to_string();
                        }
                    }
                }
                ui.add(egui::TextEdit::singleline(&mut ps.picture_path).hint_text(self.app.t("shell:profile.picturePath")).desired_width(320.0));
                if ui.add_enabled(!busy && !ps.picture_path.trim().is_empty(), egui::Button::new(self.app.t("shell:common.save"))).clicked() {
                    match std::fs::read(ps.picture_path.trim()) {
                        Ok(bytes) => {
                            let content_type = if ps.picture_path.to_lowercase().ends_with(".png") { "image/png" } else { "image/jpeg" };
                            let dto = AvatarDto { content_type: content_type.into(), data_base64: base64::engine::general_purpose::STANDARD.encode(bytes) };
                            let api = self.app.api.clone();
                            let done = self.app.t("shell:profile.pictureSaved");
                            ps.busy = Some(Promise::spawn(async move { api.put::<Value, _>("/api/identity/me/avatar", &dto).await.map(|_| done) }));
                        }
                        Err(e) => ps.error = Some(Error::validation("avatar.file", "errors:validation").with_metadata("detail", e.to_string())),
                    }
                }
                if profile.has_avatar && ui.add_enabled(!busy, egui::Button::new(self.app.t("shell:profile.removePicture"))).clicked() {
                    let api = self.app.api.clone();
                    let done = self.app.t("shell:profile.pictureSaved");
                    ps.busy = Some(Promise::spawn(async move { api.delete::<Value>("/api/identity/me/avatar").await.map(|_| done) }));
                }
            });
        });
        widgets::card(ui, &self.app.t("shell:profile.password"), |ui| {
            egui::Grid::new("pw").num_columns(2).show(ui, |ui| {
                ui.label(self.app.t("shell:profile.current"));
                ui.add(egui::TextEdit::singleline(&mut ps.current).password(true));
                ui.end_row();
                ui.label(self.app.t("shell:profile.new"));
                ui.add(egui::TextEdit::singleline(&mut ps.new).password(true));
                ui.end_row();
                ui.label(self.app.t("shell:profile.repeat"));
                ui.add(egui::TextEdit::singleline(&mut ps.repeat).password(true));
                ui.end_row();
            });
            if ui.add_enabled(!busy && !ps.new.is_empty(), egui::Button::new(self.app.t("shell:common.save"))).clicked() {
                if ps.new != ps.repeat {
                    ps.error = Some(Error::validation("password.mismatch", "shell:profile.mismatch"));
                } else {
                    let api = self.app.api.clone();
                    let req = ChangePasswordRequest { current_password: ps.current.clone(), new_password: ps.new.clone() };
                    let done = self.app.t("shell:profile.changed");
                    ps.busy = Some(Promise::spawn(async move { api.put::<Value, _>("/api/identity/me/password", &req).await.map(|_| done) }));
                }
            }
        });
    }
}
