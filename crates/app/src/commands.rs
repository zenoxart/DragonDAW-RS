//! Every Dragon DAW action as a registered command: the main menu, shortcuts and the command palette are
//! generated from this table.

use crate::state::State;
use dragon_engine::device::Command as Cmd;
use dragon_engine::model::*;
use redhive_client_framework::commands::CommandHandler;
use redhive_contracts::commands::{placements, CommandDescriptor, CommandGroupDescriptor, CommandPlacement, KeyGesture, ShortcutBinding, ShortcutScope};
use redhive_contracts::ui::Severity;
use redhive_contracts::{IconRef, TextKey};

/// (id, label, menu path, section, shortcut)
/// Shortcuts follow FL Studio's defaults: Ctrl+N saves a new version, Ctrl+R / Ctrl+Shift+R export WAV /
/// MP3, Ctrl+Z undoes (pressed again it redoes, like FL), Ctrl+Alt+Z steps further back, Ctrl+B duplicates,
/// Ctrl+D deselects, Ctrl+Q quick-quantizes, Ctrl+L makes legato, Alt+R randomizes, Alt+S strums, Alt+T adds
/// a time marker, L switches pattern / song, Ctrl+M the metronome, Ctrl+T the typing keyboard, F4 the
/// next empty pattern, F5–F11 the windows, Alt+F8 the browser, F12 closes the floating windows, and the
/// editor tools are P (draw), B (paint), D (delete), T (mute), C (slice) and E (select).
const TABLE: &[(&str, &str, &str, i32, &str)] = &[
    ("file.new", "New project", "File", 0, ""),
    ("file.open", "Open…", "File", 0, "Ctrl+O"),
    ("file.save", "Save", "File", 1, "Ctrl+S"),
    ("file.save_as", "Save as…", "File", 1, "Ctrl+Shift+S"),
    ("file.save_new_version", "Save new version", "File", 1, "Ctrl+N"),
    ("file.collect", "Collect all files into project folder", "File", 1, ""),
    ("file.import_audio", "Import audio…", "File/Import", 2, ""),
    ("file.import_midi", "Import MIDI file…", "File/Import", 2, ""),
    ("file.import_video", "Import video…", "File/Import", 2, ""),
    ("file.export", "Export WAV / mix…", "File/Export", 3, "Ctrl+R"),
    ("file.export_mp3", "Export MP3…", "File/Export", 3, "Ctrl+Shift+R"),
    ("file.export_stems", "Export stems…", "File/Export", 3, ""),
    ("file.export_midi", "Export pattern as MIDI…", "File/Export", 3, "Ctrl+Shift+M"),
    ("file.recover", "Recover autosave…", "File", 4, ""),
    ("file.quit", "Exit", "File", 5, "Alt+F4"),
    ("edit.undo_toggle", "Undo / redo last", "Edit", 0, "Ctrl+Z"),
    ("edit.undo", "Undo (step back)", "Edit", 0, "Ctrl+Alt+Z"),
    ("edit.redo", "Redo (step forward)", "Edit", 0, "Ctrl+Shift+Z"),
    ("edit.cut", "Cut", "Edit", 1, "Ctrl+X"),
    ("edit.copy", "Copy", "Edit", 1, "Ctrl+C"),
    ("edit.paste", "Paste", "Edit", 1, "Ctrl+V"),
    ("edit.delete", "Delete selection", "Edit", 1, "Delete"),
    ("edit.duplicate", "Duplicate selection", "Edit", 1, "Ctrl+B"),
    ("edit.select_all", "Select all", "Edit", 1, "Ctrl+A"),
    ("edit.deselect", "Deselect all", "Edit", 1, "Ctrl+D"),
    ("edit.split", "Split clips at playhead", "Edit", 2, ""),
    ("edit.quantize", "Quick quantize", "Edit/Notes", 3, "Ctrl+Q"),
    ("edit.legato", "Legato", "Edit/Notes", 3, "Ctrl+L"),
    ("edit.humanize", "Randomize (humanize)", "Edit/Notes", 3, "Alt+R"),
    ("edit.strum", "Strum chords", "Edit/Notes", 3, "Alt+S"),
    ("edit.transpose_up", "Transpose +1 octave", "Edit/Notes", 4, "Ctrl+Up"),
    ("edit.transpose_down", "Transpose −1 octave", "Edit/Notes", 4, "Ctrl+Down"),
    ("edit.capture_midi", "Dump score log (retro record)", "Edit/Notes", 5, ""),
    ("tool.draw", "Draw tool", "Edit/Tools", 6, "P"),
    ("tool.paint", "Paint tool", "Edit/Tools", 6, "B"),
    ("tool.delete", "Delete tool", "Edit/Tools", 6, "D"),
    ("tool.mute", "Mute tool", "Edit/Tools", 6, "T"),
    ("tool.slice", "Slice tool", "Edit/Tools", 6, "C"),
    ("tool.select", "Select tool", "Edit/Tools", 6, "E"),
    ("transport.play", "Play / stop", "Transport", 0, "Space"),
    ("transport.stop", "Stop", "Transport", 0, ""),
    ("transport.record", "Record", "Transport", 0, "R"),
    ("transport.home", "Go to start", "Transport", 1, "Home"),
    ("transport.mode", "Toggle pattern / song mode", "Transport", 1, "L"),
    ("transport.loop", "Toggle loop", "Transport", 2, ""),
    ("transport.punch", "Toggle punch in/out", "Transport", 2, ""),
    ("transport.metronome", "Toggle metronome", "Transport", 3, "Ctrl+M"),
    ("transport.tap", "Tap tempo", "Transport", 3, ""),
    ("transport.typing_keyboard", "Typing keyboard to piano", "Transport", 4, "Ctrl+T"),
    ("add.track", "Audio track", "Add", 0, ""),
    ("add.pattern", "Next empty pattern", "Add", 0, "F4"),
    ("add.sampler", "Sampler channel", "Add/Instrument", 1, ""),
    ("add.synth", "Synth channel", "Add/Instrument", 1, ""),
    ("add.drums", "Drum pads channel", "Add/Instrument", 1, ""),
    ("add.mixer", "Mixer bus", "Add", 2, ""),
    ("add.vca", "VCA fader", "Add", 2, ""),
    ("add.marker", "Time marker", "Add", 3, "Alt+T"),
    ("view.playlist", "Playlist", "View", 0, "F5"),
    ("view.rack", "Channel rack", "View", 0, "F6"),
    ("view.piano", "Piano roll", "View", 0, "F7"),
    ("view.mixer", "Mixer", "View", 0, "F9"),
    ("view.launcher", "Performance (clip launcher)", "View", 0, ""),
    ("view.browser", "Browser", "View", 1, "Alt+F8"),
    ("view.master", "Mastering & loudness", "View", 1, ""),
    ("view.video", "Video", "View", 1, ""),
    ("view.project", "Project info & tempo", "View", 1, "F11"),
    ("view.automation", "Automation & MIDI mappings", "View", 1, ""),
    ("view.history", "Undo history", "View", 1, "Alt+Z"),
    ("view.plugins", "Plugin manager", "View", 2, ""),
    ("view.fx_gallery", "Plugin picker (effect gallery)", "View", 2, "F8"),
    ("view.options", "Options", "View", 2, "F10"),
    ("view.close_windows", "Close all floating windows", "View", 2, "F12"),
    ("view.palette", "Command palette", "View", 3, "Ctrl+Shift+P"),
    ("view.reset_layout", "Reset layout", "View", 3, ""),
    ("tools.scan_plugins", "Scan plugins", "Tools", 0, ""),
    ("tools.detect", "Detect tempo & key of selected clip", "Tools", 1, ""),
    ("tools.normalize", "Normalize selected clips", "Tools", 1, ""),
    ("tools.bounce", "Bounce selected track to audio", "Tools", 1, ""),
    ("tools.slice", "Slice selected sample to rack", "Tools", 1, ""),
    ("tools.to_midi_melody", "Convert selected clip to MIDI (melody)", "Tools", 2, ""),
    ("tools.to_midi_drums", "Convert selected clip to MIDI (drums)", "Tools", 2, ""),
    ("tools.separate", "Separate selected clip into stems (drums / harmonic / centre)", "Tools", 2, ""),
    ("help.about", "About Dragon DAW", "Help", 0, ""),
];

