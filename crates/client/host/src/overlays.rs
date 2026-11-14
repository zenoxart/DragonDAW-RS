//! Overlays of the shell: command palette, shortcut cheat sheet, toasts, "save layout as" and about.

use crate::shell::Shell;
use eframe::egui;
use redhive_client_framework::context::AppContext;
use redhive_client_framework::widgets;
use redhive_contracts::commands::CommandTreeNode;
use serde_json::Value;

#[derive(Default)]
pub struct Overlays {
    pub palette: bool,
    pub query: String,
    pub selected: usize,
    pub focus: bool,
    pub cheat_sheet: bool,
    pub about: bool,
    pub save_layout: Option<String>,
}

impl Overlays {
    pub fn open_palette(&mut self) {
        self.palette = true;
        self.query.clear();
        self.selected = 0;
        self.focus = true;
    }
}

/// Renders a node of a command tree as menu entries.
pub fn menu_node(ui: &mut egui::Ui, app: &mut AppContext, node: &CommandTreeNode) {
    match node {
        CommandTreeNode::Group { label, icon, children, .. } => {
            ui.menu_button(app.icons.label(&icon.0, &app.t(&label.0)), |ui| {
                for c in children {
                    menu_node(ui, app, c);
                }
            });
        }
        CommandTreeNode::Command { command_id, label, icon, shortcut, .. } => {
            let mut b = egui::Button::new(app.icons.label(&icon.0, &app.t(&label.0)));
            if let Some(s) = shortcut {
                b = b.shortcut_text(s.clone());
            }
            if ui.add(b).clicked() {
                app.run(command_id, Value::Null);
                ui.close_menu();
            }
        }
        CommandTreeNode::Separator { .. } => {
            ui.separator();
        }
    }
}

pub fn draw(shell: &mut Shell, ctx: &egui::Context) {
    palette(shell, ctx);
    cheat_sheet(shell, ctx);
    if shell.overlays.about {
        let mut open = true;
        egui::Window::new("Red Hive").open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            widgets::brand(ui, 26.0);
            ui.label(shell.app.t("shell:about.text"));
            ui.label(format!("Host {} · Platform contracts {}", redhive_client_framework::HOST_VERSION, redhive_contracts::modules::CONTRACTS_VERSION));
            ui.label(egui::RichText::new(shell.app.api.base()).small().weak());
        });
        shell.overlays.about = open;
    }
    if let Some(mut name) = shell.overlays.save_layout.take() {
        let mut keep = true;
        egui::Window::new(shell.app.t("shell:cmd.saveLayout")).collapsible(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            let r = ui.text_edit_singleline(&mut name);
            r.request_focus();
            ui.horizontal(|ui| {
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (ui.button(shell.app.t("shell:common.save")).clicked() || enter) && !name.trim().is_empty() {
                    shell.dock.save_named(name.trim());
                    keep = false;
                }
                if ui.button(shell.app.t("shell:common.cancel")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    keep = false;
                }
            });
            let names: Vec<String> = shell.dock.named.keys().cloned().collect();
            if !names.is_empty() {
                ui.separator();
                for n in names {
                    if ui.link(&n).clicked() {
                        shell.dock.apply_named(&n);
                        keep = false;
                    }
                }
            }
        });
        if keep {
            shell.overlays.save_layout = Some(name);
        }
    }
}

