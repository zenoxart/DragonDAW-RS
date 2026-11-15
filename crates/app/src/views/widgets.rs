//! Audio widgets: knobs, faders, meters – and the gesture glue that turns drags into one undo step.

use crate::state::State;
use crate::theme;
use dragon_engine::model::Project;
use egui::{Color32, Pos2, Rect, Response, RichText, Sense, Stroke, Ui, Vec2};

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

/// Tempo examples offered on right-click of the BPM display, with the styles they are typical of.
pub const BPM_PRESETS: &[(f64, &str)] = &[
    (70.0, "Hip-hop / trap half-time, ballad"),
    (75.0, "Lo-fi, boom bap"),
    (80.0, "Reggaeton (slow), R&B"),
    (85.0, "Boom bap, soul"),
    (87.5, "Dubstep / drum & bass half-time"),
    (90.0, "Hip-hop, reggaeton"),
    (95.0, "Hip-hop, dancehall"),
    (100.0, "Pop, moombahton"),
    (105.0, "Pop, funk"),
    (110.0, "Pop, disco (slow)"),
    (115.0, "Nu-disco, pop"),
    (120.0, "House (deep), pop"),
    (124.0, "House, tech house"),
    (126.0, "Tech house"),
    (128.0, "House, EDM"),
    (130.0, "Techno"),
    (135.0, "Techno, hard dance"),
    (140.0, "Trap, dubstep, grime, drill"),
    (145.0, "Trap, jersey club"),
    (150.0, "Hardstyle (slow), footwork"),
    (155.0, "Hardstyle, trance"),
    (160.0, "Footwork, juke, drum & bass half-speed"),
];

/// The FL Studio-style tempo display. Dragging up/down on the whole number changes the tempo in whole
/// BPM (the decimals stay); dragging on the decimals changes only the decimals (0.01 per pixel, Shift
/// 0.001). The wheel works the same on either part, double-click types a value, right-click offers
/// tempo examples from 70 to 160 BPM. Returns the response (changed while dragging) and the value.
pub fn bpm_display(ui: &mut Ui, bpm: &mut f64) -> Response {
    let id = ui.id().with("bpm.display");
    let editing: Option<String> = ui.data(|d| d.get_temp(id.with("edit")));
    if let Some(mut text) = editing {
        let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(78.0).font(egui::TextStyle::Monospace));
        r.request_focus();
        let done = ui.input(|i| i.key_pressed(egui::Key::Enter)) || r.lost_focus();
        let mut r = r;
        if done {
            if let Ok(v) = text.trim().replace(',', ".").parse::<f64>() {
                *bpm = v.clamp(10.0, 522.0);
                r.mark_changed();
            }
            ui.data_mut(|d| d.remove::<String>(id.with("edit")));
        } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            ui.data_mut(|d| d.remove::<String>(id.with("edit")));
        } else {
            ui.data_mut(|d| d.insert_temp(id.with("edit"), text));
        }
        return r;
    }
    let whole = bpm.trunc();
    let int_txt = format!("{}", whole as i64);
    let dec_txt = format!(".{:03}", ((*bpm - whole) * 1000.0).round().min(999.0) as i64);
    let font = egui::FontId::monospace(17.0);
    let gi = ui.painter().layout_no_wrap(int_txt.clone(), font.clone(), theme::ACCENT);
    let gd = ui.painter().layout_no_wrap(dec_txt.clone(), egui::FontId::monospace(13.0), theme::ACCENT);
    let pad = 6.0;
    let w = gi.size().x + gd.size().x + pad * 2.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w.max(70.0), 30.0), Sense::hover());
    let mut changed = false;
    let int_rect = Rect::from_min_max(rect.min, Pos2::new(rect.left() + pad + gi.size().x, rect.bottom()));
    let dec_rect = Rect::from_min_max(Pos2::new(int_rect.right(), rect.top()), rect.max);
    ui.data_mut(|d| d.insert_temp(egui::Id::new("bpm.parts"), (int_rect, dec_rect)));
    let ri = ui.interact(int_rect, id.with("int"), Sense::click_and_drag());
    let rd = ui.interact(dec_rect, id.with("dec"), Sense::click_and_drag());
    let fine = ui.input(|i| i.modifiers.shift);
    let start_key = id.with("start");
    for (r, is_int) in [(&ri, true), (&rd, false)] {
        if r.drag_started() {
            ui.data_mut(|d| d.insert_temp(start_key, *bpm));
        }
        if r.dragged() {
            let start: f64 = ui.data(|d| d.get_temp(start_key)).unwrap_or(*bpm);
            let dy = -(r.interact_pointer_pos().unwrap_or_default().y - ui.input(|i| i.pointer.press_origin()).unwrap_or_default().y) as f64;
            let v = if is_int {
                // Whole BPM per 4 px; the decimals stay.
                start + (dy / 4.0).round()
            } else {
                // Only the decimals change; the whole number stays.
                let step = if fine { 0.001 } else { 0.01 };
                let frac = ((start - start.trunc()) + (dy * step)).clamp(0.0, 0.999);
                start.trunc() + (frac * 1000.0).round() / 1000.0
            };
            let v = v.clamp(10.0, 522.0);
            if (v - *bpm).abs() > 1e-9 {
                *bpm = v;
                changed = true;
            }
        }
        if r.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            let s = ui.input(|i| i.raw_scroll_delta.y);
            if s != 0.0 {
                let d = if is_int {
                    s.signum() as f64
                } else if fine {
                    0.001 * s.signum() as f64
                } else {
                    0.01 * s.signum() as f64
                };
                let v = if is_int { (*bpm + d).clamp(10.0, 522.0) } else { bpm.trunc() + ((*bpm - bpm.trunc()) + d).clamp(0.0, 0.999) };
                *bpm = (v * 1000.0).round() / 1000.0;
                changed = true;
            }
        }
        if r.double_clicked() {
            ui.data_mut(|d| d.insert_temp(id.with("edit"), format!("{:.3}", *bpm)));
        }
    }
    // Drawing.
    let active_int = ri.hovered() || ri.dragged();
    let active_dec = rd.hovered() || rd.dragged();
    let p = ui.painter();
    p.rect_filled(rect, 4.0, theme::BG);
    if active_int {
        p.rect_filled(int_rect.shrink(2.0), 3.0, theme::ACCENT.gamma_multiply(0.15));
    }
    if active_dec {
        p.rect_filled(dec_rect.shrink(2.0), 3.0, theme::ACCENT.gamma_multiply(0.15));
    }
    let base_y = rect.center().y + gi.size().y / 2.0 - 1.0;
    p.galley(Pos2::new(int_rect.left() + pad, base_y - gi.size().y), gi, theme::ACCENT);
    p.galley(Pos2::new(dec_rect.left(), base_y - gd.size().y - 1.0), gd, theme::ACCENT.gamma_multiply(0.8));
    let tip = "Tempo – drag the whole number for whole BPM, the decimals for fine steps (Shift = finer) · wheel · double-click to type · right-click for tempo examples";
    let mut resp = ri.union(rd);
    resp = resp.on_hover_text(tip);
    resp.context_menu(|ui| {
        ui.label(RichText::new("TEMPO EXAMPLES").small().color(theme::TEXT_DIM));
        for (v, style) in BPM_PRESETS {
            let txt = if v.fract() == 0.0 { format!("{v:.0} BPM") } else { format!("{v} BPM") };
            let b = ui.add(egui::Button::new(RichText::new(txt).monospace()).shortcut_text(*style).selected((*bpm - v).abs() < 1e-6));
            if b.clicked() {
                *bpm = *v;
                changed = true;
                ui.close_menu();
            }
        }
    });
    if changed {
        resp.mark_changed();
    }
    resp
}