pub fn register(st: &mut State) {
    for (i, (id, label, path, section, keys)) in TABLE.iter().enumerate() {
        let shortcuts = KeyGesture::parse(keys).map(|g| vec![ShortcutBinding { gesture: g, scope: ShortcutScope::Global }]).unwrap_or_default();
        let menu_path = path.to_ascii_lowercase();
        let d = CommandDescriptor {
            id: format!("dragon.{id}"),
            label: TextKey::new(*label),
            icon: IconRef::new(""),
            permission: String::new(),
            placements: vec![CommandPlacement::new(placements::MAIN_MENU, &menu_path, *section, i as i32)],
            shortcuts,
            workflow_capable: false,
            ai_allowed: false,
            module_id: "dragon".into(),
        };
        // The palette is the shell's own reserved gesture (Ctrl+Shift+P).
        let owner = if *id == "view.palette" { "shell" } else { "dragon" };
        st.ctx.commands.register(owner, d, CommandHandler::App);
    }
    let groups = [
        ("file", "File"),
        ("edit", "Edit"),
        ("add", "Add"),
        ("transport", "Transport"),
        ("view", "View"),
        ("tools", "Tools"),
        ("help", "Help"),
        ("file/import", "Import"),
        ("file/export", "Export"),
        ("edit/notes", "Notes"),
        ("edit/tools", "Tools"),
        ("add/instrument", "Instrument"),
    ];
    for (i, (path, label)) in groups.iter().enumerate() {
        st.ctx.commands.register_group("dragon", CommandGroupDescriptor { menu: placements::MAIN_MENU.into(), path: (*path).into(), label: TextKey::new(*label), icon: IconRef::new(""), order: i as i32 });
    }
    st.ctx.shortcuts.rebuild(&st.ctx.commands);
}

