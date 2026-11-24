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
    (125.0, "House, garage"),
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

/// The classic tempo display. Dragging up/down on the whole number changes the tempo in whole
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
    value_hook(ui, &mut r, value, min, max, default, &fmt);
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
    // Screen readers: a slider with its name and value; keyboard: arrows step it when focused.
    let shown = fmt(*value);
    let v = *value as f64;
    r.widget_info(|| egui::WidgetInfo { value: Some(v), current_text_value: Some(shown.clone()), ..egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, label) });
    if keyboard_step(ui, &r, value, min, max) {
        r.mark_changed();
    }
    r.on_hover_text(format!("{label}: {}", fmt(*value)))
}

/// Arrow keys on a focused knob / fader: Up / Right one step (a 100th of the range), Down / Left back;
/// Shift for fine steps. Returns whether the value changed.
fn keyboard_step(ui: &Ui, r: &Response, value: &mut f32, min: f32, max: f32) -> bool {
    if !r.has_focus() {
        return false;
    }
    let (up, down, fine) = ui.input(|i| (i.key_pressed(egui::Key::ArrowUp) || i.key_pressed(egui::Key::ArrowRight), i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::ArrowLeft), i.modifiers.shift));
    let step = (max - min) / if fine { 1000.0 } else { 100.0 };
    let before = *value;
    if up {
        *value = (*value + step).min(max);
    }
    if down {
        *value = (*value - step).max(min);
    }
    *value != before
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
    // Mini knobs are mix / amount controls: shown in percent of their range.
    let show = move |v: f32| format!("{:.0}%", (v - min) / (max - min) * 100.0);
    value_hook(ui, &mut r, value, min, max, default, &show);
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

/// The height of every control in the app's toolbars (transport, channel rack, playlist, piano roll):
/// buttons, menus, combo boxes and fields all line up.
pub const BAR_H: f32 = 26.0;
/// The height of every control in the main transport bar.
pub const SHELL_H: f32 = 30.0;

/// The level scale shared by the mixer's dB scale, meters and faders, so a number, a meter's top and a
/// fader cap at the same level sit at the same height. A console taper: linear in dB between these
/// points (more room around 0 dB, where you balance), the last stretch to −∞ linear in gain.
const LEVEL_POINTS: [(f32, f32); 8] = [(6.0206, 1.0), (0.0, 0.80), (-5.0, 0.69), (-10.0, 0.58), (-20.0, 0.40), (-30.0, 0.27), (-40.0, 0.16), (-60.0, 0.04)];
/// Inset at the top and bottom of every part of the strip (the fader cap's half height): the scale, the
/// meter and the fader track all start and end at the same place.
pub const LEVEL_INSET: f32 = 6.0;

/// A level in dB → 0 … 1 up the scale.
pub fn level_pos(db: f32) -> f32 {
    if db >= LEVEL_POINTS[0].0 {
        return 1.0;
    }
    for w in LEVEL_POINTS.windows(2) {
        let ((d0, n0), (d1, n1)) = (w[0], w[1]);
        if db >= d1 {
            return n1 + (n0 - n1) * (db - d1) / (d0 - d1);
        }
    }
    // Below −60 dB: linear in gain down to silence.
    let (d, n) = LEVEL_POINTS[LEVEL_POINTS.len() - 1];
    (n * dragon_engine::dsp::db_to_gain(db) / dragon_engine::dsp::db_to_gain(d)).clamp(0.0, n)
}

/// 0 … 1 up the scale → the level in dB (−∞ as a very low number).
pub fn level_db(n: f32) -> f32 {
    let n = n.clamp(0.0, 1.0);
    for w in LEVEL_POINTS.windows(2) {
        let ((d0, n0), (d1, n1)) = (w[0], w[1]);
        if n >= n1 {
            return d1 + (d0 - d1) * (n - n1) / (n0 - n1);
        }
    }
    let (d, nn) = LEVEL_POINTS[LEVEL_POINTS.len() - 1];
    if n <= 0.0 {
        -200.0
    } else {
        dragon_engine::dsp::gain_to_db(dragon_engine::dsp::db_to_gain(d) * n / nn)
    }
}

