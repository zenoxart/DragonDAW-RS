//! Navigation: the view registry, the most-recently-used module list and the navigation wheel
//! (hold **Ctrl+Tab**: inner ring = modules, outer ring = their views, Alt+Tab semantics).

use redhive_contracts::ui::has_permission;
use redhive_contracts::ui::{DockSide, ViewRole};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub enum ViewSource {
    /// Built into the shell (output, settings, …).
    Shell,
    /// A Dragon DAW editor (playlist, mixer, …).
    App,
}

#[derive(Debug, Clone)]
pub struct ViewEntry {
    pub key: String,
    pub title: String,
    pub icon: String,
    pub role: ViewRole,
    pub dock: DockSide,
    pub home: bool,
    pub permission: String,
    pub order: i32,
    /// Module id (or `shell`).
    pub owner: String,
    pub source: ViewSource,
}

#[derive(Default)]
pub struct NavigationRegistry {
    views: BTreeMap<String, ViewEntry>,
    /// Owners, most recently used first.
    pub mru: Vec<String>,
    pub revision: u64,
}

impl NavigationRegistry {
    pub fn register(&mut self, entry: ViewEntry) {
        self.views.insert(entry.key.clone(), entry);
        self.revision += 1;
    }

    pub fn unregister_owner(&mut self, owner: &str) {
        self.views.retain(|_, v| v.owner != owner);
        self.revision += 1;
    }

    pub fn get(&self, key: &str) -> Option<&ViewEntry> {
        self.views.get(key)
    }

    pub fn all(&self) -> impl Iterator<Item = &ViewEntry> {
        self.views.values()
    }

    /// Records that a module (owner) was used.
    pub fn touch(&mut self, owner: &str) {
        self.mru.retain(|o| o != owner);
        self.mru.insert(0, owner.to_string());
        self.mru.truncate(50);
    }

    /// Home views of the modules the user may see, most recently used first (inner ring of the wheel).
    pub fn homes(&self, permissions: &[String]) -> Vec<&ViewEntry> {
        let mut homes: Vec<&ViewEntry> = self.views.values().filter(|v| v.home && has_permission(permissions, &v.permission)).collect();
        homes.sort_by_key(|v| (self.mru.iter().position(|o| o == &v.owner).unwrap_or(usize::MAX), v.order, v.key.clone()));
        homes
    }

    /// All views of an owner (outer ring).
    pub fn views_of(&self, owner: &str, permissions: &[String]) -> Vec<&ViewEntry> {
        let mut v: Vec<&ViewEntry> = self.views.values().filter(|v| v.owner == owner && has_permission(permissions, &v.permission)).collect();
        v.sort_by_key(|v| (v.order, v.key.clone()));
        v
    }
}

/// The radial navigation wheel.
#[derive(Default)]
pub struct NavigationWheel {
    pub open: bool,
    opened_at: Option<Instant>,
    inner: usize,
    outer: Option<usize>,
    shown: bool,
}

/// One segment of the wheel.
pub struct WheelItem {
    pub key: String,
    pub label: String,
    pub glyph: String,
    pub badge: Option<usize>,
}

impl NavigationWheel {
    /// Ctrl+Tab pressed: open with the previous module preselected (Shift: backwards).
    pub fn begin(&mut self, backwards: bool, count: usize) {
        if !self.open {
            self.open = true;
            self.opened_at = Some(Instant::now());
            self.inner = if count > 1 {
                if backwards {
                    count - 1
                } else {
                    1
                }
            } else {
                0
            };
            self.outer = None;
            self.shown = false;
        } else if count > 0 {
            self.inner = if backwards { (self.inner + count - 1) % count } else { (self.inner + 1) % count };
            self.outer = None;
        }
    }

