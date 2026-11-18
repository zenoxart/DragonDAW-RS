//! The audio clip window (double-click an audio clip): the clip's waveform with its fades and gain, click
//! to listen from anywhere, the two big tools – pitch correction (note editor) and audio repair – quick
//! actions, and the clip's settings in plain groups (level, fades, pitch & time).

use crate::state::State;
use crate::theme;
use dragon_engine::model::{AudioClip, Clip, FadeCurve, Id, Project, StretchMode};
use dragon_engine::samples::AudioData;
use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use redhive_contracts::ui::Severity;

use super::repair::Tool;
use super::widgets;

fn secs_text(s: f64) -> String {
    let s = s.max(0.0);
    format!("{}:{:04.1}", (s / 60.0) as u32, s % 60.0)
}

/// Changes the clip `cid` on `tid`.
fn set(p: &mut Project, tid: Id, cid: Id, f: &dyn Fn(&mut Clip)) {
    if let Some(c) = p.track_mut(tid).and_then(|t| t.clips.iter_mut().find(|c| c.id == cid)) {
        f(c);
    }
}

fn set_audio(p: &mut Project, tid: Id, cid: Id, f: &dyn Fn(&mut AudioClip)) {
    set(p, tid, cid, &|c| {
        if let Some(a) = c.audio_mut() {
            f(a)
        }
    });
}

/// The two big tool buttons.
fn tool_card(ui: &mut Ui, w: f32, title: &str, sub: &str, icon: fn(&egui::Painter, Rect, Color32)) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 74.0), Sense::click());
    let hot = resp.hovered();
    let p = ui.painter();
    p.rect_filled(r, 10.0, if hot { Color32::from_rgb(0x3a, 0x1c, 0x16) } else { Color32::from_rgb(0x24, 0x1a, 0x19) });
    p.rect_stroke(r, 10.0, Stroke::new(if hot { 2.0_f32 } else { 1.0 }, if hot { theme::ACCENT } else { Color32::from_white_alpha(30) }), egui::StrokeKind::Inside);
    icon(p, Rect::from_center_size(Pos2::new(r.left() + 34.0, r.center().y), Vec2::splat(36.0)), if hot { theme::ACCENT_2 } else { theme::ACCENT });
    p.text(Pos2::new(r.left() + 64.0, r.top() + 14.0), egui::Align2::LEFT_TOP, title, egui::FontId::proportional(15.0), Color32::WHITE);
    let g = p.layout(sub.to_string(), egui::FontId::proportional(11.0), theme::TEXT_DIM, w - 76.0);
    p.galley(Pos2::new(r.left() + 64.0, r.top() + 34.0), g, theme::TEXT_DIM);
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Note blobs on a staff: the pitch tool.
fn pitch_icon(p: &egui::Painter, r: Rect, c: Color32) {
    for k in 0..3 {
        let y = r.top() + r.height() * (0.25 + 0.25 * k as f32);
        p.line_segment([Pos2::new(r.left(), y), Pos2::new(r.right(), y)], Stroke::new(1.0_f32, c.gamma_multiply(0.35)));
    }
    for (x, y, w) in [(0.0f32, 0.62f32, 0.32f32), (0.38, 0.38, 0.28), (0.7, 0.5, 0.3)] {
        let b = Rect::from_min_size(Pos2::new(r.left() + x * r.width(), r.top() + y * r.height() - 4.0), Vec2::new(w * r.width(), 8.0));
        p.rect_filled(b, 4.0, c);
    }
    // A bent pitch line being straightened.
    p.add(egui::Shape::line((0..=12).map(|k| Pos2::new(r.left() + k as f32 * r.width() / 12.0, r.top() + r.height() * 0.15 + 2.5 * (k as f32 * 1.3).sin())).collect(), Stroke::new(1.5_f32, c)));
}

