//! UI contracts shared by the framework and the app: where a view docks, the severity of a message and
//! the permission check used by commands and views. (The server-driven view packs of the enterprise
//! edition were removed: Dragon DAW is a native, offline app.)

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ViewRole {
    /// Opens as a tab in the document well.
    #[default]
    Document,
    /// Docks at a side of the window.
    ToolWindow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum DockSide {
    Left,
    #[default]
    Right,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Success => "success",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

/// `true` when `permission` is empty or granted (exact, `*` or a `prefix.*` wildcard).
/// The offline app grants `*`; the check stays so command trees keep working unchanged.
pub fn has_permission(granted: &[String], permission: &str) -> bool {
    permission.is_empty() || granted.iter().any(|g| g == "*" || g == permission || (g.ends_with(".*") && permission.starts_with(&g[..g.len() - 1])))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn permissions() {
        assert!(has_permission(&[], ""));
        assert!(has_permission(&["*".into()], "mixer.edit"));
        assert!(has_permission(&["mixer.*".into()], "mixer.edit"));
        assert!(!has_permission(&["mixer.view".into()], "mixer.edit"));
    }
}