pub fn view_of(id: &str) -> Option<&'static str> {
    Some(match id {
        "dragon.view.playlist" => "dragon.playlist",
        "dragon.view.rack" => "dragon.rack",
        "dragon.view.piano" => "dragon.piano",
        "dragon.view.mixer" => "dragon.mixer",
        "dragon.view.launcher" => "dragon.launcher",
        "dragon.view.browser" => "dragon.browser",
        "dragon.view.master" => "dragon.master",
        "dragon.view.video" => "dragon.video",
        "dragon.view.project" => "dragon.project",
        "dragon.view.automation" => "dragon.automation",
        "dragon.view.history" => "dragon.history",
        "dragon.view.plugins" => "dragon.plugins",
        "dragon.view.options" => "dragon.options",
        _ => return None,
    })
}

/// Executes one command. Returns `false` for unknown ids.
pub fn execute(st: &mut State, id: &str, ctx: &egui::Context) -> bool {
    if let Some(v) = view_of(id) {
        st.ctx.open(v, None);
        return true;
    }
    let id = id.strip_prefix("dragon.").unwrap_or(id);
    match id {
        "file.new" => st.new_project(),
        "file.open" => st.open_dialog(),
        "file.save" => {
            st.save(false);
        }
        "file.save_as" => {
            st.save(true);
        }
        "file.collect" => {
            if st.session.path.is_none() && !st.save(true) {
                return true;
            }
            let path = st.session.path.clone().unwrap();
            let mut p = st.session.project.clone();
            match dragon_engine::project_io::collect_files(&mut p, &path) {
                Ok(n) => {
                    st.session.edit("Collect files", |x| *x = p);
                    st.save(false);
                    st.toast(format!("Copied {n} file(s) into the project folder"), Severity::Success);
                }
                Err(e) => st.toast(format!("Collect failed: {e}"), Severity::Error),
            }
        }
        "file.import_audio" => {
            if let Some(files) = rfd::FileDialog::new().add_filter("Audio", &["wav", "flac", "mp3", "ogg", "aif", "aiff", "m4a"]).pick_files() {
                let beat = st.beat_now();
                for f in files {
                    st.add_audio_clip(&f, None, beat);
                }
            }
        }
        "file.import_midi" => {
            if let Some(f) = rfd::FileDialog::new().add_filter("MIDI", &["mid", "midi"]).pick_file() {
                st.import_midi(&f);
            }
        }
        "file.import_video" => {
            if let Some(f) = rfd::FileDialog::new().add_filter("Video", &["mp4", "mov", "mkv", "avi", "webm", "m4v"]).pick_file() {
                st.import_video(&f);
            }
        }
        "file.export" => st.ui.export = Some(crate::views::export::ExportDialog::new(&st.session.project, false, st.session.path.as_deref())),
        "file.save_new_version" => {
            st.save_new_version();
        }
        "file.export_mp3" => {
            let mut d = crate::views::export::ExportDialog::new(&st.session.project, false, st.session.path.as_deref());
            d.set_mp3();
            st.ui.export = Some(d);
        }
        "file.export_stems" => st.ui.export = Some(crate::views::export::ExportDialog::new(&st.session.project, true, st.session.path.as_deref())),
        "file.export_midi" => {
            let pat = st.ui.pattern;
            if let Some(pt) = st.session.project.pattern(pat).cloned() {
                if let Some(f) = rfd::FileDialog::new().add_filter("MIDI", &["mid"]).set_file_name(format!("{}.mid", pt.name)).save_file() {
                    let p = &st.session.project;
                    let names = |id: u64| p.channel(id).map(|c| c.name.clone()).unwrap_or_default();
                    match dragon_engine::midi::export_pattern(&f, &pt, &names, p.bpm) {
                        Ok(()) => st.ui.status = format!("Exported {}", f.display()),
                        Err(e) => st.toast(format!("MIDI export failed: {e}"), Severity::Error),
                    }
                }
            }
        }
        "file.recover" => st.ui.recovery = Some(st.autosave.list()),
        "file.quit" => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        "edit.undo_toggle" => {
            // FL: Ctrl+Z undoes the last action; pressed again (with nothing done in between) it redoes it.
            let rev = st.session.history.revision;
            if st.ui.last_undo == Some(rev) {
                if let Some(l) = st.session.redo() {
                    st.ui.status = format!("Redo: {l}");
                }
                st.ui.last_undo = None;
            } else if let Some(l) = st.session.undo() {
                st.ui.status = format!("Undo: {l}");
                st.ui.last_undo = Some(st.session.history.revision);
            }
        }
        "edit.cut" => {
            copy_selection(st);
            st.ctx.run("dragon.edit.delete", serde_json::Value::Null);
        }
        "edit.copy" => copy_selection(st),
        "edit.paste" => paste(st),
        "edit.deselect" => {
            st.ui.clips.clear();
            st.ui.notes.clear();
        }
        "tool.draw" => st.ui.tool = crate::state::Tool::Draw,
        "tool.paint" => st.ui.tool = crate::state::Tool::Paint,
        "tool.delete" => st.ui.tool = crate::state::Tool::Erase,
        "tool.mute" => st.ui.tool = crate::state::Tool::Mute,
        "tool.slice" => st.ui.tool = crate::state::Tool::Slice,
        "tool.select" => st.ui.tool = crate::state::Tool::Select,
        "view.close_windows" => {
            st.ui.effect_windows.clear();
            st.ui.instrument_windows.clear();
            st.ui.clip_window = None;
            st.ui.fx_gallery = false;
            st.quests.window_open = false;
        }
        "edit.undo" => {
            if let Some(l) = st.session.undo() {
                st.ui.status = format!("Undo: {l}");
            }
        }
        "edit.redo" => {
            if let Some(l) = st.session.redo() {
                st.ui.status = format!("Redo: {l}");
            }
        }
        "edit.delete" => {
            if !st.ui.clips.is_empty() {
                let ids = std::mem::take(&mut st.ui.clips);
                st.session.edit("Delete clips", |p| dragon_engine::edit::delete_clips(p, &ids));
            } else if !st.ui.notes.is_empty() {
                let (pat, ch) = (st.ui.pattern, st.ui.piano_channel);
                let mut idx = std::mem::take(&mut st.ui.notes);
                idx.sort_unstable();
                st.session.edit("Delete notes", |p| {
                    if let Some(n) = ch.and_then(|c| p.pattern_mut(pat).and_then(|pt| pt.channels.get_mut(&c))) {
                        for i in idx.into_iter().rev() {
                            if i < n.notes.len() {
                                n.notes.remove(i);
                            }
                        }
                    }
                });
            }
        }
        "edit.duplicate" => {
            if !st.ui.clips.is_empty() {
                let ids = st.ui.clips.clone();
                st.ui.clips = st.session.edit("Duplicate clips", |p| dragon_engine::edit::duplicate_clips(p, &ids));
            } else {
                notes_edit(st, "Duplicate notes", |notes, sel| {
                    let (start, end) = sel.iter().fold((u32::MAX, 0), |(a, b), i| (a.min(notes[*i].start), b.max(notes[*i].end())));
                    let len = end.saturating_sub(start);
                    let copies: Vec<Note> = sel.iter().map(|i| Note { start: notes[*i].start + len, ..notes[*i] }).collect();
                    let first = notes.len();
                    notes.extend(copies);
                    (first..notes.len()).collect()
                });
            }
        }
        "edit.select_all" if st.ui.editor == crate::state::Editor::Playlist => {
            st.ui.clips = st.session.project.tracks.iter().flat_map(|t| t.clips.iter().map(|c| c.id)).collect();
        }
        "edit.select_all" => {
            let n = st.ui.piano_channel.and_then(|c| st.session.project.pattern(st.ui.pattern).and_then(|p| p.channels.get(&c))).map(|x| x.notes.len()).unwrap_or(0);
            st.ui.notes = (0..n).collect();
        }
        "edit.split" => {
            let beat = st.beat_now();
            let targets: Vec<(Id, Id)> =
                st.session.project.tracks.iter().flat_map(|t| t.clips.iter().filter(|c| c.start < beat && c.end() > beat && (st.ui.clips.is_empty() || st.ui.clips.contains(&c.id))).map(move |c| (t.id, c.id))).collect();
            st.session.edit("Split clips", |p| {
                for (t, c) in targets {
                    dragon_engine::edit::split_clip(p, t, c, beat);
                }
            });
        }
        "edit.quantize" => {
            let grid = (PPQ as f64 * st.ui.snap).max(1.0) as u32;
            notes_edit(st, "Quantize", |n, sel| {
                let mut v: Vec<Note> = sel.iter().map(|i| n[*i]).collect();
                dragon_engine::edit::quantize(&mut v, grid, 1.0, false);
                for (k, i) in sel.iter().enumerate() {
                    n[*i] = v[k];
                }
                sel.to_vec()
            });
        }
        "edit.legato" => notes_apply(st, "Legato", dragon_engine::edit::legato),
        "edit.humanize" => notes_apply(st, "Humanize", |v| dragon_engine::edit::humanize(v, 6, 0.1, 7)),
        "edit.strum" => notes_apply(st, "Strum", |v| dragon_engine::edit::strum(v, 8)),
        "edit.transpose_up" => notes_apply(st, "Transpose", |v| dragon_engine::edit::transpose(v, 12)),
        "edit.transpose_down" => notes_apply(st, "Transpose", |v| dragon_engine::edit::transpose(v, -12)),
        "edit.capture_midi" => {
            let notes = st.retro.capture(st.session.project.bpm);
            match (notes.is_empty(), st.ui.piano_channel.or(st.ui.channel)) {
                (false, Some(ch)) => {
                    let pat = st.ui.pattern;
                    let n = notes.len();
                    st.session.edit("Capture MIDI", |p| {
                        if let Some(pt) = p.pattern_mut(pat) {
                            pt.channels.entry(ch).or_default().notes.extend(notes);
                        }
                    });
                    st.toast(format!("Captured {n} notes"), Severity::Success);
                }
                _ => st.toast("Nothing played recently", Severity::Info),
            }
        }
        "transport.play" => st.toggle_play(),
        "transport.stop" => st.stop(),
        "transport.record" => {
            if st.transport().recording || st.audio.is_recording() {
                st.stop();
            } else {
                st.start_recording();
            }
        }
        "transport.home" => st.seek(0.0),
        "transport.mode" => {
            st.ui.pattern_mode = !st.ui.pattern_mode;
            if st.transport().playing {
                st.audio.send(Cmd::Pause);
                st.audio.send(Cmd::Play(st.play_mode()));
            }
        }
        "transport.loop" => st.session.edit("Loop", |p| {
            if p.loop_range.is_none() {
                p.loop_range = Some(Range::new(0.0, p.beats_per_bar(0.0) * 4.0));
            }
            p.loop_enabled = !p.loop_enabled;
        }),
        "transport.punch" => st.session.edit("Punch", |p| {
            if p.punch.is_none() {
                p.punch = Some(Range::new(p.beats_per_bar(0.0), p.beats_per_bar(0.0) * 3.0));
            }
            p.punch_enabled = !p.punch_enabled;
        }),
        "transport.metronome" => st.session.edit("Metronome", |p| p.metronome.enabled = !p.metronome.enabled),
        "transport.tap" => {
            let now = std::time::Instant::now();
            st.ui.tap_times.retain(|t| now.duration_since(*t).as_secs_f64() < 2.5);
            st.ui.tap_times.push(now);
            if st.ui.tap_times.len() >= 3 {
                let n = st.ui.tap_times.len();
                let span = st.ui.tap_times[n - 1].duration_since(st.ui.tap_times[0]).as_secs_f64();
                let bpm = ((n - 1) as f64 * 60.0 / span).clamp(20.0, 400.0);
                st.session.edit("Tap tempo", |p| p.bpm = (bpm * 10.0).round() / 10.0);
            }
        }
        "transport.typing_keyboard" => {
            st.ui.typing_keyboard = !st.ui.typing_keyboard;
            st.ui.status = format!("Typing keyboard {}", if st.ui.typing_keyboard { "on (Z–M, Q–U; , . octave)" } else { "off" });
        }
        "add.track" => {
            let n = st.session.project.tracks.len() + 1;
            st.session.edit("Add track", |p| p.add_track(&format!("Track {n}")));
        }
        "add.pattern" => {
            st.ui.pattern = st.session.edit("Add pattern", |p| p.add_pattern());
        }
        "add.sampler" => {
            if let Some(f) = rfd::FileDialog::new().add_filter("Audio", &["wav", "flac", "mp3", "ogg", "aif", "aiff"]).pick_file() {
                st.add_sampler(&f);
            }
        }
        "add.synth" => st.ui.channel = Some(st.session.edit("Add synth", |p| p.add_channel("Synth", Instrument::Synth(SynthParams::default())))),
        "add.drums" => {
            let pads = crate::state::kit_pads(&st.kit);
            let id = st.session.edit("Add drums", |p| p.add_channel("Drums", Instrument::DrumPads(pads)));
            st.ui.channel = Some(id);
            st.ui.piano_channel = Some(id);
            st.ui.instrument_windows.push(id);
        }
        "add.mixer" => {
            st.ui.mixer = st.session.edit("Add bus", |p| {
                let n = p.mixer.channels.len() + 1;
                p.add_mixer_channel(&format!("Bus {n}"))
            })
        }
        "add.vca" => {
            st.ui.mixer = st.session.edit("Add VCA", |p| {
                let id = p.add_mixer_channel("VCA");
                if let Some(m) = p.mixer.get_mut(id) {
                    m.kind = ChannelKind::Vca;
                }
                id
            })
        }
        "add.marker" => {
            let beat = st.beat_now();
            st.session.edit("Add marker", |p| {
                let id = p.next_id();
                let n = p.markers.len() + 1;
                p.markers.push(Marker { id, beat, name: format!("Marker {n}") });
                p.markers.sort_by(|a, b| a.beat.total_cmp(&b.beat));
            });
        }
        "view.palette" => st.ui.palette = Some(String::new()),
        "view.fx_gallery" => st.ui.fx_gallery = true,
        "view.reset_layout" => st.ctx.run("dragon.internal.reset_layout", serde_json::Value::Null),
        "tools.scan_plugins" => crate::views::plugins::start_scan(st),
        "tools.detect" => {
            if let Some(path) = selected_audio(st) {
                match st.session.cache.get(&path, st.audio.rate()) {
                    Ok(d) => {
                        let a = dragon_engine::analysis::analyze(&d);
                        st.toast(
                            format!(
                                "{}: {} BPM, key {}",
                                path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default(),
                                a.bpm.map(|b| format!("{b:.1}")).unwrap_or("?".into()),
                                a.key.unwrap_or("?".into())
                            ),
                            Severity::Info,
                        );
                    }
                    Err(e) => st.toast(e.to_string(), Severity::Error),
                }
            } else {
                st.toast("Select an audio clip first", Severity::Info);
            }
        }
        "tools.normalize" => {
            let ids = st.ui.clips.clone();
            let rate = st.audio.rate();
            let peaks: Vec<(Id, f32)> = st
                .session
                .project
                .tracks
                .iter()
                .flat_map(|t| t.clips.iter())
                .filter(|c| ids.contains(&c.id))
                .filter_map(|c| c.audio().and_then(|a| st.session.cache.get(&a.source, rate).ok()).map(|d| (c.id, d.peak())))
                .collect();
            st.session.edit("Normalize", |p| {
                for t in p.tracks.iter_mut() {
                    for c in t.clips.iter_mut() {
                        if let Some((_, pk)) = peaks.iter().find(|(id, _)| *id == c.id) {
                            dragon_engine::edit::normalize_clip(c, *pk, -0.3);
                        }
                    }
                }
            });
        }
        "tools.bounce" => {
            let track = st.session.project.tracks.iter().find(|t| t.clips.iter().any(|c| st.ui.clips.contains(&c.id))).map(|t| (t.id, t.name.clone()));
            let Some((tid, name)) = track else {
                st.toast("Select a clip on the track to bounce", Severity::Info);
                return true;
            };
            let dir = st.recordings_dir();
            let path = dir.join(format!("{} bounce.wav", crate::state::sanitize(&name)));
            let pool = dragon_engine::render::Pool::with_plugins(Box::new(dragon_engine::plugins::Host));
            match dragon_engine::export::bounce_track_with(&st.session.project, &st.session.cache, tid, &path, st.audio.rate(), pool) {
                Ok(start) => {
                    st.session.edit("Bounce track", |p| {
                        let new = p.add_track(&format!("{name} (bounced)"));
                        if let Some(t) = p.track_mut(tid) {
                            t.muted = true;
                        }
                        let _ = new;
                    });
                    let new = st.session.project.tracks.last().map(|t| t.id);
                    st.add_audio_clip(&path, new, start);
                    st.toast("Bounced to a new audio track (original muted)", Severity::Success);
                }
                Err(e) => st.toast(format!("Bounce failed: {e}"), Severity::Error),
            }
        }
        "tools.slice" => {
            let path = selected_audio(st).or_else(|| match st.ui.channel.and_then(|c| st.session.project.channel(c)).map(|c| &c.instrument) {
                Some(Instrument::Sampler(s)) => Some(s.path.clone()),
                _ => None,
            });
            match path.map(|p| (st.session.cache.get(&p, st.audio.rate()), p)) {
                Some((Ok(d), p)) => {
                    let (ch, pat) = st.session.edit("Slice to rack", |x| dragon_engine::edit::slice_to_rack(x, &p, &d, 0.5));
                    st.ui.channel = Some(ch);
                    st.ui.piano_channel = Some(ch);
                    st.ui.pattern = pat;
                    st.ctx.open("dragon.piano", None);
                }
                Some((Err(e), _)) => st.toast(e.to_string(), Severity::Error),
                None => st.toast("Select an audio clip or a sampler channel", Severity::Info),
            }
        }
        "tools.to_midi_melody" | "tools.to_midi_drums" => {
            let drums = id == "tools.to_midi_drums";
            let clip = st.session.project.tracks.iter().flat_map(|t| t.clips.iter()).find(|c| st.ui.clips.contains(&c.id) && c.audio().is_some()).cloned();
            let Some(clip) = clip else {
                st.toast("Select an audio clip first", Severity::Info);
                return true;
            };
            let a = clip.audio().cloned().unwrap();
            match st.session.cache.get(&a.source, st.audio.rate()) {
                Ok(d) => {
                    // Only the part of the file the clip plays.
                    let rate = d.rate as f64;
                    let start = (a.offset * rate) as usize;
                    let tm = dragon_engine::time::TempoMap::from_project(&st.session.project);
                    let secs = tm.beat_to_seconds(clip.end()) - tm.beat_to_seconds(clip.start);
                    let end = (start + (secs * rate * a.speed) as usize).min(d.frames());
                    let part = dragon_engine::samples::AudioData { rate: d.rate, l: d.l[start.min(end)..end].to_vec(), r: d.r[start.min(end)..end].to_vec(), ..Default::default() };
                    let bpm = st.session.project.bpm;
                    let notes = dragon_engine::analysis::audio_to_notes(&part, bpm, if drums { dragon_engine::analysis::TranscribeMode::Drums } else { dragon_engine::analysis::TranscribeMode::Melody });
                    if notes.is_empty() {
                        st.toast("No notes found in the clip", Severity::Info);
                        return true;
                    }
                    let n = notes.len();
                    let pads = crate::state::kit_pads(&st.kit);
                    let name = format!("{} (MIDI)", clip.name);
                    let (ch, pat) = st.session.edit("Audio to MIDI", |p| {
                        let inst = if drums { Instrument::DrumPads(pads) } else { Instrument::Synth(SynthParams::default()) };
                        let ch = p.add_channel(&name, inst);
                        let pat = p.add_pattern();
                        if let Some(pt) = p.pattern_mut(pat) {
                            pt.name = name.clone();
                            pt.channels.entry(ch).or_default().notes = notes;
                        }
                        let len = p.pattern(pat).map(|x| x.length_beats()).unwrap_or(4.0);
                        let track = p.add_track(&name);
                        let id = p.next_id();
                        let color = p.pattern(pat).map(|x| x.color).unwrap_or_default();
                        if let Some(t) = p.track_mut(track) {
                            t.clips.push(Clip { id, name: name.clone(), start: clip.start, length: len, muted: false, color, kind: ClipKind::Pattern { pattern: pat } });
                        }
                        (ch, pat)
                    });
                    st.ui.pattern = pat;
                    st.ui.channel = Some(ch);
                    st.ui.piano_channel = Some(ch);
                    st.ctx.open("dragon.piano", None);
                    st.toast(format!("Transcribed {n} notes into a new pattern"), Severity::Success);
                }
                Err(e) => st.toast(e.to_string(), Severity::Error),
            }
        }
        "tools.separate" => st.separate_selected(),
        "help.about" => st.ui.about = true,
        _ => return false,
    }
    true
}