/// The y of a level in a strip part `rect` (inset like every other part).
fn level_y(rect: Rect, db: f32) -> f32 {
    let (top, bottom) = (rect.top() + LEVEL_INSET, rect.bottom() - LEVEL_INSET);
    bottom - level_pos(db) * (bottom - top)
}

/// A vertical fader 0 … max (linear gain) on the shared level scale ([`level_pos`]).
pub fn fader(ui: &mut Ui, value: &mut f32, max: f32, height: f32) -> Response {
    let (rect, mut r) = ui.allocate_exact_size(Vec2::new(18.0, height), Sense::click_and_drag());
    let max_db = dragon_engine::dsp::gain_to_db(max);
    let to_n = |v: f32| level_pos(dragon_engine::dsp::gain_to_db(v.max(1e-9)).min(max_db));
    let from_n = |n: f32| if n <= 0.0 { 0.0 } else { dragon_engine::dsp::db_to_gain(level_db(n)).min(max) };
    let track = Rect::from_min_max(Pos2::new(rect.center().x - 2.0, rect.top() + LEVEL_INSET), Pos2::new(rect.center().x + 2.0, rect.bottom() - LEVEL_INSET));
    let mut n = to_n(*value);
    if r.dragged() {
        let fine = if ui.input(|i| i.modifiers.shift) { 0.15 } else { 1.0 };
        n = (n - r.drag_delta().y / track.height() * fine).clamp(0.0, 1.0);
        *value = from_n(n);
        r.mark_changed();
    }
    if r.double_clicked() {
        *value = 0.8;
        n = to_n(*value);
        r.mark_changed();
    }
    let show = |v: f32| if v <= 1e-6 { "-∞ dB".to_string() } else { format!("{:.1} dB", dragon_engine::dsp::gain_to_db(v)) };
    let before = *value;
    value_hook(ui, &mut r, value, 0.0, max, 0.8, &show);
    if *value != before {
        n = to_n(*value);
    }
    let p = ui.painter();
    p.rect_filled(track, 2.0, theme::BG);
    // Unity mark.
    let unity_y = level_y(rect, 0.0);
    p.line_segment([Pos2::new(rect.left(), unity_y), Pos2::new(rect.right(), unity_y)], Stroke::new(1.0_f32, theme::GRID_BAR));
    let y = track.bottom() - n * track.height();
    let cap = Rect::from_center_size(Pos2::new(rect.center().x, y), Vec2::new(18.0, 2.0 * LEVEL_INSET - 2.0));
    p.rect_filled(cap, 2.0, if r.dragged() { theme::ACCENT_2 } else { Color32::from_gray(200) });
    p.line_segment([Pos2::new(cap.left() + 2.0, y), Pos2::new(cap.right() - 2.0, y)], Stroke::new(1.0_f32, Color32::BLACK));
    let db = dragon_engine::dsp::gain_to_db(*value);
    let v = *value as f64;
    r.widget_info(|| egui::WidgetInfo { value: Some(v), current_text_value: Some(format!("{db:.1} dB")), ..egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, "Volume fader") });
    if keyboard_step(ui, &r, value, 0.0, max) {
        r.mark_changed();
    }
    r.on_hover_text(if *value <= 0.0 { "-∞ dB".to_string() } else { format!("{db:.1} dB") })
}

/// The marks of the shared level scale, with the numbers (dB) right-aligned against the meter beside
/// it; `height` as the meter's and fader's.
pub fn fader_scale(ui: &mut Ui, _max: f32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(26.0, height), Sense::hover());
    let p = ui.painter();
    let font = egui::FontId::monospace(8.5);
    let mut last_y = f32::NEG_INFINITY;
    // From the top down; a mark whose number would crowd the one above keeps only its tick.
    for db in [6.0f32, 3.0, 0.0, -3.0, -5.0, -10.0, -15.0, -20.0, -30.0, -40.0, -60.0] {
        let y = level_y(rect, db);
        let unity = db == 0.0;
        let major = matches!(db as i32, 6 | 0 | -10 | -20 | -30 | -40 | -60);
        let col = if unity { theme::TEXT } else { theme::TEXT_DIM };
        p.line_segment([Pos2::new(rect.right() - if major { 5.0 } else { 3.0 }, y), Pos2::new(rect.right(), y)], Stroke::new(1.0_f32, col));
        if y - last_y >= 9.5 || unity {
            let label = if db > 0.0 { format!("+{db:.0}") } else { format!("{db:.0}") };
            p.text(Pos2::new(rect.right() - 7.0, y), egui::Align2::RIGHT_CENTER, label, font.clone(), col);
            last_y = y;
        }
    }
    p.text(Pos2::new(rect.right() - 7.0, rect.bottom() - LEVEL_INSET), egui::Align2::RIGHT_CENTER, "-∞", font, theme::TEXT_DIM);
}

