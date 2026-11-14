//! The shell: the composition root (`ClientAppContext` in Golden Hive). It owns the app context and every
//! framework service, registers the shell commands and views, runs the startup sequence (sign-in → modules →
//! notifications), routes events, executes commands and draws the main window.

use crate::login::LoginState;
use crate::overlays::Overlays;
use crate::settings::ShellSettings;
use crate::shell_views::ShellViews;
use eframe::egui;
use egui_dock::{DockArea, DockState, Style, TabViewer};
use redhive_client_framework::commands::{CommandHandler, RegisteredCommand};
use redhive_client_framework::context::AppContext;
use redhive_client_framework::docking::{DockManager, Tab};
use redhive_client_framework::hotswap::{HotSwapManager, SwapOutcome};
use redhive_client_framework::modules::{self, LoadResult, LoadedModule};
use redhive_client_framework::navigation::{NavigationWheel, ViewEntry, ViewSource, WheelItem};
use redhive_client_framework::notifications::{system_toast, ConnectionState, NotificationCenter, NotificationHub};
use redhive_client_framework::runtime::{self, poll, Promise};
use redhive_client_framework::security::{ConnectionSettings, Session, TokenStore};
use redhive_client_framework::views::{self, ViewState};
use redhive_client_framework::windowing::{WindowManager, WindowPlacement};
use redhive_client_framework::workflows::WorkflowsUi;
use redhive_contracts::commands::{placements, CommandDescriptor, CommandPlacement, KeyGesture, ShortcutBinding, ShortcutScope};
use redhive_contracts::modules::VersionAvailable;
use redhive_contracts::notifications::{event_types, CommandExecutedEvent, NotificationEnvelope, Severity};
use redhive_contracts::viewpack::{DockSide, ViewBody, ViewRole};
use redhive_contracts::{IconRef, TextKey};
use redhive_monads::Res;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

pub enum Phase {
    Resuming(Promise<Res<Session>>),
    Login(LoginState),
    Main,
}

pub struct Shell {
    pub app: AppContext,
    pub phase: Phase,
    pub settings: ShellSettings,
    pub connection: ConnectionSettings,
    pub dock: DockManager,
    pub windows: WindowManager,
    pub hub: NotificationHub,
    pub center: NotificationCenter,
    pub modules: BTreeMap<String, LoadedModule>,
    pub rejected: Vec<(String, redhive_monads::Error)>,
    pub module_load: Option<Promise<Res<LoadResult>>>,
    pub hotswap: HotSwapManager,
    pub views: HashMap<String, ViewState>,
    pub workflows: WorkflowsUi,
    pub shell_views: ShellViews,
    pub overlays: Overlays,
    pub wheel: NavigationWheel,
    deep_link: Option<String>,
    last_inbox_refresh: Instant,
    key_changed: Option<String>,
    initial_notifications: Option<Promise<Res<Vec<NotificationEnvelope>>>>,
    applied_settings: Option<String>,
}

fn descriptor(id: &str, label: &str, icon: &str, permission: &str, placements: Vec<CommandPlacement>, shortcuts: &[&str]) -> CommandDescriptor {
    CommandDescriptor {
        id: id.into(),
        label: TextKey::new(label),
        icon: IconRef::new(icon),
        permission: permission.into(),
        placements,
        shortcuts: shortcuts.iter().filter_map(|g| KeyGesture::parse(g)).map(|gesture| ShortcutBinding { gesture, scope: ShortcutScope::Global }).collect(),
        workflow_capable: false,
        ai_allowed: false,
        module_id: "shell".into(),
    }
}

fn menu(path: &str, section: i32, order: i32) -> CommandPlacement {
    CommandPlacement::new(placements::MAIN_MENU, path, section, order)
}

/// Shell views: (key, title, icon, role, dock side, permission).
const SHELL_VIEWS: &[(&str, &str, &str, ViewRole, DockSide, &str)] = &[
    ("shell.start", "shell:cmd.start", "start", ViewRole::Document, DockSide::Right, ""),
    ("shell.navigation", "shell:cmd.navigation", "navigation", ViewRole::ToolWindow, DockSide::Left, ""),
    ("shell.notifications", "shell:cmd.notifications", "notifications", ViewRole::ToolWindow, DockSide::Bottom, ""),
    ("shell.output", "shell:cmd.output", "output", ViewRole::ToolWindow, DockSide::Bottom, ""),
    ("shell.modules", "shell:cmd.moduleExplorer", "modules", ViewRole::Document, DockSide::Right, "modules.view"),
    ("shell.settings", "shell:cmd.settings", "settings", ViewRole::Document, DockSide::Right, ""),
    ("shell.shortcuts", "shell:cmd.shortcuts", "keyboard", ViewRole::Document, DockSide::Right, ""),
    ("shell.profile", "shell:cmd.profile", "profile", ViewRole::Document, DockSide::Right, ""),
    ("workflows.inbox", "shell:cmd.inbox", "inbox", ViewRole::ToolWindow, DockSide::Right, "workflows.run"),
    ("workflows.list", "shell:cmd.workflows", "workflows", ViewRole::Document, DockSide::Right, "workflows.view"),
    ("workflows.designer", "shell:cmd.designer", "designer", ViewRole::Document, DockSide::Right, "workflows.edit"),
    ("workflows.instances", "shell:cmd.instances", "workflows", ViewRole::Document, DockSide::Right, "workflows.view"),
    ("workflows.impact", "shell:cmd.impact", "impact", ViewRole::Document, DockSide::Right, "workflows.view"),
    ("workflows.ai", "shell:cmd.aiPanel", "ai", ViewRole::ToolWindow, DockSide::Right, "ai.run"),
];