/// A waveform with a spark: the repair tool.
fn repair_icon(p: &egui::Painter, r: Rect, c: Color32) {
    let pts: Vec<Pos2> = (0..=24)
        .map(|k| {
            let t = k as f32 / 24.0;
            let a = (t * 18.0).sin() * (0.35 - 0.25 * (t - 0.5).abs());
            Pos2::new(r.left() + t * r.width(), r.center().y + a * r.height())
        })
        .collect();
    p.add(egui::Shape::line(pts, Stroke::new(1.8_f32, c)));
    let s = Pos2::new(r.right() - 6.0, r.top() + 6.0);
    for (dx, dy) in [(0.0f32, -6.0f32), (0.0, 6.0), (-6.0, 0.0), (6.0, 0.0)] {
        p.line_segment([s, s + Vec2::new(dx, dy)], Stroke::new(1.6_f32, c));
    }
}

/// The clip's file as a waveform: the clip's part bright (with its gain and fades), the rest dim, the
/// preview's playhead; click to listen from there. Returns the clicked position (seconds in the file).
fn wave(ui: &mut Ui, st: &State, d: &AudioData, a: &AudioClip, span: (f64, f64), fades: (f64, f64)) -> Option<f64> {
    let w = ui.available_width().max(300.0);
    let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 130.0), Sense::click());
    let p = ui.painter_at(r);
    p.rect_filled(r, 8.0, Color32::from_rgb(0x10, 0x11, 0x16));
    let dur = d.duration().max(1e-6);
    let x_of = |t: f64| r.left() + 6.0 + ((t / dur) as f32).clamp(0.0, 1.0) * (r.width() - 12.0);
    let t_of = |x: f32| (((x - r.left() - 6.0) / (r.width() - 12.0)) as f64 * dur).clamp(0.0, dur);
    let (c0, c1) = (span.0, (span.0 + span.1).min(dur));
    // The clip's part.
    p.rect_filled(Rect::from_min_max(Pos2::new(x_of(c0), r.top() + 4.0), Pos2::new(x_of(c1), r.bottom() - 4.0)), 6.0, theme::ACCENT.gamma_multiply(0.10));
    let gain = 10f32.powf(a.gain_db / 20.0);
    let cols = (r.width() - 12.0) as usize;
    let mid = r.center().y;
    let amp = r.height() * 0.44;
    for c in 0..cols {
        let (i0, i1) = (c * d.frames() / cols.max(1), ((c + 1) * d.frames() / cols.max(1)).min(d.frames()));
        let step = ((i1 - i0) / 48).max(1);
        let (mut lo, mut hi) = (0.0f32, 0.0f32);
        let mut i = i0;
        while i < i1 {
            let v = 0.5 * (d.l[i] + d.r[i]);
            lo = lo.min(v);
            hi = hi.max(v);
            i += step;
        }
        let t = (i0 as f64 + 0.5) / d.rate as f64;
        let inside = t >= c0 && t <= c1;
        // Gain and fades shape what is heard.
        let shape = if inside {
            let fi = if fades.0 > 0.0 { ((t - c0) / fades.0).clamp(0.0, 1.0) as f32 } else { 1.0 };
            let fo = if fades.1 > 0.0 { ((c1 - t) / fades.1).clamp(0.0, 1.0) as f32 } else { 1.0 };
            gain * fi * fo
        } else {
            1.0
        };
        let x = r.left() + 6.0 + c as f32 + 0.5;
        let col = if inside { theme::ACCENT_2 } else { Color32::from_white_alpha(45) };
        p.line_segment([Pos2::new(x, mid - (hi * shape).clamp(-1.0, 1.0) * amp), Pos2::new(x, mid - (lo * shape).clamp(-1.0, 1.0) * amp)], Stroke::new(1.0_f32, col));
    }
    // Fade curves.
    let fade_line = |from: f64, to: f64, rising: bool| {
        let pts: Vec<Pos2> = (0..=16)
            .map(|k| {
                let q = k as f32 / 16.0;
                let g = if rising { q } else { 1.0 - q };
                Pos2::new(x_of(from + (to - from) * q as f64), r.bottom() - 6.0 - g * (r.height() - 12.0))
            })
            .collect();
        p.add(egui::Shape::line(pts, Stroke::new(1.5_f32, Color32::WHITE.gamma_multiply(0.7))));
    };
    if fades.0 > 0.0 {
        fade_line(c0, c0 + fades.0, true);
    }
    if fades.1 > 0.0 {
        fade_line(c1 - fades.1, c1, false);
    }
    // Clip edges.
    for x in [x_of(c0), x_of(c1)] {
        p.line_segment([Pos2::new(x, r.top() + 4.0), Pos2::new(x, r.bottom() - 4.0)], Stroke::new(1.5_f32, theme::ACCENT));
    }
    // Playhead of the running preview.
    if let (Some(t0), Some(sel)) = (st.ui.preview_started, &st.ui.browser_sel) {
        if a.source == *sel {
            let t = st.ui.preview_offset + t0.elapsed().as_secs_f64();
            if t <= dur {
                let x = x_of(t);
                p.line_segment([Pos2::new(x, r.top()), Pos2::new(x, r.bottom())], Stroke::new(2.0_f32, Color32::WHITE));
                ui.ctx().request_repaint();
            }
        }
    }
    // Times.
    let small = egui::FontId::proportional(10.0);
    p.text(Pos2::new(x_of(c0) + 4.0, r.top() + 6.0), egui::Align2::LEFT_TOP, secs_text(c0), small.clone(), theme::TEXT_DIM);
    p.text(Pos2::new(x_of(c1) - 4.0, r.top() + 6.0), egui::Align2::RIGHT_TOP, secs_text(c1), small.clone(), theme::TEXT_DIM);
    let mut clicked = None;
    if let Some(h) = resp.hover_pos() {
        p.line_segment([Pos2::new(h.x, r.top()), Pos2::new(h.x, r.bottom())], Stroke::new(1.0_f32, Color32::from_white_alpha(70)));
        p.text(Pos2::new(h.x + 4.0, r.bottom() - 6.0), egui::Align2::LEFT_BOTTOM, format!("▶ {}", secs_text(t_of(h.x))), small, Color32::WHITE);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.clicked() {
        clicked = resp.interact_pointer_pos().map(|p| t_of(p.x));
    }
    clicked
}

