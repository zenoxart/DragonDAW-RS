//! Options: audio driver / device / sample rate / buffer / record offset, MIDI inputs, general settings and
//! keyboard shortcuts.

use crate::audio::Audio;
use crate::state::State;
use crate::theme;
use dragon_engine::device::{self, MidiInputs};
use egui::{RichText, Ui};

pub fn show(ui: &mut Ui, st: &mut State) {
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.heading("Audio");
        ui.label(RichText::new(st.audio.describe()).color(theme::TEXT_DIM));
        let mut cfg = st.settings.device.clone();
        egui::Grid::new("audio.grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Driver");
            egui::ComboBox::from_id_salt("audio.host").selected_text(cfg.host.clone().unwrap_or("default".into())).show_ui(ui, |ui| {
                ui.selectable_value(&mut cfg.host, None, "default");
                for h in device::hosts() {
                    ui.selectable_value(&mut cfg.host, Some(h.clone()), h);
                }
            });
            ui.end_row();
            ui.label("Output");
            egui::ComboBox::from_id_salt("audio.out").selected_text(cfg.output.clone().unwrap_or("default".into())).width(260.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut cfg.output, None, "default");
                for d in device::output_devices(cfg.host.as_deref()) {
                    ui.selectable_value(&mut cfg.output, Some(d.clone()), d);
                }
            });
            ui.end_row();
            ui.label("Input (recording)");
            egui::ComboBox::from_id_salt("audio.in").selected_text(cfg.input.clone().unwrap_or("default".into())).width(260.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut cfg.input, None, "default");
                for d in device::input_devices(cfg.host.as_deref()) {
                    ui.selectable_value(&mut cfg.input, Some(d.clone()), d);
                }
            });
            ui.end_row();
            ui.label("Sample rate");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut cfg.sample_rate, None, "device");
                for r in [44100u32, 48000, 88200, 96000] {
                    ui.selectable_value(&mut cfg.sample_rate, Some(r), format!("{}", r));
                }
            });
            ui.end_row();
            ui.label("Buffer");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut cfg.buffer, None, "driver");
                for b in [64u32, 128, 256, 512, 1024, 2048] {
                    ui.selectable_value(&mut cfg.buffer, Some(b), b.to_string());
                }
            });
            ui.end_row();
            ui.label("Recording offset");
            ui.add(egui::Slider::new(&mut cfg.record_offset_ms, -50.0..=50.0).suffix(" ms")).on_hover_text("Fine-tunes latency compensation of recordings (positive = earlier)");
            ui.end_row();
        });
        let changed = cfg != st.settings.device;
        st.settings.device = cfg;
        if ui.add_enabled(changed, egui::Button::new("Apply audio settings")).clicked() {
            restart_audio(st);
        }
        if ui.button("Restart audio engine").clicked() {
            restart_audio(st);
        }

        ui.separator();
        ui.heading("MIDI inputs");
        let ports = device::midi_inputs();
        if ports.is_empty() {
            ui.label(RichText::new("No MIDI inputs found.").color(theme::TEXT_DIM));
        }
        let mut sel = st.settings.device.midi_inputs.clone();
        ui.label(RichText::new("None selected = all inputs are opened.").small().color(theme::TEXT_DIM));
        for p in &ports {
            let mut on = sel.contains(p);
            if ui.checkbox(&mut on, p).changed() {
                if on {
                    sel.push(p.clone());
                } else {
                    sel.retain(|x| x != p);
                }
            }
        }
        if sel != st.settings.device.midi_inputs || ui.button("Reconnect MIDI").clicked() {
            st.settings.device.midi_inputs = sel;
            st.midi = None;
            st.midi = Some(MidiInputs::open(&st.settings.device.midi_inputs, Some(st.audio.shared_transport())));
            st.save_settings();
        }

        ui.separator();
        ui.heading("General");
        let s = &mut st.settings;
        ui.add(egui::Slider::new(&mut s.autosave_minutes, 0..=30).text("autosave every (min, 0 = off)"));
        ui.add(egui::Slider::new(&mut s.undo_limit, 20..=2000).text("undo steps"));
        if ui.add(egui::Slider::new(&mut s.ui_scale, 0.7..=2.0).text("interface scale")).drag_stopped() {
            ui.ctx().set_zoom_factor(s.ui_scale);
        }
        ui.checkbox(&mut s.show_splash, "Show animated splash screen at start");
        ui.checkbox(&mut s.reduced_motion, "Reduced motion");
        ui.checkbox(&mut s.follow_playhead, "Playlist follows the playhead");
        st.session.history.limit = st.settings.undo_limit;
        if ui.button("Save settings").clicked() {
            st.save_settings();
        }

        ui.separator();
        ui.heading("Keyboard shortcuts");
        ui.label(RichText::new("Type a gesture such as Ctrl+Shift+K and press Enter; leave empty to remove. Chords: “Ctrl+K, S”.").small().color(theme::TEXT_DIM));
        let cmds: Vec<(String, String)> = st.ctx.commands.all().map(|c| (c.descriptor.id.clone(), c.descriptor.label.0.clone())).collect();
        egui::Grid::new("shortcuts.grid").striped(true).num_columns(3).show(ui, |ui| {
            for (id, label) in cmds {
                ui.label(label);
                let cur = st.ctx.shortcuts.text(&id, false).unwrap_or_default();
                let key = egui::Id::new(("sc", &id));
                let mut text: String = ui.data_mut(|d| d.get_temp(key).unwrap_or(cur.clone()));
                let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(140.0));
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && text != cur {
                    st.ctx.shortcuts.set_override(&id, if text.trim().is_empty() { None } else { Some(text.trim()) });
                    st.ctx.shortcuts.rebuild(&st.ctx.commands);
                    st.ctx.shortcuts.save();
                }
                ui.data_mut(|d| d.insert_temp(key, text));
                if ui.small_button("reset").clicked() {
                    st.ctx.shortcuts.reset(Some(&id));
                    st.ctx.shortcuts.rebuild(&st.ctx.commands);
                    st.ctx.shortcuts.save();
                    ui.data_mut(|d| d.remove::<String>(key));
                }
                ui.end_row();
            }
        });
    });
}

fn restart_audio(st: &mut State) {
    st.stop();
    st.save_settings();
    // Drop the old engine before opening the device again.
    let song = st.session.compile(48000.0);
    st.audio = Audio::Null(crate::audio::NullEngine::start(song.clone()), "restarting".into());
    st.audio = Audio::start(&st.settings.device, song);
    st.session.force_recompile();
    st.midi = Some(MidiInputs::open(&st.settings.device.midi_inputs, Some(st.audio.shared_transport())));
    st.ui.status = st.audio.describe();
}
