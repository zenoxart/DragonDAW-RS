//! The Dragon DAW window: menu bar, transport bar, dock area with the editors, status bar and overlays.

use crate::state::State;
use crate::{commands, splash::Splash, theme, views};
use dragon_engine::device::Command as Cmd;
use redhive_client_framework::docking::{DockManager, Tab};
use redhive_contracts::commands::{placements, CommandTreeNode, ShortcutScope};
use redhive_contracts::ui::{DockSide, Severity, ViewRole};

pub struct DragonApp {
    pub st: State,
    dock: DockManager,
    splash: Option<Splash>,
    styled: bool,
    quit_confirmed: bool,
    confirm_quit: bool,
    last_compile: std::time::Instant,
}

/// View key ➡ (title, role, side).
pub fn view_meta(key: &str) -> Option<(&'static str, ViewRole, DockSide)> {
    Some(match key {
        "dragon.playlist" => ("Playlist", ViewRole::Document, DockSide::Left),
        "dragon.rack" => ("Channel rack", ViewRole::Document, DockSide::Left),
        "dragon.piano" => ("Piano roll", ViewRole::Document, DockSide::Left),
        "dragon.mixer" => ("Mixer", ViewRole::Document, DockSide::Left),
        "dragon.master" => ("Mastering", ViewRole::Document, DockSide::Left),
        "dragon.launcher" => ("Performance", ViewRole::Document, DockSide::Left),
        "dragon.video" => ("Video", ViewRole::ToolWindow, DockSide::Right),
        "dragon.automation" => ("Automation", ViewRole::ToolWindow, DockSide::Bottom),
        "dragon.browser" => ("Browser", ViewRole::ToolWindow, DockSide::Left),
        "dragon.project" => ("Project", ViewRole::ToolWindow, DockSide::Right),
        "dragon.history" => ("History", ViewRole::ToolWindow, DockSide::Right),
        "dragon.plugins" => ("Plugins", ViewRole::Document, DockSide::Left),
        "dragon.options" => ("Options", ViewRole::Document, DockSide::Left),
        _ => return None,
    })
}

fn tool_side(key: &str) -> Option<DockSide> {
    match view_meta(key) {
        Some((_, ViewRole::ToolWindow, side)) => Some(side),
        Some(_) => None,
        None => None,
    }
}

impl DragonApp {
    pub fn new(cc: &eframe::CreationContext, mut st: State) -> Self {
        redhive_client_framework::runtime::set_repaint_context(&cc.egui_ctx);
        commands::register(&mut st);
        let dock = DockManager::load(&st.ctx.paths);
        let splash = st.settings.show_splash.then(|| Splash::new(&cc.egui_ctx, st.settings.reduced_motion));
        cc.egui_ctx.set_zoom_factor(st.settings.ui_scale.clamp(0.6, 2.5));
        DragonApp { st, dock, splash, styled: false, quit_confirmed: false, confirm_quit: false, last_compile: std::time::Instant::now() }
    }

