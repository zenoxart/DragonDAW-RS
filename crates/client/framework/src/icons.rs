//! Icons by symbolic name. Shell icons are built in; modules add their own (`documents.invoice`) from the
//! view pack. Names resolve to a glyph (emoji or symbol); `glyph:E74E` / `emoji:📄` are accepted directly.

use std::collections::HashMap;

pub struct IconCatalog {
    icons: HashMap<String, String>,
    owners: HashMap<String, String>,
}

const SHELL: &[(&str, &str)] = &[
    ("save", "💾"),
    ("refresh", "⟳"),
    ("search", "🔍"),
    ("close", "✖"),
    ("settings", "⚙"),
    ("profile", "👤"),
    ("signout", "🚪"),
    ("exit", "🚪"),
    ("palette", "⌘"),
    ("notifications", "🔔"),
    ("output", "📃"),
    ("start", "🏠"),
    ("modules", "📦"),
    ("inbox", "📥"),
    ("workflows", "🔀"),
    ("designer", "✏"),
    ("impact", "🛡"),
    ("ai", "💡"),
    ("record", "⏺"),
    ("stop", "⏹"),
    ("detach", "↗"),
    ("attach", "↙"),
    ("keyboard", "⌨"),
    ("help", "❓"),
    ("info", "ℹ"),
    ("success", "✔"),
    ("warning", "⚠"),
    ("error", "⛔"),
    ("add", "➕"),
    ("edit", "✏"),
    ("delete", "🗑"),
    ("play", "▶"),
    ("pause", "⏸"),
    ("layout", "⊞"),
    ("wheel", "◎"),
    ("navigation", "☰"),
    ("hive", "⬣"),
];

impl Default for IconCatalog {
    fn default() -> Self {
        IconCatalog { icons: SHELL.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(), owners: HashMap::new() }
    }
}

impl IconCatalog {
    /// Registers module icons (`home` → `documents.home`).
    pub fn register_module(&mut self, module_id: &str, prefix: &str, icons: &std::collections::BTreeMap<String, String>) {
        for (k, v) in icons {
            let name = format!("{prefix}.{k}");
            self.icons.insert(name.clone(), v.clone());
            self.owners.insert(name, module_id.to_string());
        }
    }

    pub fn unregister_module(&mut self, module_id: &str) {
        let names: Vec<String> = self.owners.iter().filter(|(_, o)| o.as_str() == module_id).map(|(k, _)| k.clone()).collect();
        for n in names {
            self.owners.remove(&n);
            self.icons.remove(&n);
        }
    }

    /// The glyph of an icon name ("" when unknown).
    pub fn glyph(&self, name: &str) -> String {
        if let Some(code) = name.strip_prefix("glyph:") {
            return u32::from_str_radix(code, 16).ok().and_then(char::from_u32).map(String::from).unwrap_or_default();
        }
        if let Some(e) = name.strip_prefix("emoji:") {
            return e.to_string();
        }
        self.icons.get(name).cloned().unwrap_or_default()
    }

    /// `"<glyph> <text>"` (or just the text).
    pub fn label(&self, icon: &str, text: &str) -> String {
        let g = self.glyph(icon);
        if g.is_empty() {
            text.to_string()
        } else {
            format!("{g}  {text}")
        }
    }

    pub fn known(&self, name: &str) -> bool {
        name.starts_with("glyph:") || name.starts_with("emoji:") || self.icons.contains_key(name)
    }
}

#[cfg(test)]
mod glyph_tests {
    /// Every icon must be drawable with egui's bundled fonts (no tofu boxes on a fresh Windows install).
    #[test]
    fn all_icons_have_glyphs() {
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |_| {});
        let names: Vec<(String, String)> = super::SHELL.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let font = egui::FontId::proportional(14.0);
        let missing: Vec<String> = names
            .iter()
            .filter(|(_, g)| ctx.fonts(|f| g.chars().any(|c| !f.has_glyph(&font, c))))
            .map(|(k, g)| format!("{k}={g}"))
            .collect();
        assert!(missing.is_empty(), "icons without glyph: {missing:?}");
        // every non-ASCII character in the UI sources and text tables
        let mut chars = std::collections::BTreeSet::new();
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = vec![];
        for dir in [base.join("src"), base.join("src/widgets"), base.join("../../app/src"), base.join("../../app/src/views")] {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                if e.path().extension().is_some_and(|x| x == "rs") {
                    files.push(e.path());
                }
            }
        }
        for f in files {
            for line in std::fs::read_to_string(&f).unwrap().lines() {
                if line.trim_start().starts_with("//") {
                    continue;
                }
                chars.extend(line.chars().filter(|c| !c.is_ascii()));
            }
        }
        let tofu: String = chars.into_iter().filter(|c| ctx.fonts(|f| !f.has_glyph(&font, *c))).collect();
        assert!(tofu.is_empty(), "characters without glyph: {tofu}");
    }
}
