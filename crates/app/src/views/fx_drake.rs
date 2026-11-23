//! Drakesong's display: a column of backlit voicing keys (the lit one is in use – click another), and
//! the EQ curve on the phosphor screen, drawn from the same filters the audio runs through. Every band
//! has a node: drag LF / LMF / HMF / HF (sideways = frequency within the band's range, up / down = gain),
//! SUB (up / down) and AIR (sideways snaps to 2.5 / 5 / 10 / 20 / 40 kHz, up = more air, never a cut).
//! Double-click a node to put it back to 0 dB.

use super::fx_ui::Hero;
use dragon_engine::effects::drakesong::{self, Settings, AIR_FREQS, MODELS};
use egui::{Color32, Pos2, Rect, Stroke, Ui, Vec2};

const KEYS_W: f32 = 132.0;
const DB: f32 = 15.0;

fn settings(h: &Hero) -> Settings {
    Settings::from_params(|id| dragon_engine::effects::value(h.def, h.params, id))
}

/// The keys column and the plot.
fn layout(r: Rect) -> (Vec<Rect>, Rect) {
    let col = Rect::from_min_max(r.min + Vec2::new(10.0, 10.0), Pos2::new(r.left() + 10.0 + KEYS_W, r.bottom() - 10.0));
    let n = MODELS.len() as f32;
    let gap = 5.0;
    let kh = ((col.height() - gap * (n - 1.0)) / n).min(30.0);
    let keys = (0..MODELS.len()).map(|k| Rect::from_min_size(Pos2::new(col.left(), col.top() + k as f32 * (kh + gap)), Vec2::new(col.width(), kh))).collect();
    let plot = Rect::from_min_max(Pos2::new(col.right() + 30.0, r.top() + 22.0), r.max - Vec2::new(12.0, 20.0));
    (keys, plot)
}

fn x_of(plot: Rect, f: f32) -> f32 {
    plot.left() + ((f.max(10.0) / 20.0).log10() / 3.0) * plot.width()
}

fn f_of(plot: Rect, x: f32) -> f32 {
    20.0 * 10f32.powf(((x - plot.left()) / plot.width()).clamp(0.0, 1.0) * 3.0)
}

fn y_of(plot: Rect, db: f32) -> f32 {
    plot.center().y - db.clamp(-DB, DB) / DB * plot.height() * 0.5
}

fn db_of(plot: Rect, y: f32) -> f32 {
    (plot.center().y - y) / (plot.height() * 0.5) * DB
}

const AIR_HZ: [f32; 5] = [2500.0, 5000.0, 10000.0, 20000.0, 40000.0];