    fn run_command(&mut self, id: &str, ctx: &egui::Context) {
        match id {
            "dragon.internal.reset_layout" => self.dock.reset(),
            _ => {
                if !commands::execute(&mut self.st, id, ctx) {
                    self.st.ctx.log.warn("commands", format!("unknown command {id}"));
                }
            }
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        let tree = self.st.ctx.commands.tree(placements::MAIN_MENU, &self.st.ctx.permissions, |id| self.st.ctx.shortcuts.text(id, false));
        let mut clicked = None;
        egui::menu::bar(ui, |ui| {
            ui.add_space(4.0);
            crate::splash::logo(ui, 18.0);
            ui.add_space(4.0);
            for node in &tree {
                menu_node(ui, node, &mut clicked);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(self.st.session.title()).color(theme::TEXT_DIM));
            });
        });
        // Recent files under the menu bar's File menu are reached through the palette and the start toast;
        // this keeps the generated menu pure.
        if let Some(id) = clicked {
            self.st.ctx.run(&id, serde_json::Value::Null);
        }
    }

    fn handle_input(&mut self, ctx: &egui::Context) {
        let text_focus = ctx.wants_keyboard_input();
        let typing = self.st.ui.typing_keyboard && !text_focus;
        let cmds = self.st.ctx.shortcuts.process(ctx, text_focus || typing, &[ShortcutScope::Global]);
        for c in cmds {
            self.st.ctx.run(&c, serde_json::Value::Null);
        }
        if typing {
            views::keyboard::typing_keyboard(&mut self.st, ctx);
        }
        // Space and a few others must work while typing keyboard is active too.
        if typing && ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.st.toggle_play();
        }
        // Dropped files.
        let dropped: Vec<std::path::PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        let at = ctx.input(|i| i.pointer.latest_pos());
        // The rack records where its rows were drawn; only use them if it was drawn in the last couple of frames.
        let now = ctx.cumulative_pass_nr();
        let fresh = |f: u64| now.saturating_sub(f) <= 2;
        let rack_rows: Vec<(dragon_engine::model::Id, egui::Rect)> =
            ctx.data(|d| d.get_temp::<(u64, Vec<(dragon_engine::model::Id, egui::Rect)>)>(egui::Id::new("rack.rows"))).filter(|(f, _)| fresh(*f)).map(|(_, r)| r).unwrap_or_default();
        let rack_zone: Option<egui::Rect> = ctx.data(|d| d.get_temp::<(u64, egui::Rect)>(egui::Id::new("rack.dropzone"))).filter(|(f, _)| fresh(*f)).map(|(_, r)| r);
        for f in dropped {
            let ext = f.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
            // Audio dropped on a channel-rack row replaces that channel's sound; on the rack's drop zone it
            // adds a sampler.
            if dragon_engine::edit::is_audio_file(&f) {
                if let Some(pos) = at {
                    if let Some((cid, _)) = rack_rows.iter().find(|(_, r)| r.contains(pos)) {
                        self.st.replace_sample(*cid, &f);
                        continue;
                    }
                    if rack_zone.is_some_and(|r| r.contains(pos)) {
                        self.st.add_sampler(&f);
                        continue;
                    }
                }
            }
            match ext.as_str() {
                "dragon" => self.st.open_path(&f),
                "mid" | "midi" => self.st.import_midi(&f),
                _ => {
                    let beat = self.st.beat_now();
                    self.st.add_audio_clip(&f, None, beat);
                }
            }
        }
    }

    fn pump(&mut self, ctx: &egui::Context) {
        while let Some((id, _)) = self.st.ctx.pending_commands.pop_front() {
            self.run_command(&id, ctx);
        }
        while let Some((key, param)) = self.st.ctx.open_requests.pop_front() {
            if let Some((_, role, side)) = view_meta(&key) {
                self.dock.open(&key, param, role, side, &tool_side);
            }
        }
        self.st.handle_midi();
        self.st.mcu_feedback();
        self.st.poll_stem_job();
        // Recompile at most every 40 ms while dragging; immediately otherwise.
        if !self.st.session.in_gesture() || self.last_compile.elapsed().as_millis() >= 40 {
            self.last_compile = std::time::Instant::now();
            if let Some(song) = self.st.session.compile_if_changed(self.st.audio.rate() as f32) {
                self.st.audio.send(Cmd::SetSong(song));
            }
        }
        // Finish background plugin scans.
        views::plugins::poll_scan(&mut self.st);
        if self.st.pump_plugin_editors() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        self.st.autosave_tick();
        self.st.audio.meters().decay(0.86);
    }
}

fn menu_node(ui: &mut egui::Ui, node: &CommandTreeNode, clicked: &mut Option<String>) {
    match node {
        CommandTreeNode::Group { label, children, .. } => {
            ui.menu_button(&label.0, |ui| {
                ui.set_min_width(240.0);
                for c in children {
                    menu_node(ui, c, clicked);
                }
            });
        }
        CommandTreeNode::Command { command_id, label, shortcut, .. } => {
            let b = egui::Button::new(&label.0).shortcut_text(shortcut.clone().unwrap_or_default());
            if ui.add(b).clicked() {
                *clicked = Some(command_id.clone());
                ui.close_menu();
            }
        }
        CommandTreeNode::Separator { .. } => {
            ui.separator();
        }
    }
}

struct Viewer<'a> {
    st: &'a mut State,
}

