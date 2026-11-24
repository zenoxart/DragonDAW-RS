//! The displays (hero screens) of the newer stock units: Reliquary, Tidecaller, Scrying Glass, Siren's Fork,
//! Wyrmstack and the time, voice and colour units. Each reads the unit's parameters and its telemetry, like
//! the displays in [`super::fx_ui`].

use super::fx_ui::{filled_curve, fx, grid, plot, x_axis, y_axis, Hero};
use crate::theme;
use egui::{Color32, Pos2, Rect, Shape, Stroke, Vec2};
use std::f32::consts::TAU;

fn label(p: &egui::Painter, at: Pos2, align: egui::Align2, text: impl ToString, size: f32, c: Color32) {
    p.text(at, align, text.to_string(), egui::FontId::proportional(size), c);
}

fn mono(p: &egui::Painter, at: Pos2, align: egui::Align2, text: impl ToString, size: f32, c: Color32) {
    p.text(at, align, text.to_string(), egui::FontId::monospace(size), c);
}

/// The ids drawn here.
pub fn draws(id: &str) -> bool {
    matches!(id, "convolver" | "volume_shaper" | "analyzer" | "tuner" | "amp" | "vocoder" | "freqshift" | "stutter" | "tape_stop" | "granular" | "lofi" | "exciter" | "autofilter" | "shimmer" | "freeze")
}

pub fn draw(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    match h.def.id {
        "convolver" => convolver(p, r, h, col),
        "volume_shaper" => volume_shaper(p, r, h, col),
        "analyzer" => analyzer(p, r, h, col),
        "tuner" => tuner(p, r, h, col),
        "amp" => amp(p, r, h, col),
        "vocoder" => vocoder(p, r, h, col),
        "freqshift" => freqshift(p, r, h, col),
        "stutter" => stutter(p, r, h, col),
        "tape_stop" => tape_stop(p, r, h, col),
        "granular" => granular(p, r, h, col),
        "lofi" => lofi(p, r, h, col),
        "exciter" => exciter(p, r, h, col),
        "autofilter" => autofilter(p, r, h, col),
        "shimmer" => shimmer(p, r, h, col),
        "freeze" => freeze(p, r, h, col),
        _ => {}
    }
}

/// Reliquary: the response's envelope over time (dB), pre-delay and length, a swell when reversed.
fn convolver(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    use dragon_engine::effects::convolver::{ENV_POINTS, SPACES};
    grid(p, r, false);
    let pr = plot(r);
    let y_of = |db: f32| pr.top() + (-db / 72.0).clamp(0.0, 1.0) * pr.height();
    y_axis(p, pr, &[(0.0, "0"), (-24.0, "-24"), (-48.0, "-48"), (-72.0, "-72")], y_of);
    let pre = h.v("predelay");
    let (env, secs) = if h.tele.len() > ENV_POINTS { (&h.tele[..ENV_POINTS], h.tele[ENV_POINTS]) } else { (&[][..], 0.0) };
    let total = (secs + pre * 0.001).max(0.05);
    let x_of = |t: f32| pr.left() + (t / total).clamp(0.0, 1.0) * pr.width();
    let marks: Vec<(f32, String)> = (0..=4).map(|k| total * k as f32 / 4.0).map(|t| (t, if t < 1.0 { format!("{:.0} ms", t * 1000.0) } else { format!("{t:.1} s") })).collect();
    x_axis(p, pr, &marks, x_of);
    if !env.is_empty() {
        let pts: Vec<Pos2> = env.iter().enumerate().map(|(i, db)| Pos2::new(x_of(pre * 0.001 + secs * i as f32 / ENV_POINTS as f32), y_of(*db))).collect();
        filled_curve(p, &pts, pr.bottom(), col);
    }
    // The pre-delay gap.
    if pre > 0.5 {
        let x = x_of(pre * 0.001);
        p.line_segment([Pos2::new(x, pr.top()), Pos2::new(x, pr.bottom())], Stroke::new(1.0_f32, col.gamma_multiply(0.5)));
    }
    let space = SPACES.get(h.v("space").round() as usize).copied().unwrap_or("?");
    let reverse = if h.v("reverse") > 0.5 { " · REVERSE" } else { "" };
    let busy = if h.tele.get(ENV_POINTS + 2).copied().unwrap_or(0.0) > 0.5 { " · loading…" } else { "" };
    label(p, Pos2::new(pr.right() - 4.0, pr.top() + 2.0), egui::Align2::RIGHT_TOP, format!("{}  {:.2} s{reverse}{busy}", space.to_uppercase(), secs), 11.0, theme::TEXT);
}

