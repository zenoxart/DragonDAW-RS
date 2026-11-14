//! Audio widgets: knobs, faders, meters – and the gesture glue that turns drags into one undo step.

use crate::state::State;
use crate::theme;
use dragon_engine::model::Project;
use egui::{Color32, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};

/// Applies `f` as part of a continuous gesture when `r` changed; commits the undo step when the
/// interaction ends (drag released, or a click / typed value).
pub fn apply(st: &mut State, r: &Response, label: &str, f: impl FnOnce(&mut Project)) {
    if r.changed() {
        st.session.gesture(label, f);
    }
    if st.session.in_gesture() && !r.dragged() && (r.drag_stopped() || r.changed() || r.lost_focus()) {
        st.session.end_gesture();
    }
}

/// A rotary knob. Drag up/down (Shift = fine), double-click resets, scroll adjusts.
pub fn knob(ui: &mut Ui, value: &mut f32, min: f32, max: f32, default: f32, label: &str, fmt: impl Fn(f32) -> String) -> Response {
    let size = 34.0;
    let (rect, mut r) = ui.allocate_exact_size(Vec2::new(size + 10.0, size + 14.0), Sense::click_and_drag());
    let range = max - min;
    let mut n = ((*value - min) / range).clamp(0.0, 1.0);
    if r.dragged() {
        let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
        n = (n - r.drag_delta().y / 160.0 * fine).clamp(0.0, 1.0);
        *value = min + n * range;
        r.mark_changed();
    }
    if r.hovered() {
        let s = ui.input(|i| i.smooth_scroll_delta.y);
        if s != 0.0 {
            n = (n + s / 600.0).clamp(0.0, 1.0);
            *value = min + n * range;
            r.mark_changed();
        }
    }
    if r.double_clicked() {
        *value = default;
        r.mark_changed();
    }
    let c = Pos2::new(rect.center().x, rect.top() + size / 2.0 + 2.0);
    let p = ui.painter();
    let rad = size / 2.0 - 3.0;
    p.circle_filled(c, rad, theme::PANEL_2);
    let a0 = std::f32::consts::PI * 0.75;
    let sweep = std::f32::consts::PI * 1.5;
    let arc = |from: f32, to: f32| -> Vec<Pos2> { (0..=24).map(|i| from + (to - from) * i as f32 / 24.0).map(|a| Pos2::new(c.x + rad * a.cos(), c.y + rad * a.sin())).collect() };
    p.add(egui::Shape::line(arc(a0, a0 + sweep), Stroke::new(3.0_f32, theme::GRID)));
    // Bipolar knobs fill from the centre.
    let (fa, fb) = if min < 0.0 && max > 0.0 { let z = (-min / range).clamp(0.0, 1.0); (a0 + sweep * z.min(n), a0 + sweep * z.max(n)) } else { (a0, a0 + sweep * n) };
    p.add(egui::Shape::line(arc(fa, fb), Stroke::new(3.0_f32, theme::ACCENT_2)));
    let a = a0 + sweep * n;
    p.line_segment([c, Pos2::new(c.x + (rad - 4.0) * a.cos(), c.y + (rad - 4.0) * a.sin())], Stroke::new(2.0_f32, Color32::WHITE));
    p.text(Pos2::new(rect.center().x, rect.bottom()), egui::Align2::CENTER_BOTTOM, label, egui::FontId::proportional(10.0), theme::TEXT_DIM);
    r.on_hover_text(format!("{label}: {}", fmt(*value)))
}

