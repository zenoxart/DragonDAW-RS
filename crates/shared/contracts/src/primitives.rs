//! Basic value types.

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// A translatable text: `<scope>:<key>`, e.g. `documents:invoice.approve`.
///
/// Commands need one key; `.label`, `.tooltip`, `.description` and `.disabled.<reason>` follow by convention.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct TextKey(pub String);

impl TextKey {
    pub fn new(key: impl Into<String>) -> Self {
        TextKey(key.into())
    }

    /// The scope part (`documents` in `documents:invoice.approve`); empty when there is no scope.
    pub fn scope(&self) -> &str {
        self.0.split_once(':').map(|(s, _)| s).unwrap_or("")
    }

    /// The key part (`invoice.approve`).
    pub fn key(&self) -> &str {
        self.0.split_once(':').map(|(_, k)| k).unwrap_or(&self.0)
    }

    /// Whether the value looks like a text key (otherwise it is literal text).
    pub fn is_key(s: &str) -> bool {
        match s.split_once(':') {
            Some((scope, key)) => !scope.is_empty() && !key.is_empty() && !key.contains(' ') && scope.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            None => false,
        }
    }

    /// Derived key with a suffix (`label`, `tooltip`, …).
    pub fn child(&self, suffix: &str) -> TextKey {
        TextKey(format!("{}.{}", self.0, suffix))
    }
}

impl fmt::Display for TextKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for TextKey {
    fn from(s: &str) -> Self {
        TextKey(s.to_owned())
    }
}

/// An icon by symbolic name: `save`, `documents.invoice`, `glyph:E74E`, `emoji:📄`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct IconRef(pub String);

impl IconRef {
    pub fn new(name: impl Into<String>) -> Self {
        IconRef(name.into())
    }
}

impl From<&str> for IconRef {
    fn from(s: &str) -> Self {
        IconRef(s.to_owned())
    }
}

/// Well-known ids of the demo tenant and system actors.
pub mod ids {
    use uuid::Uuid;

    /// The demo tenant ("Red Hive Demo GmbH").
    pub const DEMO_TENANT: Uuid = Uuid::from_u128(0x00000000_0000_0000_0000_000000000001);
    /// The system user the workflow engine uses for its own bookkeeping.
    pub const SYSTEM_USER: Uuid = Uuid::from_u128(0x00000000_0000_0000_0000_0000000000ff);
}

/// One page of a list.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub skip: i64,
    pub take: i64,
}

/// Request for an id only.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdResponse {
    pub id: Uuid,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_key_parts() {
        let k = TextKey::new("documents:invoice.approve");
        assert_eq!(k.scope(), "documents");
        assert_eq!(k.key(), "invoice.approve");
        assert!(TextKey::is_key("shell:common.save"));
        assert!(!TextKey::is_key("Hello: world"));
        assert_eq!(k.child("tooltip").0, "documents:invoice.approve.tooltip");
    }
}
