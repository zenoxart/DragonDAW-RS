//! # Red Hive contracts
//!
//! Wire types compiled into both the desktop client and every backend service, so there is exactly one
//! wire format (JSON, camelCase).
//!
//! | Module | Content |
//! |---|---|
//! | [`primitives`] | `TextKey`, `IconRef`, well-known ids, paging |
//! | [`api`] | Problem details (`ApiError`), identity/profile/settings DTOs |
//! | [`notifications`] | Notification envelope, audience, event type names |
//! | [`commands`] | Command descriptors, placements, key gestures, command trees |
//! | [`modules`] | Module manifest, contract surface + diff, compatibility rules, registry DTOs |
//! | [`viewpack`] | Server-driven module UI (views, columns, forms, actions) rendered by the host |
//! | [`workflows`] | Definitions, steps, triggers, AI policy, expression language, input schemas, analysis, impact |

pub mod api;
pub mod commands;
pub mod modules;
pub mod notifications;
pub mod primitives;
pub mod viewpack;
pub mod workflows;

pub use primitives::{IconRef, TextKey};
