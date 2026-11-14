//! Commands: descriptors, placements, key gestures and command trees.
//!
//! Every user action is a command with a stable id `<module>.<action>`. Menus, the navigation tree, the
//! navigation wheel and the command palette are generated from the commands' placements – never built by hand.

use crate::primitives::{IconRef, TextKey};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Well-known placements (menus).
pub mod placements {
    pub const MAIN_MENU: &str = "shell.mainmenu";
    pub const NAVIGATION: &str = "shell.navigation";
    pub const WHEEL: &str = "shell.wheel";
    pub const TOOLBAR: &str = "shell.toolbar";
    pub const PROFILE: &str = "shell.profile";
}

/// Where a command appears: menu + path inside it + section + order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandPlacement {
    /// Menu id, e.g. `shell.mainmenu`.
    pub menu: String,
    /// Group path inside the menu, `/`-separated (`file/export`); empty = top level.
    #[serde(default)]
    pub path: String,
    /// Section inside the group; separators are inserted between sections.
    #[serde(default)]
    pub section: i32,
    #[serde(default)]
    pub order: i32,
}

impl CommandPlacement {
    pub fn new(menu: &str, path: &str, section: i32, order: i32) -> Self {
        CommandPlacement { menu: menu.into(), path: path.into(), section, order }
    }
}

/// Scope in which a shortcut is active; inner scopes win.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ShortcutScope {
    Focus,
    View,
    Module,
    Window,
    #[default]
    Global,
}

/// Modifier set of one key stroke.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Modifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// One key stroke: modifiers + a key name (`P`, `F1`, `Tab`, `Enter`, `Slash`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeyStroke {
    pub modifiers: Modifiers,
    pub key: String,
}

impl KeyStroke {
    pub fn parse(text: &str) -> Option<KeyStroke> {
        let mut modifiers = Modifiers::default();
        let mut key = None;
        for part in text.split('+').map(str::trim).filter(|p| !p.is_empty()) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "strg" => modifiers.ctrl = true,
                "shift" | "umschalt" => modifiers.shift = true,
                "alt" => modifiers.alt = true,
                _ => key = Some(normalize_key(part)),
            }
        }
        // "Ctrl++" / "Ctrl+-" style keys
        if key.is_none() && text.ends_with("++") {
            key = Some("Plus".into());
        }
        key.map(|key| KeyStroke { modifiers, key })
    }

    /// Plain keys (no Ctrl/Alt, not a function key) are suppressed while a text field has focus.
    pub fn is_plain(&self) -> bool {
        !self.modifiers.ctrl && !self.modifiers.alt && !(self.key.starts_with('F') && self.key.len() > 1 && self.key[1..].parse::<u8>().is_ok()) && self.key != "Escape"
    }

    /// Display text; `german` uses the German key names (Strg, Umschalt).
    pub fn display(&self, german: bool) -> String {
        let mut parts = Vec::new();
        if self.modifiers.ctrl {
            parts.push(if german { "Strg" } else { "Ctrl" }.to_string());
        }
        if self.modifiers.shift {
            parts.push(if german { "Umschalt" } else { "Shift" }.to_string());
        }
        if self.modifiers.alt {
            parts.push("Alt".into());
        }
        parts.push(match self.key.as_str() {
            "Slash" => "/".into(),
            "Minus" => "-".into(),
            "Plus" => "+".into(),
            "Escape" => "Esc".into(),
            k => k.to_string(),
        });
        parts.join("+")
    }
}

fn normalize_key(part: &str) -> String {
    match part.to_ascii_lowercase().as_str() {
        "/" | "slash" => "Slash".into(),
        "-" | "minus" => "Minus".into(),
        "esc" | "escape" => "Escape".into(),
        "enter" | "return" => "Enter".into(),
        "tab" => "Tab".into(),
        "space" => "Space".into(),
        "del" | "delete" => "Delete".into(),
        p if p.len() == 1 => p.to_ascii_uppercase(),
        p if p.starts_with('f') && p[1..].parse::<u8>().is_ok() => p.to_ascii_uppercase(),
        _ => {
            let mut c = part.chars();
            match c.next() {
                Some(f) => f.to_ascii_uppercase().to_string() + c.as_str(),
                None => String::new(),
            }
        }
    }
}