/// Tidecaller: one period of the gain curve, the playhead on it, and the live gain.
fn volume_shaper(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    use dragon_engine::effects::shaper::{shape, MODES, RATES, SHAPES};
    grid(p, r, false);
    let pr = plot(r);
    let y_of = |g: f32| pr.bottom() - g.clamp(0.0, 1.0) * pr.height();
    y_axis(p, pr, &[(1.0, "0"), (0.5, "-6"), (0.25, "-12"), (0.0, "-inf")], y_of);
    let mode = h.v("mode").round() as usize;
    let depth = h.v("depth") / 100.0;
    let (s, tension, offset) = (h.v("shape").round() as usize, h.v("tension") / 100.0, h.v("offset") / 100.0);
    if mode != 1 {
        let pts: Vec<Pos2> = (0..=160)
            .map(|k| k as f32 / 160.0)
            .map(|u| {
                let g = shape(s, u + offset, tension);
                Pos2::new(pr.left() + u * pr.width(), y_of(1.0 - depth * (1.0 - g)))
            })
            .collect();
        filled_curve(p, &pts, pr.bottom(), col);
    }
    if let [phase, gain, duck, ..] = h.tele {
        let x = pr.left() + ((phase - offset).rem_euclid(1.0)) * pr.width();
        p.line_segment([Pos2::new(x, pr.top()), Pos2::new(x, pr.bottom())], Stroke::new(1.5_f32, theme::TEXT.gamma_multiply(0.7)));
        p.circle_filled(Pos2::new(x, y_of(*gain)), 4.5, theme::TEXT);
        // Ducking meter on the right edge.
        if mode != 0 {
            let m = Rect::from_min_max(Pos2::new(pr.right() - 10.0, pr.top()), pr.right_bottom());
            p.rect_filled(m, 2.0, Color32::from_white_alpha(12));
            p.rect_filled(Rect::from_min_max(Pos2::new(m.left(), y_of(*duck)), m.max), 2.0, Color32::from_rgb(0x9c, 0xd8, 0xff));
        }
    }
    let rate = RATES.get(h.v("rate").round() as usize).copied().unwrap_or("?");
    let name = if mode == 1 { "SIDECHAIN DUCK".to_string() } else { format!("{} · {rate}", SHAPES.get(s).copied().unwrap_or("?").to_uppercase()) };
    label(p, Pos2::new(pr.right() - 14.0, pr.top() + 2.0), egui::Align2::RIGHT_TOP, format!("{name}  ·  {}", MODES.get(mode).copied().unwrap_or("")), 11.0, theme::TEXT);
}

/// Scrying Glass: spectrum (left), level and loudness readouts with a correlation bar (middle), vectorscope
/// (right).
fn analyzer(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    use dragon_engine::dsp::spectrum::{band_freq, BANDS};
    use dragon_engine::effects::meters::*;
    super::fx_mod::screen(p, r, 6.0);
    let scope_w = r.height().min(r.width() * 0.3);
    let spec = Rect::from_min_max(r.min, Pos2::new(r.right() - scope_w - 120.0, r.bottom()));
    let pr = plot(spec);
    let range = [60.0, 90.0, 120.0][(h.v("range").round() as usize).min(2)];
    let y_of = |db: f32| pr.top() + (-db / range).clamp(0.0, 1.0) * pr.height();
    let marks: Vec<(f32, String)> = [50.0, 200.0, 1000.0, 5000.0, 15000.0].iter().map(|f| (*f, if *f >= 1000.0 { format!("{}k", f / 1000.0) } else { format!("{f}") })).collect();
    x_axis(p, pr, &marks, |f| fx(pr, f));
    let ticks: Vec<(f32, String)> = (0..=4).map(|k| -(range * k as f32 / 4.0)).map(|d| (d, format!("{d:.0}"))).collect();
    for (v, t) in &ticks {
        let y = y_of(*v);
        p.line_segment([Pos2::new(pr.left(), y), Pos2::new(pr.right(), y)], Stroke::new(1.0_f32, Color32::from_white_alpha(10)));
        label(p, Pos2::new(pr.left() - 5.0, y), egui::Align2::RIGHT_CENTER, t, 9.0, Color32::from_white_alpha(110));
    }
    let hold = h.v("hold") > 0.5;
    if h.tele.len() >= T_SCOPE + 2 * SCOPE_POINTS {
        let pts: Vec<Pos2> = (0..BANDS).map(|b| Pos2::new(fx(pr, band_freq(b)), y_of(h.tele[b]))).collect();
        filled_curve(p, &pts, pr.bottom(), col);
        if hold {
            // Peak line kept per insert, falling slowly.
            let id = h.key.with("scry_peaks");
            let ctx = p.ctx();
            let mut peaks: Vec<f32> = ctx.data(|d| d.get_temp(id)).unwrap_or_else(|| vec![-200.0; BANDS]);
            for (pk, v) in peaks.iter_mut().zip(&h.tele[..BANDS]) {
                *pk = (*pk - 0.15).max(*v);
            }
            let pl: Vec<Pos2> = (0..BANDS).map(|b| Pos2::new(fx(pr, band_freq(b)), y_of(peaks[b]))).collect();
            p.add(Shape::line(pl, Stroke::new(1.0_f32, theme::TEXT.gamma_multiply(0.45))));
            ctx.data_mut(|d| d.insert_temp(id, peaks));
        }
        // Readouts.
        let mid = Rect::from_min_max(Pos2::new(spec.right() + 6.0, r.top() + 8.0), Pos2::new(r.right() - scope_w - 6.0, r.bottom() - 8.0));
        let rows = [
            ("PEAK", format!("{:5.1} {:5.1}", h.tele[T_PEAK_L], h.tele[T_PEAK_R])),
            ("RMS", format!("{:5.1} {:5.1}", h.tele[T_RMS_L], h.tele[T_RMS_R])),
            ("TRUE PK", format!("{:5.1} dBTP", h.tele[T_TRUE_PEAK])),
            ("MOMENT.", format!("{:5.1} LUFS", h.tele[T_MOMENTARY])),
            ("SHORT", format!("{:5.1} LUFS", h.tele[T_SHORT])),
        ];
        let lh = (mid.height() - 26.0) / rows.len() as f32;
        for (i, (k, v)) in rows.iter().enumerate() {
            let y = mid.top() + i as f32 * lh;
            label(p, Pos2::new(mid.left(), y), egui::Align2::LEFT_TOP, k, 9.0, theme::TEXT_DIM);
            mono(p, Pos2::new(mid.left(), y + 10.0), egui::Align2::LEFT_TOP, v, 10.5, theme::TEXT);
        }
        // Correlation bar −1 … +1.
        let c = h.tele[T_CORR].clamp(-1.0, 1.0);
        let bar = Rect::from_min_max(Pos2::new(mid.left(), mid.bottom() - 12.0), Pos2::new(mid.right(), mid.bottom() - 4.0));
        p.rect_filled(bar, 3.0, Color32::from_white_alpha(14));
        let x0 = bar.center().x;
        let x1 = x0 + c * bar.width() * 0.5;
        let cc = if c < 0.0 { Color32::from_rgb(0xff, 0x5a, 0x4a) } else { col };
        p.rect_filled(Rect::from_min_max(Pos2::new(x0.min(x1), bar.top()), Pos2::new(x0.max(x1), bar.bottom())), 3.0, cc);
        label(p, Pos2::new(mid.left(), bar.top() - 2.0), egui::Align2::LEFT_BOTTOM, format!("CORRELATION {c:+.2}"), 9.0, theme::TEXT_DIM);
        // Vectorscope.
        let sr = Rect::from_center_size(Pos2::new(r.right() - scope_w * 0.5, r.center().y), Vec2::splat(scope_w - 12.0));
        super::fx_mod::inset(p, sr, sr.width() * 0.5);
        p.line_segment([Pos2::new(sr.center().x, sr.top() + 4.0), Pos2::new(sr.center().x, sr.bottom() - 4.0)], Stroke::new(1.0_f32, Color32::from_white_alpha(18)));
        p.line_segment([Pos2::new(sr.left() + 4.0, sr.center().y), Pos2::new(sr.right() - 4.0, sr.center().y)], Stroke::new(1.0_f32, Color32::from_white_alpha(18)));
        let rad = sr.width() * 0.45;
        for k in 0..SCOPE_POINTS {
            let (x, y) = (h.tele[T_SCOPE + 2 * k], h.tele[T_SCOPE + 2 * k + 1]);
            let pnt = sr.center() + Vec2::new(x.clamp(-1.0, 1.0), -y.clamp(-1.0, 1.0)) * rad;
            p.circle_filled(pnt, 1.6, col.gamma_multiply(0.85));
        }
        label(p, Pos2::new(sr.center().x, sr.top() - 1.0), egui::Align2::CENTER_BOTTOM, "M", 9.0, theme::TEXT_DIM);
    }
    let ch = CHANNELS.get(h.v("channel").round() as usize).copied().unwrap_or("");
    label(p, Pos2::new(pr.right() - 4.0, pr.top() + 2.0), egui::Align2::RIGHT_TOP, ch.to_uppercase(), 11.0, theme::TEXT);
}