/// A rotary knob. Drag up/down (Shift = fine), double-click resets, scroll adjusts.
pub fn knob(ui: &mut Ui, value: &mut f32, min: f32, max: f32, default: f32, label: &str, fmt: impl Fn(f32) -> String) -> Response {
    knob_colored(ui, value, min, max, default, label, fmt, theme::ACCENT_2)
}

/// [`knob`] with its own arc colour (effect editors use their category colour).
#[allow(clippy::too_many_arguments)]
pub fn knob_colored(ui: &mut Ui, value: &mut f32, min: f32, max: f32, default: f32, label: &str, fmt: impl Fn(f32) -> String, color: Color32) -> Response {
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
    let (fa, fb) = if min < 0.0 && max > 0.0 {
        let z = (-min / range).clamp(0.0, 1.0);
        (a0 + sweep * z.min(n), a0 + sweep * z.max(n))
    } else {
        (a0, a0 + sweep * n)
    };
    if r.hovered() || r.dragged() {
        p.circle_filled(c, rad + 3.0, color.gamma_multiply(0.12));
    }
    p.add(egui::Shape::line(arc(fa, fb), Stroke::new(3.0_f32, color)));
    let a = a0 + sweep * n;
    p.line_segment([c, Pos2::new(c.x + (rad - 4.0) * a.cos(), c.y + (rad - 4.0) * a.sin())], Stroke::new(2.0_f32, Color32::WHITE));
    p.text(Pos2::new(rect.center().x, rect.bottom()), egui::Align2::CENTER_BOTTOM, label, egui::FontId::proportional(10.0), theme::TEXT_DIM);
    r.on_hover_text(format!("{label}: {}", fmt(*value)))
}

