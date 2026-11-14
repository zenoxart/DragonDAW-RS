//! Performance mode / clip launcher: a grid of tracks × patterns. Clicking a cell starts that pattern looping
//! on the track's lane at the next bar (or beat, or immediately); launched patterns play on top of the song.

use crate::state::State;
use crate::theme;
use dragon_engine::device::Command as Cmd;
use dragon_engine::render::{LiveEvent, PlayMode};
use egui::{Color32, RichText, Ui, Vec2};

const QUANT: &[(&str, f64)] = &[("Bar", 4.0), ("Beat", 1.0), ("Off", 0.0)];

pub fn show(ui: &mut Ui, st: &mut State) {
    let qid = egui::Id::new("launcher.quant");
    let mut quant: f64 = ui.data(|d| d.get_temp(qid).unwrap_or(4.0));
    let m = st.audio.meters().snapshot();
    ui.horizontal(|ui| {
        ui.label("Launch quantise");
        for (n, v) in QUANT {
            ui.selectable_value(&mut quant, *v, *n);
        }
        ui.separator();
        if ui.button("⏹ Stop all").clicked() {
            st.audio.send(Cmd::Live(LiveEvent::StopAllLaunches { quantize: quant }));
        }
        ui.label(RichText::new("Click a cell to launch a pattern on that track; launched patterns loop on top of the song. Right-click a cell or use ⏹ to stop the lane.").small().color(theme::TEXT_DIM));
    });
    ui.data_mut(|d| d.insert_temp(qid, quant));
    ui.separator();
    let tracks: Vec<(u64, String, dragon_engine::model::Rgb)> = st.session.project.tracks.iter().map(|t| (t.id, t.name.clone(), t.color)).collect();
    let patterns: Vec<(u64, String, dragon_engine::model::Rgb)> = st.session.project.patterns.iter().map(|p| (p.id, p.name.clone(), p.color)).collect();
    let blink = (ui.input(|i| i.time) * 4.0).floor() as i64 % 2 == 0;
    egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
        egui::Grid::new("launcher.grid").spacing([4.0, 4.0]).show(ui, |ui| {
            ui.label("");
            for (_, name, _) in &patterns {
                ui.add_sized([110.0, 18.0], egui::Label::new(RichText::new(name).small().strong()).truncate());
            }
            ui.end_row();
            for (tid, tname, tcol) in &tracks {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::new(4.0, 28.0), egui::Sense::hover());
                    ui.painter().rect_filled(r, 1.0, theme::rgb(*tcol));
                    ui.add_sized([90.0, 28.0], egui::Label::new(tname).truncate());
                    let playing_lane = m.launched.iter().any(|(l, _)| l == tid);
                    if ui.add_enabled(playing_lane, egui::Button::new("⏹")).on_hover_text("Stop this lane").clicked() {
                        st.audio.send(Cmd::Live(LiveEvent::Launch { lane: *tid, pattern: None, quantize: quant }));
                    }
                });
                for (pid, pname, pcol) in &patterns {
                    let playing = m.launched.contains(&(*tid, *pid));
                    let queued = m.queued.iter().any(|(l, p)| l == tid && *p == Some(*pid));
                    let fill = if playing {
                        theme::OK
                    } else if queued && blink {
                        theme::ACCENT_2
                    } else {
                        theme::rgb(*pcol).gamma_multiply(0.35)
                    };
                    let text = RichText::new(if playing { format!("▶ {pname}") } else { pname.clone() }).small().color(if playing { Color32::BLACK } else { Color32::WHITE });
                    let r = ui.add(egui::Button::new(text).fill(fill).min_size(Vec2::new(110.0, 28.0)));
                    if r.clicked() {
                        if !st.transport().playing {
                            st.ui.pattern_mode = false;
                            st.audio.send(Cmd::Play(PlayMode::Song));
                        }
                        st.audio.send(Cmd::Live(LiveEvent::Launch { lane: *tid, pattern: Some(*pid), quantize: if st.transport().playing { quant } else { 0.0 } }));
                    }
                    if r.secondary_clicked() {
                        st.audio.send(Cmd::Live(LiveEvent::Launch { lane: *tid, pattern: None, quantize: quant }));
                    }
                }
                ui.end_row();
            }
        });
    });
    if !m.launched.is_empty() || !m.queued.is_empty() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
    }
}
