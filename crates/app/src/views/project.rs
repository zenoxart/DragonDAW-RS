//! Project info (metadata, tempo map, time signatures, markers, metronome, missing files) and the undo
//! history list.

use super::widgets;
use crate::state::State;
use crate::theme;
use dragon_engine::model::*;
use egui::{RichText, Ui};

pub fn show(ui: &mut Ui, st: &mut State) {
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.heading("Song");
        let meta = st.session.project.meta.clone();
        egui::Grid::new("meta").num_columns(2).show(ui, |ui| {
            for (label, val, k) in [("Title", meta.name.clone(), 0), ("Artist", meta.artist.clone(), 1), ("Genre", meta.genre.clone(), 2)] {
                ui.label(label);
                let mut v = val;
                let r = ui.text_edit_singleline(&mut v);
                widgets::apply(st, &r, "Song info", |p| match k {
                    0 => p.meta.name = v,
                    1 => p.meta.artist = v,
                    _ => p.meta.genre = v,
                });
                ui.end_row();
            }
        });
        let mut d = meta.description;
        let r = ui.add(egui::TextEdit::multiline(&mut d).hint_text("Notes").desired_rows(3));
        widgets::apply(st, &r, "Song notes", |p| p.meta.description = d);

        ui.separator();
        ui.heading("Tempo & meter");
        let mut bpm = st.session.project.bpm;
        let r = ui.add(egui::DragValue::new(&mut bpm).range(20.0..=400.0).speed(0.1).prefix("Base tempo ").suffix(" BPM"));
        widgets::apply(st, &r, "Tempo", |p| p.bpm = bpm);
        ui.label(RichText::new("Tempo changes").strong());
        let tps = st.session.project.tempo_points.clone();
        for (i, tp) in tps.iter().enumerate() {
            ui.horizontal(|ui| {
                let (mut b, mut t) = (tp.beat, tp.bpm);
                let r1 = ui.add(egui::DragValue::new(&mut b).range(0.0..=100000.0).prefix("beat "));
                let r2 = ui.add(egui::DragValue::new(&mut t).range(20.0..=400.0).speed(0.1).suffix(" BPM"));
                widgets::apply(st, &r1, "Tempo change", |p| p.tempo_points[i].beat = b);
                widgets::apply(st, &r2, "Tempo change", |p| p.tempo_points[i].bpm = t);
                if ui.small_button("🗙").clicked() {
                    st.session.edit("Remove tempo change", |p| {
                        p.tempo_points.remove(i);
                    });
                }
            });
        }
        if ui.small_button("+ tempo change at playhead").clicked() {
            let beat = st.beat_now().max(0.0).round();
            st.session.edit("Add tempo change", |p| {
                let bpm = p.tempo_at(beat);
                p.tempo_points.push(TempoPoint { beat, bpm });
                p.tempo_points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
            });
        }
        ui.label(RichText::new("Time signatures").strong());
        let sigs = st.session.project.time_signatures.clone();
        let (n0, d0) = st.session.project.signature_at(0.0);
        if sigs.is_empty() {
            ui.label(format!("{n0}/{d0} throughout"));
        }
        for (i, s) in sigs.iter().enumerate() {
            ui.horizontal(|ui| {
                let (mut b, mut n, mut d) = (s.beat, s.numerator, s.denominator);
                let a = ui.add(egui::DragValue::new(&mut b).prefix("beat "));
                let c = ui.add(egui::DragValue::new(&mut n).range(1..=32));
                ui.label("/");
                let e = ui.add(egui::DragValue::new(&mut d).range(1..=32));
                if a.changed() || c.changed() || e.changed() {
                    st.session.gesture("Time signature", |p| p.time_signatures[i] = TimeSignature { beat: b, numerator: n, denominator: d });
                }
                if ui.small_button("🗙").clicked() {
                    st.session.edit("Remove time signature", |p| {
                        p.time_signatures.remove(i);
                    });
                }
            });
        }
        if ui.small_button("+ time signature").clicked() {
            let beat = st.beat_now().max(0.0).round();
            st.session.edit("Add time signature", |p| {
                p.time_signatures.push(TimeSignature { beat, numerator: 3, denominator: 4 });
                p.time_signatures.sort_by(|a, b| a.beat.total_cmp(&b.beat));
            });
        }

        ui.separator();
        ui.heading("Surround");
        let cur = st.session.project.surround;
        ui.horizontal(|ui| {
            let mut f = cur;
            ui.selectable_value(&mut f, None, "Stereo");
            ui.selectable_value(&mut f, Some(SurroundFormat::S5_1), "5.1");
            ui.selectable_value(&mut f, Some(SurroundFormat::S7_1), "7.1");
            ui.selectable_value(&mut f, Some(SurroundFormat::S7_1_4), "7.1.4 (Atmos bed)");
            if f != cur {
                st.session.edit("Surround format", |p| p.surround = f);
            }
        });
        if cur.is_some() {
            ui.label(RichText::new("Each top-level mixer channel gets a surround panner on its strip. Monitoring stays stereo; File › Export writes the surround mix as a multichannel WAV, or the channels as Dolby Atmos objects in an ADM BWF file (master inserts are not applied to either).").small().color(theme::TEXT_DIM));
        }

        ui.separator();
        ui.heading("Metronome");
        let met = st.session.project.metronome;
        ui.horizontal(|ui| {
            let mut m = met;
            let mut changed = ui.checkbox(&mut m.enabled, "on").changed();
            changed |= ui.checkbox(&mut m.only_when_recording, "only while recording").changed();
            changed |= ui.add(egui::DragValue::new(&mut m.count_in_bars).range(0..=4).prefix("count-in bars ")).changed();
            let r = ui.add(egui::Slider::new(&mut m.volume, 0.0..=1.0).text("volume"));
            if changed {
                st.session.edit("Metronome", |p| p.metronome = m);
            }
            widgets::apply(st, &r, "Metronome volume", |p| p.metronome.volume = m.volume);
        });

        ui.separator();
        ui.heading("Markers");
        let markers = st.session.project.markers.clone();
        if markers.is_empty() {
            ui.label(RichText::new("Ctrl+K adds a marker at the playhead; double-click the ruler too.").small().color(theme::TEXT_DIM));
        }
        for m in &markers {
            let id = m.id;
            ui.horizontal(|ui| {
                if ui.small_button("▶").on_hover_text("Jump").clicked() {
                    st.seek(m.beat);
                }
                let mut name = m.name.clone();
                let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(140.0));
                widgets::apply(st, &r, "Rename marker", |p| p.markers.iter_mut().filter(|x| x.id == id).for_each(|x| x.name = name.clone()));
                ui.label(RichText::new(dragon_engine::time::bbt(m.beat, st.session.project.beats_per_bar(m.beat))).monospace());
                if ui.small_button("🗙").clicked() {
                    st.session.edit("Delete marker", |p| p.markers.retain(|x| x.id != id));
                }
            });
        }

        ui.separator();
        ui.heading("Files");
        let missing = dragon_engine::project_io::missing_files(&st.session.project);
        if missing.is_empty() {
            ui.label(RichText::new("All audio files are present.").color(theme::OK));
        } else {
            ui.colored_label(theme::WARN, format!("{} missing file(s)", missing.len()));
            for f in &missing {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(f.display().to_string()).small());
                    if ui.small_button("Locate…").clicked() {
                        if let Some(new) = rfd::FileDialog::new().set_file_name(f.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default()).pick_file() {
                            let old = f.clone();
                            let dir = new.parent().map(|d| d.to_path_buf());
                            st.session.edit("Relink file", |p| {
                                dragon_engine::project_io::rewrite_paths(p, |x| {
                                    if x == old {
                                        new.clone()
                                    } else if let (false, Some(d)) = (x.exists(), &dir) {
                                        // Also relink siblings found in the same new folder.
                                        let cand = d.join(x.file_name().unwrap_or_default());
                                        if cand.exists() { cand } else { x.to_path_buf() }
                                    } else {
                                        x.to_path_buf()
                                    }
                                })
                            });
                            st.session.cache.set_base(st.session.path.as_ref().and_then(|p| p.parent().map(|d| d.to_path_buf())));
                        }
                    }
                });
            }
        }
        if ui.button("Collect all files into the project folder").clicked() {
            st.ctx.run("dragon.file.collect", serde_json::Value::Null);
        }
    });
}