/// A shortcut: one stroke (`Ctrl+S`) or a chord (`Ctrl+K, Ctrl+S`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct KeyGesture {
    pub strokes: Vec<KeyStroke>,
}

impl KeyGesture {
    pub fn parse(text: &str) -> Option<KeyGesture> {
        let strokes: Option<Vec<_>> = text.split(',').map(str::trim).filter(|s| !s.is_empty()).map(KeyStroke::parse).collect();
        strokes.filter(|s| !s.is_empty() && s.len() <= 2).map(|strokes| KeyGesture { strokes })
    }

    pub fn is_chord(&self) -> bool {
        self.strokes.len() > 1
    }

    pub fn display(&self, german: bool) -> String {
        self.strokes.iter().map(|s| s.display(german)).collect::<Vec<_>>().join(", ")
    }
}

impl fmt::Display for KeyGesture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display(false))
    }
}

impl TryFrom<String> for KeyGesture {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        KeyGesture::parse(&s).ok_or_else(|| format!("invalid key gesture '{s}'"))
    }
}

impl From<KeyGesture> for String {
    fn from(g: KeyGesture) -> String {
        g.display(false)
    }
}

/// A default shortcut declared by a command.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutBinding {
    pub gesture: KeyGesture,
    #[serde(default)]
    pub scope: ShortcutScope,
}

/// Everything the shell needs to know about a command.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandDescriptor {
    /// `<module>.<action>`; stable across versions.
    pub id: String,
    /// Label text key; `.tooltip`, `.description` follow by convention.
    pub label: TextKey,
    #[serde(default)]
    pub icon: IconRef,
    /// Required permission (empty = everyone).
    #[serde(default)]
    pub permission: String,
    #[serde(default)]
    pub placements: Vec<CommandPlacement>,
    #[serde(default)]
    pub shortcuts: Vec<ShortcutBinding>,
    /// Usable as a workflow step / command trigger.
    #[serde(default)]
    pub workflow_capable: bool,
    /// The AI agent may call it.
    #[serde(default)]
    pub ai_allowed: bool,
    /// Owning module (`shell` for host commands).
    #[serde(default)]
    pub module_id: String,
}

/// A node of a generated command tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "nodeKind")]
pub enum CommandTreeNode {
    Group { id: String, label: TextKey, icon: IconRef, children: Vec<CommandTreeNode> },
    Command { id: String, command_id: String, label: TextKey, icon: IconRef, shortcut: Option<String> },
    Separator { id: String },
}

impl CommandTreeNode {
    pub fn id(&self) -> &str {
        match self {
            CommandTreeNode::Group { id, .. } | CommandTreeNode::Command { id, .. } | CommandTreeNode::Separator { id } => id,
        }
    }
}

/// Menu group declared by a module or the shell (gives a path segment its label, icon and order).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandGroupDescriptor {
    pub menu: String,
    pub path: String,
    pub label: TextKey,
    #[serde(default)]
    pub icon: IconRef,
    #[serde(default)]
    pub order: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gesture_parse_and_display() {
        let g = KeyGesture::parse("Ctrl+Shift+P").unwrap();
        assert_eq!(g.display(false), "Ctrl+Shift+P");
        assert_eq!(g.display(true), "Strg+Umschalt+P");
        let chord = KeyGesture::parse("ctrl+k, ctrl+s").unwrap();
        assert!(chord.is_chord());
        assert_eq!(chord.to_string(), "Ctrl+K, Ctrl+S");
        assert_eq!(KeyGesture::parse("Ctrl+/").unwrap().display(false), "Ctrl+/");
        assert!(KeyGesture::parse("").is_none());
        assert!(!KeyStroke::parse("F1").unwrap().is_plain());
        assert!(KeyStroke::parse("A").unwrap().is_plain());
    }

    #[test]
    fn gesture_serde() {
        let b: ShortcutBinding = serde_json::from_str(r#"{"gesture":"Ctrl+Shift+A","scope":"view"}"#).unwrap();
        assert_eq!(b.scope, ShortcutScope::View);
        assert_eq!(serde_json::to_string(&b.gesture).unwrap(), "\"Ctrl+Shift+A\"");
    }
}