/// Stereo peak meter (linear values) on the shared level scale ([`level_pos`]), with the same ticks as the
/// scale beside it and a 0 dB line.
pub fn meter(ui: &mut Ui, l: f32, r: f32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(12.0, height), Sense::hover());
    let p = ui.painter();
    let well = Rect::from_min_max(Pos2::new(rect.left(), rect.top() + LEVEL_INSET), Pos2::new(rect.right(), rect.bottom() - LEVEL_INSET));
    p.rect_filled(well.expand2(Vec2::new(0.0, 1.0)), 1.0, theme::BG);
    for (i, v) in [l, r].into_iter().enumerate() {
        let x0 = rect.left() + i as f32 * 6.0 + 0.5;
        let db = dragon_engine::dsp::gain_to_db(v.max(1e-9));
        let bar = Rect::from_min_max(Pos2::new(x0, level_y(rect, db)), Pos2::new(x0 + 5.0, well.bottom()));
        if bar.height() > 0.0 {
            p.rect_filled(bar, 0.0, theme::meter_color(v));
        }
    }
    for db in [-10.0f32, -20.0, -30.0, -40.0] {
        let y = level_y(rect, db);
        p.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(1.0_f32, Color32::from_black_alpha(90)));
    }
    let zero = level_y(rect, 0.0);
    p.line_segment([Pos2::new(rect.left(), zero), Pos2::new(rect.right(), zero)], Stroke::new(1.0_f32, theme::REC.gamma_multiply(0.6)));
}

/// A small toggle button with an accent colour when on.
pub fn toggle(ui: &mut Ui, on: bool, text: &str, color: Color32) -> Response {
    let rt = egui::RichText::new(text).color(if on { Color32::BLACK } else { theme::TEXT_DIM });
    // Icon glyphs need the normal size to render crisply; words use the small style.
    let rt = if text.chars().count() <= 2 { rt } else { rt.small() };
    let b = egui::Button::new(rt).fill(if on { color } else { theme::PANEL_2 }).min_size(Vec2::new(ui.spacing().interact_size.y, 16.0));
    let r = ui.add(b);
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, on, text));
    r
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

/// The editing tools as large buttons: icon over its name, the active one lit, with a tooltip that says
/// what the tool does and its key.
pub fn tool_buttons(ui: &mut Ui, st: &mut State) {
    use crate::state::Tool;
    ui.spacing_mut().item_spacing.x = 3.0;
    // As tall as the bar's controls: square icon buttons (name and key in the tooltip); with room for
    // it (a bar 34 px or taller), the name under the icon.
    let h = ui.spacing().interact_size.y.max(BAR_H);
    let labelled = h >= 34.0;
    for (ti, (t, icon, key)) in Tool::ALL.into_iter().enumerate() {
        let (name, what) = t.describe();
        let on = st.ui.tool == t;
        let (r, resp) = ui.allocate_exact_size(if labelled { Vec2::new(46.0, h) } else { Vec2::new(h + 6.0, h) }, Sense::click());
        resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, on, format!("{name} tool")));
        let p = ui.painter();
        let fill = if on {
            theme::ACCENT
        } else if resp.hovered() {
            theme::GRID_BAR
        } else {
            theme::PANEL_2
        };
        p.rect_filled(r, 6.0, fill);
        if on {
            p.rect_stroke(r, 6.0, Stroke::new(1.5_f32, theme::ACCENT_2), egui::StrokeKind::Inside);
        }
        let ink = if on { Color32::BLACK } else { theme::TEXT };
        // The Higgsfield icon (dark on the lit button), the glyph if the icon set is missing.
        let ir = if labelled { Rect::from_center_size(Pos2::new(r.center().x, r.top() + 14.0), Vec2::splat(22.0)) } else { Rect::from_center_size(r.center(), Vec2::splat(h - 6.0)) };
        if on {
            p.rect_filled(ir.expand(1.0), 5.0, Color32::from_black_alpha(150));
        }
        if !crate::brand::paint_icon(p, ir, crate::brand::icon::DRAW + ti, Color32::WHITE) {
            p.text(ir.center(), egui::Align2::CENTER_CENTER, icon, egui::FontId::proportional(16.0), ink);
        }
        if labelled {
            p.text(Pos2::new(r.center().x, r.bottom() - 8.0), egui::Align2::CENTER_CENTER, name, egui::FontId::proportional(10.0), if on { Color32::BLACK } else { theme::TEXT_DIM });
        }
        let key = key.rsplit('(').next().unwrap_or("").trim_end_matches(')');
        if resp.on_hover_text(format!("{name} tool  ·  key {key}\n{what}")).clicked() {
            st.ui.tool = t;
        }
    }
}