/// A draggable node: (name, gain param, freq param, frequency range, position).
type Node = (&'static str, &'static str, Option<&'static str>, (f32, f32), Pos2);

fn nodes(s: &Settings, plot: Rect) -> Vec<Node> {
    let at = |f: f32, g: f32| Pos2::new(x_of(plot, f), y_of(plot, g));
    vec![
        ("SUB", "sub", None, (30.0, 30.0), at(30.0, s.sub)),
        ("LF", "lf_gain", Some("lf_freq"), (40.0, 400.0), at(s.lf.1, s.lf.0)),
        ("LMF", "lmf_gain", Some("lmf_freq"), (200.0, 2000.0), at(s.lmf.1, s.lmf.0)),
        ("HMF", "hmf_gain", Some("hmf_freq"), (1000.0, 8000.0), at(s.hmf.1, s.hmf.0)),
        ("HF", "hf_gain", Some("hf_freq"), (4000.0, 16000.0), at(s.hf.1, s.hf.0)),
        ("AIR", "air", None, (2500.0, 20000.0), at(AIR_HZ[s.air_freq].min(19000.0), s.air)),
    ]
}

pub fn draw(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    let s = settings(h);
    let (keys, plot) = layout(r);
    super::fx_mod::screen(p, r, 6.0);
    // The voicing keys: backlit when chosen, a warm glow behind the lit one.
    for (k, kr) in keys.iter().enumerate() {
        let lit = k == s.model;
        p.rect_filled(kr.expand(1.5), 4.0, Color32::from_black_alpha(160));
        if lit {
            super::fx_mod::halo(p, kr.center(), kr.width() * 0.6, Color32::from_rgb(0xff, 0xf2, 0xd8), 0.35);
        }
        let face = if lit { Color32::from_rgb(0xf6, 0xf2, 0xe8) } else { Color32::from_rgb(0x9a, 0x9c, 0x9e) };
        p.rect_filled(*kr, 3.0, face);
        // A bevel: light top edge, dark bottom edge.
        p.line_segment([kr.left_top() + Vec2::new(2.0, 1.0), kr.right_top() + Vec2::new(-2.0, 1.0)], Stroke::new(1.0_f32, Color32::from_white_alpha(if lit { 200 } else { 90 })));
        p.line_segment([kr.left_bottom() + Vec2::new(2.0, -1.0), kr.right_bottom() + Vec2::new(-2.0, -1.0)], Stroke::new(1.0_f32, Color32::from_black_alpha(120)));
        p.text(kr.center(), egui::Align2::CENTER_CENTER, MODELS[k].to_uppercase(), egui::FontId::proportional(11.5), Color32::from_rgb(0x1a, 0x1a, 0x1c));
    }
    // The plot: grid, the 0 dB line, the curve.
    super::fx_mod::inset(p, plot.expand(4.0), 4.0);
    let dim = |a: f32| Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), (a * 255.0) as u8);
    for f in [50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0] {
        let x = x_of(plot, f);
        p.line_segment([Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())], Stroke::new(1.0_f32, dim(0.07)));
        let label = if f >= 1000.0 { format!("{}k", f / 1000.0) } else { format!("{f}") };
        p.text(Pos2::new(x, plot.bottom() + 9.0), egui::Align2::CENTER_CENTER, label, egui::FontId::monospace(8.0), dim(0.45));
    }
    for db in [-12.0, -6.0, 6.0, 12.0] {
        let y = y_of(plot, db);
        p.line_segment([Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)], Stroke::new(1.0_f32, dim(0.06)));
        p.text(Pos2::new(plot.left() - 3.0, y), egui::Align2::RIGHT_CENTER, format!("{db:+.0}"), egui::FontId::monospace(7.5), dim(0.4));
    }
    p.line_segment([Pos2::new(plot.left(), plot.center().y), Pos2::new(plot.right(), plot.center().y)], Stroke::new(1.0_f32, dim(0.18)));
    let rate = 48000.0;
    let curve: Vec<Pos2> = (0..=200)
        .map(|i| {
            let x = plot.left() + i as f32 / 200.0 * plot.width();
            Pos2::new(x, y_of(plot, drakesong::response_db(&Settings { output: 0.0, ..s }, rate, f_of(plot, x))))
        })
        .collect();
    let pc = p.with_clip_rect(plot.expand(2.0));
    // A soft glow under the curve, breathing a little with the signal.
    let breath = out_breath(h);
    for (k, w) in [(0.10 + 0.08 * breath, 8.0_f32), (0.25, 4.0_f32)] {
        pc.add(egui::Shape::line(curve.clone(), Stroke::new(w, dim(k))));
    }
    pc.add(egui::Shape::line(curve, Stroke::new(2.0_f32, col)));
    // Nodes.
    for (name, g, _, _, pos) in nodes(&s, plot) {
        let active = match g {
            "sub" => s.sub.abs() > 0.01,
            "air" => s.air > 0.01,
            _ => true,
        };
        pc.circle_filled(pos, 5.5, Color32::from_black_alpha(170));
        pc.circle_stroke(pos, 5.5, Stroke::new(1.5_f32, if active { col } else { dim(0.4) }));
        if active {
            pc.circle_filled(pos, 2.5, col);
        }
        p.text(pos + Vec2::new(0.0, -11.0), egui::Align2::CENTER_CENTER, name, egui::FontId::proportional(8.5), dim(if active { 0.9 } else { 0.45 }));
    }
    p.text(Pos2::new(plot.right(), r.top() + 11.0), egui::Align2::RIGHT_CENTER, format!("{}  ·  AIR {}", MODELS[s.model].to_uppercase(), AIR_FREQS[s.air_freq]), egui::FontId::monospace(9.0), dim(0.7));
}