/// Siren's Fork: the note large, the cents on a needle (or a strobe band), the frequency.
fn tuner(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    use dragon_engine::effects::meters::{T_CENTS, T_FREQ, T_NOTE};
    super::fx_mod::screen(p, r, 10.0);
    let (freq, note, cents) = match h.tele {
        t if t.len() > T_CENTS => (t[T_FREQ], t[T_NOTE], t[T_CENTS]),
        _ => (0.0, 0.0, 0.0),
    };
    let have = freq > 0.0;
    let in_tune = have && cents.abs() < 3.0;
    let c_note = if in_tune { Color32::from_rgb(0x6c, 0xff, 0x7a) } else { col };
    let name = if have { dragon_engine::instruments::note_name(note.round().clamp(0.0, 127.0) as u8) } else { "–".into() };
    let strobe = h.v("display") > 0.5;
    let face = Rect::from_min_max(r.min + Vec2::new(16.0, 12.0), r.max - Vec2::new(16.0, 12.0));
    // The note.
    p.text(Pos2::new(face.center().x, face.top() + face.height() * 0.42), egui::Align2::CENTER_CENTER, &name, egui::FontId::proportional((face.height() * 0.42).clamp(28.0, 96.0)), c_note);
    let scale = Rect::from_min_max(Pos2::new(face.left() + 20.0, face.bottom() - 34.0), Pos2::new(face.right() - 20.0, face.bottom() - 14.0));
    if strobe {
        // Bands drifting at a speed proportional to the cents off (still when in tune).
        let t = h.t();
        let speed = if have { cents * 0.08 } else { 0.0 };
        let n = 24;
        let id = h.key.with("strobe");
        let ctx = p.ctx();
        let mut ph: f32 = ctx.data(|d| d.get_temp(id)).unwrap_or(0.0);
        ph = (ph + speed * 0.016).rem_euclid(1.0);
        ctx.data_mut(|d| d.insert_temp(id, ph));
        let _ = t;
        p.rect_filled(scale, 3.0, Color32::from_white_alpha(10));
        for k in 0..n {
            let x = scale.left() + ((k as f32 + ph) / n as f32).rem_euclid(1.0) * scale.width();
            let w = scale.width() / n as f32 * 0.5;
            p.rect_filled(Rect::from_min_max(Pos2::new(x, scale.top()), Pos2::new((x + w).min(scale.right()), scale.bottom())), 1.0, c_note.gamma_multiply(if have { 0.9 } else { 0.2 }));
        }
    } else {
        for k in -5i32..=5 {
            let x = scale.center().x + k as f32 / 5.0 * scale.width() * 0.5;
            let tall = k == 0 || k.abs() == 5;
            p.line_segment([Pos2::new(x, scale.bottom()), Pos2::new(x, scale.bottom() - if tall { 16.0 } else { 8.0 })], Stroke::new(if k == 0 { 2.0_f32 } else { 1.0 }, theme::TEXT_DIM));
        }
        if have {
            let x = scale.center().x + (cents / 50.0).clamp(-1.0, 1.0) * scale.width() * 0.5;
            let pivot = Pos2::new(scale.center().x, face.bottom() + 40.0);
            p.line_segment([pivot, Pos2::new(x, scale.top() - 8.0)], Stroke::new(2.5_f32, c_note));
        }
        for (k, t) in [(-1.0f32, "-50"), (0.0, "0"), (1.0, "+50")] {
            label(p, Pos2::new(scale.center().x + k * scale.width() * 0.5, scale.bottom() + 2.0), egui::Align2::CENTER_TOP, t, 9.0, theme::TEXT_DIM);
        }
    }
    if have {
        mono(p, Pos2::new(face.right(), face.top()), egui::Align2::RIGHT_TOP, format!("{freq:.2} Hz  {cents:+.1} ct"), 11.0, theme::TEXT);
    }
    mono(p, Pos2::new(face.left(), face.top()), egui::Align2::LEFT_TOP, format!("A4 = {:.0} Hz", h.v("reference")), 10.0, theme::TEXT_DIM);
    if h.v("mute") > 0.5 {
        label(p, Pos2::new(face.left(), face.top() + 14.0), egui::Align2::LEFT_TOP, "MUTED", 10.0, Color32::from_rgb(0xff, 0x6a, 0x4a));
    }
}