fn group(ui: &mut Ui, title: &str, body: impl FnOnce(&mut Ui)) {
    egui::Frame::NONE.fill(Color32::from_white_alpha(6)).corner_radius(8.0).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title.to_uppercase()).small().strong().color(theme::ACCENT));
        body(ui);
    });
    ui.add_space(6.0);
}

pub fn show(ui: &mut Ui, st: &mut State, cid: Id) {
    let Some((tid, clip)) = st.session.project.tracks.iter().find_map(|t| t.clips.iter().find(|c| c.id == cid).map(|c| (t.id, c.clone()))) else {
        st.ui.clip_window = None;
        return;
    };
    let Some(a) = clip.audio().cloned() else { return };
    let data = st.session.cache.get(&a.source, st.audio.rate()).ok();
    let bpm = st.session.project.bpm.max(1.0);
    let beat_secs = 60.0 / bpm;
    // What part of the file the clip plays, in file seconds.
    let span = (a.offset, clip.length * beat_secs * a.speed.max(1e-6));
    let fades = (a.fade_in * beat_secs * a.speed, a.fade_out * beat_secs * a.speed);

    // Header: name, file, mute.
    ui.horizontal(|ui| {
        let mut name = clip.name.clone();
        let r = ui.add(egui::TextEdit::singleline(&mut name).font(egui::FontId::proportional(17.0)).desired_width(ui.available_width() - 120.0).hint_text("Clip name"));
        if r.changed() {
            st.session.gesture("Rename clip", |p| set(p, tid, cid, &|c| c.name = name.clone()));
        }
        let mut muted = clip.muted;
        let label = if muted { "Muted" } else { "Mute" };
        if ui.toggle_value(&mut muted, label).changed() {
            st.session.edit("Mute clip", |p| set(p, tid, cid, &|c| c.muted = muted));
        }
    });
    ui.horizontal(|ui| {
        let file = a.source.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let info = match &data {
            Some(d) => format!("{} of {}  ·  {:.1} kHz {}  ·  {file}", secs_text(span.1.min(d.duration())), secs_text(d.duration()), d.source_rate as f32 / 1000.0, if d.source_channels == 1 { "mono" } else { "stereo" }),
            None => format!("file missing  ·  {file}"),
        };
        ui.label(RichText::new(info).small().color(theme::TEXT_DIM)).on_hover_text(a.source.display().to_string());
        if ui.small_button("Show in folder").clicked() {
            if let Some(dir) = a.source.parent() {
                crate::chains::reveal(dir);
            }
        }
    });
    ui.add_space(4.0);

    // The waveform and listening.
    let Some(d) = data else {
        ui.label(RichText::new("The audio file could not be loaded.").color(theme::WARN));
        return;
    };
    if let Some(t) = wave(ui, st, &d, &a, span, fades) {
        st.preview_from(&a.source, t);
    }
    let playing = st.ui.preview_started.is_some() && st.ui.browser_sel.as_ref() == Some(&a.source);
    ui.horizontal(|ui| {
        let play = egui::Button::new(RichText::new(if playing { "■  Stop" } else { "▶  Play clip" }).strong().size(14.0)).min_size(Vec2::new(120.0, 30.0)).fill(theme::ACCENT.gamma_multiply(0.55));
        if ui.add(play).on_hover_text("Listen to the clip's part of the file (click the waveform to start anywhere)").clicked() {
            if playing {
                st.stop_preview();
            } else {
                st.preview_from(&a.source, span.0);
            }
        }
        if ui.button("▶  Whole file").clicked() {
            st.preview_from(&a.source, 0.0);
        }
        ui.label(RichText::new("Click the waveform to listen from any point.").small().color(theme::TEXT_DIM));
    });
    ui.add_space(8.0);

    // The two big tools.
    let w = ((ui.available_width() - 10.0) / 2.0).max(180.0);
    ui.horizontal(|ui| {
        if tool_card(ui, w, "Pitch correction", "See every note, drag it to the right pitch, centre it, flatten drift and vibrato", pitch_icon).clicked() {
            super::repair::open_with(st, cid, Tool::Pitch);
            // Out of the way of the tool.
            st.ui.clip_window = None;
        }
        if tool_card(ui, w, "Audio repair", "Remove noise, hum, clicks, clipping, distortion and room reverb", repair_icon).clicked() {
            super::repair::open_with(st, cid, Tool::Denoise);
            st.ui.clip_window = None;
        }
    });
    ui.add_space(8.0);

    // Quick actions.
    ui.horizontal_wrapped(|ui| {
        if ui.button("Normalize").on_hover_text("Sets the clip gain so its loudest peak reaches −0.3 dB").clicked() {
            let (a0, a1) = ((span.0 * d.rate as f64) as usize, (((span.0 + span.1) * d.rate as f64) as usize).min(d.frames()));
            let peak = d.l[a0.min(a1)..a1].iter().chain(&d.r[a0.min(a1)..a1]).fold(0.0f32, |m, v| m.max(v.abs()));
            if peak > 1e-6 {
                let g = -0.3 - 20.0 * peak.log10();
                st.session.edit("Normalize clip", |p| set_audio(p, tid, cid, &|x| x.gain_db = g));
                st.toast(format!("Clip gain {g:+.1} dB"), Severity::Info);
            }
        }
        let mut rev = a.reverse;
        if ui.toggle_value(&mut rev, "Reverse").changed() {
            st.session.edit("Reverse clip", |p| set_audio(p, tid, cid, &|x| x.reverse = rev));
        }
        if ui.button("Fit to project tempo").on_hover_text("Detects the clip's tempo and stretches it to the project tempo").clicked() {
            match dragon_engine::analysis::detect_bpm(&d).0 {
                Some(b) => {
                    st.session.edit("Fit to tempo", |p| {
                        set_audio(p, tid, cid, &|x| {
                            x.source_bpm = Some(b);
                            x.stretch = StretchMode::Stretch;
                            x.speed = bpm / b;
                        })
                    });
                    st.toast(format!("Detected {b:.1} BPM – stretched to {bpm:.1}"), Severity::Info);
                }
                None => st.toast("No clear tempo found in this clip", Severity::Info),
            }
        }
        if ui.button("Reset").on_hover_text("Back to the plain file: no gain, pan, fades, pitch or speed change").clicked() {
            st.session.edit("Reset clip", |p| {
                set_audio(p, tid, cid, &|x| {
                    x.gain_db = 0.0;
                    x.pan = 0.0;
                    x.fade_in = 0.0;
                    x.fade_out = 0.0;
                    x.pitch = 0.0;
                    x.speed = 1.0;
                    x.reverse = false;
                    x.source_bpm = None;
                    x.gain_line.clear();
                })
            });
        }
    });
    ui.add_space(8.0);

    group(ui, "Level", |ui| {
        ui.horizontal(|ui| {
            let mut g = a.gain_db;
            ui.label("Volume");
            let r = ui.add(egui::Slider::new(&mut g, -48.0..=24.0).suffix(" dB").fixed_decimals(1));
            widgets::apply(st, &r, "Clip gain", |p| set_audio(p, tid, cid, &|x| x.gain_db = g));
        });
        ui.horizontal(|ui| {
            let mut pan = a.pan;
            ui.label("Pan");
            let r = ui.add(egui::Slider::new(&mut pan, -1.0..=1.0).custom_formatter(|v, _| {
                if v.abs() < 0.005 {
                    "centre".into()
                } else if v < 0.0 {
                    format!("L {:.0}", -v * 100.0)
                } else {
                    format!("R {:.0}", v * 100.0)
                }
            }));
            widgets::apply(st, &r, "Clip pan", |p| set_audio(p, tid, cid, &|x| x.pan = pan));
            if pan != 0.0 && ui.small_button("centre").clicked() {
                st.session.edit("Clip pan", |p| set_audio(p, tid, cid, &|x| x.pan = 0.0));
            }
        });
    });

    group(ui, "Fades", |ui| {
        // Stored in beats; shown in milliseconds at the project tempo.
        let to_ms = |beats: f64| (beats * beat_secs * 1000.0) as f32;
        let to_beats = |ms: f32| ms as f64 / 1000.0 / beat_secs;
        let max_ms = (clip.length * beat_secs * 1000.0 * 0.5).max(10.0) as f32;
        ui.horizontal(|ui| {
            let mut fi = to_ms(a.fade_in);
            ui.label("Fade in");
            let r = ui.add(egui::Slider::new(&mut fi, 0.0..=max_ms).suffix(" ms").logarithmic(true).fixed_decimals(0));
            widgets::apply(st, &r, "Fade in", |p| set_audio(p, tid, cid, &|x| x.fade_in = to_beats(fi)));
        });
        ui.horizontal(|ui| {
            let mut fo = to_ms(a.fade_out);
            ui.label("Fade out");
            let r = ui.add(egui::Slider::new(&mut fo, 0.0..=max_ms).suffix(" ms").logarithmic(true).fixed_decimals(0));
            widgets::apply(st, &r, "Fade out", |p| set_audio(p, tid, cid, &|x| x.fade_out = to_beats(fo)));
        });
        ui.horizontal(|ui| {
            ui.label("Shape");
            let mut fc = a.fade_curve;
            for (v, n, tip) in [
                (FadeCurve::Linear, "Straight", "A straight line"),
                (FadeCurve::EqualPower, "Smooth", "Equal power: no dip in loudness, best for crossfades"),
                (FadeCurve::Exponential, "Natural", "Exponential: like a sound dying away"),
            ] {
                ui.selectable_value(&mut fc, v, n).on_hover_text(tip);
            }
            if fc != a.fade_curve {
                st.session.edit("Fade curve", |p| set_audio(p, tid, cid, &|x| x.fade_curve = fc));
            }
        });
    });

    group(ui, "Pitch & time", |ui| {
        ui.horizontal(|ui| {
            let mut pitch = a.pitch;
            ui.label("Transpose");
            let r = ui.add(egui::Slider::new(&mut pitch, -24.0..=24.0).suffix(" semitones").step_by(0.01));
            widgets::apply(st, &r, "Clip pitch", |p| set_audio(p, tid, cid, &|x| x.pitch = pitch));
            for (d, n) in [(-12.0f32, "−12"), (12.0, "+12")] {
                if ui.small_button(n).on_hover_text("An octave").clicked() {
                    let v = (a.pitch + d).clamp(-24.0, 24.0);
                    st.session.edit("Clip pitch", |p| set_audio(p, tid, cid, &|x| x.pitch = v));
                }
            }
        });
        ui.horizontal(|ui| {
            let mut speed = (a.speed * 100.0) as f32;
            ui.label("Speed");
            let r = ui.add(egui::Slider::new(&mut speed, 25.0..=400.0).suffix(" %").logarithmic(true).fixed_decimals(0));
            widgets::apply(st, &r, "Clip speed", |p| set_audio(p, tid, cid, &|x| x.speed = speed as f64 / 100.0));
        });
        ui.horizontal(|ui| {
            ui.label("When the speed changes");
            let mut mode = a.stretch;
            ui.selectable_value(&mut mode, StretchMode::Stretch, "keep the pitch").on_hover_text("Time-stretch: faster or slower, same pitch");
            ui.selectable_value(&mut mode, StretchMode::Resample, "like a tape").on_hover_text("Resample: faster is higher, slower is lower");
            if mode != a.stretch {
                st.session.edit("Stretch mode", |p| set_audio(p, tid, cid, &|x| x.stretch = mode));
            }
        });
        if let Some(b) = a.source_bpm {
            ui.label(RichText::new(format!("Recorded at {b:.1} BPM, played at {bpm:.1} BPM")).small().color(theme::TEXT_DIM));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tool_cards_open_the_note_editor_and_the_repair_tools() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("vox.wav");
        let rate = 48_000u32;
        let tone: Vec<f32> = (0..rate).map(|i| 0.4 * (std::f32::consts::TAU * 220.0 * i as f32 / rate as f32).sin()).collect();
        dragon_engine::repair::write_wav(&src, &AudioData::from_planar(rate, tone.clone(), tone)).unwrap();
        let mut st = State::new(redhive_client_framework::AppContext::new("en"), crate::state::Settings { show_splash: false, ..Default::default() });
        st.new_project();
        st.add_audio_clip_at(&src, None, 0.0);
        let cid = st.ui.clips[0];
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(700.0, 900.0));
        let frame = |st: &mut State, events: Vec<egui::Event>| {
            let _ = ctx.run(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| show(ui, st, cid));
            });
        };
        frame(&mut st, vec![]);
        frame(&mut st, vec![]);
        // The cards sit under the waveform and the play row: find them by trying points across the width.
        let click = |at: Pos2| {
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
                egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() },
            ]
        };
        let mut found = (false, false);
        for y in (180..340).step_by(6) {
            for (x, which) in [(150.0f32, 0), (520.0, 1)] {
                st.ui.repair.clip = None;
                st.ui.clip_window = Some(cid);
                frame(&mut st, click(Pos2::new(x, y as f32)));
                frame(&mut st, vec![]);
                if st.ui.repair.clip == Some(cid) {
                    if which == 0 {
                        found.0 = st.ui.repair.tool_is(Tool::Pitch);
                    } else {
                        found.1 = st.ui.repair.tool_is(Tool::Denoise);
                    }
                }
            }
            if found.0 && found.1 {
                break;
            }
        }
        assert!(found.0, "Pitch correction opens the note editor");
        assert!(found.1, "Audio repair opens the repair tools");
    }
}