/// 0 … 1 from the unit's measured output, for the glow.
fn out_breath(h: &Hero) -> f32 {
    let t = h.t();
    (0.5 + 0.5 * (t * 1.3).sin()) * if h.enabled { 1.0 } else { 0.0 }
}

/// Clicks on the keys and drags of the nodes.
pub fn interact(ui: &Ui, r: Rect, resp: &egui::Response, h: &Hero) -> Vec<(String, f32)> {
    let s = settings(h);
    let (keys, plot) = layout(r);
    let mut out = Vec::new();
    let pos = resp.interact_pointer_pos().or_else(|| ui.input(|i| i.pointer.hover_pos()));
    if resp.clicked() {
        if let Some(k) = pos.and_then(|p| keys.iter().position(|kr| kr.contains(p))) {
            out.push(("model".to_string(), k as f32));
            return out;
        }
    }
    let grab_key = h.key.with("drake.grab");
    let ns = nodes(&s, plot);
    let nearest = |p: Pos2| ns.iter().enumerate().map(|(k, n)| (k, n.4.distance(p))).filter(|x| x.1 < 14.0).min_by(|a, b| a.1.total_cmp(&b.1)).map(|x| x.0);
    if resp.double_clicked() {
        if let Some(k) = pos.and_then(nearest) {
            out.push((ns[k].1.to_string(), 0.0));
            return out;
        }
    }
    if resp.drag_started() {
        let k = pos.and_then(nearest);
        ui.ctx().data_mut(|d| d.insert_temp(grab_key, k));
    }
    if resp.dragged() {
        let grabbed: Option<usize> = ui.ctx().data(|d| d.get_temp(grab_key)).flatten();
        if let (Some(k), Some(p)) = (grabbed, pos) {
            let (_, g, f, range, _) = ns[k];
            match g {
                "sub" => out.push(("sub".into(), db_of(plot, p.y).clamp(-10.0, 10.0))),
                "air" => {
                    out.push(("air".into(), db_of(plot, p.y).clamp(0.0, 15.0)));
                    let fx = f_of(plot, p.x);
                    let choice = AIR_HZ.iter().enumerate().min_by(|a, b| (a.1.ln() - fx.ln()).abs().total_cmp(&(b.1.ln() - fx.ln()).abs())).map(|x| x.0).unwrap_or(3);
                    out.push(("air_freq".into(), choice as f32));
                }
                _ => {
                    out.push((g.into(), db_of(plot, p.y).clamp(-12.0, 12.0)));
                    if let Some(f) = f {
                        out.push((f.into(), f_of(plot, p.x).clamp(range.0, range.1)));
                    }
                }
            }
        }
    }
    if resp.drag_stopped() {
        ui.ctx().data_mut(|d| d.remove::<Option<usize>>(grab_key));
    }
    if let Some(p) = pos {
        if keys.iter().any(|k| k.contains(p)) || nearest(p).is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plot_mapping_round_trips() {
        let r = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 200.0));
        let (keys, plot) = layout(r);
        assert_eq!(keys.len(), MODELS.len());
        assert!(keys.iter().all(|k| k.right() < plot.left()), "keys left of the plot");
        for f in [40.0, 1000.0, 16000.0] {
            assert!((f_of(plot, x_of(plot, f)) - f).abs() / f < 1e-3);
        }
        assert!((db_of(plot, y_of(plot, 7.5)) - 7.5).abs() < 1e-3);
    }
}