    /// Draws the wheel (after a short delay, so a quick tap just switches back) and handles its keys.
    /// Returns the chosen view key when the wheel closes with a selection.
    pub fn show(&mut self, ctx: &egui::Context, inner: &[WheelItem], outer_of: &dyn Fn(usize) -> Vec<WheelItem>, title: &str, reduced_motion: bool) -> Option<String> {
        if !self.open || inner.is_empty() {
            self.open = false;
            return None;
        }
        let (ctrl, tab, shift, left, right, up, down, enter, esc) = ctx.input(|i| {
            (
                i.modifiers.ctrl || i.modifiers.command,
                i.key_pressed(egui::Key::Tab),
                i.modifiers.shift,
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Escape),
            )
        });
        let n = inner.len();
        self.inner %= n;
        if tab {
            self.begin(shift, n);
        }
        let outer = outer_of(self.inner);
        if right && !outer.is_empty() {
            self.outer = Some(self.outer.map(|o| (o + 1) % outer.len()).unwrap_or(0));
        }
        if left {
            self.outer = None;
        }
        if down || up {
            match self.outer {
                Some(o) if !outer.is_empty() => self.outer = Some(if down { (o + 1) % outer.len() } else { (o + outer.len() - 1) % outer.len() }),
                _ => self.inner = if down { (self.inner + 1) % n } else { (self.inner + n - 1) % n },
            }
        }
        if esc {
            self.open = false;
            return None;
        }
        let quick = self.opened_at.is_some_and(|t| t.elapsed() < Duration::from_millis(220));
        let pick = |s: &Self| match s.outer {
            Some(o) if o < outer.len() => Some(outer[o].key.clone()),
            _ => Some(inner[s.inner].key.clone()),
        };
        if enter || !ctrl {
            self.open = false;
            return pick(self);
        }
        if quick && !self.shown {
            ctx.request_repaint_after(Duration::from_millis(60));
            return None;
        }
        self.shown = true;