impl egui_dock::TabViewer for Viewer<'_> {
    type Tab = Tab;

    fn title(&mut self, tab: &mut Tab) -> egui::WidgetText {
        view_meta(&tab.key).map(|m| m.0).unwrap_or("(unavailable)").into()
    }

    fn id(&mut self, tab: &mut Tab) -> egui::Id {
        egui::Id::new(&tab.key)
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Tab) {
        let st = &mut *self.st;
        match tab.key.as_str() {
            "dragon.playlist" => views::playlist::show(ui, st),
            "dragon.rack" => views::rack::show(ui, st),
            "dragon.piano" => views::piano_roll::show(ui, st),
            "dragon.mixer" => views::mixer::show(ui, st),
            "dragon.master" => views::master::show(ui, st),
            "dragon.launcher" => views::launcher::show(ui, st),
            "dragon.video" => views::video::show(ui, st),
            "dragon.automation" => views::automation::show(ui, st),
            "dragon.browser" => views::browser::show(ui, st),
            "dragon.project" => views::project::show(ui, st),
            "dragon.history" => views::project::history(ui, st),
            "dragon.plugins" => views::plugins::show(ui, st),
            "dragon.options" => views::options::show(ui, st),
            other => {
                ui.label(format!("The view '{other}' is not part of Dragon DAW. Close this tab or reset the layout (View › Reset layout)."));
            }
        }
    }

    fn closeable(&mut self, _tab: &mut Tab) -> bool {
        true
    }

    fn scroll_bars(&self, _tab: &Tab) -> [bool; 2] {
        [false, false]
    }
}

impl eframe::App for DragonApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.styled {
            theme::apply(ctx);
            self.styled = true;
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.quit_confirmed && self.st.session.history.is_dirty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_quit = true;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("{} – Dragon DAW", self.st.session.title())));

        self.handle_input(ctx);
        self.pump(ctx);

        egui::TopBottomPanel::top("dragon.menu").show(ctx, |ui| self.menu_bar(ui));
        egui::TopBottomPanel::top("dragon.transport").exact_height(46.0).show(ctx, |ui| views::transport::show(ui, &mut self.st));
        egui::TopBottomPanel::bottom("dragon.status").exact_height(22.0).show(ctx, |ui| views::transport::status_bar(ui, &mut self.st));
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(theme::BG)).show(ctx, |ui| {
            let mut style = egui_dock::Style::from_egui(ui.style());
            style.tab_bar.bg_fill = theme::PANEL;
            style.tab.active.bg_fill = theme::PANEL_2;
            style.tab.focused.bg_fill = theme::PANEL_2;
            egui_dock::DockArea::new(&mut self.dock.state).style(style).show_inside(ui, &mut Viewer { st: &mut self.st });
        });

        views::windows::show(ctx, &mut self.st);
        views::quests::show(ctx, &mut self.st);
        views::fx_gallery::show(ctx, &mut self.st);
        views::note_tools::show(ctx, &mut self.st);
        views::overlays::palette(ctx, &mut self.st);
        views::overlays::toasts(ctx, &mut self.st);
        views::overlays::recovery(ctx, &mut self.st);
        views::overlays::about(ctx, &mut self.st);
        if let Some(mut d) = self.st.ui.export.take() {
            if d.show(ctx, &mut self.st) {
                self.st.ui.export = Some(d);
            }
        }
        if self.confirm_quit {
            egui::Modal::new(egui::Id::new("dragon.quit")).show(ctx, |ui| {
                ui.heading("Save changes?");
                ui.label(format!("{} has unsaved changes.", self.st.session.title()));
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() && self.st.save(false) {
                        self.quit_confirmed = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if ui.button("Don't save").clicked() {
                        self.quit_confirmed = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if ui.button("Cancel").clicked() {
                        self.confirm_quit = false;
                    }
                });
            });
        }
        if let Some(s) = &mut self.splash {
            if !s.show(ctx) {
                self.splash = None;
            }
        }

        let playing = self.st.transport().playing || self.splash.is_some();
        if playing {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if self.st.ctx.toasts.iter().any(|t| t.severity == Severity::Error) {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.dock.save();
        self.st.save_settings();
        self.st.ctx.shortcuts.save();
        self.st.autosave.end_session();
    }
}