/// The drag-and-drop payload released on `resp`, if it is a `T`. egui's own `dnd_release_payload` takes
/// the payload whatever its type, so asking for another type first (say the picker's) would swallow a
/// file dropped from the browser.
pub fn dnd_release<T: std::any::Any + Send + Sync>(resp: &egui::Response) -> Option<std::sync::Arc<T>> {
    if egui::DragAndDrop::has_payload_of_type::<T>(&resp.ctx) {
        resp.dnd_release_payload::<T>()
    } else {
        None
    }
}

/// One row of [`plugin_picker`]: the name, a dim detail (category or vendor · format) and the group
/// heading it is listed under ("Stock", "Plugins").
pub struct PickItem {
    pub name: String,
    pub detail: String,
    pub group: &'static str,
    pub tip: String,
}

/// A searchable, scrollable list of plugin names (stock first, then third-party), for the add-synth
/// and add-effect menus. The search box has the focus when the menu opens; Enter picks the first match.
/// Returns the index of the item picked.
pub fn plugin_picker(ui: &mut Ui, id: &str, items: &[PickItem]) -> Option<usize> {
    let qid = egui::Id::new(("plugin_picker.q", id));
    let mut q: String = ui.data(|d| d.get_temp(qid)).unwrap_or_default();
    ui.set_min_width(300.0);
    let edit = ui.add(egui::TextEdit::singleline(&mut q).hint_text("🔍 Search plugins …").desired_width(290.0));
    if !edit.has_focus() && !ui.ctx().memory(|m| m.focused().is_some()) {
        edit.request_focus();
    }
    ui.data_mut(|d| d.insert_temp(qid, q.clone()));
    let words: Vec<String> = q.to_lowercase().split_whitespace().map(str::to_string).collect();
    let hits: Vec<usize> = (0..items.len())
        .filter(|&k| {
            let hay = format!("{} {} {}", items[k].name, items[k].detail, items[k].group).to_lowercase();
            words.iter().all(|w| hay.contains(w.as_str()))
        })
        .collect();
    let mut picked = None;
    if edit.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        picked = hits.first().copied();
    }
    ui.add_space(4.0);
    egui::ScrollArea::vertical().id_salt(("plugin_picker.list", id)).max_height(380.0).auto_shrink([false, true]).show(ui, |ui| {
        if hits.is_empty() {
            ui.label(RichText::new("Nothing matches – plugins are found in View › Plugin manager").small().color(theme::TEXT_DIM));
        }
        let mut last_group = "";
        for &k in &hits {
            let it = &items[k];
            if it.group != last_group {
                last_group = it.group;
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    let k = if it.group.starts_with("Third") { crate::brand::icon::PLUGIN } else { crate::brand::icon::STAR };
                    if let Some(img) = crate::brand::icon_image(ui.ctx(), k, 13.0) {
                        ui.add(img);
                    }
                    ui.label(RichText::new(it.group).small().strong().color(theme::ACCENT_2));
                });
            }
            let w = ui.available_width();
            let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 22.0), egui::Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(r, 4.0, Color32::from_white_alpha(20));
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            ui.painter().text(r.left_center() + Vec2::new(8.0, 0.0), egui::Align2::LEFT_CENTER, &it.name, egui::FontId::proportional(13.0), theme::TEXT);
            ui.painter().text(r.right_center() - Vec2::new(8.0, 0.0), egui::Align2::RIGHT_CENTER, &it.detail, egui::FontId::proportional(10.5), theme::TEXT_DIM);
            let resp = if it.tip.is_empty() { resp } else { resp.on_hover_text(&it.tip) };
            if resp.clicked() {
                picked = Some(k);
            }
        }
    });
    if picked.is_some() {
        ui.data_mut(|d| d.remove::<String>(qid));
    }
    picked
}