pub fn history(ui: &mut Ui, st: &mut State) {
    ui.horizontal(|ui| {
        if ui.add_enabled(st.session.history.can_undo().is_some(), egui::Button::new("↩ Undo")).on_hover_text("Ctrl+Z (again and again)").clicked() {
            st.session.undo();
        }
        if ui.add_enabled(st.session.history.can_redo().is_some(), egui::Button::new("↪ Redo")).on_hover_text("Ctrl+Y or Ctrl+Shift+Z").clicked() {
            st.session.redo();
        }
        ui.label(RichText::new(format!("{} / {} steps", st.session.history.undo_len(), dragon_engine::history::UNDO_STEPS)).small().color(theme::TEXT_DIM));
    });
    ui.separator();
    let labels = st.session.history.labels();
    let redo = st.session.history.redo_labels();
    egui::ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
        if ui.selectable_label(labels.is_empty(), RichText::new("(opened)").color(theme::TEXT_DIM)).on_hover_text("Click to undo everything back to here").clicked() {
            for _ in 0..labels.len() {
                st.session.undo();
            }
        }
        let n = labels.len();
        for (i, l) in labels.iter().enumerate() {
            if ui.selectable_label(i + 1 == n, format!("{}. {l}", i + 1)).on_hover_text("Click to go back to here").clicked() {
                for _ in (i + 1)..n {
                    st.session.undo();
                }
            }
        }
        // Undone steps (greyed): a click redoes up to there.
        for (k, l) in redo.iter().enumerate() {
            if ui.selectable_label(false, RichText::new(format!("{}. {l}", n + k + 1)).color(theme::TEXT_DIM).italics()).on_hover_text("Undone – click to redo up to here").clicked() {
                for _ in 0..=k {
                    st.session.redo();
                }
            }
        }
    });
}
