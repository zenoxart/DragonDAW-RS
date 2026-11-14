//! # Red Hive contracts (offline edition)
//!
//! Plain data types shared by the client framework and the Dragon DAW app.
//!
//! | Module | Content |
//! |---|---|
//! | [`primitives`] | `TextKey`, `IconRef`, well-known ids |
//! | [`commands`] | Command descriptors, placements, key gestures, command trees |
//! | [`ui`] | Dock roles and sides, severities, permission check |

pub mod commands;
pub mod primitives;
pub mod ui;

pub use primitives::{IconRef, TextKey};
