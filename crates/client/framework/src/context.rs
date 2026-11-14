//! The application context (Golden Hive's `ApplicationContext`, after the WIFI.Anwendung AppKontext
//! architecture): the root object that owns every framework service, the paths, the log and the
//! in-process queues (commands to run, views to open, executed commands). Everything a module registers is
//! recorded with the module id as owner, so unloading removes it in one call.

use crate::api::ApiClient;
use crate::commands::CommandPool;
use crate::i18n::LanguageManager;
use crate::icons::IconCatalog;
use crate::navigation::NavigationRegistry;
use crate::security::Session;
use crate::shortcuts::ShortcutManager;
use chrono::{DateTime, Local};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::VecDeque;
use std::path::PathBuf;

/// Where the client keeps its files.
#[derive(Debug, Clone)]
pub struct Paths {
    /// Roaming configuration (settings, connection, shortcut overrides).
    pub config: PathBuf,
    /// Local data (layouts, window placement – per machine, monitors differ).
    pub local: PathBuf,
    /// Downloaded module packages.
    pub modules: PathBuf,
}

impl Paths {
    pub fn new() -> Self {
        let base_config = dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("RedHive");
        let base_local = dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("RedHive");
        let p = Paths { config: base_config, modules: base_local.join("Modules"), local: base_local };
        for d in [&p.config, &p.local, &p.modules, &p.local.join("Layout")] {
            let _ = std::fs::create_dir_all(d);
        }
        p
    }

    pub fn load_json<T: DeserializeOwned>(&self, name: &str) -> Option<T> {
        self.load_from(&self.config.join(name))
    }

    pub fn save_json<T: Serialize>(&self, name: &str, value: &T) {
        self.save_to(&self.config.join(name), value);
    }

    pub fn load_from<T: DeserializeOwned>(&self, path: &std::path::Path) -> Option<T> {
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok())
    }

    /// Writes atomically (temp file + rename).
    pub fn save_to<T: Serialize>(&self, path: &std::path::Path, value: &T) {
        if let Ok(text) = serde_json::to_string_pretty(value) {
            let tmp = path.with_extension("tmp");
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }
}

impl Default for Paths {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub at: DateTime<Local>,
    pub level: LogLevel,
    pub source: String,
    pub message: String,
}

/// The application log (shown in the Output tool window).
#[derive(Default)]
pub struct LogManager {
    pub entries: VecDeque<LogEntry>,
}

impl LogManager {
    pub fn add(&mut self, level: LogLevel, source: &str, message: impl Into<String>) {
        let message = message.into();
        match level {
            LogLevel::Info => tracing::info!(source, "{message}"),
            LogLevel::Warning => tracing::warn!(source, "{message}"),
            LogLevel::Error => tracing::error!(source, "{message}"),
        }
        self.entries.push_back(LogEntry { at: Local::now(), level, source: source.to_string(), message });
        while self.entries.len() > 2000 {
            self.entries.pop_front();
        }
    }
    pub fn info(&mut self, source: &str, m: impl Into<String>) {
        self.add(LogLevel::Info, source, m)
    }
    pub fn warn(&mut self, source: &str, m: impl Into<String>) {
        self.add(LogLevel::Warning, source, m)
    }
    pub fn error(&mut self, source: &str, m: impl Into<String>) {
        self.add(LogLevel::Error, source, m)
    }
}

/// A toast message shown in the corner for a few seconds.
#[derive(Debug, Clone)]
pub struct Toast {
    pub text: String,
    pub severity: redhive_contracts::notifications::Severity,
    pub until: std::time::Instant,
    pub navigate: Option<(String, Option<String>)>,
}

/// The root context.
pub struct AppContext {
    pub paths: Paths,
    pub api: ApiClient,
    pub lang: LanguageManager,
    pub icons: IconCatalog,
    pub commands: CommandPool,
    pub shortcuts: ShortcutManager,
    pub navigation: NavigationRegistry,
    pub log: LogManager,
    pub session: Option<Session>,
    pub toasts: Vec<Toast>,
    /// Commands queued for execution (from menus, palette, shortcuts, the wheel, deep links).
    pub pending_commands: VecDeque<(String, serde_json::Value)>,
    /// Workflow-capable commands that ran successfully (for command triggers and the workflow recorder).
    pub executed_commands: Vec<(String, serde_json::Value)>,
    /// Views to open (key, parameter) – processed by the shell.
    pub open_requests: VecDeque<(String, Option<String>)>,
}

impl AppContext {
    pub fn new(language: &str) -> Self {
        let paths = Paths::new();
        let shortcuts = ShortcutManager::load(&paths);
        AppContext {
            api: ApiClient::default(),
            lang: LanguageManager::new(language),
            icons: IconCatalog::default(),
            commands: CommandPool::default(),
            shortcuts,
            navigation: NavigationRegistry::default(),
            log: LogManager::default(),
            session: None,
            toasts: Vec::new(),
            pending_commands: VecDeque::new(),
            executed_commands: Vec::new(),
            open_requests: VecDeque::new(),
            paths,
        }
    }

    /// Text of a key (or literal text).
    pub fn t(&self, key: &str) -> String {
        self.lang.t(key)
    }

    pub fn has(&self, permission: &str) -> bool {
        self.session.as_ref().is_some_and(|s| s.has(permission))
    }

    /// Queues a command (executed by the shell at the start of the next frame).
    pub fn run(&mut self, command_id: &str, parameter: serde_json::Value) {
        self.pending_commands.push_back((command_id.to_string(), parameter));
    }

    /// Opens a view (with an optional parameter) at the next frame.
    pub fn open(&mut self, key: &str, parameter: Option<String>) {
        self.open_requests.push_back((key.to_string(), parameter));
    }

    pub fn toast(&mut self, text: impl Into<String>, severity: redhive_contracts::notifications::Severity) {
        self.toasts.push(Toast { text: text.into(), severity, until: std::time::Instant::now() + std::time::Duration::from_secs(6), navigate: None });
    }

    pub fn toast_error(&mut self, e: &redhive_monads::Error) {
        let text = self.lang.error(e);
        self.log.error("ui", format!("{} ({})", text, e.code));
        self.toast(text, redhive_contracts::notifications::Severity::Error);
    }
}