impl Shell {
    pub fn new(cc: &eframe::CreationContext<'_>, server: Option<String>, deep_link: Option<String>) -> Self {
        runtime::set_repaint_context(&cc.egui_ctx);
        egui_extras_install(&cc.egui_ctx);
        let paths = redhive_client_framework::context::Paths::new();
        let settings = ShellSettings::load(&paths);
        let mut connection = ConnectionSettings::load(&paths);
        if let Some(s) = server {
            connection.server_url = s;
        }
        let app = AppContext::new(&settings.language);
        app.api.set_base(&connection.server_url);
        let dock = DockManager::load(&app.paths);
        let windows = WindowManager::load(&app.paths);
        let phase = match TokenStore::load(&app.paths, &connection.server_url).filter(|_| connection.remember) {
            Some(token) => {
                let api = app.api.clone();
                Phase::Resuming(Promise::spawn(async move { redhive_client_framework::security::resume(&api, token).await }))
            }
            None => Phase::Login(LoginState::new(&connection)),
        };
        let mut shell = Shell {
            app,
            phase,
            settings,
            connection,
            dock,
            windows,
            hub: NotificationHub::default(),
            center: NotificationCenter::default(),
            modules: BTreeMap::new(),
            rejected: Vec::new(),
            module_load: None,
            hotswap: HotSwapManager::default(),
            views: HashMap::new(),
            workflows: WorkflowsUi::default(),
            shell_views: ShellViews::default(),
            overlays: Overlays::default(),
            wheel: NavigationWheel::default(),
            deep_link,
            last_inbox_refresh: Instant::now(),
            key_changed: None,
            initial_notifications: None,
            applied_settings: None,
        };
        shell.register_shell();
        shell
    }

    fn register_shell(&mut self) {
        let reg = &mut self.app.commands;
        reg.register_group("shell", redhive_contracts::commands::CommandGroupDescriptor { menu: placements::MAIN_MENU.into(), path: "file".into(), label: TextKey::new("shell:menu.file"), icon: IconRef::default(), order: 0 });
        reg.register_group("shell", redhive_contracts::commands::CommandGroupDescriptor { menu: placements::MAIN_MENU.into(), path: "view".into(), label: TextKey::new("shell:menu.view"), icon: IconRef::default(), order: 5 });
        reg.register_group("shell", redhive_contracts::commands::CommandGroupDescriptor { menu: placements::MAIN_MENU.into(), path: "workflows".into(), label: TextKey::new("shell:menu.workflows"), icon: IconRef::default(), order: 60 });
        reg.register_group("shell", redhive_contracts::commands::CommandGroupDescriptor { menu: placements::MAIN_MENU.into(), path: "admin".into(), label: TextKey::new("shell:menu.admin"), icon: IconRef::default(), order: 80 });
        reg.register_group("shell", redhive_contracts::commands::CommandGroupDescriptor { menu: placements::MAIN_MENU.into(), path: "help".into(), label: TextKey::new("shell:menu.help"), icon: IconRef::default(), order: 99 });
        let shell_cmds: Vec<(CommandDescriptor, CommandHandler)> = vec![
            (descriptor("shell.palette", "shell:cmd.palette", "palette", "", vec![menu("view", 0, 0), CommandPlacement::new(placements::TOOLBAR, "", 0, 0)], &["Ctrl+Shift+P"]), CommandHandler::Shell),
            (descriptor("shell.signOut", "shell:cmd.signOut", "signout", "", vec![menu("file", 8, 0), CommandPlacement::new(placements::PROFILE, "", 9, 0)], &[]), CommandHandler::Shell),
            (descriptor("shell.exit", "shell:cmd.exit", "exit", "", vec![menu("file", 9, 0)], &["Alt+F4"]), CommandHandler::Shell),
            (descriptor("shell.reloadModules", "shell:cmd.reloadModules", "refresh", "", vec![menu("admin", 1, 0)], &[]), CommandHandler::Shell),
            (descriptor("shell.applyUpdates", "shell:cmd.applyUpdates", "modules", "", vec![menu("admin", 1, 1)], &[]), CommandHandler::Shell),
            (descriptor("shell.cheatSheet", "shell:cmd.cheatSheet", "keyboard", "", vec![menu("help", 0, 1)], &["Ctrl+/"]), CommandHandler::Shell),
            (descriptor("shell.help", "shell:cmd.help", "help", "", vec![menu("help", 0, 0)], &["F1"]), CommandHandler::Shell),
            (descriptor("shell.about", "shell:cmd.about", "info", "", vec![menu("help", 9, 0)], &[]), CommandHandler::Shell),
            (descriptor("shell.detach", "shell:cmd.detach", "detach", "", vec![menu("view", 3, 0)], &["Ctrl+Shift+D"]), CommandHandler::Shell),
            (descriptor("shell.attach", "shell:cmd.attach", "attach", "", vec![menu("view", 3, 1)], &["Ctrl+Shift+A"]), CommandHandler::Shell),
            (descriptor("shell.closeTab", "shell:cmd.closeTab", "close", "", vec![menu("view", 4, 0)], &["Ctrl+F4"]), CommandHandler::Shell),
            (descriptor("shell.nextTab", "shell:cmd.nextTab", "", "", vec![], &["Ctrl+F6"]), CommandHandler::Shell),
            (descriptor("shell.previousTab", "shell:cmd.previousTab", "", "", vec![], &["Ctrl+Shift+F6"]), CommandHandler::Shell),
            (descriptor("shell.resetLayout", "shell:cmd.resetLayout", "layout", "", vec![menu("view", 5, 0)], &[]), CommandHandler::Shell),
            (descriptor("shell.saveLayout", "shell:cmd.saveLayout", "layout", "", vec![menu("view", 5, 1)], &[]), CommandHandler::Shell),
            (descriptor("shell.record", "shell:cmd.record", "record", "workflows.edit", vec![menu("workflows", 2, 0)], &[]), CommandHandler::Shell),
            (descriptor("shell.stopRecording", "shell:cmd.stopRecording", "stop", "workflows.edit", vec![menu("workflows", 2, 1)], &[]), CommandHandler::Shell),
        ];
        for (d, h) in shell_cmds {
            self.app.commands.register("shell", d, h);
        }
        for (key, title, icon, role, dock, perm) in SHELL_VIEWS {
            self.app.navigation.register(ViewEntry { key: key.to_string(), title: title.to_string(), icon: icon.to_string(), role: *role, dock: *dock, home: false, permission: perm.to_string(), order: 0, owner: "shell".into(), source: ViewSource::Shell });
            let (path, section, shortcut): (&str, i32, &[&str]) = match *key {
                "shell.start" => ("view", 1, &["Ctrl+Alt+H"]),
                "shell.navigation" => ("view", 1, &[]),
                "shell.notifications" => ("view", 2, &["Ctrl+Alt+N"]),
                "shell.output" => ("view", 2, &[]),
                "shell.shortcuts" => ("admin", 2, &["Ctrl+K, Ctrl+S"]),
                "shell.settings" => ("admin", 2, &[]),
                "shell.profile" => ("file", 1, &[]),
                "shell.modules" => ("admin", 0, &[]),
                "workflows.inbox" => ("workflows", 0, &["Ctrl+Shift+I"]),
                "workflows.ai" => ("workflows", 1, &[]),
                _ => ("workflows", 0, &[]),
            };
            let mut placements = vec![menu(path, section, 0)];
            if *key == "shell.profile" || *key == "shell.settings" {
                placements.push(CommandPlacement::new(placements::PROFILE, "", 0, 0));
            }
            if *key == "workflows.inbox" || *key == "shell.notifications" {
                placements.push(CommandPlacement::new(placements::TOOLBAR, "", 1, 0));
            }
            self.app.commands.register("shell", descriptor(&format!("navigate.{key}"), title, icon, perm, placements, shortcut), CommandHandler::Navigate(key.to_string()));
        }
    }