/// Wyrmstack: the preamp's transfer curve at the current gain (left) and the tone stack + cabinet response
/// (right).
fn amp(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    use dragon_engine::dsp::{Biquad, BiquadKind};
    use dragon_engine::effects::amp::{CABS, MODELS};
    super::fx_mod::screen(p, r, 6.0);
    let model = h.v("model").round() as usize;
    let drive = h.v("drive") / 100.0;
    let (lo, hi, stages, bias) = match model {
        0 => (0.0, 26.0, 1, 0.10),
        1 => (8.0, 38.0, 2, 0.20),
        2 => (16.0, 56.0, 3, 0.25),
        3 => (0.0, 30.0, 1, 0.15),
        _ => (24.0, 52.0, 1, 0.30),
    };
    let g = 10f32.powf((lo + (hi - lo) * drive) / 20.0);
    let curve = |x: f32| -> f32 {
        let mut y = x * g;
        for k in 0..stages {
            y = if model == 4 {
                let v = if y > 0.0 { y * 1.6 } else { y };
                v.signum() * (1.0 - (-v.abs()).exp())
            } else {
                (y + bias).tanh() - bias.tanh()
            };
            if k + 1 < stages {
                y *= 4.0;
            }
        }
        y
    };
    let sq = Rect::from_min_size(r.min + Vec2::new(10.0, 10.0), Vec2::splat(r.height() - 20.0));
    super::fx_mod::inset(p, sq, 6.0);
    let pts: Vec<Pos2> = (0..=120)
        .map(|k| k as f32 / 60.0 - 1.0)
        .map(|x| {
            let y = curve(x * 0.25).clamp(-1.2, 1.2) / 1.2;
            Pos2::new(sq.center().x + x * sq.width() * 0.45, sq.center().y - y * sq.height() * 0.45)
        })
        .collect();
    p.line_segment([Pos2::new(sq.left() + 6.0, sq.center().y), Pos2::new(sq.right() - 6.0, sq.center().y)], Stroke::new(1.0_f32, Color32::from_white_alpha(18)));
    p.line_segment([Pos2::new(sq.center().x, sq.top() + 6.0), Pos2::new(sq.center().x, sq.bottom() - 6.0)], Stroke::new(1.0_f32, Color32::from_white_alpha(18)));
    p.add(Shape::line(pts, Stroke::new(2.0_f32, col)));
    label(p, Pos2::new(sq.center().x, sq.bottom() - 4.0), egui::Align2::CENTER_BOTTOM, MODELS.get(model).copied().unwrap_or("").to_uppercase(), 10.0, theme::TEXT);
    // Response of tone stack and cabinet (the same filters as the engine, evaluated at 48 kHz).
    let fr = Rect::from_min_max(Pos2::new(sq.right() + 14.0, r.top()), r.max);
    let pr = plot(fr);
    let rate = 48000.0;
    let bass_model = model == 3;
    let mut fl: Vec<Biquad> = vec![
        Biquad::new(BiquadKind::LowShelf, if bass_model { 80.0 } else { 110.0 }, 0.6, (h.v("bass") - 5.0) * 2.4, rate),
        Biquad::new(BiquadKind::Peak, if bass_model { 500.0 } else { 700.0 }, 0.7, (h.v("mid") - 5.0) * 2.4, rate),
        Biquad::new(BiquadKind::HighShelf, 3200.0, 0.6, (h.v("treble") - 5.0) * 2.4, rate),
        Biquad::new(BiquadKind::HighShelf, 5500.0, 0.7, (h.v("presence") - 5.0) * 1.6, rate),
    ];
    let cab = h.v("cab").round() as usize;
    fl.extend(dragon_engine::effects::amp::cab_response(cab, h.v("mic").round() as usize, rate));
    let y_of = |db: f32| pr.center().y - (db / 24.0).clamp(-1.0, 1.0) * pr.height() * 0.5;
    y_axis(p, pr, &[(12.0, "+12"), (0.0, "0"), (-12.0, "-12")], y_of);
    let marks: Vec<(f32, String)> = [100.0, 1000.0, 10000.0].iter().map(|f| (*f, if *f >= 1000.0 { format!("{}k", f / 1000.0) } else { format!("{f}") })).collect();
    x_axis(p, pr, &marks, |f| fx(pr, f));
    let pts: Vec<Pos2> = (0..=140)
        .map(|k| 20.0 * 1000f32.powf(k as f32 / 140.0))
        .map(|f| {
            let db: f32 = fl.iter().map(|b| b.response_db(f, rate)).sum();
            Pos2::new(fx(pr, f), y_of(db))
        })
        .collect();
    filled_curve(p, &pts, y_of(-24.0), col);
    label(p, Pos2::new(pr.right() - 4.0, pr.top() + 2.0), egui::Align2::RIGHT_TOP, CABS.get(cab).copied().unwrap_or("").to_uppercase(), 11.0, theme::TEXT);
    let _ = TAU;
}