/// A toggle with a Higgsfield icon before its label (the plain [`toggle`] if the icon set is missing).
pub fn icon_toggle(ui: &mut Ui, on: bool, icon: usize, label: &str, color: Color32) -> egui::Response {
    let Some(img) = crate::brand::icon_image(ui.ctx(), icon, 15.0) else { return toggle(ui, on, label, color) };
    let text = RichText::new(label).small().strong().color(if on { Color32::BLACK } else { theme::TEXT_DIM });
    let img = img.tint(if on { Color32::BLACK } else { Color32::from_gray(200) });
    // As tall as the bar's other controls (its interaction height).
    let r = ui.add(egui::Button::image_and_text(img, text).fill(if on { color } else { theme::PANEL_2 }).corner_radius(4.0).min_size(Vec2::new(0.0, ui.spacing().interact_size.y)));
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, on, label));
    r
}

#[cfg(test)]
mod tests {
    /// Right-click on the tempo display lists examples from 70 to 160 BPM, every 5 BPM, each with styles.
    #[test]
    fn bpm_examples_span_70_to_160() {
        let v: Vec<f64> = super::BPM_PRESETS.iter().map(|p| p.0).collect();
        assert_eq!(v.first(), Some(&70.0));
        assert_eq!(v.last(), Some(&160.0));
        assert!(v.windows(2).all(|w| w[1] > w[0]));
        assert!((70..=160).step_by(5).all(|b| v.contains(&(b as f64))), "{v:?}");
        assert!(super::BPM_PRESETS.iter().all(|p| !p.1.is_empty()));
    }

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

#[cfg(test)]
mod level_scale_tests {
    use super::*;

    /// The scale, the meter and the fader share one mapping: monotonic, round-trips, the marks where
    /// they should be, and a fader at a level shows the same dB as the scale at its height.
    #[test]
    fn one_scale_for_numbers_meters_and_faders() {
        let mut last = -1.0;
        for k in 0..=1000 {
            let db = -90.0 + k as f32 * 0.096;
            let n = level_pos(db);
            assert!(n >= last, "monotonic at {db}");
            last = n;
            if db > -80.0 {
                assert!((level_db(n) - db).abs() < 0.01, "round trip at {db}: {}", level_db(n));
            }
        }
        assert!((level_pos(6.0206) - 1.0).abs() < 1e-6);
        assert!((level_pos(0.0) - 0.80).abs() < 1e-6);
        assert!(level_pos(-200.0) < 1e-6);
        // Gain → fader position → gain.
        for g in [0.01f32, 0.1, 0.5, 0.8, 1.0, 1.5, 2.0] {
            let n = level_pos(dragon_engine::dsp::gain_to_db(g));
            let back = dragon_engine::dsp::db_to_gain(level_db(n));
            assert!((back - g).abs() / g < 1e-3, "{g} -> {back}");
        }
    }
}

// ------------------------------------------------------------------------------- set value (typing)
//
// Every knob and fader takes a typed value: right-click › "Set value…" opens a small text box under the
// control with its current value; Enter sets it. The value goes through the control's own change path, so
// it is one undo / redo step like a drag. Controls call [`value_hook`] (directly or through
// [`super::fx_panel::knob_input`]); controls with a right-click menu of their own put [`set_value_item`]
// in it, the others get a small default menu (Set value… / Reset) from [`value_menus`].

/// The open text box: which control, its text, whether it still has to take the focus.
#[derive(Clone)]
struct Prompt {
    id: egui::Id,
    text: String,
    focus: bool,
}

/// What every value control registered this frame (for the default menu).
#[derive(Clone, Default)]
struct Registry {
    pass: u64,
    /// (control, its rect, current text, right-clicked this frame)
    items: Vec<(egui::Id, Rect, String, bool)>,
}

/// The default menu: open for a control at a position.
#[derive(Clone)]
struct DefaultMenu {
    id: egui::Id,
    pos: Pos2,
    text: String,
    opened: u64,
}

fn prompt_key() -> egui::Id {
    egui::Id::new("value.prompt")
}
fn registry_key() -> egui::Id {
    egui::Id::new("value.registry")
}
fn menu_key() -> egui::Id {
    egui::Id::new("value.menu")
}
fn reset_key() -> egui::Id {
    egui::Id::new("value.reset")
}

/// Opens the text box for control `id` with `text` in it.
pub fn open_value_prompt(ctx: &egui::Context, id: egui::Id, text: String) {
    ctx.data_mut(|d| d.insert_temp(prompt_key(), Prompt { id, text, focus: true }));
}

/// Whether the default value menu is open (tests).
#[cfg(test)]
pub fn value_menu_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<DefaultMenu>(menu_key()).is_some())
}