    // ------------------------------------------------------------------ session

    fn after_login(&mut self, session: Session) {
        self.app.api.set_token(Some(session.token.clone()));
        self.app.log.info("security", format!("signed in as {}", session.profile.user_name));
        self.connection.remembered_user = Some(session.profile.user_name.clone());
        if let Phase::Login(l) = &self.phase {
            self.connection.remember = l.remember;
            self.connection.server_url = l.server.trim().to_string();
        }
        self.connection.save(&self.app.paths);
        if self.connection.remember {
            TokenStore::save(&self.app.paths, &self.connection.server_url, &session.token);
        }
        self.app.session = Some(session);
        self.hub.start(self.app.api.clone());
        let api = self.app.api.clone();
        self.initial_notifications = Some(Promise::spawn(async move { api.get::<Vec<NotificationEnvelope>>("/api/notifications?take=50").await }));
        self.reload_modules();
        self.workflows.refresh_inbox(&self.app);
        self.phase = Phase::Main;
    }

    fn sign_out(&mut self) {
        self.hub.stop();
        for id in self.modules.keys().cloned().collect::<Vec<_>>() {
            modules::unregister(&mut self.app, &id);
        }
        self.modules.clear();
        self.views.clear();
        self.center.clear();
        self.workflows = WorkflowsUi::default();
        TokenStore::clear(&self.app.paths, &self.connection.server_url);
        self.app.session = None;
        self.app.api.set_token(None);
        self.dock.save();
        self.phase = Phase::Login(LoginState::new(&self.connection));
    }

    fn reload_modules(&mut self) {
        let api = self.app.api.clone();
        let paths = self.app.paths.clone();
        let pinned = self.connection.pinned_keys.get(&self.connection.server_url).cloned();
        self.module_load = Some(Promise::spawn(async move { modules::load_assigned(&api, &paths, pinned, None).await }));
    }

    fn modules_loaded(&mut self, r: LoadResult) {
        if let Some(k) = &r.key_changed {
            self.key_changed = Some(k.clone());
            self.app.toast_error(&redhive_monads::Error::conflict("modules.signingKey.changed", "modules:signingKey.changed"));
        } else if let Some(k) = &r.server_key {
            // Trust on first use.
            if !self.connection.pinned_keys.contains_key(&self.connection.server_url) {
                self.connection.pinned_keys.insert(self.connection.server_url.clone(), k.clone());
                self.connection.save(&self.app.paths);
            }
        }
        for id in self.modules.keys().cloned().collect::<Vec<_>>() {
            modules::unregister(&mut self.app, &id);
        }
        self.modules.clear();
        for m in r.modules {
            modules::register(&mut self.app, &m);
            self.modules.insert(m.manifest.module_id.clone(), m);
        }
        for (id, e) in &r.rejected {
            let mut a = serde_json::Map::new();
            a.insert("module".into(), json!(id));
            a.insert("reason".into(), json!(self.app.lang.error(e)));
            let text = self.app.lang.t_args("shell:modules.rejected", &a);
            self.app.log.warn("modules", text.clone());
            self.app.toast(text, Severity::Warning);
        }
        self.rejected = r.rejected;
        for v in self.views.values_mut() {
            v.dirty = true;
        }
        // Deep link (from a WNS toast or the command line).
        if let Some(link) = self.deep_link.take() {
            if let Some(q) = link.strip_prefix("redhive://navigate?") {
                let mut target = None;
                let mut param = None;
                for kv in q.split('&') {
                    match kv.split_once('=') {
                        Some(("target", t)) => target = Some(t.to_string()),
                        Some(("parameter", p)) if !p.is_empty() => param = Some(p.to_string()),
                        _ => {}
                    }
                }
                if let Some(t) = target {
                    self.app.open(&t, param);
                }
            }
        }
    }

