//! The caves: Dragon's Lair (reverb) and Echo Cavern (delay). Each display is a scene painted over a
//! Higgsfield backdrop, and every graph in it is a control: grab a glowing handle to move what it
//! shows, drag empty space for the two main settings, double-click a handle to reset it. Live levels
//! come from the effects themselves (with no audio they rest).

use super::fx_mod::{art, glow, halo};
use super::fx_ui::{decay_curve, freq_at, fx, Hero};
use dragon_engine::dsp::{db_to_gain, gain_to_db};
use egui::{Color32, Mesh, Pos2, Rect, Shape, Stroke, Ui, Vec2};

const LAIR: &[u8] = include_bytes!("../../assets/fx/lair.jpg");
const CAVERN: &[u8] = include_bytes!("../../assets/fx/cavern.jpg");
const CHAMBER: &[u8] = include_bytes!("../../assets/fx/chamber.jpg");
const STALACTITE: &[u8] = include_bytes!("../../assets/fx/stalactite.png");
const DROP: &[u8] = include_bytes!("../../assets/fx/drop.png");
const EYE: &[u8] = include_bytes!("../../assets/fx/dragon_eye.png");

const VIOLET: Color32 = Color32::from_rgb(0xb4, 0x7c, 0xff);
const AMBER: Color32 = Color32::from_rgb(0xff, 0xa8, 0x48);
const TEAL: Color32 = Color32::from_rgb(0x52, 0xf0, 0xd8);
const BONE: Color32 = Color32::from_rgb(0xe8, 0xe2, 0xd4);

/// A backdrop photograph covering `r` (cropped to keep its aspect), dimmed by `k`.
fn backdrop(p: &egui::Painter, r: Rect, name: &'static str, bytes: &[u8], k: f32) {
    let Some(t) = art(p.ctx(), name, bytes) else { return };
    let ts = t.size_vec2();
    let (ra, ta) = (r.width() / r.height(), ts.x / ts.y);
    let uv = if ra > ta {
        let h = ta / ra;
        Rect::from_min_max(Pos2::new(0.0, 0.5 - h / 2.0), Pos2::new(1.0, 0.5 + h / 2.0))
    } else {
        let w = ra / ta;
        Rect::from_min_max(Pos2::new(0.5 - w / 2.0, 0.0), Pos2::new(0.5 + w / 2.0, 1.0))
    };
    let g = (255.0 * k) as u8;
    p.image(t.id(), r, uv, Color32::from_rgb(g, g, g));
}

/// A glass pane over the scene, for a graph.
fn pane(p: &egui::Painter, r: Rect, edge: Color32) {
    p.rect_filled(r, 6.0, Color32::from_black_alpha(75));
    p.rect_stroke(r, 6.0, Stroke::new(1.0_f32, edge.gamma_multiply(0.35)), egui::StrokeKind::Inside);
}

/// A grabbable handle: a glowing bead with its name; bigger and brighter while hovered or held.
fn handle(p: &egui::Painter, at: Pos2, label: &str, c: Color32, hot: bool) {
    halo(p, at, if hot { 22.0 } else { 14.0 }, c, if hot { 0.7 } else { 0.4 });
    p.circle_filled(at, if hot { 6.5 } else { 5.0 }, c);
    p.circle_stroke(at, if hot { 6.5 } else { 5.0 }, Stroke::new(1.5_f32, Color32::WHITE));
    let right = at.x + 9.0 + 7.0 * label.len() as f32 > p.clip_rect().right() - 6.0;
    let (off, align) = if right { (Vec2::new(-9.0, -9.0), egui::Align2::RIGHT_BOTTOM) } else { (Vec2::new(9.0, -9.0), egui::Align2::LEFT_BOTTOM) };
    p.text(at + off, align, label, egui::FontId::proportional(9.0), if hot { Color32::WHITE } else { c });
}

fn small(p: &egui::Painter, at: Pos2, align: egui::Align2, text: impl ToString, c: Color32) {
    p.text(at, align, text.to_string(), egui::FontId::proportional(9.0), c);
}

/// The id of the handle under the pointer or held (0 = none), kept per display.
fn hot(ui_ctx: &egui::Context, key: egui::Id) -> usize {
    ui_ctx.data(|d| d.get_temp(key.with("cave.hot"))).unwrap_or(0)
}

fn set_hot(ui: &Ui, key: egui::Id, v: usize) {
    ui.data_mut(|d| d.insert_temp(key.with("cave.hot"), v));
}

/// Pushes `id` set to the value at normalized `u`, or its default on double-click.
fn param_at(h: &Hero, id: &str, v: f32, out: &mut Vec<(String, f32)>) {
    if let Some(p) = h.def.params.iter().find(|p| p.id == id) {
        out.push((id.to_string(), v.clamp(p.min, p.max)));
    }
}

fn reset(h: &Hero, ids: &[&str], out: &mut Vec<(String, f32)>) {
    for id in ids {
        if let Some(p) = h.def.params.iter().find(|p| p.id == *id) {
            out.push((id.to_string(), p.default));
        }
    }
}

fn nudge(h: &Hero, id: &str, du: f32, out: &mut Vec<(String, f32)>) {
    if let (Some(prm), true) = (h.def.params.iter().find(|p| p.id == id), du != 0.0) {
        out.push((id.to_string(), prm.denormalize((prm.normalize(h.v(id)) + du).clamp(0.0, 1.0))));
    }
}

// ---------------------------------------------------------------------------------- Dragon's Lair