/// The "Set value…" entry for a control's own right-click menu (`r` is the control).
pub fn set_value_item(ui: &mut Ui, r: &Response) {
    let text = ui.ctx().data(|d| d.get_temp::<Registry>(registry_key())).and_then(|reg| reg.items.iter().find(|x| x.0 == r.id).map(|x| x.2.clone()));
    if ui.add_enabled(text.is_some(), egui::Button::new("Set value…")).on_hover_text("Type an exact value (Enter sets it)").clicked() {
        open_value_prompt(ui.ctx(), r.id, text.unwrap_or_default());
        ui.close_menu();
    }
}

/// The number in a shown value ("−6.0 dB" → −6, "L 40" → −40, "2.5 kHz" → 2500, "-∞" → −∞).
fn number_of(s: &str) -> Option<f32> {
    let t = s.trim().to_lowercase().replace('−', "-");
    if t.contains('∞') || t.contains("inf") {
        return Some(if t.starts_with('+') { f32::INFINITY } else { f32::NEG_INFINITY });
    }
    // Pan style: "L 40" / "R 40" / "C".
    if let Some(rest) = t.strip_prefix('l').filter(|r| r.trim_start().starts_with(|c: char| c.is_ascii_digit())) {
        return parse_value(rest).map(|x| -x);
    }
    if let Some(rest) = t.strip_prefix('r').filter(|r| r.trim_start().starts_with(|c: char| c.is_ascii_digit())) {
        return parse_value(rest);
    }
    // "40L" / "40R" (how pan is shown).
    if let Some(num) = t.strip_suffix('l').filter(|n| n.trim().parse::<f32>().is_ok()) {
        return num.trim().parse::<f32>().ok().map(|x| -x);
    }
    if let Some(num) = t.strip_suffix('r').filter(|n| n.trim().parse::<f32>().is_ok()) {
        return num.trim().parse::<f32>().ok();
    }
    if t == "c" || t == "center" || t == "centre" {
        return Some(0.0);
    }
    let start = t.find(|c: char| c.is_ascii_digit() || matches!(c, '-' | '+' | '.'))?;
    parse_value(&t[start..])
}