/// Mandrake: one bar per band (the voice's envelope in it), centred like a mouth; the carrier's pitch.
fn vocoder(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    use dragon_engine::effects::voxfx::{BAND_COUNTS, CARRIERS, T_PITCH};
    super::fx_mod::screen(p, r, 8.0);
    let n: usize = BAND_COUNTS.get(h.v("bands").round() as usize).and_then(|s| s.parse().ok()).unwrap_or(16);
    let face = Rect::from_min_max(r.min + Vec2::new(16.0, 26.0), r.max - Vec2::new(16.0, 22.0));
    let bw = face.width() / n as f32;
    let t = h.t();
    for k in 0..n {
        let e = h.tele.get(k).copied().unwrap_or(0.0);
        // Without signal (or in tests) a slow breathing shape.
        let v = if h.tele.is_empty() { 0.15 + 0.1 * ((k as f32 * 0.7 + t * 2.0).sin() + 1.0) } else { (e * 6.0).clamp(0.0, 1.0) };
        let hgt = v * face.height() * 0.5;
        let x = face.left() + (k as f32 + 0.5) * bw;
        let bar = Rect::from_center_size(Pos2::new(x, face.center().y), Vec2::new(bw * 0.7, (hgt * 2.0).max(2.0)));
        p.rect_filled(bar, 2.0, col.gamma_multiply(0.35 + 0.65 * v));
    }
    let carrier = CARRIERS.get(h.v("carrier").round() as usize).copied().unwrap_or("");
    label(p, Pos2::new(r.left() + 12.0, r.top() + 6.0), egui::Align2::LEFT_TOP, format!("{n} BANDS · {}", carrier.to_uppercase()), 11.0, theme::TEXT);
    if let Some(f) = h.tele.get(T_PITCH).filter(|f| **f > 0.0) {
        mono(p, Pos2::new(r.right() - 12.0, r.top() + 6.0), egui::Align2::RIGHT_TOP, format!("carrier {f:.0} Hz"), 10.0, theme::TEXT_DIM);
    }
    mono(p, Pos2::new(r.center().x, r.bottom() - 6.0), egui::Align2::CENTER_BOTTOM, format!("formant {:+.1} st · pitch {:+.1} st", h.v("formant"), h.v("pitch")), 10.0, theme::TEXT_DIM);
}

/// Mercury Wheel: the harmonics of a 220 Hz tone (grey) and where the shift puts them (no longer harmonic).
fn freqshift(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    use dragon_engine::effects::voxfx::SHIFT_MODES;
    grid(p, r, true);
    let pr = plot(r);
    let mode = h.v("mode").round() as usize;
    let f = h.tele.first().copied().filter(|v| *v != 0.0).unwrap_or(h.v("coarse") + h.v("fine"));
    for k in 1..=24 {
        let base = 220.0 * k as f32;
        if base > 20000.0 {
            break;
        }
        let x = fx(pr, base);
        let a = 1.0 / k as f32;
        p.line_segment([Pos2::new(x, pr.bottom()), Pos2::new(x, pr.bottom() - pr.height() * 0.85 * a.sqrt())], Stroke::new(1.0_f32, Color32::from_white_alpha(50)));
        let targets: Vec<f32> = match mode {
            0 => vec![base + f],
            1 => vec![base - f],
            2 => vec![base + f, base - f],
            _ => vec![base + f, (base - f).abs()],
        };
        for t in targets.into_iter().filter(|t| *t > 20.0) {
            let xt = fx(pr, t);
            p.line_segment([Pos2::new(xt, pr.bottom()), Pos2::new(xt, pr.bottom() - pr.height() * 0.85 * a.sqrt())], Stroke::new(2.0_f32, col));
        }
    }
    // The wheel: turns with the shifting oscillator.
    let c = Pos2::new(pr.right() - 30.0, pr.top() + 30.0);
    let ang = h.tele.get(1).copied().unwrap_or(h.t() * 0.2) * TAU;
    p.circle_stroke(c, 20.0, Stroke::new(1.5_f32, col.gamma_multiply(0.6)));
    for k in 0..6 {
        let a = ang + k as f32 * TAU / 6.0;
        p.line_segment([c, c + Vec2::new(a.cos(), a.sin()) * 20.0], Stroke::new(1.0_f32, col.gamma_multiply(0.5)));
    }
    label(p, Pos2::new(pr.left() + 4.0, pr.top() + 2.0), egui::Align2::LEFT_TOP, format!("{} · {f:+.1} Hz", SHIFT_MODES.get(mode).copied().unwrap_or("").to_uppercase()), 11.0, theme::TEXT);
}