    // ------------------------------------------------------------------ events

    fn route(&mut self, env: NotificationEnvelope, focused: bool) {
        let me = self.app.session.as_ref().map(|s| s.profile.id);
        if env.is_visible() {
            let text = self.app.lang.t_args(&env.title, &env.text_args);
            if !focused && self.settings.os_toasts {
                system_toast("Red Hive", &text);
            }
            self.app.toasts.push(redhive_client_framework::context::Toast {
                text,
                severity: env.severity,
                until: Instant::now() + Duration::from_secs(7),
                navigate: env.navigation_target.clone().map(|t| (t, env.navigation_parameter.clone())),
            });
            self.center.add(env.clone(), false);
        }
        // live refresh of open views
        for (key, state) in self.views.iter_mut() {
            if let Some(ViewEntry { source: ViewSource::Module(spec), .. }) = self.app.navigation.get(key) {
                if spec.live_events.iter().any(|p| env.matches(p)) {
                    state.dirty = true;
                }
            }
        }
        if env.event_type.starts_with("workflow.") {
            self.workflows.invalidate();
            if env.event_type == event_types::WORKFLOW_IMPACT_ANALYZED {
                self.workflows.impact.dirty = true;
            }
        }
        if env.event_type.starts_with("modules.") || env.event_type.starts_with("updates.") {
            self.shell_views.explorer_dirty = true;
        }
        if env.event_type == event_types::AI_SWITCH_CHANGED {
            self.workflows.ai.dirty = true;
        }
        if env.event_type == event_types::MODULE_VERSION_AVAILABLE {
            if let (Ok(n), Some(me)) = (serde_json::from_value::<VersionAvailable>(env.payload.clone()), me) {
                self.app.log.info("hotswap", format!("{} {} available ({:?})", n.module_id, n.version, n.stage));
                self.hotswap.notice(n, me);
            }
        }
    }

    // ------------------------------------------------------------------ commands

    fn execute(&mut self, id: &str, parameter: Value, ctx: &egui::Context) {
        let Some(RegisteredCommand { handler, descriptor, .. }) = self.app.commands.get(id).cloned() else {
            self.app.log.warn("commands", format!("unknown command {id}"));
            return;
        };
        if !descriptor.permission.is_empty() && !self.app.has(&descriptor.permission) {
            self.app.toast_error(&redhive_monads::Error::forbidden("auth.permission.missing", "errors:forbidden").with_metadata("permission", descriptor.permission.clone()));
            return;
        }
        match handler {
            CommandHandler::Navigate(view) => self.open(&view, parameter.as_str().map(str::to_string)),
            CommandHandler::Action { view, action, .. } => match view {
                Some(view_key) => {
                    self.open(&view_key, None);
                    let spec = match self.app.navigation.get(&view_key).map(|v| v.source.clone()) {
                        Some(ViewSource::Module(spec)) => spec,
                        _ => return,
                    };
                    let state = self.views.entry(view_key.clone()).or_default();
                    let row = match &spec.body {
                        ViewBody::List(l) if l.row_actions.iter().any(|a| a.id == action.id) => match state.selected_row(l) {
                            Some(r) => r,
                            None => {
                                let t = self.app.t("shell:list.select");
                                self.app.toast(t, Severity::Warning);
                                return;
                            }
                        },
                        _ => Value::Null,
                    };
                    views::start_action(&mut self.app, state, &action, row);
                }
                None => {
                    let state = self.views.entry("__commands".into()).or_default();
                    views::start_action(&mut self.app, state, &action, Value::Null);
                }
            },
            CommandHandler::Shell => match id {
                "shell.palette" => self.overlays.open_palette(),
                "shell.signOut" => self.sign_out(),
                "shell.exit" => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                "shell.reloadModules" => self.reload_modules(),
                "shell.applyUpdates" => {
                    // Re-reads the assignment; modules whose version changed are swapped.
                    if let Some(me) = self.app.session.as_ref().map(|s| s.profile.id) {
                        for (mid, m) in &self.modules {
                            self.hotswap.notice(
                                VersionAvailable { module_id: mid.clone(), version: m.newer_version.clone().unwrap_or_default(), previous_version: None, stage: redhive_contracts::modules::RolloutStage::Complete, hot_swap: m.manifest.client.hot_swap, users: vec![me] },
                                me,
                            );
                        }
                    }
                }
                "shell.cheatSheet" => self.overlays.cheat_sheet = !self.overlays.cheat_sheet,
                "shell.help" => {
                    let t = self.app.t("shell:help.text");
                    self.app.toast(t, Severity::Info);
                }
                "shell.about" => self.overlays.about = true,
                "shell.detach" => {
                    if let Some(k) = self.dock.focused() {
                        self.windows.detach(&k);
                        self.dock.close(&k);
                    }
                }
                "shell.attach" => {
                    if let Some(k) = self.windows.layout.open.last().cloned() {
                        self.windows.attach(&k);
                        self.open(&k, None);
                    }
                }
                "shell.closeTab" => {
                    if let Some(k) = self.dock.focused() {
                        self.dock.close(&k);
                    }
                }
                "shell.nextTab" => self.dock.cycle(true),
                "shell.previousTab" => self.dock.cycle(false),
                "shell.resetLayout" => self.dock.reset(),
                "shell.saveLayout" => self.overlays.save_layout = Some(String::new()),
                "shell.record" => {
                    self.workflows.recorder.active = true;
                    self.workflows.recorder.steps.clear();
                    let t = self.app.t("workflow:recorder.started");
                    self.app.toast(t, Severity::Info);
                }
                "shell.stopRecording" => {
                    if self.workflows.recorder.active {
                        let n = self.workflows.recorder.steps.len();
                        let def = self.workflows.recorder.finish("Recorded workflow");
                        self.workflows.open_definition(&def);
                        let mut a = serde_json::Map::new();
                        a.insert("count".into(), json!(n));
                        let t = self.app.lang.t_args("workflow:recorder.stopped", &a);
                        self.app.toast(t, Severity::Success);
                        self.open("workflows.designer", None);
                    }
                }
                other => self.app.log.warn("commands", format!("no handler for {other}")),
            },
        }
    }