/// The value in `min … max` whose shown text means what was typed: an exact text match (note names, option
/// words), else the number, found along the control's own display (so "−6 dB" on a gain fader, "40 %" on a
/// 0 … 1 knob and "250 ms" on a knob in seconds all land right).
pub fn value_from_text(typed: &str, min: f32, max: f32, show: &dyn Fn(f32) -> String) -> Option<f32> {
    let (lo, hi) = (min.min(max), min.max(max));
    let typed_l = typed.trim().to_lowercase();
    const N: usize = 1024;
    let at = |k: usize| lo + (hi - lo) * k as f32 / N as f32;
    // Exact text (note names and the like).
    if let Some(k0) = (0..=N).find(|&k| show(at(k)).trim().to_lowercase() == typed_l) {
        // The middle of what shows this text (the ends stay the ends).
        let k1 = (k0..=N).take_while(|&k| show(at(k)).trim().to_lowercase() == typed_l).last().unwrap_or(k0);
        return Some(if k0 == 0 {
            lo
        } else if k1 == N {
            hi
        } else {
            0.5 * (at(k0) + at(k1))
        });
    }
    let target = number_of(typed)?;
    let num = |v: f32| number_of(&show(v));
    if target == f32::NEG_INFINITY {
        return Some(lo);
    }
    // The display in steps: runs of samples that show the same number become one point at the run's
    // centre (a value shown rounded to "40 %" lands in the middle of what shows as 40 %), then the typed
    // number is interpolated between the neighbouring points.
    let mut pts: Vec<(f32, f32)> = Vec::new();
    let mut run: Option<(f32, f32, f32)> = None; // (first v, last v, number)
    for k in 0..=N {
        let v = at(k);
        let Some(x) = num(v).filter(|x| x.is_finite()) else { continue };
        match run {
            Some((a, _, y)) if y == x => run = Some((a, v, y)),
            _ => {
                if let Some((a, b, y)) = run {
                    pts.push((0.5 * (a + b), y));
                }
                run = Some((v, v, x));
            }
        }
    }
    if let Some((a, b, y)) = run {
        // The ends of the range stay the ends.
        pts.push((if b >= hi { b } else { 0.5 * (a + b) }, y));
    }
    if let Some(first) = pts.first_mut() {
        if first.0 - lo < (hi - lo) / N as f32 * 8.0 {
            first.0 = lo;
        }
    }
    let first = *pts.first()?;
    let last = *pts.last()?;
    let rising = last.1 >= first.1;
    let (bottom, top) = if rising { (first, last) } else { (last, first) };
    if target >= top.1 {
        return Some(top.0);
    }
    if target <= bottom.1 {
        return Some(bottom.0);
    }
    for w in pts.windows(2) {
        let ((v0, y0), (v1, y1)) = (w[0], w[1]);
        if (target - y0) * (target - y1) <= 0.0 && y0 != y1 {
            return Some((v0 + (v1 - v0) * (target - y0) / (y1 - y0)).clamp(lo, hi));
        }
    }
    // Not monotonic and not between two neighbours: the nearest point.
    pts.iter().min_by(|a, b| (a.1 - target).abs().total_cmp(&(b.1 - target).abs())).map(|p| p.0)
}

/// Gives a control its typed value: registers it for the default right-click menu, shows its text box when
/// open and, on Enter, sets `value` (in the control's own range `min … max`, shown by `show`) and marks
/// `r` changed – the caller's usual change handling makes it an undo step. `default` is for Reset.
pub fn value_hook(ui: &Ui, r: &mut Response, value: &mut f32, min: f32, max: f32, default: f32, show: &dyn Fn(f32) -> String) {
    let ctx = ui.ctx().clone();
    let pass = ctx.cumulative_pass_nr();
    let text = show(*value);
    // Read before taking the data lock (the response asks the input state).
    let right = r.secondary_clicked();
    ctx.data_mut(|d| {
        let reg = d.get_temp_mut_or_default::<Registry>(registry_key());
        if reg.pass != pass {
            reg.pass = pass;
            reg.items.clear();
        }
        reg.items.push((r.id, r.rect, text.clone(), right));
    });
    // Reset from the default menu.
    if ctx.data(|d| d.get_temp::<egui::Id>(reset_key())) == Some(r.id) {
        ctx.data_mut(|d| d.remove::<egui::Id>(reset_key()));
        *value = default;
        r.mark_changed();
    }
    let Some(mut pr) = ctx.data(|d| d.get_temp::<Prompt>(prompt_key())).filter(|p| p.id == r.id) else { return };
    let mut close = false;
    let mut set = None;
    egui::Area::new(r.id.with("value.box")).order(egui::Order::Foreground).fixed_pos(r.rect.left_bottom() + Vec2::new(0.0, 2.0)).show(&ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.label(RichText::new("Set value – Enter to set, Esc to cancel").small().color(theme::TEXT_DIM));
            let out = egui::TextEdit::singleline(&mut pr.text).desired_width(120.0).show(ui);
            let te = out.response;
            if pr.focus {
                // Focused with the whole value selected: typing replaces it.
                te.request_focus();
                let mut state = out.state;
                let n = pr.text.chars().count();
                state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(n))));
                state.store(ui.ctx(), te.id);
                pr.focus = false;
            }
            let (enter, esc) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
            if enter {
                match value_from_text(&pr.text, min, max, show) {
                    Some(v) => {
                        set = Some(v);
                        close = true;
                    }
                    None => {
                        pr.text = show(*value);
                    }
                }
            } else if esc || (te.lost_focus() && !enter) {
                close = true;
            }
        });
    });
    if let Some(v) = set {
        *value = v;
        r.mark_changed();
    }
    ctx.data_mut(|d| if close { d.remove::<Prompt>(prompt_key()) } else { d.insert_temp(prompt_key(), pr) });
}