/// Copies the selected clips (playlist) or notes (piano roll) to the clipboard.
fn copy_selection(st: &mut State) {
    if st.ui.editor == crate::state::Editor::Playlist && !st.ui.clips.is_empty() {
        let p = &st.session.project;
        let clips: Vec<(usize, Clip)> = p.tracks.iter().enumerate().flat_map(|(ti, t)| t.clips.iter().filter(|c| st.ui.clips.contains(&c.id)).map(move |c| (ti, c.clone()))).collect();
        st.ui.status = format!("Copied {} clip(s)", clips.len());
        st.ui.clipboard = Some(crate::state::Clipboard::Clips(clips));
    } else if let Some(ch) = st.ui.piano_channel {
        let notes: Vec<Note> = st.session.project.pattern(st.ui.pattern).and_then(|p| p.channels.get(&ch)).map(|c| st.ui.notes.iter().filter_map(|i| c.notes.get(*i).copied()).collect()).unwrap_or_default();
        if !notes.is_empty() {
            st.ui.status = format!("Copied {} note(s)", notes.len());
            st.ui.clipboard = Some(crate::state::Clipboard::Notes(notes));
        }
    }
}

/// Pastes: clips at the playhead on their tracks (keeping their spacing), notes at their positions in the
/// current pattern. The pasted items become the selection.
fn paste(st: &mut State) {
    match st.ui.clipboard.clone() {
        Some(crate::state::Clipboard::Clips(clips)) if !clips.is_empty() => {
            let beat = st.beat_now().max(0.0);
            let first = clips.iter().map(|c| c.1.start).fold(f64::MAX, f64::min);
            st.ui.clips = st.session.edit("Paste clips", |p| {
                let mut out = Vec::new();
                for (ti, mut c) in clips {
                    let ti = ti.min(p.tracks.len().saturating_sub(1));
                    c.id = p.next_id();
                    c.start = beat + (c.start - first);
                    out.push(c.id);
                    if let Some(t) = p.tracks.get_mut(ti) {
                        t.clips.push(c);
                    }
                }
                out
            });
        }
        Some(crate::state::Clipboard::Notes(notes)) => {
            let (pat, Some(ch)) = (st.ui.pattern, st.ui.piano_channel) else { return };
            st.ui.notes = st.session.edit("Paste notes", |p| {
                let Some(c) = p.pattern_mut(pat).map(|pt| pt.channels.entry(ch).or_default()) else { return Vec::new() };
                let first = c.notes.len();
                c.notes.extend(notes);
                (first..c.notes.len()).collect()
            });
        }
        _ => {}
    }
}