    pub fn open(&mut self, key: &str, parameter: Option<String>) {
        let Some(entry) = self.app.navigation.get(key).cloned() else {
            let mut a = serde_json::Map::new();
            a.insert("view".into(), json!(key));
            let t = self.app.lang.t_args("shell:view.notFound", &a);
            self.app.toast(t, Severity::Warning);
            return;
        };
        if !entry.permission.is_empty() && !self.app.has(&entry.permission) {
            self.app.toast_error(&redhive_monads::Error::forbidden("auth.permission.missing", "errors:forbidden").with_metadata("permission", entry.permission.clone()));
            return;
        }
        self.app.navigation.touch(&entry.owner);
        if self.windows.is_detached(key) {
            // focus is handled by the OS window
            return;
        }
        if let ViewSource::Module(_) = entry.source {
            self.views.entry(key.to_string()).or_default();
        }
        let nav = &self.app.navigation;
        let tool_sides = |k: &str| nav.get(k).filter(|v| v.role == ViewRole::ToolWindow).map(|v| v.dock);
        self.dock.open(key, parameter, entry.role, entry.dock, &tool_sides);
    }

    fn busy_modules(&self) -> Vec<String> {
        self.views.iter().filter(|(_, s)| s.busy()).filter_map(|(k, _)| self.app.navigation.get(k).map(|v| v.owner.clone())).collect()
    }

    // ------------------------------------------------------------------ frame

