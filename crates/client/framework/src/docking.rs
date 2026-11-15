//! Visual Studio-style dock layout (egui_dock): a document well with tab groups, tool windows docked
//! left/right/bottom, floating windows, saved and named layouts.

use crate::context::Paths;
use egui_dock::{DockState, NodeIndex, SurfaceIndex};
use redhive_contracts::ui::{DockSide, ViewRole};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One tab: a view key plus an optional parameter (e.g. the selected entity).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Tab {
    pub key: String,
    #[serde(default)]
    pub parameter: Option<String>,
}

impl Tab {
    pub fn new(key: &str) -> Self {
        Tab { key: key.to_string(), parameter: None }
    }
}

pub struct DockManager {
    pub state: DockState<Tab>,
    paths: Paths,
    pub named: BTreeMap<String, DockState<Tab>>,
}

/// The default layout of Dragon DAW: browser left; playlist, mixer, mastering, channel rack and piano roll as
/// tabs of the document well.
pub fn default_layout() -> DockState<Tab> {
    let mut state = DockState::new(vec![Tab::new("dragon.playlist"), Tab::new("dragon.mixer"), Tab::new("dragon.master"), Tab::new("dragon.rack"), Tab::new("dragon.piano")]);
    let surface = state.main_surface_mut();
    let [_docs, _browser] = surface.split_left(NodeIndex::root(), 0.17, vec![Tab::new("dragon.browser")]);
    state
}

/// Bumped when the default layout changes; a saved layout from an older version is replaced once.
pub const LAYOUT_VERSION: u32 = 2;

impl DockManager {
    pub fn load(paths: &Paths) -> Self {
        let dir = paths.local.join("Layout");
        let version: u32 = paths.load_from(&dir.join("LayoutVersion.json")).unwrap_or(1);
        let saved = if version >= LAYOUT_VERSION { paths.load_from(&dir.join("DockLayout.json")) } else { None };
        let state = saved.unwrap_or_else(default_layout);
        paths.save_to(&dir.join("LayoutVersion.json"), &LAYOUT_VERSION);
        let named = paths.load_from(&paths.local.join("Layout").join("NamedLayouts.json")).unwrap_or_default();
        DockManager { state, paths: paths.clone(), named }
    }

    pub fn save(&self) {
        self.paths.save_to(&self.paths.local.join("Layout").join("DockLayout.json"), &self.state);
    }

    pub fn save_named(&mut self, name: &str) {
        self.named.insert(name.to_string(), self.state.clone());
        self.paths.save_to(&self.paths.local.join("Layout").join("NamedLayouts.json"), &self.named);
    }

    pub fn apply_named(&mut self, name: &str) {
        if let Some(s) = self.named.get(name) {
            self.state = s.clone();
        }
    }

    pub fn reset(&mut self) {
        self.state = default_layout();
    }

    pub fn find(&self, key: &str) -> Option<(SurfaceIndex, NodeIndex, egui_dock::TabIndex)> {
        let tab = self.state.iter_all_tabs().find(|(_, t)| t.key == key).map(|(_, t)| t.clone())?;
        self.state.find_tab(&tab)
    }

    pub fn is_open(&self, key: &str) -> bool {
        self.find(key).is_some()
    }

    /// Opens a view: activates it when it is already open, otherwise docks it by its role.
    pub fn open(&mut self, key: &str, parameter: Option<String>, role: ViewRole, side: DockSide, tool_sides: &dyn Fn(&str) -> Option<DockSide>) {
        if let Some(found) = self.find(key) {
            if let Some(p) = parameter {
                if let Some((_, tab)) = self.state.iter_all_tabs_mut().find(|(_, t)| t.key == key) {
                    tab.parameter = Some(p);
                }
            }
            self.state.set_active_tab(found);
            self.state.set_focused_node_and_surface((found.0, found.1));
            return;
        }
        let tab = Tab { key: key.to_string(), parameter };
        match role {
            ViewRole::Document => {
                // The document well is the leaf that holds documents (any tab whose key is not a tool window).
                let leaf = self.state.iter_all_tabs().find(|(_, t)| tool_sides(&t.key).is_none()).map(|(loc, _)| loc);
                match leaf {
                    Some((s, n)) => {
                        self.state.set_focused_node_and_surface((s, n));
                        self.state.push_to_focused_leaf(tab);
                    }
                    None => self.state.push_to_first_leaf(tab),
                }
            }
            ViewRole::ToolWindow => {
                let leaf = self.state.iter_all_tabs().find(|(_, t)| tool_sides(&t.key) == Some(side)).map(|(loc, _)| loc);
                match leaf {
                    Some((s, n)) => {
                        self.state.set_focused_node_and_surface((s, n));
                        self.state.push_to_focused_leaf(tab);
                    }
                    None => {
                        let surface = self.state.main_surface_mut();
                        match side {
                            DockSide::Left => {
                                surface.split_left(NodeIndex::root(), 0.2, vec![tab]);
                            }
                            DockSide::Right => {
                                surface.split_right(NodeIndex::root(), 0.78, vec![tab]);
                            }
                            DockSide::Bottom => {
                                surface.split_below(NodeIndex::root(), 0.75, vec![tab]);
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn close(&mut self, key: &str) {
        if let Some(found) = self.find(key) {
            let _ = self.state.remove_tab(found);
        }
    }

    /// The active tab of the focused leaf.
    pub fn focused(&mut self) -> Option<String> {
        self.state.find_active_focused().map(|(_, t)| t.key.clone())
    }

    /// Cycles the active tab of the focused leaf.
    pub fn cycle(&mut self, forward: bool) {
        let Some((surface, node)) = self.state.focused_leaf() else { return };
        if let egui_dock::Node::Leaf { tabs, active, .. } = &mut self.state[surface][node] {
            if tabs.is_empty() {
                return;
            }
            let n = tabs.len();
            active.0 = if forward { (active.0 + 1) % n } else { (active.0 + n - 1) % n };
        }
    }

    pub fn open_keys(&self) -> Vec<String> {
        self.state.iter_all_tabs().map(|(_, t)| t.key.clone()).collect()
    }
}