/// Hex: one group of slices – the captured one and its repeats (height = pitch, a mark for reverse).
fn stutter(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    use dragon_engine::effects::timefx::{DIVISIONS, HOLDS, STUTTER_MODES};
    super::fx_mod::screen(p, r, 8.0);
    let n: usize = HOLDS.get(h.v("hold").round() as usize).and_then(|s| s.parse().ok()).unwrap_or(4);
    let gate = h.v("gate") / 100.0;
    let step = h.v("pitch");
    let face = Rect::from_min_max(r.min + Vec2::new(16.0, 28.0), r.max - Vec2::new(16.0, 22.0));
    let w = face.width() / n as f32;
    let active = h.tele.first().copied().unwrap_or(1.0) > 0.5;
    let pos = h.tele.get(1).copied().unwrap_or(0.0);
    for k in 0..n {
        let speed = 2f32.powf(k as f32 * step / 12.0);
        let hgt = (0.35 + 0.25 * (speed.log2())).clamp(0.1, 0.95) * face.height();
        let x0 = face.left() + k as f32 * w + 2.0;
        let cell = Rect::from_min_max(Pos2::new(x0, face.bottom() - hgt), Pos2::new(x0 + (w - 4.0) * gate, face.bottom()));
        let c = if k == 0 { theme::TEXT.gamma_multiply(0.8) } else { col.gamma_multiply(if active { 0.9 } else { 0.35 }) };
        p.rect_filled(cell, 3.0, c);
        // The slice's waveform, compressed by the pitch step.
        let pts: Vec<Pos2> = (0..=24).map(|j| j as f32 / 24.0).map(|u| Pos2::new(cell.left() + u * cell.width(), cell.center().y + (u * 6.0 * speed * TAU).sin() * cell.height() * 0.3)).collect();
        p.add(Shape::line(pts, Stroke::new(1.0_f32, Color32::from_black_alpha(140))));
    }
    let x = face.left() + pos * face.width() / n as f32;
    p.line_segment([Pos2::new(x, face.top()), Pos2::new(x, face.bottom())], Stroke::new(1.0_f32, theme::TEXT.gamma_multiply(0.5)));
    let mode = STUTTER_MODES.get(h.v("mode").round() as usize).copied().unwrap_or("");
    let div = DIVISIONS.get(h.v("division").round() as usize).copied().unwrap_or("");
    label(p, Pos2::new(r.left() + 12.0, r.top() + 6.0), egui::Align2::LEFT_TOP, format!("{} · {div} × {n}{}", mode.to_uppercase(), if active { "  · STUTTERING" } else { "" }), 11.0, theme::TEXT);
}

/// Petrify: the speed over a stop and a start, with the reel turning at the current speed.
fn tape_stop(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    grid(p, r, false);
    let pr = plot(r);
    let (stop_ms, start_ms, curve) = (h.v("stop_time"), h.v("start_time"), h.v("curve") / 100.0);
    let k = 2f32.powf(2.0 * curve);
    let total = stop_ms + 200.0 + start_ms;
    let x_of = |ms: f32| pr.left() + ms / total * pr.width();
    let y_of = |s: f32| pr.bottom() - s * pr.height();
    y_axis(p, pr, &[(1.0, "1×"), (0.5, "½"), (0.0, "0")], y_of);
    let mut pts = Vec::new();
    for j in 0..=80 {
        let u = j as f32 / 80.0;
        pts.push(Pos2::new(x_of(u * stop_ms), y_of((1.0 - u).powf(k))));
    }
    pts.push(Pos2::new(x_of(stop_ms + 200.0), y_of(0.0)));
    for j in 0..=60 {
        let u = j as f32 / 60.0;
        pts.push(Pos2::new(x_of(stop_ms + 200.0 + u * start_ms), y_of(1.0 - (1.0 - u).powf(k))));
    }
    filled_curve(p, &pts, pr.bottom(), col);
    let speed = h.tele.first().copied().unwrap_or(1.0);
    // The reel.
    let c = Pos2::new(pr.right() - 34.0, pr.top() + 34.0);
    let id = h.key.with("reel");
    let ctx = p.ctx();
    let mut a: f32 = ctx.data(|d| d.get_temp(id)).unwrap_or(0.0);
    a += speed * 0.15;
    ctx.data_mut(|d| d.insert_temp(id, a));
    p.circle_filled(c, 24.0, Color32::from_black_alpha(120));
    p.circle_stroke(c, 24.0, Stroke::new(1.5_f32, col.gamma_multiply(0.7)));
    for j in 0..3 {
        let b = a + j as f32 * TAU / 3.0;
        p.circle_filled(c + Vec2::new(b.cos(), b.sin()) * 13.0, 5.0, col.gamma_multiply(0.5));
    }
    let state = if h.v("stop") > 0.5 { "STOPPED" } else { "RUNNING" };
    label(p, Pos2::new(pr.left() + 4.0, pr.top() + 2.0), egui::Align2::LEFT_TOP, format!("{state} · speed {speed:.2}×"), 11.0, theme::TEXT);
}

/// Sandglass: the recent past (right = now), the delay and spray window, and each grain where it reads.
fn granular(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    super::fx_mod::screen(p, r, 8.0);
    let face = Rect::from_min_max(r.min + Vec2::new(14.0, 26.0), r.max - Vec2::new(14.0, 22.0));
    let span = ((h.v("position") + h.v("spray")) * 0.001 * 1.4).max(0.5);
    let x_of = |sec: f32| face.right() - (sec / span).clamp(0.0, 1.0) * face.width();
    let win = Rect::from_min_max(Pos2::new(x_of((h.v("position") + h.v("spray")) * 0.001), face.top()), Pos2::new(x_of(h.v("position") * 0.001), face.bottom()));
    p.rect_filled(win, 4.0, col.gamma_multiply(0.12));
    let size = h.v("size") * 0.001;
    let t = h.t();
    let shown: Vec<f32> = if h.tele.iter().any(|v| *v >= 0.0) {
        h.tele.iter().copied().filter(|v| *v >= 0.0).collect()
    } else {
        (0..12).map(|k| (h.v("position") + h.v("spray") * (((k as f32 * 1.7 + t).sin() + 1.0) * 0.5)) * 0.001).collect()
    };
    for (k, sec) in shown.iter().enumerate() {
        let x = x_of(*sec);
        let y = face.top() + ((k as f32 * 0.618).fract()) * face.height();
        let w = (size / span * face.width()).max(4.0);
        p.circle_filled(Pos2::new(x, y), (w * 0.25).clamp(2.5, 10.0), col.gamma_multiply(0.7));
    }
    p.line_segment([Pos2::new(face.right(), face.top()), Pos2::new(face.right(), face.bottom())], Stroke::new(1.5_f32, theme::TEXT.gamma_multiply(0.6)));
    label(p, Pos2::new(face.right(), face.bottom() + 2.0), egui::Align2::RIGHT_TOP, "now", 9.0, theme::TEXT_DIM);
    label(p, Pos2::new(face.left(), face.bottom() + 2.0), egui::Align2::LEFT_TOP, format!("-{:.2} s", span), 9.0, theme::TEXT_DIM);
    let frz = if h.v("freeze") > 0.5 { " · FROZEN" } else { "" };
    label(p, Pos2::new(r.left() + 12.0, r.top() + 6.0), egui::Align2::LEFT_TOP, format!("{:.0} grains/s · {:.0} ms · {:+.1} st{frz}", h.v("density"), h.v("size"), h.v("pitch")), 11.0, theme::TEXT);
}

