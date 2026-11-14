//! Video window: the project's video at the playhead (decoded on demand in the background), with timecode.

use crate::state::State;
use crate::theme;
use dragon_engine::time::{timecode, TempoMap};
use egui::{RichText, Ui};
use redhive_client_framework::runtime::{poll, Promise};

type Frame = Result<(f64, u32, u32, Vec<u8>), String>;

#[derive(Default)]
pub struct VideoState {
    texture: Option<(f64, egui::TextureHandle)>,
    job: Option<Promise<Frame>>,
    wanted: f64,
}

/// Seconds into the video file for the current playhead (`None` before the video starts).
pub fn video_time(st: &State) -> Option<f64> {
    let v = st.session.project.video.as_ref()?;
    let tm = TempoMap::from_project(&st.session.project);
    let t = tm.beat_to_seconds(st.beat_now().max(0.0)) - tm.beat_to_seconds(v.start_beat) + v.offset;
    (t >= 0.0 && t <= v.duration + 0.5).then_some(t.min(v.duration - 0.04).max(0.0))
}

pub fn show(ui: &mut Ui, st: &mut State) {
    let Some(v) = st.session.project.video.clone() else {
        ui.vertical_centered(|ui| {
            ui.add_space(30.0);
            ui.label("No video in this project.");
            if ui.button("Import video…").clicked() {
                st.ctx.run("dragon.file.import_video", serde_json::Value::Null);
            }
            if !dragon_engine::video::available() {
                ui.label(RichText::new("Video needs ffmpeg and ffprobe on the PATH (or DRAGON_FFMPEG / DRAGON_FFPROBE).").small().color(theme::WARN));
            }
        });
        return;
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(v.path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()).strong());
        ui.label(RichText::new(format!("{}×{} · {:.3} fps · {:.1} s", v.width, v.height, v.fps, v.duration)).small().color(theme::TEXT_DIM));
        let mut start = v.start_beat;
        let r = ui.add(egui::DragValue::new(&mut start).range(0.0..=100000.0).speed(0.25).prefix("starts at beat "));
        crate::views::widgets::apply(st, &r, "Video position", |p| {
            if let Some(x) = &mut p.video {
                x.start_beat = start;
            }
        });
        if ui.small_button("Remove video").clicked() {
            st.session.edit("Remove video", |p| p.video = None);
        }
    });
    let t = video_time(st);
    let vs = &mut st.ui.video;
    if let Some(r) = poll(&mut vs.job) {
        match r {
            Ok((secs, w, h, rgba)) => {
                let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                match &mut vs.texture {
                    Some((s, tex)) => {
                        tex.set(img, egui::TextureOptions::LINEAR);
                        *s = secs;
                    }
                    None => vs.texture = Some((secs, ui.ctx().load_texture("dragon.video", img, egui::TextureOptions::LINEAR))),
                }
            }
            Err(e) => st.ui.status = e,
        }
    }
    let vs = &mut st.ui.video;
    if let Some(t) = t {
        let frame_dur = 1.0 / v.fps.max(1.0);
        let shown = vs.texture.as_ref().map(|x| x.0).unwrap_or(-1.0);
        vs.wanted = t;
        if vs.job.is_none() && (t - shown).abs() >= frame_dur * 0.5 {
            let (path, w, h) = (v.path.clone(), v.width, v.height);
            let width = (ui.clip_rect().width() as u32).clamp(160, 960);
            vs.job = Some(Promise::spawn(async move {
                tokio::task::spawn_blocking(move || dragon_engine::video::frame_at(&path, t, width, w, h).map(|(a, b, c)| (t, a, b, c)).map_err(|e| e.to_string())).await.unwrap_or_else(|e| Err(e.to_string()))
            }));
        }
    }
    // Stay inside the visible part of the panel.
    let rect = ui.available_rect_before_wrap().intersect(ui.clip_rect());
    ui.allocate_rect(rect, egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, egui::Color32::BLACK);
    if let Some((secs, tex)) = &st.ui.video.texture {
        let aspect = v.width as f32 / v.height.max(1) as f32;
        let mut size = egui::vec2(rect.width(), rect.width() / aspect);
        if size.y > rect.height() {
            size = egui::vec2(rect.height() * aspect, rect.height());
        }
        let r = egui::Rect::from_center_size(rect.center(), size);
        ui.painter().image(tex.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
        ui.painter().text(r.left_bottom() + egui::vec2(8.0, -8.0), egui::Align2::LEFT_BOTTOM, timecode(*secs, v.fps), egui::FontId::monospace(16.0), egui::Color32::WHITE);
    }
    if t.is_none() {
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "(before / after the video)", egui::FontId::proportional(14.0), theme::TEXT_DIM);
    }
    if st.transport().playing || st.ui.video.job.is_some() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
    }
}
