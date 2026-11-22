//! Recording from the mixer: recording channels (an armed playlist track that records into a mixer
//! channel), their input (which inputs of the audio interface), where they record into, and how the
//! input is heard while recording (monitoring).

use super::widgets;
use crate::state::State;
use crate::theme;
use dragon_engine::model::{Id, InputSource, MixerChannel, MonitorMode, Project};
use egui::{Color32, RichText, Ui, Vec2};
use redhive_contracts::ui::Severity;

/// "In 1", "In 1+2".
pub fn input_label(src: InputSource) -> String {
    match src.right {
        Some(r) => format!("In {}+{}", src.left + 1, r + 1),
        None => format!("In {}", src.left + 1),
    }
}

/// What the input is for, in plain words.
fn input_hint(src: InputSource) -> &'static str {
    if src.right.is_some() {
        "stereo: a keyboard, a synth, a stereo mic pair"
    } else {
        "mono: one microphone, a guitar, a bass"
    }
}

/// The inputs to choose from: every channel on its own (mono) and the stereo pairs 1+2, 3+4 …
/// (an interface with no channels known offers 2).
pub fn input_options(channels: u16) -> Vec<InputSource> {
    let n = if channels == 0 { 2 } else { channels };
    let mut v: Vec<InputSource> = (0..n).map(|k| InputSource { left: k, right: None }).collect();
    v.extend((0..n / 2).map(|k| InputSource { left: 2 * k, right: Some(2 * k + 1) }));
    v
}

/// The playlist tracks that record into mixer channel `mixer`.
pub fn tracks_into(p: &Project, mixer: Id) -> Vec<Id> {
    p.tracks.iter().filter(|t| t.mixer == mixer && t.input.is_some()).map(|t| t.id).collect()
}

/// A track shown as a recording track: armed, or made as one ("Recording 2", "Vocals rec").
fn is_recording_track(t: &dragon_engine::model::Track) -> bool {
    t.armed || t.audio || t.name.starts_with("Recording") || t.name.ends_with(" rec")
}

fn channels(st: &State) -> u16 {
    st.audio.input().map(|(_, n)| n).unwrap_or(0)
}

/// An input not used by another armed track yet (mono 1, 2 … when the interface has several, else 1+2).
fn free_input(st: &State) -> InputSource {
    let used: Vec<InputSource> = st.session.project.tracks.iter().filter(|t| t.armed).filter_map(|t| t.input).collect();
    let n = channels(st);
    if n > 2 {
        if let Some(s) = (0..n).map(|k| InputSource { left: k, right: None }).find(|s| !used.iter().any(|u| u.left == s.left || u.right == Some(s.left))) {
            return s;
        }
    }
    InputSource { left: 0, right: if n == 1 { None } else { Some(1) } }
}

/// Adds a recording channel: a new mixer channel with an armed track recording into it, and selects it.
pub fn add_recording_channel(st: &mut State) -> Id {
    let input = free_input(st);
    let (track, mixer) = st.session.edit("Add recording channel", |p| {
        let n = p.tracks.iter().filter(|t| t.name.starts_with("Recording")).count() + 1;
        p.add_recording_track(&format!("Recording {n}"), input, None)
    });
    st.ui.mixer = mixer;
    st.ui.mixer_sel = vec![mixer];
    st.ui.track = Some(track);
    if !st.audio.can_record() {
        st.toast("Recording channel added – open an input device in Options › Audio to record", Severity::Warning);
    } else {
        st.toast(format!("Recording channel added on {} – press R to record", input_label(input)), Severity::Success);
    }
    mixer
}

/// Adds a recording track that records into the existing mixer channel `mixer`.
pub fn record_into(st: &mut State, mixer: Id) -> Id {
    let input = free_input(st);
    let name = st.session.project.mixer.get(mixer).map(|c| format!("{} rec", c.name)).unwrap_or_else(|| "Recording".into());
    let (track, _) = st.session.edit("Record into channel", |p| p.add_recording_track(&name, input, Some(mixer)));
    st.ui.track = Some(track);
    track
}

fn set_input(st: &mut State, tid: Id, src: InputSource) {
    st.session.edit("Recording input", |p| p.track_mut(tid).iter_mut().for_each(|t| t.input = Some(src)));
}

