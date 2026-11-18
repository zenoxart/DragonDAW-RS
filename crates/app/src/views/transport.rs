//! Transport bar (play / stop / record, song–pattern mode, tempo, time display, pattern picker, master
//! meter, CPU) and the status bar.

use super::widgets;
use crate::state::State;
use crate::theme;
use dragon_engine::time::{bbt, clock};
use egui::{Color32, RichText, Ui, Vec2};

pub fn show(ui: &mut Ui, st: &mut State) {
    let t = st.audio.transport();
    let ts = t.state;
    let beat = t.beat_now();
    let rec = ts.recording || st.audio.is_recording() || st.ui.record_started.is_some();
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        // Song / pattern mode.
        let pat_mode = st.ui.pattern_mode;
        if widgets::toggle(ui, pat_mode, "PAT", theme::ACCENT_2).on_hover_text("Pattern mode (L)").clicked() && !pat_mode {
            st.ctx.run("dragon.transport.mode", serde_json::Value::Null);
        }
        if widgets::toggle(ui, !pat_mode, "SONG", theme::PLAYHEAD).on_hover_text("Song mode (L)").clicked() && pat_mode {
            st.ctx.run("dragon.transport.mode", serde_json::Value::Null);
        }
        ui.separator();
        let big = |s: &str| RichText::new(s).size(18.0);
        let play = ui.add(egui::Button::new(big(if ts.playing { "⏸" } else { "▶" }).color(if ts.playing { theme::OK } else { Color32::WHITE })).min_size(Vec2::new(36.0, 30.0)));
        crate::tutorial::anchor(ui.ctx(), "transport.play", play.rect);
        if play.on_hover_text("Play / pause (Space)").clicked() {
            st.toggle_play();
        }
        if ui.add(egui::Button::new(big("⏹")).min_size(Vec2::new(36.0, 30.0))).on_hover_text("Stop (Ctrl+Space)").clicked() {
            st.stop();
        }
        if ui
            .add(egui::Button::new(big("⏺").color(if rec { theme::REC } else { theme::TEXT_DIM })).min_size(Vec2::new(36.0, 30.0)))
            .on_hover_text("Record (R): armed tracks record audio, otherwise notes are recorded into the pattern")
            .clicked()
        {
            st.ctx.run("dragon.transport.record", serde_json::Value::Null);
        }
        ui.separator();

        // Time display.
        let p = &st.session.project;
        let bpb = p.beats_per_bar(beat.max(0.0));
        let time = if beat < 0.0 { format!("count-in {:.0}", (-beat).ceil()) } else { bbt(beat, bpb) };
        egui::Frame::NONE.fill(theme::BG).corner_radius(4.0).inner_margin(egui::Margin::symmetric(8, 2)).show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(time).monospace().size(17.0).color(theme::PLAYHEAD));
                ui.label(RichText::new(clock(ts.seconds.max(0.0))).monospace().small().color(theme::TEXT_DIM));
            });
        });

        // Tempo.
        let mut bpm = st.session.project.bpm;
        let r = widgets::bpm_display(ui, &mut bpm);
        crate::tutorial::anchor(ui.ctx(), "transport.tempo", r.rect);
        widgets::apply(st, &r, "Tempo", |p| p.bpm = bpm);
        ui.label(RichText::new("BPM").small().color(theme::TEXT_DIM));
        if ui.small_button("TAP").on_hover_text("Tap tempo (T)").clicked() {
            st.ctx.run("dragon.transport.tap", serde_json::Value::Null);
        }
        let (num, den) = st.session.project.signature_at(0.0);
        ui.label(RichText::new(format!("{num}/{den}")).color(theme::TEXT_DIM));
        ui.separator();

        // Toggles.
        let p = &st.session.project;
        let (met, lp, punch) = (p.metronome.enabled, p.loop_enabled, p.punch_enabled);
        let count_in = p.metronome.count_in_bars;
        if widgets::toggle(ui, met, "♩ CLICK", theme::ACCENT_2).on_hover_text("Metronome (M)").clicked() {
            st.ctx.run("dragon.transport.metronome", serde_json::Value::Null);
        }
        if widgets::toggle(ui, count_in > 0, "COUNT", theme::ACCENT_2).on_hover_text("Count-in before recording (1 bar)").clicked() {
            st.session.edit("Count-in", |p| p.metronome.count_in_bars = if count_in > 0 { 0 } else { 1 });
        }
        if widgets::toggle(ui, lp, "⟲ LOOP", theme::PLAYHEAD).on_hover_text("Loop (Ctrl+L)").clicked() {
            st.ctx.run("dragon.transport.loop", serde_json::Value::Null);
        }
        if widgets::toggle(ui, punch, "PUNCH", theme::REC).on_hover_text("Punch in/out range").clicked() {
            st.ctx.run("dragon.transport.punch", serde_json::Value::Null);
        }
        if widgets::toggle(ui, st.ui.typing_keyboard, "⌨", theme::OK).on_hover_text("Typing keyboard to piano (Ctrl+T)").clicked() {
            st.ctx.run("dragon.transport.typing_keyboard", serde_json::Value::Null);
        }
        ui.separator();

        // Pattern picker.
        let cur = st.session.project.pattern(st.ui.pattern).map(|p| p.name.clone()).unwrap_or_default();
        egui::ComboBox::from_id_salt("dragon.pattern.pick").selected_text(cur).width(110.0).show_ui(ui, |ui| {
            for p in &st.session.project.patterns {
                ui.selectable_value(&mut st.ui.pattern, p.id, &p.name);
            }
        });
        if ui.small_button("+").on_hover_text("New pattern").clicked() {
            st.ctx.run("dragon.add.pattern", serde_json::Value::Null);
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            super::quests::badge(ui, st);
            ui.add_space(8.0);
            let m = st.audio.meters().snapshot();
            ui.label(RichText::new(format!("CPU {:>3.0}%  ·  {} voices", m.cpu_load * 100.0, m.voices)).small().color(if m.cpu_load > 0.8 { theme::REC } else { theme::TEXT_DIM }));
            let (l, r) = m.strips.get(&st.session.project.mixer.master.id).copied().unwrap_or((0.0, 0.0));
            ui.add_space(4.0);
            stereo_bar(ui, l, r);
            ui.label(RichText::new(format!("{:.1} LUFS-S", m.short_term)).small().monospace());
        });
    });
}