fn palette(shell: &mut Shell, ctx: &egui::Context) {
    if !shell.overlays.palette {
        return;
    }
    let perms = shell.app.session.as_ref().map(|s| s.profile.permissions.clone()).unwrap_or_default();
    let german = shell.app.lang.language().starts_with("de");
    let app = &shell.app;
    let hits = app.commands.search(&shell.overlays.query, &perms, |k| app.t(k));
    let mut run: Option<String> = None;
    let mut close = false;
    let (up, down, enter, esc) = ctx.input(|i| (i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::ArrowDown), i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
    let ov = &mut shell.overlays;
    if down && ov.selected + 1 < hits.len() {
        ov.selected += 1;
    }
    if up && ov.selected > 0 {
        ov.selected -= 1;
    }
    if esc {
        close = true;
    }
    if enter {
        if let Some(h) = hits.get(ov.selected) {
            run = Some(h.command_id.clone());
        }
    }
    let width = (ctx.screen_rect().width() * 0.5).clamp(360.0, 720.0);
    egui::Area::new(egui::Id::new("palette")).anchor(egui::Align2::CENTER_TOP, [0.0, 60.0]).order(egui::Order::Foreground).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_width(width);
            let r = ui.add(egui::TextEdit::singleline(&mut ov.query).hint_text(app.t("shell:palette.placeholder")).desired_width(f32::INFINITY));
            if ov.focus {
                r.request_focus();
                ov.focus = false;
            }
            if r.changed() {
                ov.selected = 0;
            }
            ui.separator();
            if hits.is_empty() {
                ui.label(egui::RichText::new(app.t("shell:palette.noResults")).weak());
            }
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for (i, h) in hits.iter().take(40).enumerate() {
                    let sel = i == ov.selected;
                    let r = ui.horizontal(|ui| {
                        let r = ui.selectable_label(sel, app.icons.label(&h.icon.0, &h.label));
                        ui.label(egui::RichText::new(&h.path).small().weak());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if let Some(s) = app.shortcuts.text(&h.command_id, german) {
                                ui.label(egui::RichText::new(s).small().monospace());
                            }
                        });
                        r
                    });
                    if sel && (up || down) {
                        r.inner.scroll_to_me(None);
                    }
                    if r.inner.clicked() {
                        run = Some(h.command_id.clone());
                    }
                }
            });
        });
    });
    if let Some(id) = run {
        shell.app.run(&id, Value::Null);
        close = true;
    }
    if close {
        shell.overlays.palette = false;
    }
}

fn cheat_sheet(shell: &mut Shell, ctx: &egui::Context) {
    if !shell.overlays.cheat_sheet {
        return;
    }
    let german = shell.app.lang.language().starts_with("de");
    let perms = shell.app.session.as_ref().map(|s| s.profile.permissions.clone()).unwrap_or_default();
    let mut rows: Vec<(String, String)> = shell
        .app
        .commands
        .all()
        .filter(|c| shell.app.commands.can_execute(&c.descriptor.id, &perms))
        .filter_map(|c| shell.app.shortcuts.text(&c.descriptor.id, german).map(|s| (shell.app.t(&c.descriptor.label.0), s)))
        .collect();
    rows.sort();
    rows.push(("Navigation wheel".into(), "Ctrl+Tab".into()));
    let mut open = true;
    egui::Window::new(shell.app.t("shell:shortcuts.cheatSheet")).open(&mut open).default_width(520.0).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
        egui::ScrollArea::vertical().max_height(480.0).show(ui, |ui| {
            egui::Grid::new("cheat").striped(true).num_columns(2).show(ui, |ui| {
                for (l, s) in rows {
                    ui.label(l);
                    ui.label(egui::RichText::new(s).monospace());
                    ui.end_row();
                }
            });
        });
    });
    if !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        shell.overlays.cheat_sheet = false;
    }
}

pub fn toasts(shell: &mut Shell, ctx: &egui::Context) {
    if shell.app.toasts.is_empty() {
        return;
    }
    let mut navigate = None;
    let mut dismiss = None;
    egui::Area::new(egui::Id::new("toasts")).anchor(egui::Align2::RIGHT_BOTTOM, [-12.0, -36.0]).order(egui::Order::Tooltip).show(ctx, |ui| {
        for (i, t) in shell.app.toasts.iter().enumerate().rev().take(5) {
            egui::Frame::popup(ui.style()).stroke(egui::Stroke::new(1.5_f32, widgets::severity_color(t.severity))).show(ui, |ui| {
                ui.set_max_width(380.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(widgets::severity_icon(t.severity)).color(widgets::severity_color(t.severity)));
                    let r = if t.navigate.is_some() { ui.link(&t.text) } else { ui.label(&t.text) };
                    if r.clicked() {
                        navigate = t.navigate.clone();
                        dismiss = Some(i);
                    }
                    if ui.small_button("✖").clicked() {
                        dismiss = Some(i);
                    }
                });
            });
            ui.add_space(4.0);
        }
    });
    if let Some(i) = dismiss {
        shell.app.toasts.remove(i);
    }
    if let Some((t, p)) = navigate {
        shell.app.open(&t, p);
    }
}
