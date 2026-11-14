//! Responsive layout and detachable views.
//!
//! * Size classes (`Compact` < 640, `Medium` < 1008, `Expanded` < 1920, `Wide`) are computed per view from the
//!   width it actually gets, with hysteresis so a view does not flicker at a boundary.
//! * Any view can be torn off into its own OS window (Ctrl+Shift+D) and attached back (Ctrl+Shift+A). The view
//!   keeps its state. Positions are saved per view in local app data, relative to the monitor; a window is
//!   never restored off-screen.

use crate::context::Paths;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub enum SizeClass {
    Compact,
    Medium,
    #[default]
    Expanded,
    Wide,
}

impl SizeClass {
    const BOUNDS: [f32; 3] = [640.0, 1008.0, 1920.0];

    pub fn of(width: f32) -> Self {
        match width {
            w if w < Self::BOUNDS[0] => SizeClass::Compact,
            w if w < Self::BOUNDS[1] => SizeClass::Medium,
            w if w < Self::BOUNDS[2] => SizeClass::Expanded,
            _ => SizeClass::Wide,
        }
    }

    /// Like [`SizeClass::of`] but stays in `previous` while within 16 px of a boundary.
    pub fn with_hysteresis(width: f32, previous: SizeClass) -> Self {
        let next = Self::of(width);
        if next == previous {
            return next;
        }
        let near = Self::BOUNDS.iter().any(|b| (width - b).abs() < 16.0);
        if near {
            previous
        } else {
            next
        }
    }

    /// Columns for forms and card grids.
    pub fn columns(self) -> usize {
        match self {
            SizeClass::Compact => 1,
            SizeClass::Medium => 2,
            SizeClass::Expanded => 2,
            SizeClass::Wide => 3,
        }
    }

    pub fn text_key(self) -> &'static str {
        match self {
            SizeClass::Compact => "shell:size.compact",
            SizeClass::Medium => "shell:size.medium",
            SizeClass::Expanded => "shell:size.expanded",
            SizeClass::Wide => "shell:size.wide",
        }
    }
}

/// Saved placement of a window, relative to its monitor.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WindowPlacement {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Size of the monitor it was on (identifies the monitor setup).
    pub monitor_width: f32,
    pub monitor_height: f32,
    #[serde(default)]
    pub maximized: bool,
}

impl WindowPlacement {
    /// Clamps the placement into the given monitor (rescaled when the monitor differs).
    pub fn fit(&self, monitor: egui::Vec2) -> WindowPlacement {
        let mut p = *self;
        if (self.monitor_width - monitor.x).abs() > 1.0 || (self.monitor_height - monitor.y).abs() > 1.0 {
            let sx = monitor.x / self.monitor_width.max(1.0);
            let sy = monitor.y / self.monitor_height.max(1.0);
            p.x *= sx;
            p.y *= sy;
        }
        p.width = p.width.clamp(320.0, monitor.x.max(320.0));
        p.height = p.height.clamp(240.0, monitor.y.max(240.0));
        p.x = p.x.clamp(0.0, (monitor.x - p.width).max(0.0));
        p.y = p.y.clamp(0.0, (monitor.y - p.height).max(0.0));
        p.monitor_width = monitor.x;
        p.monitor_height = monitor.y;
        p
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowLayout {
    /// View key → placement of its detached window.
    pub detached: BTreeMap<String, WindowPlacement>,
    /// Views that were detached when the app closed (reopened after the modules are loaded).
    #[serde(default)]
    pub open: Vec<String>,
}

pub struct WindowManager {
    pub layout: WindowLayout,
    paths: Paths,
    dirty_since: Option<std::time::Instant>,
}

impl WindowManager {
    pub fn load(paths: &Paths) -> Self {
        WindowManager { layout: paths.load_from(&paths.local.join("Layout").join("WindowLayout.json")).unwrap_or_default(), paths: paths.clone(), dirty_since: None }
    }

    pub fn save(&mut self) {
        self.paths.save_to(&self.paths.local.join("Layout").join("WindowLayout.json"), &self.layout);
        self.dirty_since = None;
    }

    /// Remembers a moved/resized window; saved debounced.
    pub fn moved(&mut self, key: &str, p: WindowPlacement) {
        if self.layout.detached.get(key) != Some(&p) {
            self.layout.detached.insert(key.to_string(), p);
            self.dirty_since.get_or_insert_with(std::time::Instant::now);
        }
    }

    pub fn tick(&mut self) {
        if self.dirty_since.is_some_and(|t| t.elapsed() > std::time::Duration::from_millis(800)) {
            self.save();
        }
    }

    pub fn is_detached(&self, key: &str) -> bool {
        self.layout.open.iter().any(|k| k == key)
    }

    pub fn detach(&mut self, key: &str) {
        if !self.is_detached(key) {
            self.layout.open.push(key.to_string());
            self.save();
        }
    }

    pub fn attach(&mut self, key: &str) {
        self.layout.open.retain(|k| k != key);
        self.save();
    }

    pub fn placement(&self, key: &str, monitor: egui::Vec2) -> Option<WindowPlacement> {
        self.layout.detached.get(key).map(|p| p.fit(monitor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_classes() {
        assert_eq!(SizeClass::of(500.0), SizeClass::Compact);
        assert_eq!(SizeClass::of(900.0), SizeClass::Medium);
        assert_eq!(SizeClass::of(2000.0), SizeClass::Wide);
        assert_eq!(SizeClass::with_hysteresis(645.0, SizeClass::Compact), SizeClass::Compact);
        assert_eq!(SizeClass::with_hysteresis(700.0, SizeClass::Compact), SizeClass::Medium);
    }

    #[test]
    fn placement_is_clamped() {
        let p = WindowPlacement { x: 3000.0, y: 100.0, width: 800.0, height: 600.0, monitor_width: 3840.0, monitor_height: 2160.0, maximized: false };
        let f = p.fit(egui::vec2(1920.0, 1080.0));
        assert!(f.x + f.width <= 1920.0 && f.y + f.height <= 1080.0);
    }
}