    fn frame_main(&mut self, ctx: &egui::Context) {
        let focused = ctx.input(|i| i.focused);
        for env in self.hub.drain() {
            self.route(env, focused);
        }
        if let Some(r) = poll(&mut self.initial_notifications) {
            if let Ok(list) = r {
                for env in list.into_iter().rev() {
                    self.center.add(env, true);
                }
            }
        }
        if let Some(r) = poll(&mut self.module_load) {
            match r {
                Ok(r) => self.modules_loaded(r),
                Err(e) => {
                    if e.kind == redhive_monads::ErrorKind::Unauthorized {
                        self.sign_out();
                        return;
                    }
                    self.app.toast_error(&e);
                }
            }
        }
        self.workflows.poll(&mut self.app);
        if self.last_inbox_refresh.elapsed() > Duration::from_secs(60) {
            self.workflows.inbox.dirty = true;
            self.last_inbox_refresh = Instant::now();
        }
        // hot-swap
        let busy = self.busy_modules();
        let pinned = self.connection.pinned_keys.get(&self.connection.server_url).cloned();
        let paths = self.app.paths.clone();
        for o in self.hotswap.tick(&mut self.app, &paths, pinned, &mut self.modules, &|m| busy.iter().any(|b| b == m)) {
            let (key, sev, mut args) = match &o {
                SwapOutcome::Swapped { module_id, to, rollback, .. } => {
                    let mut a = serde_json::Map::new();
                    a.insert("module".into(), json!(module_id));
                    a.insert("version".into(), json!(to));
                    for (k, v) in self.views.iter_mut() {
                        if self.app.navigation.get(k).is_some_and(|e| &e.owner == module_id) {
                            v.dirty = true;
                        }
                    }
                    (if *rollback { "shell:updates.rolledBack" } else { "shell:updates.swapped" }, Severity::Success, a)
                }
                SwapOutcome::RestartRequired { module_id, version } => {
                    let mut a = serde_json::Map::new();
                    a.insert("module".into(), json!(module_id));
                    a.insert("version".into(), json!(version));
                    ("shell:updates.restart", Severity::Warning, a)
                }
                SwapOutcome::Failed { module_id, error } => {
                    let mut a = serde_json::Map::new();
                    a.insert("module".into(), json!(module_id));
                    self.app.log.error("hotswap", self.app.lang.error(error));
                    ("shell:updates.failed", Severity::Error, a)
                }
            };
            let text = self.app.lang.t_args(key, &std::mem::take(&mut args));
            self.app.log.info("hotswap", text.clone());
            self.app.toast(text, sev);
        }
        // shortcuts (not while the wheel or a recording is active)
        self.app.shortcuts.rebuild(&self.app.commands);
        let text_focus = ctx.memory(|m| m.focused().is_some()) && ctx.wants_keyboard_input();
        let (ctrl_tab, shift) = ctx.input(|i| ((i.modifiers.ctrl || i.modifiers.command) && i.key_pressed(egui::Key::Tab), i.modifiers.shift));
        if ctrl_tab && !self.wheel.open {
            let perms = self.app.session.as_ref().map(|s| s.profile.permissions.clone()).unwrap_or_default();
            let n = self.app.navigation.homes(&perms).len();
            self.wheel.begin(shift, n);
        } else if !self.wheel.open {
            for cmd in self.app.shortcuts.process(ctx, text_focus, &[ShortcutScope::View, ShortcutScope::Module]) {
                self.app.run(&cmd, Value::Null);
            }
        }
        while let Some((cmd, p)) = self.app.pending_commands.pop_front() {
            self.execute(&cmd, p, ctx);
        }
        while let Some((key, p)) = self.app.open_requests.pop_front() {
            self.open(&key, p);
        }
        for (cmd, param) in std::mem::take(&mut self.app.executed_commands) {
            self.workflows.recorder.record(&cmd, param.clone());
            let api = self.app.api.clone();
            runtime::spawn(async move {
                let _ = api.post::<Value, _>("/api/workflows/commands", &CommandExecutedEvent { command_id: cmd, parameter: param }).await;
            });
        }

        self.top_bar(ctx);
        self.status_bar(ctx);
        let mut state = std::mem::replace(&mut self.dock.state, DockState::new(vec![]));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(ctx.style().visuals.panel_fill)).show(ctx, |ui| {
            let mut viewer = Viewer { shell: self, detach: None, close: Vec::new() };
            DockArea::new(&mut state).style(Style::from_egui(ui.style().as_ref())).show_add_buttons(false).show_leaf_collapse_buttons(false).show_inside(ui, &mut viewer);
            let (detach, close) = (viewer.detach.take(), std::mem::take(&mut viewer.close));
            if let Some(k) = detach {
                self.windows.detach(&k);
                close_in(&mut state, &k);
            }
            for k in close {
                close_in(&mut state, &k);
            }
        });
        self.dock.state = state;
        self.detached_windows(ctx);
        self.overlays_ui(ctx);
        self.windows.tick();
    }

    fn detached_windows(&mut self, ctx: &egui::Context) {
        let monitor = ctx.input(|i| i.viewport().monitor_size).unwrap_or(egui::vec2(1920.0, 1080.0));
        for key in self.windows.layout.open.clone() {
            let Some(entry) = self.app.navigation.get(&key).cloned() else { continue };
            let title = format!("{} – Red Hive", self.app.t(&entry.title));
            let mut builder = egui::ViewportBuilder::default().with_title(title).with_inner_size([900.0, 640.0]);
            if let Some(p) = self.windows.placement(&key, monitor) {
                builder = builder.with_position([p.x, p.y]).with_inner_size([p.width, p.height]);
            }
            let vid = egui::ViewportId::from_hash_of(("redhive.detached", &key));
            let mut close = false;
            ctx.show_viewport_immediate(vid, builder, |vctx, _class| {
                egui::TopBottomPanel::top(egui::Id::new(("detached-top", &key))).show(vctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(self.app.icons.label(&entry.icon, &self.app.t(&entry.title))).strong());
                        if ui.button(self.app.icons.label("attach", &self.app.t("shell:cmd.attach"))).clicked() {
                            close = true;
                        }
                    });
                });
                egui::CentralPanel::default().show(vctx, |ui| {
                    self.render_view(ui, &key, None);
                });
                let (closing, rect, outer) = vctx.input(|i| (i.viewport().close_requested(), i.viewport().inner_rect, i.viewport().outer_rect));
                if closing {
                    close = true;
                }
                if let (Some(r), Some(o)) = (rect, outer) {
                    self.windows.moved(&key, WindowPlacement { x: o.min.x, y: o.min.y, width: r.width(), height: r.height(), monitor_width: monitor.x, monitor_height: monitor.y, maximized: false });
                }
                // Shortcuts of a detached window run in the shell too.
                let text_focus = vctx.wants_keyboard_input();
                for cmd in self.app.shortcuts.process(vctx, text_focus, &[ShortcutScope::View]) {
                    self.app.run(&cmd, Value::Null);
                }
            });
            if close {
                self.windows.attach(&key);
                self.open(&key, None);
            }
        }
    }

    /// Renders any view by key (dock tab or detached window).
    pub fn render_view(&mut self, ui: &mut egui::Ui, key: &str, parameter: Option<String>) {
        match key {
            "shell.start" => self.start_page(ui),
            "shell.navigation" => self.navigation_tree(ui),
            "shell.notifications" => self.notifications_view(ui),
            "shell.output" => self.output_view(ui),
            "shell.modules" => self.module_explorer(ui),
            "shell.settings" => self.settings_view(ui),
            "shell.shortcuts" => self.shortcuts_view(ui),
            "shell.profile" => self.profile_view(ui),
            "workflows.inbox" => self.workflows.inbox_ui(ui, &mut self.app, parameter),
            "workflows.list" => {
                let mut open = None;
                self.workflows.list_ui(ui, &mut self.app, &mut open);
                if let Some(id) = open {
                    self.workflows.open_in_designer(&self.app, id);
                    self.open("workflows.designer", None);
                }
            }
            "workflows.designer" => self.workflows.designer_ui(ui, &mut self.app),
            "workflows.instances" => self.workflows.instances_ui(ui, &mut self.app),
            "workflows.impact" => self.workflows.impact_ui(ui, &mut self.app, parameter),
            "workflows.ai" => self.workflows.ai_ui(ui, &mut self.app),
            _ => match self.app.navigation.get(key).cloned() {
                Some(ViewEntry { source: ViewSource::Module(spec), owner, .. }) => {
                    let prefix = self.modules.get(&owner).map(|m| m.manifest.prefix.clone()).unwrap_or_default();
                    let state = self.views.entry(key.to_string()).or_default();
                    match (&parameter, &spec.body) {
                        (Some(p), ViewBody::List(_)) if state.selected.is_none() => state.selected = Some(p.clone()),
                        (Some(p), ViewBody::ReportRunner { .. }) => state.report.selected = Some(p.clone()),
                        _ => {}
                    }
                    views::render(ui, &mut self.app, &spec, &prefix, state);
                }
                _ => {
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new(self.app.t("shell:view.placeholder")).weak());
                        ui.label(egui::RichText::new(key).small().weak());
                    });
                }
            },
        }
    }

    fn top_bar(&mut self, ctx: &egui::Context) {
        let perms = self.app.session.as_ref().map(|s| s.profile.permissions.clone()).unwrap_or_default();
        let german = self.app.lang.language().starts_with("de");
        let shortcuts = &self.app.shortcuts;
        let main_tree = self.app.commands.tree(placements::MAIN_MENU, &perms, |id| shortcuts.text(id, german));
        let toolbar = self.app.commands.tree(placements::TOOLBAR, &perms, |id| shortcuts.text(id, german));
        egui::TopBottomPanel::top("titlebar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                redhive_client_framework::widgets::brand(ui, 15.0);
                ui.separator();
                egui::menu::bar(ui, |ui| {
                    for node in &main_tree {
                        crate::overlays::menu_node(ui, &mut self.app, node);
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let name = self.app.session.as_ref().map(|s| s.profile.display_name.clone()).unwrap_or_default();
                    let profile_tree = self.app.commands.tree(placements::PROFILE, &perms, |_| None);
                    ui.menu_button(self.app.icons.label("profile", &name), |ui| {
                        if let Some(s) = &self.app.session {
                            ui.label(egui::RichText::new(&s.profile.display_name).strong());
                            ui.label(egui::RichText::new(format!("{} · {}", s.profile.user_name, s.profile.roles.join(", "))).small().weak());
                            ui.separator();
                        }
                        for node in &profile_tree {
                            crate::overlays::menu_node(ui, &mut self.app, node);
                        }
                    });
                    if self.workflows.recorder.active {
                        ui.label(egui::RichText::new(self.app.t("shell:status.recording")).color(egui::Color32::from_rgb(229, 57, 53)));
                    }
                    if ui.button(format!("⌘ {}", self.app.t("shell:palette.placeholder"))).on_hover_text("Ctrl+Shift+P").clicked() {
                        self.overlays.open_palette();
                    }
                    for node in toolbar.iter().rev() {
                        if let redhive_contracts::commands::CommandTreeNode::Command { command_id, label, icon, shortcut, .. } = node {
                            let glyph = self.app.icons.glyph(&icon.0);
                            let mut text = glyph.clone();
                            if command_id == "navigate.workflows.inbox" && self.workflows.inbox_count() > 0 {
                                text = format!("{glyph} {}", self.workflows.inbox_count());
                            }
                            if command_id == "navigate.shell.notifications" && self.center.unread() > 0 {
                                text = format!("{glyph} {}", self.center.unread());
                            }
                            let tip = match shortcut {
                                Some(s) => format!("{}   {s}", self.app.t(&label.0)),
                                None => self.app.t(&label.0),
                            };
                            if ui.button(text).on_hover_text(tip).clicked() {
                                self.app.run(command_id, Value::Null);
                            }
                        }
                    }
                });
            });
        });
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("statusbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let (text, color) = match self.hub.state() {
                    ConnectionState::Connected => (self.app.t("shell:status.connected"), egui::Color32::from_rgb(67, 160, 71)),
                    ConnectionState::Connecting => (self.app.t("shell:status.reconnecting"), egui::Color32::from_rgb(251, 140, 0)),
                    ConnectionState::Offline => (self.app.t("shell:status.offline"), egui::Color32::from_rgb(229, 57, 53)),
                };
                ui.label(egui::RichText::new("⏺").color(color));
                ui.label(egui::RichText::new(text).small());
                ui.separator();
                ui.label(egui::RichText::new(self.app.api.base()).small().weak());
                if let Some(s) = &self.app.session {
                    ui.separator();
                    ui.label(egui::RichText::new(format!("👤 {}", s.profile.display_name)).small());
                }
                ui.separator();
                let mut a = serde_json::Map::new();
                a.insert("count".into(), json!(self.modules.len()));
                ui.label(egui::RichText::new(self.app.lang.t_args("shell:status.modules", &a)).small());
                let unread = self.center.unread();
                if unread > 0 {
                    ui.separator();
                    let mut a = serde_json::Map::new();
                    a.insert("count".into(), json!(unread));
                    if ui.link(egui::RichText::new(format!("🔔 {}", self.app.lang.t_args("shell:status.unread", &a))).small()).clicked() {
                        self.open("shell.notifications", None);
                    }
                }
                if self.hotswap.pending() > 0 {
                    ui.separator();
                    ui.spinner();
                }
                if let Some(c) = self.app.shortcuts.chord_pending() {
                    ui.separator();
                    ui.label(egui::RichText::new(format!("{c}, …")).small().monospace());
                }
            });
        });
    }

    fn overlays_ui(&mut self, ctx: &egui::Context) {
        // dialogs of commands that are not tied to a view
        if let Some(state) = self.views.get_mut("__commands") {
            if state.dialog.is_some() {
                // Commands that are not tied to a view still open their form/confirmation dialogs: the view
                // renderer is driven with an empty dashboard outside the visible area, its dialogs are windows.
                egui::Area::new(egui::Id::new("command-dialogs")).show(ctx, |ui| {
                    let mut tmp = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(egui::pos2(-5000.0, -5000.0), egui::vec2(1.0, 1.0))));
                    state.dirty = false;
                    state.data.get_or_insert(json!({}));
                    views::render(&mut tmp, &mut self.app, &spec_stub(), "", state);
                });
            }
        }
        let perms = self.app.session.as_ref().map(|s| s.profile.permissions.clone()).unwrap_or_default();
        // navigation wheel
        if self.wheel.open {
            let homes: Vec<(String, String, String, String)> = self.app.navigation.homes(&perms).iter().map(|v| (v.key.clone(), self.app.t(&v.title), self.app.icons.glyph(&v.icon), v.owner.clone())).collect();
            let inbox = self.workflows.inbox_count();
            let inner: Vec<WheelItem> = homes.iter().map(|(k, l, g, _)| WheelItem { key: k.clone(), label: l.clone(), glyph: g.clone(), badge: None }).chain(std::iter::once(WheelItem { key: "workflows.inbox".into(), label: self.app.t("shell:cmd.inbox"), glyph: self.app.icons.glyph("inbox"), badge: Some(inbox) }).filter(|_| self.app.has("workflows.run"))).collect();
            let owners: Vec<String> = homes.iter().map(|h| h.3.clone()).collect();
            let nav = &self.app.navigation;
            let app_ref = &self.app;
            let outer_of = |i: usize| -> Vec<WheelItem> {
                match owners.get(i) {
                    Some(o) => nav.views_of(o, &perms).iter().map(|v| WheelItem { key: v.key.clone(), label: app_ref.t(&v.title), glyph: app_ref.icons.glyph(&v.icon), badge: None }).collect(),
                    None => vec![],
                }
            };
            let title = self.app.t("shell:wheel.title");
            if let Some(key) = self.wheel.show(ctx, &inner, &outer_of, &title, self.settings.reduced_motion) {
                self.open(&key, None);
            }
        }
        crate::overlays::draw(self, ctx);
    }
}