/// The input picker of track `tid`.
pub fn input_combo(ui: &mut Ui, st: &mut State, tid: Id, cur: InputSource, width: f32, small: bool) {
    let n = channels(st);
    let text = RichText::new(input_label(cur));
    let text = if small { text.small() } else { text };
    egui::ComboBox::from_id_salt(("rec.input", tid, small))
        .selected_text(text)
        .width(width)
        .show_ui(ui, |ui| {
            let opts = input_options(n);
            ui.label(RichText::new("Mono").small().color(theme::TEXT_DIM));
            for s in opts.iter().filter(|s| s.right.is_none()) {
                if ui.selectable_label(*s == cur, input_label(*s)).on_hover_text(input_hint(*s)).clicked() {
                    set_input(st, tid, *s);
                }
            }
            ui.label(RichText::new("Stereo").small().color(theme::TEXT_DIM));
            for s in opts.iter().filter(|s| s.right.is_some()) {
                if ui.selectable_label(*s == cur, input_label(*s)).on_hover_text(input_hint(*s)).clicked() {
                    set_input(st, tid, *s);
                }
            }
        })
        .response
        .on_hover_text("Which input of your audio interface this records");
}

fn toggle_arm(st: &mut State, tid: Id) {
    st.session.edit("Arm track", |p| p.track_mut(tid).iter_mut().for_each(|t| t.armed = !t.armed));
}

/// The strip's recording row (arm + input) – an empty row of the same height on channels that do not
/// record, so the faders stay aligned.
pub fn strip_row(ui: &mut Ui, st: &mut State, m: &MixerChannel, width: f32) {
    let tracks = tracks_into(&st.session.project, m.id);
    let rec: Vec<(Id, bool, InputSource)> = tracks.iter().filter_map(|id| st.session.project.track(*id)).filter(|t| is_recording_track(t)).filter_map(|t| t.input.map(|i| (t.id, t.armed, i))).collect();
    let row_h = 20.0;
    let Some(&(tid, armed, input)) = rec.first() else {
        ui.allocate_exact_size(Vec2::new(width, row_h), egui::Sense::hover());
        return;
    };
    let r = ui
        .horizontal(|ui| {
            ui.set_height(row_h);
            ui.spacing_mut().item_spacing.x = 2.0;
            let b = egui::Button::new(RichText::new("⚫").color(if armed { Color32::WHITE } else { theme::REC })).fill(if armed { theme::REC } else { theme::PANEL_2 }).min_size(Vec2::new(22.0, 18.0));
            if ui.add(b).on_hover_text(if armed { "Armed: records when you press R. Click to disarm." } else { "Arm for recording" }).clicked() {
                toggle_arm(st, tid);
            }
            input_combo(ui, st, tid, input, width - 30.0, true);
        })
        .response
        .rect;
    crate::tutorial::anchor(ui.ctx(), "mixer.rec_strip", r);
}