/// The lair's three panes: the decay envelope, the chamber (size × width) and the decay-rate EQ.
pub fn lair_layout(r: Rect) -> (Rect, Rect, Rect) {
    let inner = r.shrink(8.0);
    let side = inner.height().min(inner.width() * 0.2);
    let dr_w = inner.width() * 0.27;
    let env = Rect::from_min_size(inner.min, Vec2::new(inner.width() - side - dr_w - 20.0, inner.height()));
    let room = Rect::from_min_size(Pos2::new(env.right() + 10.0, inner.top()), Vec2::new(side, inner.height()));
    let dr = Rect::from_min_max(Pos2::new(room.right() + 10.0, inner.top()), inner.max);
    (env, room, dr)
}

/// The envelope's plotting area, and its time span (s) — frozen while a handle is held so the axis
/// does not slide under the pointer.
fn lair_plot(env: Rect, h: &Hero, frozen: Option<f32>) -> (Rect, f32) {
    let pr = Rect::from_min_max(env.min + Vec2::new(30.0, 30.0), env.max - Vec2::new(10.0, 16.0));
    let span = frozen.unwrap_or_else(|| (h.v("predelay") / 1000.0 + h.v("decay") * 0.9).clamp(0.3, 22.0));
    (pr, span)
}

const LAIR_DB: f32 = 48.0;

struct LairNodes {
    pre: Pos2,
    late: Pos2,
    decay: Pos2,
    early: Pos2,
}

fn lair_nodes(pr: Rect, span: f32, h: &Hero) -> LairNodes {
    let x_of = |s: f32| pr.left() + s / span * pr.width();
    let y_of = |db: f32| pr.top() + (-db.clamp(-LAIR_DB, 0.0) / LAIR_DB) * pr.height();
    let start = h.v("predelay") / 1000.0;
    let late_db = gain_to_db(h.v("late").max(0.004));
    let er_end = start + 0.072 * h.v("er_size") * (1.0 + h.v("size"));
    LairNodes {
        pre: Pos2::new(x_of(start), pr.bottom() - 1.0),
        late: Pos2::new(x_of(start), y_of(late_db)),
        decay: Pos2::new(x_of(start + h.v("decay") * 0.5), y_of(late_db - 30.0)),
        early: Pos2::new(x_of(er_end), y_of(gain_to_db(h.v("early").max(0.004)))),
    }
}

/// The room pad's square and its value position.
fn room_pad(room: Rect) -> Rect {
    let s = room.width().min(room.height() - 22.0);
    Rect::from_center_size(Pos2::new(room.center().x, room.top() + 4.0 + s / 2.0), Vec2::splat(s))
}