fn spec_stub() -> redhive_contracts::viewpack::ViewSpec {
    redhive_contracts::viewpack::ViewSpec {
        key: "__commands".into(),
        title: String::new(),
        icon: String::new(),
        role: ViewRole::Document,
        dock: DockSide::Right,
        home: false,
        permission: String::new(),
        live_events: vec![],
        order: 0,
        body: ViewBody::Dashboard { endpoint: String::new(), cards: vec![], chart: None, lists: vec![], actions: vec![] },
    }
}

fn close_in(state: &mut DockState<Tab>, key: &str) {
    let tab = state.iter_all_tabs().find(|(_, t)| t.key == key).map(|(_, t)| t.clone());
    if let Some(t) = tab {
        if let Some(found) = state.find_tab(&t) {
            let _ = state.remove_tab(found);
        }
    }
}

fn egui_extras_install(_ctx: &egui::Context) {}

struct Viewer<'a> {
    shell: &'a mut Shell,
    detach: Option<String>,
    close: Vec<String>,
}

impl TabViewer for Viewer<'_> {
    type Tab = Tab;

    fn title(&mut self, tab: &mut Tab) -> egui::WidgetText {
        let app = &self.shell.app;
        match app.navigation.get(&tab.key) {
            Some(v) => {
                let mut t = app.icons.label(&v.icon, &app.t(&v.title));
                if tab.key == "workflows.inbox" && self.shell.workflows.inbox_count() > 0 {
                    t = format!("{t} ({})", self.shell.workflows.inbox_count());
                }
                if tab.key == "shell.notifications" && self.shell.center.unread() > 0 {
                    t = format!("{t} ({})", self.shell.center.unread());
                }
                t.into()
            }
            None => tab.key.clone().into(),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Tab) {
        egui::Frame::new().inner_margin(egui::Margin::same(6)).show(ui, |ui| {
            let param = tab.parameter.take();
            self.shell.render_view(ui, &tab.key, param);
        });
    }

    fn id(&mut self, tab: &mut Tab) -> egui::Id {
        egui::Id::new(("tab", &tab.key))
    }

    fn context_menu(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab, _surface: egui_dock::SurfaceIndex, _node: egui_dock::NodeIndex) {
        if ui.button(self.shell.app.icons.label("detach", &self.shell.app.t("shell:cmd.detach"))).clicked() {
            self.detach = Some(tab.key.clone());
            ui.close_menu();
        }
        if ui.button(self.shell.app.icons.label("close", &self.shell.app.t("shell:cmd.closeTab"))).clicked() {
            self.close.push(tab.key.clone());
            ui.close_menu();
        }
    }
}