fn selected_audio(st: &State) -> Option<std::path::PathBuf> {
    st.session.project.tracks.iter().flat_map(|t| t.clips.iter()).find(|c| st.ui.clips.contains(&c.id)).and_then(|c| c.audio()).map(|a| a.source.clone())
}

/// Applies a note operation to the selected notes (or all notes of the piano-roll channel).
fn notes_edit(st: &mut State, label: &str, f: impl FnOnce(&mut Vec<Note>, &[usize]) -> Vec<usize>) {
    let (pat, Some(ch)) = (st.ui.pattern, st.ui.piano_channel) else { return };
    let sel = st.ui.notes.clone();
    let new_sel = st.session.edit(label, |p| {
        let n = p.pattern_mut(pat).map(|pt| &mut pt.channels.entry(ch).or_default().notes)?;
        let sel: Vec<usize> = if sel.is_empty() { (0..n.len()).collect() } else { sel.into_iter().filter(|i| *i < n.len()).collect() };
        Some(f(n, &sel))
    });
    if let Some(s) = new_sel {
        st.ui.notes = s;
    }
}

fn notes_apply(st: &mut State, label: &str, f: impl FnOnce(&mut [Note])) {
    notes_edit(st, label, |n, sel| {
        let mut v: Vec<Note> = sel.iter().map(|i| n[*i]).collect();
        f(&mut v);
        for (k, i) in sel.iter().enumerate() {
            n[*i] = v[k];
        }
        sel.to_vec()
    });
}
