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
//! | [`api`] | `BackendConnection` | HTTP client, problem details → `Error` |
//! | [`security`] | Security | Session, token store, connection settings, pinned signing keys |
//! | [`commands`] | Commands (`RelayCommandPool`) | Central command manager, command trees, palette search |
//! | [`shortcuts`] | `ShortcutManager` | Layered shortcuts, chords, scopes, conflicts, reserved gestures |
//! | [`navigation`] | Navigation | View registry, MRU, navigation wheel |
//! | [`docking`] | Docking | VS-style dock layout (documents + tool windows), saved layouts |
//! | [`windowing`] | Windowing | Size classes, detached views, window placement |
//! | [`notifications`] | NotificationClient | SSE with catch-up, notification centre, toasts |
//! | [`modules`] | ModuleLoader | Catalogue, download, hash + signature, dependencies, view packs |
//! | [`hotswap`] | HotSwap | Quiesce → swap → rebuild, rollback |
//! | [`views`] | Module SDK (`UiKit`, `DataList`, `InputForm`, `SimpleChart`) | Renders view packs |
//! | [`workflows`] | Workflows | Inbox, designer, recorder, impact overview, AI panel |

pub mod api;
pub mod commands;
pub mod context;
pub mod docking;
pub mod hotswap;
pub mod i18n;
pub mod icons;
pub mod modules;
pub mod navigation;
pub mod notifications;
pub mod runtime;
pub mod security;
pub mod shortcuts;
pub mod views;
pub mod widgets;
pub mod windowing;
pub mod workflows;

pub use context::AppContext;
pub use redhive_monads::{Error, ErrorKind, Res};

/// Version of the host (checked against `minHostVersion` of module manifests).
pub const HOST_VERSION: &str = env!("CARGO_PKG_VERSION");
