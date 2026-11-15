//! Navigation shared by the playlist and the piano roll (FL Studio style):
//!
//! * **Ctrl + right-drag** draws a box and zooms into it (time and height); **Ctrl + right-click** zooms out.
//! * **Middle-drag** pans in both directions.
//! * Zoom jumps are animated: a view eases towards a [`ViewTarget`] over a few frames.

use crate::theme;
use egui::{Id, LayerId, Order, PointerButton, Pos2, Rect, Stroke, Ui, Vec2};

/// Result of a Ctrl + right mouse gesture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AreaZoom {
    /// Zoom so that this screen rectangle fills the view.
    To(Rect),
    /// Zoom out around this point.
    Out(Pos2),
}

/// Where an animated view is heading: horizontal zoom, horizontal scroll, vertical scale, vertical scroll.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewTarget {
    pub zoom: f64,
    pub x: f64,
    pub vscale: f32,
    pub y: f32,
}

/// Tracks the Ctrl + right gesture inside `area`, draws the zoom box, and reports the result on release.
pub fn area_zoom(ui: &Ui, area: Rect, key: &str) -> Option<AreaZoom> {
    let id = Id::new(("area_zoom", key));
    let (ctrl, pressed, down, pos) = ui.input(|i| (i.modifiers.ctrl || i.modifiers.command, i.pointer.button_pressed(PointerButton::Secondary), i.pointer.button_down(PointerButton::Secondary), i.pointer.latest_pos()));
    let start: Option<Pos2> = ui.data(|d| d.get_temp(id));
    match (start, pos) {
        (None, Some(p)) if pressed && ctrl && area.contains(p) => {
            ui.data_mut(|d| d.insert_temp(id, p));
            None
        }
        (Some(s), Some(p)) if down => {
            let r = Rect::from_two_pos(s, p.clamp(area.min, area.max));
            let painter = ui.ctx().layer_painter(LayerId::new(Order::Foreground, id));
            painter.rect(r, 0.0, theme::PLAYHEAD.gamma_multiply(0.10), Stroke::new(1.0_f32, theme::PLAYHEAD), egui::StrokeKind::Inside);
            None
        }
        (Some(s), p) => {
            ui.data_mut(|d| d.remove::<Pos2>(id));
            let p = p.unwrap_or(s).clamp(area.min, area.max);
            let r = Rect::from_two_pos(s, p);
            Some(if r.width() > 6.0 { AreaZoom::To(r) } else { AreaZoom::Out(s) })
        }
        _ => None,
    }
}

/// Middle-button drag delta (for panning), or zero.
pub fn middle_drag(ui: &Ui, area: Rect) -> Vec2 {
    ui.input(|i| {
        let inside = i.pointer.press_origin().map(|p| area.contains(p)).unwrap_or(false);
        if i.pointer.button_down(PointerButton::Middle) && inside {
            i.pointer.delta()
        } else {
            Vec2::ZERO
        }
    })
}

/// Moves the current view one animation step towards `target`; returns the new values and whether the
/// animation is still running.
pub fn step(cur: ViewTarget, target: ViewTarget) -> (ViewTarget, bool) {
    const K: f64 = 0.35;
    let lerp = |a: f64, b: f64| a + (b - a) * K;
    // Zoom moves on a log scale so zooming in and out feel the same.
    let z = (cur.zoom.ln() + (target.zoom.ln() - cur.zoom.ln()) * K).exp();
    let next = ViewTarget { zoom: z, x: lerp(cur.x, target.x), vscale: lerp(cur.vscale as f64, target.vscale as f64) as f32, y: lerp(cur.y as f64, target.y as f64) as f32 };
    let done = (next.zoom / target.zoom - 1.0).abs() < 0.002 && (next.x - target.x).abs() * next.zoom < 0.5 && (next.vscale - target.vscale).abs() < 0.002 && (next.y - target.y).abs() < 0.5;
    if done {
        (target, false)
    } else {
        (next, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn animation_converges() {
        let mut v = ViewTarget { zoom: 10.0, x: 0.0, vscale: 1.0, y: 0.0 };
        let t = ViewTarget { zoom: 160.0, x: 32.0, vscale: 2.0, y: 300.0 };
        let mut frames = 0;
        loop {
            let (n, running) = step(v, t);
            v = n;
            frames += 1;
            if !running {
                break;
            }
            assert!(frames < 60, "does not converge");
        }
        assert_eq!(v, t);
        assert!(frames > 3, "should ease over several frames");
    }
}