impl eframe::App for Shell {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let fingerprint = format!("{:?}{}{}{}", self.settings.theme, self.settings.scale, self.settings.reduced_motion, self.settings.language);
        if self.applied_settings.as_deref() != Some(fingerprint.as_str()) {
            self.settings.apply(ctx);
            self.app.lang.set_language(&self.settings.language);
            self.applied_settings = Some(fingerprint);
        }
        // toasts disappear on their own
        self.app.toasts.retain(|t| t.until > Instant::now());
        if !self.app.toasts.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
        match &mut self.phase {
            Phase::Resuming(p) => {
                if let Some(r) = p.take() {
                    match r {
                        Ok(session) => self.after_login(session),
                        Err(e) => {
                            self.app.log.info("security", format!("stored session not valid: {}", e.code));
                            TokenStore::clear(&self.app.paths, &self.connection.server_url);
                            self.phase = Phase::Login(LoginState::new(&self.connection));
                        }
                    }
                } else {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.centered_and_justified(|ui| ui.spinner());
                    });
                }
            }
            Phase::Login(login) => {
                if let Some(session) = login.ui(ctx, &mut self.app) {
                    self.after_login(session);
                }
                if self.app.lang.language() != self.settings.language {
                    self.settings.language = self.app.lang.language().to_string();
                    self.settings.save(&self.app.paths);
                }
            }
            Phase::Main => self.frame_main(ctx),
        }
        crate::overlays::toasts(self, ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.dock.save();
        self.windows.save();
        self.settings.save(&self.app.paths);
        self.hub.stop();
    }
}