fn stereo_bar(ui: &mut Ui, l: f32, r: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(120.0, 14.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 2.0, theme::BG);
    for (i, v) in [l, r].into_iter().enumerate() {
        let db = dragon_engine::dsp::gain_to_db(v.max(1e-6));
        let n = ((db + 60.0) / 66.0).clamp(0.0, 1.0);
        let y = rect.top() + 1.0 + i as f32 * 6.5;
        p.rect_filled(egui::Rect::from_min_size(egui::pos2(rect.left(), y), Vec2::new(rect.width() * n, 5.5)), 1.0, theme::meter_color(v));
    }
}

pub fn status_bar(ui: &mut Ui, st: &mut State) {
    ui.horizontal_centered(|ui| {
        ui.label(RichText::new(st.audio.describe()).small().color(theme::TEXT_DIM));
        ui.separator();
        if let Some(m) = &st.midi {
            ui.label(RichText::new(if m.names.is_empty() { "No MIDI input".to_string() } else { format!("MIDI: {}", m.names.join(", ")) }).small().color(theme::TEXT_DIM));
            ui.separator();
        }
        if !st.session.warnings.is_empty() {
            ui.label(RichText::new(format!("⚠ {}", st.session.warnings[0])).small().color(theme::WARN)).on_hover_text(st.session.warnings.join("\n"));
            ui.separator();
        }
        if let Some(t) = &st.ui.midi_learn {
            ui.label(RichText::new(format!("MIDI learn: move a controller for {}", crate::state::target_name(&st.session.project, t))).small().color(theme::ACCENT_2));
            if ui.small_button("cancel").clicked() {
                st.ui.midi_learn = None;
            }
            ui.separator();
        }
        ui.label(RichText::new(&st.ui.status).small());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(l) = st.session.history.can_redo() {
                ui.label(RichText::new(format!("redo: {l}")).small().color(theme::TEXT_DIM));
            }
            if let Some(l) = st.session.history.can_undo() {
                ui.label(RichText::new(format!("undo: {l}")).small().color(theme::TEXT_DIM));
            }
        });
    });
}