/// The effect-editor knob from the design reference: the name in capitals above, a dark knob with a
/// coloured value arc and a white pointer, the value printed below. Same gestures as [`knob`].
/// Parses a typed parameter value: "-6", "-6 dB", "2.5k", "2.5 kHz", "40%", "4:1".
pub fn parse_value(s: &str) -> Option<f32> {
    let t = s.trim().to_lowercase().replace(',', ".");
    let num: String = t.chars().take_while(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+')).collect();
    let x: f32 = num.parse().ok()?;
    let rest = t[num.len()..].trim_start();
    Some(if rest.starts_with('k') { x * 1000.0 } else { x })
}

pub fn fx_knob(ui: &mut Ui, n: &mut f32, default: f32, label: &str, value: &str, color: Color32) -> Response {
    let size = 40.0;
    let w = 64.0;
    let (rect, mut r) = ui.allocate_exact_size(Vec2::new(w, size + 32.0), Sense::click_and_drag());
    if r.dragged() {
        let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
        *n = (*n - r.drag_delta().y / 180.0 * fine).clamp(0.0, 1.0);
        r.mark_changed();
    }
    if r.hovered() {
        let s = ui.input(|i| i.smooth_scroll_delta.y);
        if s != 0.0 {
            *n = (*n + s / 600.0).clamp(0.0, 1.0);
            r.mark_changed();
        }
    }
    if r.double_clicked() || (r.clicked() && ui.input(|i| i.modifiers.command)) {
        *n = default;
        r.mark_changed();
    }
    let p = ui.painter();
    p.text(Pos2::new(rect.center().x, rect.top()), egui::Align2::CENTER_TOP, label.to_uppercase(), egui::FontId::proportional(9.5), theme::TEXT);
    let c = Pos2::new(rect.center().x, rect.top() + 14.0 + size / 2.0);
    let rad = size / 2.0 - 2.0;
    let active = r.hovered() || r.dragged();
    if active {
        p.circle_filled(c, rad + 4.0, color.gamma_multiply(0.12));
    }
    p.circle_filled(c, rad - 4.0, Color32::from_rgb(0x2a, 0x2e, 0x36));
    p.circle_stroke(c, rad - 4.0, Stroke::new(1.0_f32, Color32::from_white_alpha(25)));
    let a0 = std::f32::consts::PI * 0.75;
    let sweep = std::f32::consts::PI * 1.5;
    let arc = |from: f32, to: f32| -> Vec<Pos2> { (0..=28).map(|i| from + (to - from) * i as f32 / 28.0).map(|a| Pos2::new(c.x + rad * a.cos(), c.y + rad * a.sin())).collect() };
    p.add(egui::Shape::line(arc(a0, a0 + sweep), Stroke::new(3.0_f32, Color32::from_white_alpha(18))));
    p.add(egui::Shape::line(arc(a0, a0 + sweep * *n), Stroke::new(3.0_f32, color)));
    let a = a0 + sweep * *n;
    p.line_segment([Pos2::new(c.x + (rad - 13.0) * a.cos(), c.y + (rad - 13.0) * a.sin()), Pos2::new(c.x + (rad - 5.0) * a.cos(), c.y + (rad - 5.0) * a.sin())], Stroke::new(2.0_f32, Color32::WHITE));
    p.text(Pos2::new(rect.center().x, rect.bottom()), egui::Align2::CENTER_BOTTOM, value, egui::FontId::proportional(10.0), if active { color } else { theme::TEXT_DIM });
    r
}

/// A small coloured knob without a label (effect-slot mix, compact rows). Same gestures as [`knob`].
pub fn mini_knob(ui: &mut Ui, value: &mut f32, min: f32, max: f32, default: f32, color: Color32) -> Response {
    let size = 24.0;
    let (rect, mut r) = ui.allocate_exact_size(Vec2::splat(size), Sense::click_and_drag());
    let range = max - min;
    let mut n = ((*value - min) / range).clamp(0.0, 1.0);
    if r.dragged() {
        let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
        n = (n - r.drag_delta().y / 140.0 * fine).clamp(0.0, 1.0);
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
    let c = rect.center();
    let p = ui.painter();
    let rad = size / 2.0 - 2.5;
    p.circle_filled(c, rad - 2.0, if r.hovered() { theme::PANEL_2 } else { theme::PANEL });
    let a0 = std::f32::consts::PI * 0.75;
    let sweep = std::f32::consts::PI * 1.5;
    let arc = |from: f32, to: f32| -> Vec<Pos2> { (0..=18).map(|i| from + (to - from) * i as f32 / 18.0).map(|a| Pos2::new(c.x + rad * a.cos(), c.y + rad * a.sin())).collect() };
    p.add(egui::Shape::line(arc(a0, a0 + sweep), Stroke::new(2.5_f32, theme::GRID)));
    p.add(egui::Shape::line(arc(a0, a0 + sweep * n), Stroke::new(2.5_f32, color)));
    let a = a0 + sweep * n;
    p.line_segment([c, Pos2::new(c.x + (rad - 3.0) * a.cos(), c.y + (rad - 3.0) * a.sin())], Stroke::new(1.5_f32, Color32::WHITE));
    r
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

#[cfg(test)]
mod tests {
    #[test]
    fn typed_values_parse_with_units() {
        use super::parse_value as p;
        assert_eq!(p("-6"), Some(-6.0));
        assert_eq!(p(" -6 dB"), Some(-6.0));
        assert_eq!(p("2.5k"), Some(2500.0));
        assert_eq!(p("2,5 kHz"), Some(2500.0));
        assert_eq!(p("4:1"), Some(4.0));
        assert_eq!(p("abc"), None);
    }
}