/// Codex: the worn band (the age filters' response) and a wobbling pitch line (wow and flutter).
fn lofi(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    use dragon_engine::dsp::{Biquad, BiquadKind};
    grid(p, r, true);
    let pr = plot(r);
    let age = h.v("age") / 100.0;
    let rate = 48000.0;
    let hp = Biquad::new(BiquadKind::HighPass, 20.0 * 11f32.powf(age), 0.6, 0.0, rate);
    let lpf = 20_000.0 * (3500.0f32 / 20_000.0).powf(age);
    let lp = Biquad::new(BiquadKind::LowPass, lpf, 0.7, 0.0, rate);
    let lp2 = Biquad::new(BiquadKind::LowPass, lpf * 1.2, 0.6, 0.0, rate);
    let y_of = |db: f32| pr.top() + (-db / 36.0).clamp(0.0, 1.0) * pr.height() * 0.7 + pr.height() * 0.15;
    let pts: Vec<Pos2> = (0..=140).map(|k| 20.0 * 1000f32.powf(k as f32 / 140.0)).map(|f| Pos2::new(fx(pr, f), y_of(hp.response_db(f, rate) + lp.response_db(f, rate) + lp2.response_db(f, rate)))).collect();
    filled_curve(p, &pts, pr.bottom(), col);
    // Wow / flutter as a wavering line across the top.
    let t = h.t();
    let (wow, flut) = (h.v("wow") / 100.0, h.v("flutter") / 100.0);
    let wv: Vec<Pos2> = (0..=120).map(|k| k as f32 / 120.0).map(|u| Pos2::new(pr.left() + u * pr.width(), pr.top() + 14.0 + (wow * 8.0 * (u * 3.0 + t * 0.6).sin() + flut * 3.0 * (u * 40.0 + t * 9.0).sin()))).collect();
    p.add(Shape::line(wv, Stroke::new(1.5_f32, theme::TEXT.gamma_multiply(0.6))));
    let bits = h.v("bits");
    let sr = h.v("sample_rate");
    let digital = if bits < 23.5 || sr < 47_000.0 { format!(" · {bits:.0} bit / {:.1} kHz", sr / 1000.0) } else { String::new() };
    label(p, Pos2::new(pr.right() - 4.0, pr.top() + 2.0), egui::Align2::RIGHT_TOP, format!("AGE {:.0} %{digital}", age * 100.0), 11.0, theme::TEXT);
}

/// Wisp: the split frequency, the new harmonics above it (odd / even) and the air shelf.
fn exciter(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    grid(p, r, true);
    let pr = plot(r);
    let f = h.v("freq");
    let amount = h.v("amount") / 100.0;
    let character = h.v("character") / 100.0;
    let xs = fx(pr, f);
    p.rect_filled(Rect::from_min_max(Pos2::new(xs, pr.top()), pr.right_bottom()), 0.0, col.gamma_multiply(0.08));
    p.line_segment([Pos2::new(xs, pr.top()), Pos2::new(xs, pr.bottom())], Stroke::new(1.5_f32, col.gamma_multiply(0.7)));
    // A partial at the split and the harmonics the shaper adds (even ones taller with positive character).
    for k in 2..=12 {
        let hf = f * k as f32 / 2.0;
        if hf > 20000.0 {
            break;
        }
        let even = k % 2 == 0;
        let w = if even { 0.5 + 0.5 * character } else { 0.5 - 0.5 * character };
        let hgt = amount * w / (k as f32).sqrt() * pr.height() * 0.9;
        let x = fx(pr, hf);
        p.line_segment([Pos2::new(x, pr.bottom()), Pos2::new(x, pr.bottom() - hgt)], Stroke::new(2.5_f32, col));
    }
    let air = h.v("air");
    if air > 0.05 {
        let y = pr.bottom() - pr.height() * 0.1 - air / 12.0 * pr.height() * 0.3;
        p.line_segment([Pos2::new(fx(pr, 9000.0), pr.bottom() - pr.height() * 0.1), Pos2::new(fx(pr, 14000.0), y)], Stroke::new(1.5_f32, theme::TEXT.gamma_multiply(0.6)));
        p.line_segment([Pos2::new(fx(pr, 14000.0), y), Pos2::new(pr.right(), y)], Stroke::new(1.5_f32, theme::TEXT.gamma_multiply(0.6)));
    }
    let listen = if h.v("listen") > 0.5 { " · LISTEN" } else { "" };
    label(p, Pos2::new(pr.left() + 4.0, pr.top() + 2.0), egui::Align2::LEFT_TOP, format!("ABOVE {:.1} kHz{listen}", f / 1000.0), 11.0, theme::TEXT);
}