pub fn lair(p: &egui::Painter, r: Rect, h: &Hero) {
    let t = h.t();
    p.rect_filled(r, 6.0, Color32::from_rgb(0x06, 0x04, 0x0a));
    backdrop(p, r, "fx-lair", LAIR, 0.85);
    let (env, room, dr) = lair_layout(r);
    let hot_id = hot(p.ctx(), h.key);
    let frozen: Option<f32> = p.ctx().data(|d| d.get_temp(h.key.with("cave.span")));
    // ---- the decay envelope
    pane(p, env, VIOLET);
    let (pr, span) = lair_plot(env, h, if hot_id > 0 { frozen } else { None });
    let x_of = |s: f32| pr.left() + s / span * pr.width();
    let y_of = |db: f32| pr.top() + (-db.clamp(-LAIR_DB, 0.0) / LAIR_DB) * pr.height();
    for db in [0.0, -12.0, -24.0, -36.0, -48.0] {
        let y = y_of(db);
        p.line_segment([Pos2::new(pr.left(), y), Pos2::new(pr.right(), y)], Stroke::new(1.0_f32, Color32::from_white_alpha(14)));
        small(p, Pos2::new(pr.left() - 4.0, y), egui::Align2::RIGHT_CENTER, format!("{db:.0}"), BONE.gamma_multiply(0.6));
    }
    for k in 0..=4 {
        let s = span * k as f32 / 4.0;
        small(p, Pos2::new(x_of(s), pr.bottom() + 3.0), egui::Align2::CENTER_TOP, format!("{s:.1} s"), BONE.gamma_multiply(0.6));
    }
    let start = h.v("predelay") / 1000.0;
    let decay = h.v("decay").max(0.1);
    let late_db = gain_to_db(h.v("late").max(0.004));
    let env_db = |s: f32| late_db - 60.0 * (s - start) / decay;
    // The tail: violet mist under the envelope, and its texture shimmering through it.
    let mut mesh = Mesh::default();
    let mut line = Vec::new();
    let mut x = x_of(start);
    while x <= pr.right() {
        let s = (x - pr.left()) / pr.width() * span;
        let y = y_of(env_db(s));
        line.push(Pos2::new(x, y));
        let i = mesh.vertices.len() as u32;
        mesh.colored_vertex(Pos2::new(x, y), glow(VIOLET, 0.32));
        mesh.colored_vertex(Pos2::new(x, pr.bottom()), glow(VIOLET, 0.02));
        if i >= 2 {
            mesh.add_triangle(i - 2, i - 1, i);
            mesh.add_triangle(i - 1, i + 1, i);
        }
        x += 2.0;
    }
    p.add(Shape::mesh(mesh));
    let mut seed = 0x9e37_79b9u32;
    let mut x = x_of(start);
    while x < pr.right() {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let s = (x - pr.left()) / pr.width() * span;
        let n = (seed % 1000) as f32 / 1000.0 * (0.75 + 0.25 * (t * 2.5 + x * 0.21).sin());
        let db = env_db(s) + gain_to_db((0.3 + 0.7 * n).max(1e-3));
        if db > -LAIR_DB {
            p.line_segment([Pos2::new(x, pr.bottom()), Pos2::new(x, y_of(db))], Stroke::new(1.0_f32, glow(Color32::from_rgb(0xd8, 0xc0, 0xff), 0.22)));
        }
        x += 2.0;
    }
    if line.len() > 1 {
        p.add(Shape::line(line.clone(), Stroke::new(6.0_f32, glow(VIOLET, 0.25))));
        p.add(Shape::line(line, Stroke::new(1.8_f32, VIOLET)));
    }
    // Early reflections: amber spikes from the pre-delay to their window's end.
    let er_end = start + 0.072 * h.v("er_size") * (1.0 + h.v("size"));
    let early = h.v("early");
    for k in 0..8 {
        let s = start + (er_end - start) * (k as f32 + 1.0) / 8.0;
        let a = early * (1.0 - k as f32 / 10.0);
        if a > 0.004 {
            p.line_segment([Pos2::new(x_of(s), pr.bottom()), Pos2::new(x_of(s), y_of(gain_to_db(a)))], Stroke::new(2.0_f32, AMBER.gamma_multiply(0.85)));
        }
    }
    // The pre-delay: the gap before the cave answers.
    if start > 0.0005 {
        p.rect_filled(Rect::from_min_max(pr.left_top(), Pos2::new(x_of(start), pr.bottom())), 0.0, Color32::from_black_alpha(60));
    }
    // Live: what goes in and what the cave gives back (real peaks).
    if let [i, o, ..] = h.tele {
        for (k, (lbl, v, c)) in [("IN", *i, BONE), ("OUT", *o, VIOLET)].into_iter().enumerate() {
            let db = gain_to_db(v.max(1e-6));
            if db > -LAIR_DB {
                let y = y_of(db);
                let x = pr.left() - 2.0 - k as f32 * 6.0;
                p.add(Shape::convex_polygon(vec![Pos2::new(x, y), Pos2::new(x - 5.0, y - 3.5), Pos2::new(x - 5.0, y + 3.5)], c, Stroke::NONE));
                let _ = lbl;
            }
        }
        let fmt = |v: f32| if v < 1e-5 { "silent".to_string() } else { format!("{:.1} dB", gain_to_db(v)) };
        small(p, Pos2::new(env.right() - 8.0, env.top() + 6.0), egui::Align2::RIGHT_TOP, format!("IN {}   OUT {}", fmt(*i), fmt(*o)), BONE.gamma_multiply(0.85));
    }
    let modes = match h.def.param("mode").map(|p| p.kind) {
        Some(dragon_engine::effects::ParamKind::Choice(n)) => n,
        _ => &[],
    };
    let mode = modes.get(h.v("mode").round() as usize).copied().unwrap_or("");
    p.text(env.left_top() + Vec2::new(8.0, 5.0), egui::Align2::LEFT_TOP, format!("{}  ·  {decay:.2} s  ·  pre-delay {:.0} ms", mode.to_uppercase(), start * 1000.0), egui::FontId::proportional(10.5), BONE);
    // The handles.
    let n = lair_nodes(pr, span, h);
    let ep = p.with_clip_rect(env.expand(2.0));
    handle(&ep, n.late, "LATE", VIOLET, hot_id == 3);
    handle(&ep, n.decay, "DECAY", VIOLET, hot_id == 2);
    handle(&ep, n.early, "EARLY", AMBER, hot_id == 1);
    // Pre-delay: a diamond on the time axis.
    let d = n.pre;
    let s = if hot_id == 4 { 7.0 } else { 5.5 };
    halo(p, d, 14.0, BONE, if hot_id == 4 { 0.6 } else { 0.3 });
    p.add(Shape::convex_polygon(vec![d + Vec2::new(0.0, -s), d + Vec2::new(s, 0.0), d + Vec2::new(0.0, s), d + Vec2::new(-s, 0.0)], BONE, Stroke::new(1.0_f32, Color32::BLACK)));
    small(p, d + Vec2::new(8.0, -4.0), egui::Align2::LEFT_BOTTOM, "PRE", if hot_id == 4 { Color32::WHITE } else { BONE });
    // ---- the chamber: its size (up) and width (across), the dragon's eye on the spot
    pane(p, room, VIOLET);
    let pad = room_pad(room);
    let size = h.v("size");
    let width = h.v("width");
    if let Some(tx) = art(p.ctx(), "fx-chamber", CHAMBER) {
        let cw = pad.width() * (0.4 + 0.6 * width / 1.5);
        let ch = pad.height() * (0.3 + 0.7 * (size - 0.1) / 0.9);
        let cr = Rect::from_center_size(pad.center(), Vec2::new(cw, ch));
        p.with_clip_rect(pad).image(tx.id(), cr, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        p.rect_stroke(cr, 4.0, Stroke::new(1.0_f32, VIOLET.gamma_multiply(0.4)), egui::StrokeKind::Outside);
    }
    let eye_at = Pos2::new(pad.left() + pad.width() * (width / 1.5), pad.bottom() - pad.height() * ((size - 0.1) / 0.9));
    p.line_segment([Pos2::new(pad.left(), eye_at.y), Pos2::new(pad.right(), eye_at.y)], Stroke::new(1.0_f32, AMBER.gamma_multiply(0.25)));
    p.line_segment([Pos2::new(eye_at.x, pad.top()), Pos2::new(eye_at.x, pad.bottom())], Stroke::new(1.0_f32, AMBER.gamma_multiply(0.25)));
    halo(p, eye_at, 20.0, AMBER, if hot_id == 5 { 0.7 } else { 0.4 });
    if let Some(tx) = art(p.ctx(), "fx-dragon-eye", EYE) {
        // The eye opens a little as the room grows.
        let s = if hot_id == 5 { 30.0 } else { 26.0 };
        p.image(tx.id(), Rect::from_center_size(eye_at, Vec2::new(s, s * (0.55 + 0.4 * size))), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    }
    small(p, Pos2::new(room.center().x, room.bottom() - 4.0), egui::Align2::CENTER_BOTTOM, format!("SIZE {:.0} %   WIDTH {:.0} %", size * 100.0, width * 100.0), BONE.gamma_multiply(0.85));
    // ---- the decay-rate EQ (its handles are the band dots)
    pane(p, dr, VIOLET);
    decay_curve(p, dr.shrink(4.0), h, VIOLET);
}

/// Handles of the lair: 1 early, 2 decay, 3 late, 4 pre-delay, 5 the chamber. (The decay-rate EQ in its
/// own pane is handled by the caller.)
pub fn lair_interact(ui: &Ui, r: Rect, resp: &egui::Response, h: &Hero, out: &mut Vec<(String, f32)>) {
    let Some(pos) = resp.interact_pointer_pos().or_else(|| resp.hover_pos()) else { return };
    let (env, room, _) = lair_layout(r);
    let frozen: Option<f32> = ui.data(|d| d.get_temp(h.key.with("cave.span")));
    let held: usize = ui.data(|d| d.get_temp(h.key.with("cave.held"))).unwrap_or(0);
    let (pr, span) = lair_plot(env, h, if held > 0 { frozen } else { None });
    let n = lair_nodes(pr, span, h);
    let pad = room_pad(room);
    let pick = |at: Pos2| -> usize {
        let c = [(1, n.early), (2, n.decay), (3, n.late), (4, n.pre)];
        c.iter().filter(|(_, q)| q.distance(at) < 13.0).min_by(|a, b| a.1.distance(at).total_cmp(&b.1.distance(at))).map(|x| x.0).unwrap_or(if pad.expand(6.0).contains(at) { 5 } else { 0 })
    };
    let s_at = |x: f32| ((x - pr.left()) / pr.width() * span).max(0.0);
    let db_at = |y: f32| -((y - pr.top()) / pr.height()).clamp(0.0, 1.0) * LAIR_DB;
    if resp.drag_started() {
        let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
        let k = pick(origin);
        ui.data_mut(|d| {
            d.insert_temp(h.key.with("cave.held"), k);
            d.insert_temp(h.key.with("cave.span"), span);
        });
    }
    let held: usize = ui.data(|d| d.get_temp(h.key.with("cave.held"))).unwrap_or(0);
    let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
    if resp.dragged() {
        let start = h.v("predelay") / 1000.0;
        match held {
            1 => {
                param_at(h, "early", db_to_gain(db_at(pos.y)), out);
                param_at(h, "er_size", (s_at(pos.x) - start) / (0.072 * (1.0 + h.v("size"))), out);
            }
            2 => param_at(h, "decay", 2.0 * (s_at(pos.x) - start), out),
            3 => param_at(h, "late", db_to_gain(db_at(pos.y)), out),
            4 => param_at(h, "predelay", s_at(pos.x) * 1000.0, out),
            5 => {
                param_at(h, "width", ((pos.x - pad.left()) / pad.width()).clamp(0.0, 1.0) * 1.5, out);
                param_at(h, "size", 0.1 + 0.9 * ((pad.bottom() - pos.y) / pad.height()).clamp(0.0, 1.0), out);
            }
            _ => {
                let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
                if env.contains(origin) {
                    // Empty cave: across is the decay, up / down the size.
                    let d = resp.drag_delta() * fine;
                    nudge(h, "decay", d.x / env.width(), out);
                    nudge(h, "size", -d.y / env.height(), out);
                }
            }
        }
    }
    if resp.drag_stopped() {
        ui.data_mut(|d| d.insert_temp(h.key.with("cave.held"), 0usize));
    }
    let under = if held > 0 { held } else { pick(pos) };
    set_hot(ui, h.key, if env.contains(pos) || room.contains(pos) || held > 0 { under } else { 0 });
    if resp.double_clicked() {
        match pick(pos) {
            1 => reset(h, &["early", "er_size"], out),
            2 => reset(h, &["decay"], out),
            3 => reset(h, &["late"], out),
            4 => reset(h, &["predelay"], out),
            5 => reset(h, &["size", "width"], out),
            _ => {}
        }
    }
    if under > 0 {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
}

// ----------------------------------------------------------------------------------- Echo Cavern

/// The cavern's two panes: the echoes (left and right lanes) and the feedback filter.
pub fn cavern_layout(r: Rect) -> (Rect, Rect) {
    let inner = r.shrink(8.0);
    let fw = (inner.width() * 0.28).max(160.0);
    (Rect::from_min_size(inner.min, Vec2::new(inner.width() - fw - 10.0, inner.height())), Rect::from_min_max(Pos2::new(inner.right() - fw, inner.top()), inner.max))
}

struct Cavern {
    pr: Rect,
    lanes: [Rect; 2],
    span: f32,
    time: f32,
}

fn cavern_geo(taps: Rect, h: &Hero, frozen: Option<f32>) -> Cavern {
    let pr = Rect::from_min_max(taps.min + Vec2::new(24.0, 26.0), taps.max - Vec2::new(10.0, 16.0));
    let mid = pr.center().y;
    let lanes = [Rect::from_min_max(pr.min, Pos2::new(pr.right(), mid - 3.0)), Rect::from_min_max(Pos2::new(pr.left(), mid + 3.0), pr.max)];
    let time = effective_time(h);
    let span = frozen.unwrap_or((time * 6.5).max(400.0));
    Cavern { pr, lanes, span, time }
}

/// The echo time in effect (ms): reported by the delay (tempo-synced or free), or the free time.
fn effective_time(h: &Hero) -> f32 {
    match h.tele {
        [t, ..] if *t > 0.0 => *t,
        _ => h.v("time").max(1.0),
    }
}

/// A repeat: its lane (0 left, 1 right), time (ms) and level (linear).
fn echoes(h: &Hero, time: f32, span: f32) -> Vec<(usize, f32, f32)> {
    let fb = h.v("feedback").clamp(0.0, 0.99);
    let ping = h.v("mode").round() as i32 == 1;
    let off = h.v("offset");
    let mut v = Vec::new();
    let mut level = 1.0f32;
    for k in 1..=32 {
        let t = time * k as f32;
        if t > span || level < 0.016 {
            break;
        }
        if ping {
            v.push(((k + 1) % 2, t, level));
        } else {
            v.push((0, t, level));
            let tr = time * (1.0 + off) * k as f32;
            if tr <= span {
                v.push((1, tr, level));
            }
        }
        level *= fb;
        if fb <= 0.0 {
            break;
        }
    }
    v
}

const CAVE_DB: f32 = 36.0;

/// How far a stalactite of `level` reaches down its lane (0 … 1).
fn reach(level: f32) -> f32 {
    0.1 + 0.85 * ((gain_to_db(level.max(1e-4)) + CAVE_DB) / CAVE_DB).clamp(0.0, 1.0)
}

fn level_at(lane: Rect, y: f32) -> f32 {
    let u = ((y - lane.top()) / lane.height()).clamp(0.0, 1.0);
    db_to_gain(((u - 0.1) / 0.85).clamp(0.0, 1.0) * CAVE_DB - CAVE_DB)
}

struct CavernNodes {
    time: Pos2,
    feedback: Pos2,
    offset: Option<Pos2>,
}

fn cavern_nodes(g: &Cavern, h: &Hero) -> CavernNodes {
    let x_of = |ms: f32| g.pr.left() + ms / g.span * g.pr.width();
    let tip = |lane: usize, ms: f32, level: f32| Pos2::new(x_of(ms), g.lanes[lane].top() + g.lanes[lane].height() * reach(level));
    let ping = h.v("mode").round() as i32 == 1;
    let fb = h.v("feedback");
    CavernNodes { time: tip(0, g.time, 1.0), feedback: if ping { tip(1, g.time * 2.0, fb) } else { tip(0, g.time * 2.0, fb) }, offset: (!ping).then(|| tip(1, g.time * (1.0 + h.v("offset")), 1.0)) }
}

/// The feedback filter's response (dB) at `f`.
fn loop_response(h: &Hero, f: f32) -> f32 {
    let (lc, hc, reso) = (h.v("lowcut"), h.v("highcut"), h.v("reso"));
    let hp = (f / lc).powi(2) / (1.0 + (f / lc).powi(2));
    let lp = 1.0 / (1.0 + (f / hc).powi(2));
    let peak = 1.0 + reso * 3.0 / (1.0 + ((f / hc).ln() * 3.0).powi(2));
    gain_to_db((hp * lp).sqrt() * peak)
}

fn filter_plot(filt: Rect) -> Rect {
    Rect::from_min_max(filt.min + Vec2::new(8.0, 22.0), filt.max - Vec2::new(8.0, 16.0))
}

fn filter_y(fp: Rect, db: f32) -> f32 {
    fp.top() + (12.0 - db.clamp(-24.0, 12.0)) / 36.0 * fp.height()
}

pub fn cavern(p: &egui::Painter, r: Rect, h: &Hero) {
    let t = h.t();
    p.rect_filled(r, 6.0, Color32::from_rgb(0x03, 0x07, 0x08));
    backdrop(p, r, "fx-cavern", CAVERN, 0.9);
    let (taps, filt) = cavern_layout(r);
    let hot_id = hot(p.ctx(), h.key);
    let frozen: Option<f32> = p.ctx().data(|d| d.get_temp(h.key.with("cave.span")));
    pane(p, taps, TEAL);
    let g = cavern_geo(taps, h, if hot_id > 0 { frozen } else { None });
    let x_of = |ms: f32| g.pr.left() + ms / g.span * g.pr.width();
    for k in 0..=4 {
        let ms = g.span * k as f32 / 4.0;
        small(p, Pos2::new(x_of(ms), g.pr.bottom() + 3.0), egui::Align2::CENTER_TOP, if ms >= 1000.0 { format!("{:.2} s", ms / 1000.0) } else { format!("{ms:.0} ms") }, BONE.gamma_multiply(0.6));
    }
    // Each lane is a ceiling with its water below.
    for (k, lane) in g.lanes.iter().enumerate() {
        p.line_segment([lane.left_top(), lane.right_top()], Stroke::new(1.0_f32, BONE.gamma_multiply(0.2)));
        p.line_segment([lane.left_bottom(), lane.right_bottom()], Stroke::new(1.5_f32, TEAL.gamma_multiply(0.45)));
        small(p, Pos2::new(g.pr.left() - 6.0, lane.center().y), egui::Align2::RIGHT_CENTER, ["L", "R"][k], BONE.gamma_multiply(0.75));
    }
    // The dry sound.
    for lane in &g.lanes {
        p.line_segment([Pos2::new(x_of(0.0) + 1.0, lane.top()), Pos2::new(x_of(0.0) + 1.0, lane.bottom())], Stroke::new(2.0_f32, AMBER.gamma_multiply(0.7)));
    }
    small(p, Pos2::new(x_of(0.0) + 4.0, g.lanes[0].top() + 2.0), egui::Align2::LEFT_TOP, "DRY", AMBER);
    // The echoes: stalactites reaching down by their level; with sound coming in, a drop falls from
    // each as its echo sounds and rings the water.
    let live = matches!(h.tele, [_, i, ..] if *i > 0.003);
    let play_ms = (t * 1000.0) % (g.span + 250.0);
    let stal = art(p.ctx(), "fx-stalactite", STALACTITE);
    let drop = art(p.ctx(), "fx-drop", DROP);
    for (lane_k, ms, level) in echoes(h, g.time, g.span) {
        let lane = g.lanes[lane_k];
        let x = x_of(ms);
        let len = lane.height() * reach(level);
        let w = (len * 0.3).clamp(10.0, 22.0);
        if let Some(st) = &stal {
            p.image(
                st.id(),
                Rect::from_min_max(Pos2::new(x - w / 2.0, lane.top()), Pos2::new(x + w / 2.0, lane.top() + len)),
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::from_gray((140.0 + 115.0 * level.sqrt()) as u8),
            );
        }
        let tip = Pos2::new(x, lane.top() + len);
        halo(p, tip, 10.0, TEAL, 0.25 + 0.5 * level.sqrt());
        // Ripple where its drop lands.
        let ring = |p: &egui::Painter, rad: f32, a: f32| {
            let pts: Vec<Pos2> = (0..=28).map(|j| j as f32 / 28.0 * std::f32::consts::TAU).map(|q| Pos2::new(x + rad * q.cos(), lane.bottom() + rad * 0.22 * q.sin())).collect();
            p.add(Shape::line(pts, Stroke::new(1.0_f32, glow(TEAL, a))));
        };
        ring(p, 4.0 + 6.0 * level, 0.25 * level.sqrt() + 0.08);
        if live {
            let since = play_ms - ms;
            if (0.0..260.0).contains(&since) {
                let f = since / 260.0;
                if f < 0.55 {
                    let y = tip.y + (lane.bottom() - tip.y) * (f / 0.55).powi(2);
                    if let Some(d) = &drop {
                        p.image(d.id(), Rect::from_center_size(Pos2::new(x, y), Vec2::new(6.0, 9.0)), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                    }
                } else {
                    let g2 = (f - 0.55) / 0.45;
                    ring(p, 6.0 + 22.0 * g2 * level.sqrt(), (1.0 - g2) * 0.8);
                }
            }
        }
    }
    // Handles.
    let n = cavern_nodes(&g, h);
    let tp = p.with_clip_rect(taps.expand(2.0));
    handle(&tp, n.time, "TIME", TEAL, hot_id == 1);
    handle(&tp, n.feedback, "FEEDBACK", TEAL, hot_id == 2);
    if let Some(o) = n.offset {
        handle(&tp, o, "R OFFSET", BONE, hot_id == 3);
    }
    let sync_name = match h.def.param("sync").map(|p| p.kind) {
        Some(dragon_engine::effects::ParamKind::Choice(names)) => names.get(h.v("sync").round() as usize).copied().unwrap_or("Free"),
        _ => "Free",
    };
    let modes = match h.def.param("mode").map(|p| p.kind) {
        Some(dragon_engine::effects::ParamKind::Choice(n)) => n,
        _ => &[],
    };
    let mode = modes.get(h.v("mode").round() as usize).copied().unwrap_or("");
    let label = if sync_name == "Free" { format!("{:.0} MS", g.time) } else { format!("{}  ·  {:.0} MS", sync_name.to_uppercase(), g.time) };
    p.text(taps.left_top() + Vec2::new(8.0, 5.0), egui::Align2::LEFT_TOP, format!("{label}  ·  FEEDBACK {:.0} %  ·  {}", h.v("feedback") * 100.0, mode.to_uppercase()), egui::FontId::proportional(10.5), BONE);
    if let [_, i, o, ..] = h.tele {
        let fmt = |v: f32| if v < 1e-5 { "silent".to_string() } else { format!("{:.1} dB", gain_to_db(v)) };
        small(p, Pos2::new(taps.right() - 8.0, taps.top() + 6.0), egui::Align2::RIGHT_TOP, format!("IN {}   OUT {}", fmt(*i), fmt(*o)), BONE.gamma_multiply(0.85));
    }
    // ---- the feedback filter (what each repeat passes through)
    pane(p, filt, TEAL);
    let fp = filter_plot(filt);
    for (f, l) in [(100.0, "100"), (1000.0, "1k"), (10000.0, "10k")] {
        let x = fx(fp, f);
        p.line_segment([Pos2::new(x, fp.top()), Pos2::new(x, fp.bottom())], Stroke::new(1.0_f32, Color32::from_white_alpha(12)));
        small(p, Pos2::new(x, fp.bottom() + 3.0), egui::Align2::CENTER_TOP, l, BONE.gamma_multiply(0.6));
    }
    let y0 = filter_y(fp, 0.0);
    p.line_segment([Pos2::new(fp.left(), y0), Pos2::new(fp.right(), y0)], Stroke::new(1.0_f32, Color32::from_white_alpha(24)));
    let pts: Vec<Pos2> = (0..=160).map(|k| fp.left() + fp.width() * k as f32 / 160.0).map(|x| Pos2::new(x, filter_y(fp, loop_response(h, freq_at(fp, x))))).collect();
    let mut mesh = Mesh::default();
    for (k, q) in pts.iter().enumerate() {
        mesh.colored_vertex(*q, glow(TEAL, 0.28));
        mesh.colored_vertex(Pos2::new(q.x, fp.bottom()), glow(TEAL, 0.0));
        if k > 0 {
            let i = (k * 2) as u32;
            mesh.add_triangle(i - 2, i - 1, i);
            mesh.add_triangle(i - 1, i + 1, i);
        }
    }
    p.add(Shape::mesh(mesh));
    p.add(Shape::line(pts.clone(), Stroke::new(5.0_f32, glow(TEAL, 0.25))));
    p.add(Shape::line(pts, Stroke::new(1.8_f32, TEAL)));
    let (lo, hi) = filter_nodes(fp, h);
    let fpp = p.with_clip_rect(filt.expand(2.0));
    handle(&fpp, lo, "LOW CUT", TEAL, hot_id == 4);
    handle(&fpp, hi, "HIGH CUT · RESO", TEAL, hot_id == 5);
    p.text(filt.left_top() + Vec2::new(8.0, 5.0), egui::Align2::LEFT_TOP, format!("LOOP FILTER  ·  {:.0} Hz – {:.1} kHz", h.v("lowcut"), h.v("highcut") / 1000.0), egui::FontId::proportional(10.0), BONE);
}

fn filter_nodes(fp: Rect, h: &Hero) -> (Pos2, Pos2) {
    let lo = Pos2::new(fx(fp, h.v("lowcut")), filter_y(fp, loop_response(h, h.v("lowcut"))));
    let hi = Pos2::new(fx(fp, h.v("highcut")), filter_y(fp, loop_response(h, h.v("highcut"))));
    (lo, hi)
}

/// Handles of the cavern: 1 time, 2 feedback, 3 right offset, 4 low cut, 5 high cut / resonance.
pub fn cavern_interact(ui: &Ui, r: Rect, resp: &egui::Response, h: &Hero) -> Vec<(String, f32)> {
    let mut out = Vec::new();
    let Some(pos) = resp.interact_pointer_pos().or_else(|| resp.hover_pos()) else { return out };
    let (taps, filt) = cavern_layout(r);
    let frozen: Option<f32> = ui.data(|d| d.get_temp(h.key.with("cave.span")));
    let held: usize = ui.data(|d| d.get_temp(h.key.with("cave.held"))).unwrap_or(0);
    let g = cavern_geo(taps, h, if held > 0 { frozen } else { None });
    let n = cavern_nodes(&g, h);
    let fp = filter_plot(filt);
    let (lo, hi) = filter_nodes(fp, h);
    let pick = |at: Pos2| -> usize {
        let mut c = vec![(1, n.time), (2, n.feedback), (4, lo), (5, hi)];
        if let Some(o) = n.offset {
            c.push((3, o));
        }
        c.iter().filter(|(_, q)| q.distance(at) < 13.0).min_by(|a, b| a.1.distance(at).total_cmp(&b.1.distance(at))).map(|x| x.0).unwrap_or(0)
    };
    if resp.drag_started() {
        let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
        let k = pick(origin);
        ui.data_mut(|d| {
            d.insert_temp(h.key.with("cave.held"), k);
            d.insert_temp(h.key.with("cave.span"), g.span);
        });
    }
    let held: usize = ui.data(|d| d.get_temp(h.key.with("cave.held"))).unwrap_or(0);
    let ms_at = |x: f32| ((x - g.pr.left()) / g.pr.width() * g.span).max(1.0);
    let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
    if resp.dragged() {
        match held {
            1 => {
                let want = ms_at(pos.x);
                let sync = h.v("sync").round() as usize;
                if sync == 0 {
                    param_at(h, "time", want, &mut out);
                } else {
                    // Synced: snap to the nearest note value at the current tempo.
                    let beats = dragon_engine::effects::DELAY_SYNC_BEATS;
                    let beat_ms = g.time / beats[sync.min(beats.len() - 1)] as f32;
                    let best = (1..beats.len()).min_by(|a, b| ((beats[*a] as f32 * beat_ms) - want).abs().total_cmp(&((beats[*b] as f32 * beat_ms) - want).abs())).unwrap_or(sync);
                    out.push(("sync".into(), best as f32));
                }
            }
            2 => {
                let ping = h.v("mode").round() as i32 == 1;
                let lane = if ping { g.lanes[1] } else { g.lanes[0] };
                param_at(h, "feedback", level_at(lane, pos.y), &mut out);
            }
            3 => param_at(h, "offset", ms_at(pos.x) / g.time - 1.0, &mut out),
            4 => param_at(h, "lowcut", freq_at(fp, pos.x), &mut out),
            5 => {
                param_at(h, "highcut", freq_at(fp, pos.x), &mut out);
                // Up raises the resonance (0 … +12 dB of peak).
                let db = (filter_y(fp, 0.0) - pos.y) / fp.height() * 36.0;
                param_at(h, "reso", (db / 12.0).clamp(0.0, 1.0), &mut out);
            }
            _ => {
                let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
                if taps.contains(origin) {
                    // Empty cavern: across is the time, up / down the feedback.
                    let d = resp.drag_delta() * fine;
                    if h.v("sync").round() as usize == 0 {
                        nudge(h, "time", d.x / taps.width(), &mut out);
                    }
                    nudge(h, "feedback", -d.y / taps.height(), &mut out);
                }
            }
        }
    }
    if resp.drag_stopped() {
        ui.data_mut(|d| d.insert_temp(h.key.with("cave.held"), 0usize));
    }
    let under = if held > 0 { held } else { pick(pos) };
    set_hot(ui, h.key, under);
    if resp.double_clicked() {
        match pick(pos) {
            1 => reset(h, &["time", "sync"], &mut out),
            2 => reset(h, &["feedback"], &mut out),
            3 => reset(h, &["offset"], &mut out),
            4 => reset(h, &["lowcut"], &mut out),
            5 => reset(h, &["highcut", "reso"], &mut out),
            _ => {}
        }
    }
    if under > 0 {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use dragon_engine::effects;
    use std::collections::BTreeMap;

    /// Runs the display of `id` in a 1100 × 600 window, sending `events` per frame; returns the last
    /// frame's display rect and every edit it made.
    fn drive(id: &str, params: &BTreeMap<String, f32>, script: &dyn Fn(Rect) -> Vec<Vec<egui::Event>>) -> Vec<(String, f32)> {
        let def = effects::find(id).unwrap();
        let ctx = egui::Context::default();
        let mut area = Rect::NOTHING;
        let mut got = Vec::new();
        let tele = [375.0f32, 0.0, 0.0];
        let frame = |events: Vec<egui::Event>, area: &mut Rect, got: &mut Vec<(String, f32)>| {
            let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1100.0, 600.0))), events, ..Default::default() };
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let h = Hero { def, params, gr: 0.0, enabled: true, color: Color32::WHITE, time: None, tele: if id == "delay" { &tele } else { &[] }, key: egui::Id::new("cave") };
                    let before = ui.cursor().min;
                    let (e, _) = super::super::fx_ui::hero(ui, &h);
                    *area = Rect::from_min_max(before, Pos2::new(ui.min_rect().right(), ui.cursor().min.y));
                    got.extend(e);
                });
            });
        };
        frame(vec![], &mut area, &mut got);
        frame(vec![], &mut area, &mut got);
        // The display's own rect: as wide as the panel, from the top of the panel.
        let r = Rect::from_min_size(area.min, egui::vec2(area.width(), area.height().max(1.0)));
        for evs in script(r) {
            frame(evs, &mut area, &mut got);
        }
        got
    }

    fn drag(from: Pos2, to: Pos2) -> Vec<Vec<egui::Event>> {
        let mut v = vec![vec![egui::Event::PointerMoved(from)], vec![egui::Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }]];
        for k in 1..=6 {
            v.push(vec![egui::Event::PointerMoved(from + (to - from) * (k as f32 / 6.0))]);
        }
        v.push(vec![egui::Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }]);
        v
    }

    fn hero_rect(r: Rect, id: &str) -> Rect {
        // The display sits at the top of the panel at its computed height.
        let h = super::super::fx_ui::hero_height(id, 170.0, r.width(), 600.0 - 16.0, 600.0);
        Rect::from_min_size(r.min, egui::vec2(r.width(), h))
    }

    #[test]
    fn dragging_the_lairs_decay_bead_sets_the_decay() {
        let def = effects::find("reverb").unwrap();
        let params = def.defaults();
        let got = drive("reverb", &params, &|r| {
            let r = hero_rect(r, "reverb");
            let (env, _, _) = lair_layout(r);
            let h = Hero { def, params: &params, gr: 0.0, enabled: true, color: Color32::WHITE, time: None, tele: &[], key: egui::Id::new("x") };
            let (pr, span) = lair_plot(env, &h, None);
            let n = lair_nodes(pr, span, &h);
            drag(n.decay, n.decay + egui::vec2(pr.width() * 0.15, 0.0))
        });
        let last = got.iter().rev().find(|(id, _)| id == "decay").map(|x| x.1);
        assert!(last.is_some_and(|v| v > 2.2 + 0.3), "decay = {last:?} ({got:?})");
    }

    #[test]
    fn moving_the_dragons_eye_sets_size_and_width() {
        let def = effects::find("reverb").unwrap();
        let params = def.defaults();
        let got = drive("reverb", &params, &|r| {
            let r = hero_rect(r, "reverb");
            let (_, room, _) = lair_layout(r);
            let pad = room_pad(room);
            drag(pad.center(), Pos2::new(pad.right() - 2.0, pad.top() + 2.0))
        });
        let size = got.iter().rev().find(|(id, _)| id == "size").map(|x| x.1);
        let width = got.iter().rev().find(|(id, _)| id == "width").map(|x| x.1);
        assert!(size.is_some_and(|v| v > 0.95) && width.is_some_and(|v| v > 1.4), "size {size:?} width {width:?}");
    }

    #[test]
    fn dragging_the_first_stalactite_sets_the_free_time() {
        let def = effects::find("delay").unwrap();
        let mut params = def.defaults();
        params.insert("sync".into(), 0.0);
        params.insert("time".into(), 375.0);
        let got = drive("delay", &params, &|r| {
            let r = hero_rect(r, "delay");
            let (taps, _) = cavern_layout(r);
            let tele = [375.0f32, 0.0, 0.0];
            let h = Hero { def, params: &params, gr: 0.0, enabled: true, color: Color32::WHITE, time: None, tele: &tele, key: egui::Id::new("x") };
            let g = cavern_geo(taps, &h, None);
            let n = cavern_nodes(&g, &h);
            // Move it to the 600 ms mark.
            let to = Pos2::new(g.pr.left() + 600.0 / g.span * g.pr.width(), n.time.y);
            drag(n.time, to)
        });
        let last = got.iter().rev().find(|(id, _)| id == "time").map(|x| x.1);
        assert!(last.is_some_and(|v| (v - 600.0).abs() < 25.0), "time = {last:?}");
    }
}