/// The default right-click menu (Set value… / Reset to default) of value controls that have no menu of
/// their own. Called once per frame after all views.
pub fn value_menus(ctx: &egui::Context) {
    let pass = ctx.cumulative_pass_nr();
    let reg: Registry = ctx.data(|d| d.get_temp(registry_key())).unwrap_or_default();
    // Opened this frame: a control was right-clicked and no menu of its own came up.
    if reg.pass == pass && !ctx.is_context_menu_open() {
        if let Some((id, _, text, _)) = reg.items.iter().find(|x| x.3) {
            let pos = ctx.input(|i| i.pointer.interact_pos()).unwrap_or_default();
            ctx.data_mut(|d| d.insert_temp(menu_key(), DefaultMenu { id: *id, pos, text: text.clone(), opened: pass }));
        }
    }
    let Some(m) = ctx.data(|d| d.get_temp::<DefaultMenu>(menu_key())) else { return };
    let mut close = false;
    let area = egui::Area::new(egui::Id::new("value.menu.area")).order(egui::Order::Foreground).fixed_pos(m.pos).show(ctx, |ui| {
        egui::Frame::menu(ui.style()).show(ui, |ui| {
            ui.set_min_width(140.0);
            ui.label(RichText::new(&m.text).small().strong());
            ui.separator();
            if ui.button("Set value…").on_hover_text("Type an exact value (Enter sets it)").clicked() {
                open_value_prompt(ctx, m.id, m.text.clone());
                close = true;
            }
            if ui.button("Reset to default").clicked() {
                ctx.data_mut(|d| d.insert_temp(reset_key(), m.id));
                close = true;
            }
        });
    });
    // Any click elsewhere (after the frame that opened it) or Esc closes it.
    let clicked_out = pass > m.opened && ctx.input(|i| i.pointer.any_pressed()) && ctx.pointer_interact_pos().is_some_and(|p| !area.response.rect.contains(p));
    if close || clicked_out || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        ctx.data_mut(|d| d.remove::<DefaultMenu>(menu_key()));
    }
    if close {
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod value_tests {
    use super::*;

    #[test]
    fn typed_values_follow_the_display() {
        // Gain fader shown in dB.
        let db = |v: f32| if v <= 0.0 { "-∞ dB".to_string() } else { format!("{:.1} dB", dragon_engine::dsp::gain_to_db(v)) };
        let g = value_from_text("-6", 0.0, 2.0, &db).unwrap();
        assert!((dragon_engine::dsp::gain_to_db(g) + 6.0).abs() < 0.05, "{g}");
        assert_eq!(value_from_text("-inf", 0.0, 2.0, &db), Some(0.0));
        assert!((value_from_text("+20 dB", 0.0, 2.0, &db).unwrap() - 2.0).abs() < 1e-3, "clamped to the top");
        // Percent of 0 … 1.
        assert!((value_from_text("40%", 0.0, 1.0, &pct).unwrap() - 0.4).abs() < 1e-3);
        // Pan.
        assert!((value_from_text("L 50", -1.0, 1.0, &pan_fmt).unwrap() + 0.5).abs() < 0.01);
        assert!((value_from_text("30R", -1.0, 1.0, &pan_fmt).unwrap() - 0.3).abs() < 0.01);
        assert!(value_from_text("C", -1.0, 1.0, &pan_fmt).unwrap().abs() < 0.01);
        // Seconds shown as ms.
        let ms = |v: f32| format!("{:.0} ms", v * 1000.0);
        assert!((value_from_text("250", 0.0, 1.0, &ms).unwrap() - 0.25).abs() < 1e-3);
        // Note names.
        let note = |v: f32| dragon_engine::instruments::note_name(v.round() as u8);
        assert_eq!(value_from_text(&note(64.0), 0.0, 127.0, &note).map(|v| v.round()), Some(64.0));
        // Nonsense keeps the value.
        assert_eq!(value_from_text("abc", 0.0, 1.0, &pct), None);
    }
}