/// Familiar: the filter's response at its current cutoff, and a strip with the LFO's shape.
fn autofilter(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    use dragon_engine::dsp::{Biquad, BiquadKind};
    use dragon_engine::effects::colorfx::{FILTER_TYPES, LFO_SHAPES, SYNCS};
    grid(p, r, true);
    let pr = plot(r);
    let kind = h.v("type").round() as usize;
    let fc = h.tele.first().copied().filter(|v| *v > 0.0).unwrap_or(h.v("cutoff"));
    let reso = h.v("resonance") / 100.0;
    // The SVF's response equals an RBJ section with Q = 1 / k, k = 2 − 1.95·r.
    let q = 1.0 / (2.0 - 1.95 * reso);
    let bk = match kind {
        0 => BiquadKind::LowPass,
        1 => BiquadKind::BandPass,
        2 => BiquadKind::HighPass,
        _ => BiquadKind::Notch,
    };
    let b = Biquad::new(bk, fc, q, 0.0, 48000.0);
    let y_of = |db: f32| pr.top() + pr.height() * 0.25 - (db / 36.0) * pr.height() * 0.75;
    let pts: Vec<Pos2> = (0..=160).map(|k| 20.0 * 1000f32.powf(k as f32 / 160.0)).map(|f| Pos2::new(fx(pr, f), y_of(b.response_db(f, 48000.0).clamp(-36.0, 18.0)))).collect();
    filled_curve(p, &pts, pr.bottom(), col);
    let x = fx(pr, fc);
    p.circle_filled(Pos2::new(x, y_of(b.response_db(fc, 48000.0).clamp(-36.0, 18.0))), 4.0, theme::TEXT);
    let sync = SYNCS.get(h.v("sync").round() as usize).copied().unwrap_or("");
    let rate = if sync == "Free" { format!("{:.2} Hz", h.v("lfo_rate")) } else { sync.to_string() };
    label(
        p,
        Pos2::new(pr.right() - 4.0, pr.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        format!(
            "{} · {:.0} Hz · LFO {} {rate} ±{:.1} oct",
            FILTER_TYPES.get(kind).copied().unwrap_or("").to_uppercase(),
            fc,
            LFO_SHAPES.get(h.v("lfo_shape").round() as usize).copied().unwrap_or(""),
            h.v("lfo_depth") * 0.5
        ),
        10.5,
        theme::TEXT,
    );
}

/// Seraph: the generations of the tail, each a pitch step higher (or lower) and quieter.
fn shimmer(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    super::fx_mod::screen(p, r, 8.0);
    let face = Rect::from_min_max(r.min + Vec2::new(16.0, 26.0), r.max - Vec2::new(16.0, 20.0));
    let shift = h.v("shift");
    let sh = h.v("shimmer") / 100.0;
    let decay = h.v("decay");
    let t = h.t();
    // Vertical: pitch (octaves above the source, −1 … +4); horizontal: time in rounds.
    let y_of = |oct: f32| face.bottom() - ((oct + 1.0) / 5.0).clamp(0.0, 1.0) * face.height();
    for gen in 0..8 {
        let oct = gen as f32 * shift / 12.0;
        if !(-1.0..=4.0).contains(&oct) {
            break;
        }
        let level = sh.powi(gen) * (-(gen as f32) * 0.25 / decay.max(0.5) * 6.0).exp();
        if level < 0.02 {
            break;
        }
        let y = y_of(oct);
        let x0 = face.left() + gen as f32 * face.width() * 0.1;
        let pts: Vec<Pos2> = (0..=60).map(|k| k as f32 / 60.0).map(|u| Pos2::new(x0 + u * (face.right() - x0), y + (u * 30.0 + t * 2.0 + gen as f32).sin() * 2.0)).collect();
        p.add(Shape::line(pts, Stroke::new(1.0 + 3.0 * level, col.gamma_multiply(0.25 + 0.75 * level))));
    }
    label(p, Pos2::new(r.left() + 12.0, r.top() + 6.0), egui::Align2::LEFT_TOP, format!("{shift:+.0} st per round · shimmer {:.0} % · {decay:.1} s", sh * 100.0), 11.0, theme::TEXT);
}

/// Rime: the held (or live) spectrum, iced over as the freeze fades in.
fn freeze(p: &egui::Painter, r: Rect, h: &Hero, col: Color32) {
    use dragon_engine::effects::spacefx::FREEZE_BANDS;
    grid(p, r, true);
    let pr = plot(r);
    let amount = h.tele.get(FREEZE_BANDS).copied().unwrap_or(0.0);
    let y_of = |db: f32| pr.top() + (-db / 90.0).clamp(0.0, 1.0) * pr.height();
    if h.tele.len() >= FREEZE_BANDS {
        let pts: Vec<Pos2> = (0..FREEZE_BANDS)
            .map(|b| {
                let f = 30.0 * (16000.0f32 / 30.0).powf(b as f32 / (FREEZE_BANDS - 1) as f32);
                Pos2::new(fx(pr, f), y_of(h.tele[b]))
            })
            .collect();
        let c = if amount > 0.0 { Color32::from_rgb(0xc0, 0xe8, 0xff) } else { col };
        filled_curve(p, &pts, pr.bottom(), c);
    }
    let on = h.v("freeze") > 0.5;
    let state = if on {
        "FROZEN"
    } else if amount > 0.0 {
        "THAWING"
    } else {
        "LIVE"
    };
    label(p, Pos2::new(pr.right() - 4.0, pr.top() + 2.0), egui::Align2::RIGHT_TOP, format!("{state} · {:.0} %", amount * 100.0), 11.0, theme::TEXT);
}