        // ---- painting
        let screen = ctx.screen_rect();
        let center = screen.center();
        let r_inner = 70.0;
        let r_mid = 170.0;
        let r_outer = 260.0;
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("redhive.wheel")));
        let visuals = ctx.style().visuals.clone();
        painter.rect_filled(screen, 0.0, egui::Color32::from_black_alpha(if reduced_motion { 120 } else { 90 }));
        let accent = visuals.selection.bg_fill;
        let fill = visuals.window_fill;
        let stroke = egui::Stroke::new(1.0_f32, visuals.widgets.noninteractive.bg_stroke.color);
        let font = egui::FontId::proportional(14.0);
        let glyph_font = egui::FontId::proportional(22.0);
        let segment = |painter: &egui::Painter, a0: f32, a1: f32, r0: f32, r1: f32, color: egui::Color32| {
            let steps = 24;
            let mut pts = Vec::with_capacity(steps * 2 + 2);
            for k in 0..=steps {
                let a = a0 + (a1 - a0) * k as f32 / steps as f32;
                pts.push(center + egui::vec2(a.cos(), a.sin()) * r1);
            }
            for k in (0..=steps).rev() {
                let a = a0 + (a1 - a0) * k as f32 / steps as f32;
                pts.push(center + egui::vec2(a.cos(), a.sin()) * r0);
            }
            painter.add(egui::Shape::convex_polygon(pts.clone(), color, egui::Stroke::NONE));
            painter.add(egui::Shape::closed_line(pts, stroke));
        };
        let start = -std::f32::consts::FRAC_PI_2;
        let step = std::f32::consts::TAU / n as f32;
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let clicked = ctx.input(|i| i.pointer.primary_released());
        for (i, item) in inner.iter().enumerate() {
            let a0 = start + step * i as f32 - step / 2.0;
            let a1 = a0 + step;
            if let Some(p) = pointer {
                let d = p - center;
                let ang = (d.y.atan2(d.x) - a0).rem_euclid(std::f32::consts::TAU);
                if d.length() > r_inner && d.length() < r_mid && ang < step {
                    self.inner = i;
                    self.outer = None;
                    if clicked {
                        self.open = false;
                        return Some(item.key.clone());
                    }
                }
            }
            let selected = i == self.inner;
            segment(&painter, a0 + 0.01, a1 - 0.01, r_inner, r_mid, if selected { accent } else { fill });
            let mid = start + step * i as f32;
            let pos = center + egui::vec2(mid.cos(), mid.sin()) * ((r_inner + r_mid) / 2.0);
            painter.text(pos - egui::vec2(0.0, 10.0), egui::Align2::CENTER_CENTER, &item.glyph, glyph_font.clone(), visuals.strong_text_color());
            painter.text(pos + egui::vec2(0.0, 14.0), egui::Align2::CENTER_CENTER, &item.label, font.clone(), visuals.text_color());
            if let Some(b) = item.badge.filter(|b| *b > 0) {
                let bp = pos + egui::vec2(22.0, -22.0);
                painter.circle_filled(bp, 10.0, egui::Color32::from_rgb(198, 40, 40));
                painter.text(bp, egui::Align2::CENTER_CENTER, b.to_string(), egui::FontId::proportional(11.0), egui::Color32::WHITE);
            }
        }
        // Outer ring: the views of the selected module, fanned around it.
        if !outer.is_empty() {
            let m = outer.len() as f32;
            let span = (step * 1.6).min(std::f32::consts::PI);
            let base = start + step * self.inner as f32 - span / 2.0;
            let ostep = span / m;
            for (j, item) in outer.iter().enumerate() {
                let a0 = base + ostep * j as f32;
                let a1 = a0 + ostep;
                if let Some(p) = pointer {
                    let d = p - center;
                    let ang = (d.y.atan2(d.x) - a0).rem_euclid(std::f32::consts::TAU);
                    if d.length() > r_mid && d.length() < r_outer && ang < ostep {
                        self.outer = Some(j);
                        if clicked {
                            self.open = false;
                            return Some(item.key.clone());
                        }
                    }
                }
                let selected = self.outer == Some(j);
                segment(&painter, a0 + 0.008, a1 - 0.008, r_mid + 6.0, r_outer, if selected { accent } else { fill.gamma_multiply(0.92) });
                let mid = a0 + ostep / 2.0;
                let pos = center + egui::vec2(mid.cos(), mid.sin()) * ((r_mid + r_outer) / 2.0 + 3.0);
                painter.text(pos, egui::Align2::CENTER_CENTER, format!("{} {}", item.glyph, item.label), egui::FontId::proportional(12.0), visuals.text_color());
            }
        }
        painter.circle_filled(center, r_inner - 6.0, fill);
        painter.text(center, egui::Align2::CENTER_CENTER, title, egui::FontId::proportional(13.0), visuals.weak_text_color());
        ctx.request_repaint();
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: &str, owner: &str, home: bool) -> ViewEntry {
        ViewEntry { key: key.into(), title: key.into(), icon: String::new(), role: ViewRole::Document, dock: DockSide::Right, home, permission: String::new(), order: 0, owner: owner.into(), source: ViewSource::Shell }
    }

    #[test]
    fn homes_follow_mru() {
        let mut r = NavigationRegistry::default();
        r.register(entry("a.home", "a", true));
        r.register(entry("b.home", "b", true));
        r.register(entry("b.list", "b", false));
        r.touch("a");
        r.touch("b");
        let homes: Vec<_> = r.homes(&[]).iter().map(|v| v.key.clone()).collect();
        assert_eq!(homes, vec!["b.home", "a.home"]);
        assert_eq!(r.views_of("b", &[]).len(), 2);
        r.unregister_owner("b");
        assert_eq!(r.homes(&[]).len(), 1);
    }

    #[test]
    fn wheel_cycles() {
        let mut w = NavigationWheel::default();
        w.begin(false, 3);
        assert_eq!(w.inner, 1);
        w.begin(false, 3);
        assert_eq!(w.inner, 2);
        w.begin(true, 3);
        assert_eq!(w.inner, 1);
    }
}
