//! # Red Hive client framework
//!
//! The building blocks of the desktop host (egui):
//!
//! | Module | Golden Hive counterpart | Role |
//! |---|---|---|
//! | [`context`] | Mvvm (`ApplicationContext`) | App context: services, paths, log, events |
//! | [`i18n`] | `LanguageManager` / `TextKey` | Texts per language with fallback, live switch, placeholders |
//! | [`icons`] | `IconCatalog` | Icon names → glyphs |
//! | [`runtime`] | – | Background tasks (`Promise`) for an immediate-mode UI |
//! | [`commands`] | Commands (`RelayCommandPool`) | Central command manager, command trees, palette search |
//! | [`shortcuts`] | `ShortcutManager` | Layered shortcuts, chords, scopes, conflicts, reserved gestures |
//! | [`navigation`] | Navigation | View registry, MRU, navigation wheel |
//! | [`docking`] | Docking | VS-style dock layout (documents + tool windows), saved layouts |
//! | [`windowing`] | Windowing | Size classes, detached views, window placement |
//! | [`widgets`] | `UiKit` | Command buttons, cards, badges, the hexagon logo |
//!
//! The server parts of the enterprise edition (HTTP client, sign-in, SSE notifications, signed module
//! packages, hot-swap, view packs and workflows) were removed: this edition runs fully offline and hosts
//! one native app, Dragon DAW.

pub mod commands;
pub mod context;
pub mod docking;
pub mod i18n;
pub mod icons;
pub mod navigation;
pub mod runtime;
pub mod shortcuts;
pub mod widgets;
pub mod windowing;

pub use context::AppContext;
pub use redhive_monads::{Error, ErrorKind, Res};

/// Version of the host.
pub const HOST_VERSION: &str = env!("CARGO_PKG_VERSION");
