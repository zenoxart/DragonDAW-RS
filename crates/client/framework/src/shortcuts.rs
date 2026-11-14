//! The shortcut manager.
//!
//! Layers (lowest first): shell defaults → module defaults → preset → user overrides. Every binding points
//! to a command id, so user overrides survive module updates and hot-swaps. Supports single strokes and
//! chords (`Ctrl+K, Ctrl+S`), scopes, conflict detection and reserved shell gestures that modules can never
//! take. Plain keys never fire while a text field has focus.

use crate::commands::CommandPool;
use crate::context::Paths;
use redhive_contracts::commands::{KeyGesture, KeyStroke, Modifiers, ShortcutScope};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// Gestures that belong to the shell.
pub const RESERVED: &[&str] = &["Ctrl+Tab", "Ctrl+Shift+Tab", "Ctrl+Shift+P", "Ctrl+K, Ctrl+S", "Ctrl+/", "F1", "Escape"];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutProfile {
    /// Preset name (`default`, `minimal`).
    #[serde(default = "default_preset")]
    pub preset: String,
    /// Command id → gestures; an empty list removes the default.
    #[serde(default)]
    pub overrides: BTreeMap<String, Vec<String>>,
}

fn default_preset() -> String {
    "default".into()
}

#[derive(Debug, Clone)]
pub struct Binding {
    pub gesture: KeyGesture,
    pub scope: ShortcutScope,
    pub command_id: String,
    pub from_user: bool,
}

#[derive(Debug, Clone)]
pub struct Conflict {
    pub gesture: String,
    pub commands: Vec<String>,
}

pub struct ShortcutManager {
    pub profile: ShortcutProfile,
    pub bindings: Vec<Binding>,
    pub conflicts: Vec<Conflict>,
    built_for: u64,
    pending_chord: Option<(KeyStroke, Instant)>,
    /// Set while the settings page records a gesture.
    pub recording: Option<String>,
    pub recorded: Option<String>,
    paths: Paths,
}

impl ShortcutManager {
    pub fn load(paths: &Paths) -> Self {
        ShortcutManager {
            profile: paths.load_json("Shortcuts.json").unwrap_or_default(),
            bindings: Vec::new(),
            conflicts: Vec::new(),
            built_for: u64::MAX,
            pending_chord: None,
            recording: None,
            recorded: None,
            paths: paths.clone(),
        }
    }

    pub fn save(&self) {
        self.paths.save_json("Shortcuts.json", &self.profile);
    }

    /// Rebuilds the effective table when the command registry changed.
    pub fn rebuild(&mut self, pool: &CommandPool) {
        if self.built_for == pool.revision {
            return;
        }
        self.built_for = pool.revision;
        let reserved: Vec<KeyGesture> = RESERVED.iter().filter_map(|g| KeyGesture::parse(g)).collect();
        let mut bindings = Vec::new();
        for c in pool.all() {
            let id = &c.descriptor.id;
            if let Some(user) = self.profile.overrides.get(id) {
                for g in user.iter().filter_map(|g| KeyGesture::parse(g)) {
                    let scope = c.descriptor.shortcuts.first().map(|s| s.scope).unwrap_or_default();
                    bindings.push(Binding { gesture: g, scope, command_id: id.clone(), from_user: true });
                }
                continue;
            }
            if self.profile.preset == "minimal" && c.owner != "shell" {
                continue;
            }
            for s in &c.descriptor.shortcuts {
                // Modules can never take reserved shell gestures.
                if c.owner != "shell" && reserved.contains(&s.gesture) {
                    tracing::warn!(command = %id, gesture = %s.gesture, "module tried to bind a reserved shortcut – ignored");
                    continue;
                }
                bindings.push(Binding { gesture: s.gesture.clone(), scope: s.scope, command_id: id.clone(), from_user: false });
            }
        }
        // Shell bindings win over module bindings with the same gesture and scope.
        let mut conflicts: BTreeMap<(String, ShortcutScope), Vec<String>> = BTreeMap::new();
        for b in &bindings {
            conflicts.entry((b.gesture.to_string(), b.scope)).or_default().push(b.command_id.clone());
        }
        self.conflicts = conflicts.into_iter().filter(|(_, v)| v.len() > 1).map(|((g, _), commands)| Conflict { gesture: g, commands }).collect();
        bindings.sort_by_key(|b| (!b.command_id.starts_with("shell."), !b.from_user));
        self.bindings = bindings;
    }

    pub fn gestures_of(&self, command_id: &str) -> Vec<&KeyGesture> {
        self.bindings.iter().filter(|b| b.command_id == command_id).map(|b| &b.gesture).collect()
    }

    /// Display text of the first gesture of a command.
    pub fn text(&self, command_id: &str, german: bool) -> Option<String> {
        self.gestures_of(command_id).first().map(|g| g.display(german))
    }

    pub fn set_override(&mut self, command_id: &str, gesture: Option<&str>) {
        match gesture {
            Some(g) => {
                self.profile.overrides.insert(command_id.to_string(), vec![g.to_string()]);
            }
            None => {
                self.profile.overrides.insert(command_id.to_string(), vec![]);
            }
        }
        self.built_for = u64::MAX;
        self.save();
    }

    pub fn reset(&mut self, command_id: Option<&str>) {
        match command_id {
            Some(id) => {
                self.profile.overrides.remove(id);
            }
            None => self.profile.overrides.clear(),
        }
        self.built_for = u64::MAX;
        self.save();
    }

    pub fn set_preset(&mut self, preset: &str) {
        self.profile.preset = preset.to_string();
        self.built_for = u64::MAX;
        self.save();
    }