/// The "RECORDING" part of the selected channel's panel.
pub fn panel(ui: &mut Ui, st: &mut State, m: &MixerChannel, is_master: bool) {
    if is_master {
        return;
    }
    let tracks = tracks_into(&st.session.project, m.id);
    let rec: Vec<_> = tracks
        .iter()
        .filter_map(|id| st.session.project.track(*id))
        .filter(|t| is_recording_track(t))
        .map(|t| (t.id, t.name.clone(), t.armed, t.input.unwrap_or(InputSource { left: 0, right: Some(1) }), t.monitor))
        .collect();
    ui.add_space(6.0);
    let w = ui.available_width() - 30.0;
    let frame = egui::Frame::NONE.fill(theme::REC.gamma_multiply(0.07)).stroke(egui::Stroke::new(1.0_f32, theme::REC.gamma_multiply(0.5))).corner_radius(6.0).inner_margin(egui::Margin::same(8));
    let r = frame
        .show(ui, |ui| {
            ui.set_width(w);
            ui.horizontal(|ui| {
                ui.label(RichText::new("RECORDING").small().strong().color(theme::REC));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("How to record").on_hover_text("A short guided tour: interface, input, level, monitoring, recording").clicked() {
                        crate::tutorial::start_recording(st);
                    }
                });
            });
            match st.audio.input() {
                Some((name, n)) => {
                    ui.label(RichText::new(format!("Interface: {name} · {n} inputs")).small().color(theme::TEXT_DIM));
                }
                None => {
                    let why = st.audio.input_error().unwrap_or_else(|| "no input device".into());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(format!("No input is open: {why}.")).small().color(theme::WARN));
                        if ui.small_button("Options › Audio").clicked() {
                            st.ctx.open("dragon.options", None);
                        }
                    });
                }
            }
            if rec.is_empty() {
                ui.label(RichText::new("This channel has nothing to record. Add a recording track and pick the input your mic or instrument is plugged into.").small().color(theme::TEXT_DIM));
            }
            for (tid, name, armed, input, monitor) in rec {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let b = egui::Button::new(RichText::new(if armed { "ARMED" } else { "arm" }).small().strong().color(if armed { Color32::WHITE } else { theme::REC }))
                        .fill(if armed { theme::REC } else { theme::PANEL_2 })
                        .min_size(Vec2::new(64.0, 22.0));
                    if ui.add(b).on_hover_text("Armed tracks record when you press R (or the record button)").clicked() {
                        toggle_arm(st, tid);
                    }
                    ui.label(RichText::new(name).strong());
                });
                egui::Grid::new(("rec.grid", tid)).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                    ui.label(RichText::new("Input").small().color(theme::TEXT_DIM));
                    let rr = ui.horizontal(|ui| input_combo(ui, st, tid, input, 90.0, false)).response.rect;
                    crate::tutorial::anchor(ui.ctx(), "mixer.rec_input", rr);
                    ui.end_row();
                    ui.label(RichText::new("Hear it").small().color(theme::TEXT_DIM));
                    let rr = ui
                        .horizontal(|ui| {
                            for (mode, label, tip) in [
                                (MonitorMode::Auto, "Auto", "Hear the input while the track is armed (through the channel's effects)"),
                                (MonitorMode::Input, "Always", "Always hear the input, armed or not"),
                                (MonitorMode::Off, "Off", "Don't play the input – use your interface's direct monitoring (no delay)"),
                            ] {
                                if widgets::toggle(ui, monitor == mode, label, theme::PLAYHEAD).on_hover_text(tip).clicked() && monitor != mode {
                                    st.session.edit("Monitoring", |p| p.track_mut(tid).iter_mut().for_each(|t| t.monitor = mode));
                                }
                            }
                        })
                        .response
                        .rect;
                    crate::tutorial::anchor(ui.ctx(), "mixer.rec_monitor", rr);
                    ui.end_row();
                    ui.label(RichText::new("Into").small().color(theme::TEXT_DIM));
                    let all: Vec<(Id, String)> = std::iter::once(&st.session.project.mixer.master)
                        .chain(st.session.project.mixer.channels.iter())
                        .filter(|c| c.kind != dragon_engine::model::ChannelKind::Vca)
                        .map(|c| (c.id, c.name.clone()))
                        .collect();
                    let cur = all.iter().find(|c| c.0 == m.id).map(|c| c.1.clone()).unwrap_or_default();
                    egui::ComboBox::from_id_salt(("rec.into", tid))
                        .selected_text(cur)
                        .width(120.0)
                        .show_ui(ui, |ui| {
                            for (cid, cname) in &all {
                                if ui.selectable_label(*cid == m.id, cname).clicked() && *cid != m.id {
                                    let cid = *cid;
                                    st.session.edit("Record into", |p| p.track_mut(tid).iter_mut().for_each(|t| t.mixer = cid));
                                    st.ui.mixer = cid;
                                }
                            }
                        })
                        .response
                        .on_hover_text("The mixer channel the recording plays through (its effects, fader and routing)");
                    ui.end_row();
                });
            }
            ui.add_space(4.0);
            if ui.button("+ Record into this channel").on_hover_text("Adds an armed recording track whose sound goes through this channel").clicked() {
                record_into(st, m.id);
            }
        })
        .response
        .rect;
    crate::tutorial::anchor(ui.ctx(), "mixer.rec_panel", r);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs_are_offered_mono_and_in_stereo_pairs() {
        let v = input_options(4);
        let names: Vec<String> = v.iter().map(|s| input_label(*s)).collect();
        assert_eq!(names, ["In 1", "In 2", "In 3", "In 4", "In 1+2", "In 3+4"]);
        assert_eq!(input_options(0).len(), 3, "no device: 1, 2 and 1+2");
    }

    #[test]
    fn a_recording_channel_is_an_armed_track_into_its_own_channel_or_an_existing_one() {
        let mut st = State::new(redhive_client_framework::AppContext::new("en"), crate::state::Settings { show_splash: false, ..Default::default() });
        st.new_project();
        let before = st.session.project.mixer.channels.len();
        let mixer = add_recording_channel(&mut st);
        let p = &st.session.project;
        assert_eq!(p.mixer.channels.len(), before + 1);
        let t = p.tracks.iter().find(|t| t.mixer == mixer).expect("a track records into it");
        assert!(t.armed && t.input.is_some() && t.monitor == MonitorMode::Auto);
        assert_eq!(st.ui.mixer, mixer, "the new channel is selected");
        // Recording into an existing channel adds no channel.
        let n = st.session.project.mixer.channels.len();
        let tid = record_into(&mut st, mixer);
        assert_eq!(st.session.project.mixer.channels.len(), n);
        assert_eq!(st.session.project.track(tid).unwrap().mixer, mixer);
        assert_eq!(tracks_into(&st.session.project, mixer).len(), 2);
        // One undo removes it again.
        st.session.undo();
        assert!(st.session.project.track(tid).is_none());
    }
}