/// A vertical fader 0 … max (linear gain), with a dB scale tooltip.
pub fn fader(ui: &mut Ui, value: &mut f32, max: f32, height: f32) -> Response {
    let (rect, mut r) = ui.allocate_exact_size(Vec2::new(18.0, height), Sense::click_and_drag());
    let to_n = |v: f32| (v / max).clamp(0.0, 1.0).powf(0.5);
    let from_n = |n: f32| n.clamp(0.0, 1.0).powi(2) * max;
    let mut n = to_n(*value);
    if r.dragged() {
        let fine = if ui.input(|i| i.modifiers.shift) { 0.15 } else { 1.0 };
        n = (n - r.drag_delta().y / height * fine).clamp(0.0, 1.0);
        *value = from_n(n);
        r.mark_changed();
    }
    if r.double_clicked() {
        *value = 0.8;
        r.mark_changed();
    }
    let p = ui.painter();
    let track = Rect::from_center_size(rect.center(), Vec2::new(4.0, height - 8.0));
    p.rect_filled(track, 2.0, theme::BG);
    // Unity mark.
    let unity_y = track.bottom() - to_n(1.0) * track.height();
    p.line_segment([Pos2::new(rect.left(), unity_y), Pos2::new(rect.right(), unity_y)], Stroke::new(1.0_f32, theme::GRID_BAR));
    let y = track.bottom() - n * track.height();
    let cap = Rect::from_center_size(Pos2::new(rect.center().x, y), Vec2::new(18.0, 10.0));
    p.rect_filled(cap, 2.0, if r.dragged() { theme::ACCENT_2 } else { Color32::from_gray(200) });
    p.line_segment([Pos2::new(cap.left() + 2.0, y), Pos2::new(cap.right() - 2.0, y)], Stroke::new(1.0_f32, Color32::BLACK));
    let db = dragon_engine::dsp::gain_to_db(*value);
    r.on_hover_text(format!("{db:.1} dB"))
}

/// Stereo peak meter (linear values) with a peak-hold colour.
pub fn meter(ui: &mut Ui, l: f32, r: f32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(12.0, height), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 1.0, theme::BG);
    for (i, v) in [l, r].into_iter().enumerate() {
        let x0 = rect.left() + i as f32 * 6.0 + 0.5;
        // −60 … +6 dB display range.
        let db = dragon_engine::dsp::gain_to_db(v.max(1e-6));
        let n = ((db + 60.0) / 66.0).clamp(0.0, 1.0);
        let bar = Rect::from_min_max(Pos2::new(x0, rect.bottom() - n * rect.height()), Pos2::new(x0 + 5.0, rect.bottom()));
        p.rect_filled(bar, 0.0, theme::meter_color(v));
    }
    let zero = rect.bottom() - (60.0 / 66.0) * rect.height();
    p.line_segment([Pos2::new(rect.left(), zero), Pos2::new(rect.right(), zero)], Stroke::new(1.0_f32, theme::REC.gamma_multiply(0.6)));
}

/// A small toggle button with an accent colour when on.
pub fn toggle(ui: &mut Ui, on: bool, text: &str, color: Color32) -> Response {
    let rt = egui::RichText::new(text).color(if on { Color32::BLACK } else { theme::TEXT_DIM });
    // Icon glyphs need the normal size to render crisply; words use the small style.
    let rt = if text.chars().count() <= 2 { rt } else { rt.small() };
    let b = egui::Button::new(rt).fill(if on { color } else { theme::PANEL_2 }).min_size(Vec2::new(20.0, 16.0));
    ui.add(b)
}

pub fn db_fmt(v: f32) -> String {
    format!("{:.1} dB", dragon_engine::dsp::gain_to_db(v))
}

pub fn pan_fmt(v: f32) -> String {
    if v.abs() < 0.005 {
        "C".into()
    } else if v < 0.0 {
        format!("{:.0}L", -v * 100.0)
    } else {
        format!("{:.0}R", v * 100.0)
    }
}

pub fn pct(v: f32) -> String {
    format!("{:.0}%", v * 100.0)
}

/// Grid snap choices (beats).
pub const SNAPS: &[(&str, f64)] = &[("Bar", 4.0), ("Beat", 1.0), ("1/2 beat", 0.5), ("1/4 beat", 0.25), ("1/3 beat", 1.0 / 3.0), ("1/6 beat", 1.0 / 6.0), ("1/8 beat", 0.125), ("None", 0.0)];

pub fn snap(beat: f64, grid: f64) -> f64 {
    if grid <= 0.0 {
        beat
    } else {
        (beat / grid).round() * grid
    }
}

pub fn snap_floor(beat: f64, grid: f64) -> f64 {
    if grid <= 0.0 {
        beat
    } else {
        (beat / grid).floor() * grid
    }
}