    /// Whether a gesture is reserved for the shell.
    pub fn is_reserved(gesture: &str) -> bool {
        let g = KeyGesture::parse(gesture);
        RESERVED.iter().any(|r| KeyGesture::parse(r) == g)
    }

    /// Reads this frame's key presses and returns the commands to run.
    /// `text_focus`: a text field has focus (plain keys are ignored then).
    pub fn process(&mut self, ctx: &egui::Context, text_focus: bool, active_scopes: &[ShortcutScope]) -> Vec<String> {
        let strokes: Vec<KeyStroke> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } => Some(stroke(*key, modifiers)),
                    _ => None,
                })
                .collect()
        });
        let mut out = Vec::new();
        for s in strokes {
            if let Some(cmd) = &self.recording {
                if s.key == "Escape" && !s.modifiers.ctrl {
                    self.recording = None;
                    continue;
                }
                if is_modifier_only(&s) {
                    continue;
                }
                // Chord recording: a second stroke within 1.5 s extends the gesture.
                let text = match self.recorded.take() {
                    Some(prev) if KeyGesture::parse(&prev).is_some_and(|g| !g.is_chord()) && self.pending_chord.as_ref().is_some_and(|(_, at)| at.elapsed() < Duration::from_millis(1500)) => format!("{prev}, {}", s.display(false)),
                    _ => s.display(false),
                };
                self.pending_chord = Some((s.clone(), Instant::now()));
                self.recorded = Some(text);
                let _ = cmd;
                continue;
            }
            if text_focus && s.is_plain() {
                continue;
            }
            // Second stroke of a chord?
            if let Some((first, at)) = self.pending_chord.take() {
                if at.elapsed() < Duration::from_millis(1500) {
                    let chord = KeyGesture { strokes: vec![first, s.clone()] };
                    if let Some(b) = self.find(&chord, active_scopes) {
                        out.push(b);
                        continue;
                    }
                }
            }
            let single = KeyGesture { strokes: vec![s.clone()] };
            if let Some(b) = self.find(&single, active_scopes) {
                out.push(b);
                continue;
            }
            // First stroke of some chord: wait for the second.
            if self.bindings.iter().any(|b| b.gesture.is_chord() && b.gesture.strokes[0] == s) {
                self.pending_chord = Some((s, Instant::now()));
            }
        }
        out
    }

    fn find(&self, g: &KeyGesture, scopes: &[ShortcutScope]) -> Option<String> {
        // Inner scopes first.
        let mut candidates: Vec<&Binding> = self.bindings.iter().filter(|b| &b.gesture == g && (scopes.contains(&b.scope) || b.scope == ShortcutScope::Global || b.scope == ShortcutScope::Window)).collect();
        candidates.sort_by_key(|b| b.scope);
        candidates.first().map(|b| b.command_id.clone())
    }

    /// Whether a chord's first stroke is waiting.
    pub fn chord_pending(&self) -> Option<String> {
        self.pending_chord.as_ref().filter(|(_, at)| at.elapsed() < Duration::from_millis(1500)).map(|(s, _)| s.display(false))
    }
}

fn is_modifier_only(s: &KeyStroke) -> bool {
    s.key.is_empty()
}

pub fn stroke(key: egui::Key, m: &egui::Modifiers) -> KeyStroke {
    let name = match key {
        egui::Key::Slash => "Slash".to_string(),
        egui::Key::Minus => "Minus".to_string(),
        egui::Key::Plus | egui::Key::Equals => "Plus".to_string(),
        egui::Key::Escape => "Escape".to_string(),
        egui::Key::Enter => "Enter".to_string(),
        other => other.name().to_string(),
    };
    KeyStroke { modifiers: Modifiers { ctrl: m.ctrl || m.command, shift: m.shift, alt: m.alt }, key: name }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::CommandHandler;
    use redhive_contracts::commands::{CommandDescriptor, ShortcutBinding};
    use redhive_contracts::{IconRef, TextKey};

    fn d(id: &str, g: &str) -> CommandDescriptor {
        CommandDescriptor {
            id: id.into(),
            label: TextKey::new("x:y"),
            icon: IconRef::default(),
            permission: String::new(),
            placements: vec![],
            shortcuts: vec![ShortcutBinding { gesture: KeyGesture::parse(g).unwrap(), scope: ShortcutScope::Global }],
            workflow_capable: false,
            ai_allowed: false,
            module_id: String::new(),
        }
    }

    #[test]
    fn reserved_conflicts_and_overrides() {
        let dir = std::env::temp_dir().join(format!("rh-sc-{}", std::process::id()));
        let paths = Paths { config: dir.clone(), local: dir.clone(), modules: dir.clone() };
        let _ = std::fs::create_dir_all(&dir);
        let mut pool = CommandPool::default();
        pool.register("shell", d("shell.palette", "Ctrl+Shift+P"), CommandHandler::Shell);
        pool.register("docs", d("docs.steal", "Ctrl+Shift+P"), CommandHandler::Shell);
        pool.register("docs", d("docs.a", "Ctrl+Shift+N"), CommandHandler::Shell);
        pool.register("reports", d("reports.b", "Ctrl+Shift+N"), CommandHandler::Shell);
        let mut m = ShortcutManager::load(&paths);
        m.rebuild(&pool);
        assert!(m.gestures_of("docs.steal").is_empty(), "reserved gesture refused");
        assert_eq!(m.conflicts.len(), 1);
        m.set_override("reports.b", Some("Ctrl+K, Ctrl+R"));
        m.rebuild(&pool);
        assert!(m.conflicts.is_empty());
        assert_eq!(m.text("reports.b", true).unwrap(), "Strg+K, Strg+R");
        assert!(ShortcutManager::is_reserved("ctrl+shift+p"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
