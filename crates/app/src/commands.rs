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
/// Ctrl+D deselects, Alt+Q quantizes and Shift+Q quick-quantizes, Space plays / stops, Ctrl+Space pauses,
/// Backspace toggles snap, Shift+arrows move the selection, Alt+M / Shift+M mute / unmute, Shift+I inverts,
/// Ctrl+G glues, Ctrl+U chops, Alt+F flams, Ctrl+H stops all sound, Ctrl+P toggles precount, Alt+P the picker,
/// Insert slices at the playhead, Ctrl+L makes legato, Alt+R randomizes, Alt+S strums, Alt+T adds
/// a time marker, L switches pattern / song, Ctrl+M the metronome, Ctrl+T the typing keyboard, F4 the
/// next empty pattern, F5–F11 the windows, Alt+F8 the browser, F12 closes the floating windows, and the
/// editor tools are P (draw), B (paint), D (delete), T (mute), C (slice) and E (select).
const TABLE: &[(&str, &str, &str, i32, &str)] = &[
    ("file.new", "New project", "File", 0, ""),
    ("file.new_demo", "New project with demo beat", "File", 0, ""),
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
    ("edit.split", "Slice clips at playhead", "Edit", 2, "Insert"),
    ("edit.quantize", "Quantize", "Edit/Notes", 3, "Alt+Q"),
    ("edit.quick_quantize", "Quick quantize (piano roll) / clip starts (playlist)", "Edit/Notes", 3, "Shift+Q"),
    ("edit.legato", "Legato", "Edit/Notes", 3, "Ctrl+L"),
    ("edit.humanize", "Quick randomize (humanize)", "Edit/Notes", 3, ""),
    ("edit.strum", "Quick strum", "Edit/Notes", 3, ""),
    ("edit.flam", "Flam", "Edit/Notes", 3, "Alt+F"),
    ("edit.glue", "Glue selected notes", "Edit/Notes", 3, "Ctrl+G"),
    ("edit.chop", "Quick chop (at the snap)", "Edit/Notes", 3, "Ctrl+U"),
    ("edit.mute_sel", "Mute selection", "Edit", 2, "Alt+M"),
    ("edit.unmute_sel", "Unmute selection", "Edit", 2, "Shift+M"),
    ("edit.invert_sel", "Invert selection", "Edit", 1, "Shift+I"),
    ("edit.move_left", "Move selection left", "Edit/Move", 7, "Shift+Left"),
    ("edit.move_right", "Move selection right", "Edit/Move", 7, "Shift+Right"),
    ("edit.move_up", "Move selection up", "Edit/Move", 7, "Shift+Up"),
    ("edit.move_down", "Move selection down", "Edit/Move", 7, "Shift+Down"),
    ("edit.snap_toggle", "Toggle snap", "Edit", 2, "Backspace"),
    ("edit.transpose_up", "Transpose +1 octave", "Edit/Notes", 4, "Ctrl+Up"),
    ("edit.transpose_down", "Transpose −1 octave", "Edit/Notes", 4, "Ctrl+Down"),
    ("edit.capture_midi", "Dump score log (retro record)", "Edit/Notes", 5, ""),
    ("tool.draw", "Draw tool", "Edit/Tools", 6, "P"),
    ("tool.paint", "Paint tool", "Edit/Tools", 6, "B"),
    ("tool.delete", "Delete tool", "Edit/Tools", 6, "D"),
    ("tool.mute", "Mute tool", "Edit/Tools", 6, "T"),
    ("tool.slice", "Slice tool", "Edit/Tools", 6, "C"),
    ("tool.select", "Select tool", "Edit/Tools", 6, "E"),
    ("edit.quick_quantize2", "Quick quantize (piano roll)", "", 0, "Ctrl+Alt+Q"),
    ("ctx.alt_c", "Clone channels (rack) / colour notes (piano roll)", "", 0, "Alt+C"),
    ("ctx.alt_g", "Group channels (rack) / ungroup (piano roll, playlist)", "", 0, "Alt+G"),
    ("ctx.alt_z", "Zip channels (rack) / undo history", "", 0, "Alt+Z"),
    ("ctx.alt_u", "Unzip channels (rack) / chop tool (piano roll)", "", 0, "Alt+U"),
    ("ctx.alt_up", "Move channels up (rack) / nudge notes up (piano roll) / move tracks up (playlist)", "", 0, "Alt+Up"),
    ("ctx.alt_down", "Move channels down (rack) / nudge notes down (piano roll) / move tracks down (playlist)", "", 0, "Alt+Down"),
    ("ctx.alt_left", "Nudge notes left (piano roll) / move mixer tracks left (mixer)", "", 0, "Alt+Left"),
    ("ctx.alt_right", "Nudge notes right (piano roll) / move mixer tracks right (mixer)", "", 0, "Alt+Right"),
    ("rack.delete", "Delete selected channels", "", 0, "Alt+Delete"),
    ("rack.up", "Select channel above", "", 0, "Up"),
    ("rack.down", "Select channel below", "", 0, "Down"),
    ("ctx.ctrl_enter", "Focus playlist (rack) / select time around the selection (playlist)", "", 0, "Ctrl+Enter"),
    ("pl.time_prev", "Select time before the selection", "", 0, "Ctrl+Left"),
    ("pl.time_next", "Select time after the selection", "", 0, "Ctrl+Right"),
    ("pr.ch_up", "Piano roll: channel above", "", 0, "G"),
    ("pr.ch_down", "Piano roll: channel below", "", 0, "K"),
    ("pr.ch_up_notes", "Piano roll: channel above with notes", "", 0, "H"),
    ("pr.ch_down_notes", "Piano roll: channel below with notes", "", 0, "J"),
    ("pr.next_notes", "Piano roll: next channel with notes", "", 0, "Shift+H"),
    ("pr.next_ch", "Piano roll: next channel", "", 0, "Shift+K"),
    ("pr.keyview", "Piano roll: keyboard / note names", "", 0, "M"),
    ("pr.porta", "Piano roll: portamento for new notes", "", 0, "O"),
    ("pr.prop_next", "Piano roll: next property in the event lane", "", 0, "F"),
    ("pr.prop_cycle", "Piano roll: cycle note properties", "", 0, "Shift+F"),
    ("pr.discard_len", "Piano roll: discard note lengths", "", 0, "Shift+D"),
    ("pr.chord_off", "Piano roll: turn off the chord stamp", "", 0, "Shift+N"),
    ("pr.sel_random", "Piano roll: select at random", "", 0, "Shift+R"),
    ("ctx.shift_c", "Select by colour (piano roll) / all clips of the source (playlist)", "", 0, "Shift+C"),
    ("ctx.shift_g", "Group selected notes / clips", "", 0, "Shift+G"),
    ("pr.helpers", "Piano roll: note helpers", "", 0, "Alt+B"),
    ("pr.ghosts", "Piano roll: ghost channels", "", 0, "Alt+V"),
    ("pr.wave_helper", "Piano roll: waveform helper", "", 0, "Alt+N"),
    ("pl.consolidate", "Consolidate selected clips", "Edit/Clips", 9, "Ctrl+Alt+C"),
    ("pl.time_sig", "Set a time signature at the playhead", "Edit/Clips", 9, "Shift+Alt+T"),
    ("pl.picker", "Project picker", "", 0, "Ctrl+F8"),
    ("edit.insert_ctrl_value", "Insert controller value (piano roll) / wait for input to record", "Edit", 2, "Ctrl+I"),
    ("pl.resize_left", "Allow resizing clips from the left", "", 0, "Ctrl+Alt+Home"),
    ("pl.marker_prev", "Jump to the previous marker", "", 0, "Alt+Slash"),
    ("pl.marker_next", "Jump to the next marker", "", 0, "Alt+Plus"),
    ("transport.next_bar", "Next bar (song mode)", "", 0, ""),
    ("transport.prev_bar", "Previous bar (song mode)", "", 0, ""),
    ("tool.slip", "Slip edit tool (playlist) / slide notes (piano roll)", "Edit/Tools", 6, "S"),
    ("tool.playback", "Playback tool", "Edit/Tools", 6, "Y"),
    ("tool.zoom", "Zoom tool", "Edit/Tools", 6, "Z"),
    ("ntool.arp", "Arpeggiator…", "Edit/Piano roll tools", 8, "Alt+A"),
    ("ntool.riff", "Riff machine…", "Edit/Piano roll tools", 8, "Alt+E"),
    ("ntool.limit", "Limit…", "Edit/Piano roll tools", 8, "Alt+K"),
    ("ntool.articulate", "Articulate…", "Edit/Piano roll tools", 8, "Alt+L"),
    ("ntool.lfo", "LFO…", "Edit/Piano roll tools", 8, "Alt+O"),
    ("ntool.randomize", "Randomize…", "Edit/Piano roll tools", 8, "Alt+R"),
    ("ntool.strum", "Strum…", "Edit/Piano roll tools", 8, "Alt+S"),
    ("ntool.chop", "Chop…", "Edit/Piano roll tools", 8, ""),
    ("ntool.claw", "Claw machine…", "Edit/Piano roll tools", 8, "Alt+W"),
    ("ntool.levels", "Scale levels…", "Edit/Piano roll tools", 8, "Alt+X"),
    ("ntool.flip", "Score flipper…", "Edit/Piano roll tools", 8, "Alt+Y"),
    ("edit.transpose_clip_down", "Transpose clips −1 semitone", "Edit/Clips", 9, ""),
    ("edit.transpose_clip_up", "Transpose clips +1 semitone", "Edit/Clips", 9, ""),
    ("edit.reverse_clips", "Reverse audio clips", "Edit/Clips", 9, ""),
    ("edit.insert_time", "Insert space (loop range)", "Edit/Clips", 9, "Ctrl+Insert"),
    ("edit.delete_time", "Delete space (loop range)", "Edit/Clips", 9, "Ctrl+Delete"),
    ("zoom.in", "Zoom in", "View/Zoom", 4, "PageUp"),
    ("zoom.out", "Zoom out", "View/Zoom", 4, "PageDown"),
    ("zoom.center", "Center on the playhead", "View/Zoom", 4, "Shift+0"),
    ("zoom.level1", "Zoom level 1", "View/Zoom", 4, "Shift+1"),
    ("zoom.level2", "Zoom level 2", "View/Zoom", 4, "Shift+2"),
    ("zoom.level3", "Zoom level 3", "View/Zoom", 4, "Shift+3"),
    ("zoom.all", "Show all", "View/Zoom", 4, "Shift+4"),
    ("zoom.selection", "Zoom to selection", "View/Zoom", 4, "Shift+5"),
    ("rack.mute1", "Mute / unmute channel 1", "", 0, "1"),
    ("rack.solo1", "Solo channel 1", "", 0, "Ctrl+1"),
    ("rack.mute2", "Mute / unmute channel 2", "", 0, "2"),
    ("rack.solo2", "Solo channel 2", "", 0, "Ctrl+2"),
    ("rack.mute3", "Mute / unmute channel 3", "", 0, "3"),
    ("rack.solo3", "Solo channel 3", "", 0, "Ctrl+3"),
    ("rack.mute4", "Mute / unmute channel 4", "", 0, "4"),
    ("rack.solo4", "Solo channel 4", "", 0, "Ctrl+4"),
    ("rack.mute5", "Mute / unmute channel 5", "", 0, "5"),
    ("rack.solo5", "Solo channel 5", "", 0, "Ctrl+5"),
    ("rack.mute6", "Mute / unmute channel 6", "", 0, "6"),
    ("rack.solo6", "Solo channel 6", "", 0, "Ctrl+6"),
    ("rack.mute7", "Mute / unmute channel 7", "", 0, "7"),
    ("rack.solo7", "Solo channel 7", "", 0, "Ctrl+7"),
    ("rack.mute8", "Mute / unmute channel 8", "", 0, "8"),
    ("rack.solo8", "Solo channel 8", "", 0, "Ctrl+8"),
    ("rack.mute9", "Mute / unmute channel 9", "", 0, "9"),
    ("rack.solo9", "Solo channel 9", "", 0, "Ctrl+9"),
    ("rack.mute0", "Mute / unmute channel 10", "", 0, "0"),
    ("rack.solo0", "Solo channel 10", "", 0, "Ctrl+0"),
    ("transport.play", "Play / stop", "Transport", 0, "Space"),
    ("transport.pause", "Play / pause", "Transport", 0, "Ctrl+Space"),
    ("transport.panic", "Stop sound (panic)", "Transport", 0, "Ctrl+H"),
    ("transport.stop", "Stop", "Transport", 0, ""),
    ("transport.record", "Record", "Transport", 0, "R"),
    ("transport.home", "Go to start", "Transport", 1, "Home"),
    ("transport.mode", "Toggle pattern / song mode", "Transport", 1, "L"),
    ("transport.loop", "Toggle loop", "Transport", 2, ""),
    ("transport.punch", "Toggle punch in/out", "Transport", 2, ""),
    ("transport.metronome", "Toggle metronome", "Transport", 3, "Ctrl+M"),
    ("transport.tap", "Tap tempo", "Transport", 3, ""),
    ("transport.precount", "Toggle recording precount", "Transport", 3, "Ctrl+P"),
    ("transport.typing_keyboard", "Typing keyboard to piano", "Transport", 4, "Ctrl+T"),
    ("add.track", "Audio track", "Add", 0, ""),
    ("add.pattern", "Next empty pattern", "Add", 0, "F4"),
    ("add.sampler", "Sampler channel", "Add/Instrument", 1, ""),
    ("add.synth", "Synth channel", "Add/Instrument", 1, ""),
    ("add.drums", "Drum pads channel", "Add/Instrument", 1, ""),
    ("add.mixer", "Mixer bus", "Add", 2, ""),
    ("add.vca", "VCA fader", "Add", 2, ""),
    ("mixer.autobalance", "Auto-balance all mixer channels", "Tools", 2, ""),
    ("add.marker", "Time marker", "Add", 3, "Alt+T"),
    ("view.playlist", "Playlist", "View", 0, "F5"),
    ("view.rack", "Channel rack", "View", 0, "F6"),
    ("view.piano", "Piano roll", "View", 0, "F7"),
    ("view.mixer", "Mixer", "View", 0, "F9"),
    ("view.launcher", "Performance (clip launcher)", "View", 0, ""),
    ("view.browser", "Browser", "View", 1, "Alt+F8"),
    ("view.picker", "Picker panel", "View", 1, "Alt+P"),
    ("view.master", "Mastering & loudness", "View", 1, ""),
    ("view.video", "Video", "View", 1, ""),
    ("view.project", "Project info & tempo", "View", 1, "F11"),
    ("view.automation", "Automation & MIDI mappings", "View", 1, ""),
    ("view.history", "Undo history", "View", 1, ""),
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
            // Rows without a menu path are key-only (the number keys).
            placements: if menu_path.is_empty() { Vec::new() } else { vec![CommandPlacement::new(placements::MAIN_MENU, &menu_path, *section, i as i32)] },
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
        ("edit/move", "Move"),
        ("edit/clips", "Clips"),
        ("edit/piano roll tools", "Piano roll tools"),
        ("view/zoom", "Zoom"),
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
/// The command for egui's clipboard events: Ctrl+X / Ctrl+C / Ctrl+V arrive as Cut / Copy / Paste
/// events instead of key presses.
pub fn clipboard_command(e: &egui::Event) -> Option<&'static str> {
    match e {
        egui::Event::Cut => Some("dragon.edit.cut"),
        egui::Event::Copy => Some("dragon.edit.copy"),
        egui::Event::Paste(_) => Some("dragon.edit.paste"),
        _ => None,
    }
}

pub fn execute(st: &mut State, id: &str, ctx: &egui::Context) -> bool {
    if let Some(v) = view_of(id) {
        st.ctx.open(v, None);
        return true;
    }
    let id = id.strip_prefix("dragon.").unwrap_or(id);
    use crate::state::Editor as Ed;
    match id {
        // ---------------- FL keys that depend on the editor under the mouse (rack / piano roll / playlist)
        "edit.legato" if st.ui.editor == Ed::Rack => rack_route_free(st),
        "edit.mute_sel" if st.ui.editor == Ed::Rack => st.ui.rack_mixer_sel = !st.ui.rack_mixer_sel,
        "edit.move_up" | "edit.move_down" if st.ui.editor == Ed::Rack => rack_extend(st, if id == "edit.move_up" { -1 } else { 1 }),
        "zoom.in" | "zoom.out" if st.ui.editor == Ed::Rack => rack_group_step(st, if id == "zoom.in" { -1 } else { 1 }),
        "transport.metronome" if st.ui.editor == Ed::PianoRoll => {
            if let Some(f) = rfd::FileDialog::new().add_filter("MIDI", &["mid", "midi"]).pick_file() {
                st.import_midi(&f);
            }
        }
        "transport.precount" if st.ui.editor == Ed::Playlist => st.ctx.open("dragon.launcher", None),
        "edit.quick_quantize2" => {
            let grid = (PPQ as f64 * st.ui.snap.max(0.25)) as u32;
            notes_apply(st, "Quick quantize", |v| dragon_engine::edit::quantize(v, grid.max(1), 1.0, false));
        }
        "ctx.alt_c" if st.ui.editor == Ed::Rack => rack_clone(st),
        "ctx.alt_c" => {
            // FL: change the colour of the selected notes (to the next colour group).
            notes_apply(st, "Note colour", |v| v.iter_mut().for_each(|n| n.color = (n.color + 1) % 16));
        }
        "ctx.alt_g" if st.ui.editor == Ed::Rack => {
            let sel = rack_selection(st);
            let name = format!("Group {}", st.session.project.channels.iter().filter(|c| !c.group.is_empty()).map(|c| c.group.clone()).collect::<std::collections::BTreeSet<_>>().len() + 1);
            st.session.edit("Group channels", |p| p.channels.iter_mut().filter(|c| sel.contains(&c.id)).for_each(|c| c.group = name.clone()));
            st.ui.status = format!("Grouped {} channels as \"{name}\" (PgUp / PgDn shows a group)", sel.len());
        }
        "ctx.alt_g" if st.ui.editor == Ed::Playlist => {
            let sel = st.ui.clips.clone();
            st.session.edit("Ungroup clips", |p| {
                for g in p.clip_groups.iter_mut() {
                    g.retain(|c| !sel.contains(c));
                }
                p.clip_groups.retain(|g| g.len() > 1);
            });
        }
        "ctx.alt_g" => notes_apply(st, "Ungroup notes", |v| v.iter_mut().for_each(|n| n.group = 0)),
        "ctx.shift_g" if st.ui.editor == Ed::Playlist => {
            let sel = st.ui.clips.clone();
            if sel.len() > 1 {
                st.session.edit("Group clips", |p| {
                    for g in p.clip_groups.iter_mut() {
                        g.retain(|c| !sel.contains(c));
                    }
                    p.clip_groups.retain(|g| g.len() > 1);
                    p.clip_groups.push(sel.clone());
                });
            }
        }
        "ctx.shift_g" => {
            let next = st.ui.piano_channel.and_then(|c| st.session.project.pattern(st.ui.pattern).and_then(|p| p.channels.get(&c))).map(|c| c.notes.iter().map(|n| n.group).max().unwrap_or(0) + 1).unwrap_or(1);
            notes_apply(st, "Group notes", |v| v.iter_mut().for_each(|n| n.group = next));
        }
        "ctx.alt_z" if st.ui.editor == Ed::Rack => rack_zip(st, true),
        "ctx.alt_z" => st.ctx.open("dragon.history", None),
        "ctx.alt_u" if st.ui.editor == Ed::Rack => rack_zip(st, false),
        "ctx.alt_u" => crate::views::note_tools::open(st, crate::views::note_tools::Tool::Chop),
        "ctx.alt_up" | "ctx.alt_down" if st.ui.editor == Ed::Rack => rack_move(st, if id == "ctx.alt_up" { -1 } else { 1 }),
        "ctx.alt_left" | "ctx.alt_right" if st.ui.editor == Ed::Mixer => {
            let ids = if st.ui.mixer_sel.contains(&st.ui.mixer) { st.ui.mixer_sel.clone() } else { vec![st.ui.mixer] };
            st.session.edit("Move mixer tracks", |p| p.move_mixer_channels(&ids, if id == "ctx.alt_left" { -1 } else { 1 }));
        }
        "ctx.alt_up" | "ctx.alt_down" if st.ui.editor == Ed::Playlist => {
            // The tracks holding the selected clips (or the track under the last click).
            let p = &st.session.project;
            let mut ids: Vec<Id> = p.tracks.iter().filter(|t| t.clips.iter().any(|c| st.ui.clips.contains(&c.id))).map(|t| t.id).collect();
            if ids.is_empty() {
                ids.extend(st.ui.track);
            }
            st.session.edit("Move tracks", |p| p.move_tracks(&ids, if id == "ctx.alt_up" { -1 } else { 1 }));
        }
        "ctx.alt_up" | "ctx.alt_down" | "ctx.alt_left" | "ctx.alt_right" if st.ui.editor == Ed::PianoRoll => {
            // FL Alt+arrows: fine nudge – a quarter of the snap in time, a semitone in pitch.
            let fine = ((PPQ as f64 * st.ui.snap.max(0.0625) / 4.0) as i64).max(1);
            notes_apply(st, "Nudge notes", |v| {
                for n in v.iter_mut() {
                    match id {
                        "ctx.alt_up" => n.pitch = (n.pitch + 1).min(127),
                        "ctx.alt_down" => n.pitch = n.pitch.saturating_sub(1),
                        "ctx.alt_left" => n.start = (n.start as i64 - fine).max(0) as u32,
                        _ => n.start += fine as u32,
                    }
                }
            });
        }
        "ctx.alt_up" | "ctx.alt_down" | "ctx.alt_left" | "ctx.alt_right" => {}
        "rack.delete" => {
            let sel = rack_selection(st);
            st.session.edit("Delete channels", |p| {
                p.channels.retain(|c| !sel.contains(&c.id));
                for pt in p.patterns.iter_mut() {
                    pt.channels.retain(|k, _| !sel.contains(k));
                }
            });
            st.ui.rack_sel.clear();
            if st.ui.channel.is_some_and(|c| sel.contains(&c)) {
                st.ui.channel = st.session.project.channels.first().map(|c| c.id);
                st.ui.piano_channel = st.ui.channel;
            }
        }
        "rack.up" | "rack.down" if st.ui.editor == Ed::Rack => {
            let chans = visible_channels(st);
            let cur = st.ui.channel.and_then(|c| chans.iter().position(|x| *x == c)).unwrap_or(0);
            let k = if id == "rack.up" { cur.saturating_sub(1) } else { (cur + 1).min(chans.len().saturating_sub(1)) };
            if let Some(c) = chans.get(k).copied() {
                st.ui.channel = Some(c);
                st.ui.piano_channel = Some(c);
                st.ui.rack_sel = vec![c];
            }
        }
        "rack.up" | "rack.down" => {}
        "ctx.ctrl_enter" if st.ui.editor == Ed::Rack => st.ctx.open("dragon.playlist", None),
        "ctx.ctrl_enter" => {
            let span = st
                .session
                .project
                .tracks
                .iter()
                .flat_map(|t| t.clips.iter())
                .filter(|c| st.ui.clips.contains(&c.id))
                .fold(None, |a: Option<(f64, f64)>, c| Some(a.map_or((c.start, c.end()), |(x, y)| (x.min(c.start), y.max(c.end())))));
            if let Some((a, b)) = span {
                st.session.edit("Select time", |p| p.loop_range = Some(Range::new(a, b)));
            }
        }
        "pl.time_prev" | "pl.time_next" => {
            if let Some(r) = st.session.project.loop_range.filter(|r| r.len() > 0.0) {
                let d = if id == "pl.time_prev" { -r.len() } else { r.len() };
                let s0 = (r.start + d).max(0.0);
                st.session.edit("Select time", |p| p.loop_range = Some(Range::new(s0, s0 + r.len())));
            }
        }
        "pr.ch_up" | "pr.ch_down" | "pr.ch_up_notes" | "pr.ch_down_notes" | "pr.next_notes" | "pr.next_ch" => {
            let up = matches!(id, "pr.ch_up" | "pr.ch_up_notes");
            let with_notes = matches!(id, "pr.ch_up_notes" | "pr.ch_down_notes" | "pr.next_notes");
            let pat = st.session.project.pattern(st.ui.pattern).cloned();
            let chans: Vec<Id> = st.session.project.channels.iter().map(|c| c.id).filter(|c| !with_notes || pat.as_ref().and_then(|p| p.channels.get(c)).is_some_and(|x| !x.notes.is_empty())).collect();
            if !chans.is_empty() {
                let all: Vec<Id> = st.session.project.channels.iter().map(|c| c.id).collect();
                let cur = st.ui.piano_channel.and_then(|c| all.iter().position(|x| *x == c)).unwrap_or(0);
                let pick = if up {
                    chans.iter().rev().find(|c| all.iter().position(|x| x == *c).unwrap_or(0) < cur).or(chans.last())
                } else {
                    chans.iter().find(|c| all.iter().position(|x| x == *c).unwrap_or(0) > cur).or(chans.first())
                };
                if let Some(c) = pick.copied() {
                    st.ui.piano_channel = Some(c);
                    st.ui.channel = Some(c);
                    st.ui.notes.clear();
                }
            }
        }
        "pr.keyview" => st.ui.pr_key_names = !st.ui.pr_key_names,
        "pr.porta" => {
            st.ui.porta = !st.ui.porta;
            st.ui.status = format!("Portamento for new notes {}", if st.ui.porta { "on" } else { "off" });
        }
        "pr.prop_next" | "pr.prop_cycle" => st.ui.pr_prop = (st.ui.pr_prop + 1) % 3,
        "pr.discard_len" => notes_apply(st, "Discard lengths", |v| v.iter_mut().for_each(|n| n.length = PPQ / 4)),
        "pr.chord_off" => st.ui.chord = 0,
        "pr.sel_random" => {
            let n = st.ui.piano_channel.and_then(|c| st.session.project.pattern(st.ui.pattern).and_then(|p| p.channels.get(&c))).map(|c| c.notes.len()).unwrap_or(0);
            let mut rng = dragon_engine::dsp::Rng::new(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1));
            st.ui.notes = (0..n).filter(|_| rng.next_f32() < 0.5).collect();
        }
        "ctx.shift_c" if st.ui.editor == Ed::Playlist => {
            // All clips that play the same source (pattern, audio file or automation lane).
            let key = |k: &ClipKind| match k {
                ClipKind::Audio(a) => format!("a{}", a.source.display()),
                ClipKind::Pattern { pattern } => format!("p{pattern}"),
                ClipKind::Automation { lane } => format!("l{lane}"),
            };
            let keys: Vec<String> = st.session.project.tracks.iter().flat_map(|t| t.clips.iter()).filter(|c| st.ui.clips.contains(&c.id)).map(|c| key(&c.kind)).collect();
            st.ui.clips = st.session.project.tracks.iter().flat_map(|t| t.clips.iter()).filter(|c| keys.contains(&key(&c.kind))).map(|c| c.id).collect();
        }
        "ctx.shift_c" => {
            let notes = st.ui.piano_channel.and_then(|c| st.session.project.pattern(st.ui.pattern).and_then(|p| p.channels.get(&c))).map(|c| c.notes.clone()).unwrap_or_default();
            let colors: Vec<u8> = st.ui.notes.iter().filter_map(|i| notes.get(*i)).map(|n| n.color).collect();
            st.ui.notes = notes.iter().enumerate().filter(|(_, n)| colors.contains(&n.color)).map(|(i, _)| i).collect();
        }
        "pr.helpers" => st.ui.helpers = !st.ui.helpers,
        "pr.ghosts" => st.ui.ghosts = !st.ui.ghosts,
        "pr.wave_helper" => st.ui.wave_helper = !st.ui.wave_helper,
        "pl.picker" => st.ui.pl_picker = !st.ui.pl_picker,
        "edit.insert_ctrl_value" if st.ui.editor != Ed::PianoRoll => {
            st.ui.wait_input = !st.ui.wait_input;
            st.ui.waiting_record = false;
            st.ui.status = format!("Wait for input to start recording {}", if st.ui.wait_input { "on" } else { "off" });
        }
        "edit.insert_ctrl_value" => match st.ui.last_target.clone() {
            Some(t) if crate::views::automation::insert_current_value(st, t.clone()) => st.ui.status = "Inserted the current value into automation".into(),
            _ => st.toast("Touch a parameter first (an effect knob or the XY pad)", Severity::Info),
        },
        "pl.resize_left" => {
            st.ui.resize_left = !st.ui.resize_left;
            st.ui.status = format!("Resizing from the left {}", if st.ui.resize_left { "on" } else { "off" });
        }
        "transport.next_bar" | "transport.prev_bar" => {
            let now = st.beat_now();
            let bar = st.session.project.beats_per_bar(now).max(1.0);
            let b = if id == "transport.next_bar" { ((now + 1e-6) / bar).floor() * bar + bar } else { (((now - 1e-6) / bar).ceil() * bar - bar).max(0.0) };
            st.seek(b);
        }
        "pl.marker_prev" | "pl.marker_next" => {
            let now = st.beat_now();
            let mut beats: Vec<f64> = st.session.project.markers.iter().map(|m| m.beat).collect();
            beats.sort_by(f64::total_cmp);
            let to = if id == "pl.marker_next" { beats.iter().find(|b| **b > now + 1e-6).copied() } else { beats.iter().rev().find(|b| **b < now - 1e-6).copied() };
            if let Some(b) = to {
                st.seek(b);
            }
        }
        "pl.time_sig" => {
            let beat = st.beat_now().floor();
            let cur = st.session.project.time_signatures.iter().rfind(|t| t.beat <= beat).copied();
            if let Some(mut ts) = cur {
                ts.beat = beat;
                st.session.edit("Time signature", |p| {
                    p.time_signatures.retain(|t| (t.beat - beat).abs() > 1e-9);
                    p.time_signatures.push(ts);
                    p.time_signatures.sort_by(|a, b| a.beat.total_cmp(&b.beat));
                });
            }
            st.ctx.open("dragon.project", None);
        }
        "pl.consolidate" => {
            let ids = st.ui.clips.clone();
            let Some(tid) = st.session.project.tracks.iter().find(|t| t.clips.iter().any(|c| ids.contains(&c.id))).map(|t| t.id) else {
                st.toast("Select the clips to consolidate", Severity::Info);
                return true;
            };
            let path = st.recordings_dir().join(format!("Consolidated {}.wav", st.session.project.next_id));
            let pool = dragon_engine::render::Pool::with_plugins(Box::new(dragon_engine::plugins::Host));
            match dragon_engine::export::consolidate_clips(&st.session.project, &st.session.cache, &ids, &path, st.audio.rate(), pool) {
                Ok(start) => {
                    st.session.edit("Consolidate clips", |p| dragon_engine::edit::delete_clips(p, &ids));
                    st.add_audio_clip(&path, Some(tid), start);
                    st.toast("Consolidated into one audio clip", Severity::Success);
                }
                Err(e) => st.toast(format!("Consolidate failed: {e}"), Severity::Error),
            }
        }
        "file.new" => st.new_project(),
        "file.new_demo" => st.new_demo_project(),
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
        // Cut: copy, then delete every selected element right away (clips in the playlist, notes in the
        // piano roll, the selected channels in the channel rack).
        "edit.cut" if st.ui.editor == Ed::Rack => {
            execute(st, "rack.delete", ctx);
        }
        "edit.cut" => {
            if copy_selection(st) {
                // Something in the system clipboard too, so the next Ctrl+V reaches the app as a paste.
                ctx.copy_text("Dragon DAW selection".into());
            }
            execute(st, "edit.delete", ctx);
        }
        "edit.copy" => {
            if copy_selection(st) {
                ctx.copy_text("Dragon DAW selection".into());
            }
        }
        "edit.paste" => paste(st),
        "edit.deselect" => {
            st.ui.clips.clear();
            st.ui.notes.clear();
        }
        "tool.slip" if st.ui.editor == crate::state::Editor::PianoRoll => {
            // FL piano roll S: slide notes on / off for the selection.
            notes_apply(st, "Slide notes", |v| {
                let on = !v.iter().all(|n| n.slide);
                v.iter_mut().for_each(|n| n.slide = on);
            });
        }
        "tool.slip" => st.ui.tool = crate::state::Tool::Slip,
        "tool.playback" => st.ui.tool = crate::state::Tool::Playback,
        "tool.zoom" => st.ui.tool = crate::state::Tool::Zoom,
        x if x.starts_with("ntool.") => {
            use crate::views::note_tools::{open, Tool as T};
            let t = match &x[6..] {
                "arp" => T::Arpeggiator,
                "riff" => T::Riff,
                "limit" => T::Limit,
                "articulate" => T::Articulate,
                "lfo" => T::Lfo,
                "randomize" => T::Randomize,
                "strum" => T::Strum,
                "chop" => T::Chop,
                "claw" => T::Claw,
                "levels" => T::ScaleLevels,
                _ => T::Flip,
            };
            open(st, t);
        }
        "edit.transpose_clip_down" | "edit.transpose_clip_up" => {
            let d = if id.ends_with("up") { 1.0 } else { -1.0 };
            let ids = st.ui.clips.clone();
            st.session.edit("Transpose clips", |p| {
                for c in p.tracks.iter_mut().flat_map(|t| t.clips.iter_mut()).filter(|c| ids.contains(&c.id)) {
                    if let ClipKind::Audio(a) = &mut c.kind {
                        a.pitch = (a.pitch + d).clamp(-24.0, 24.0);
                    }
                }
            });
        }
        "edit.reverse_clips" => {
            let ids = st.ui.clips.clone();
            st.session.edit("Reverse clips", |p| {
                for c in p.tracks.iter_mut().flat_map(|t| t.clips.iter_mut()).filter(|c| ids.contains(&c.id)) {
                    if let ClipKind::Audio(a) = &mut c.kind {
                        a.reverse = !a.reverse;
                    }
                }
            });
        }
        "edit.insert_time" | "edit.delete_time" => match st.session.project.loop_range {
            Some(r) if r.len() > 0.0 => {
                if id == "edit.insert_time" {
                    st.session.edit("Insert space", |p| dragon_engine::edit::insert_time(p, r));
                } else {
                    st.session.edit("Delete space", |p| dragon_engine::edit::delete_time(p, r));
                }
            }
            _ => st.toast("Select a time range on the ruler first", Severity::Info),
        },
        x if x.starts_with("zoom.") => zoom_command(st, &x[5..], ctx),
        // The numeric-pad 0 is FL's fast forward (handled while held), not "mute channel 10".
        "rack.mute0" if crate::views::keys::side(st, crate::views::keys::SideKey::Numpad0) => {}
        x if x.starts_with("rack.mute") || x.starts_with("rack.solo") => {
            let digit: usize = x[9..].parse().unwrap_or(1);
            let k = if digit == 0 { 9 } else { digit - 1 };
            let solo = x.starts_with("rack.solo");
            if st.ui.editor == crate::state::Editor::Rack {
                if let Some(cid) = st.session.project.channels.get(k).map(|c| c.id) {
                    st.session.edit(if solo { "Solo channel" } else { "Mute channel" }, |p| {
                        if solo {
                            let was = p.channels.iter().any(|c| c.id == cid && c.solo);
                            for c in p.channels.iter_mut() {
                                c.solo = !was && c.id == cid;
                            }
                        } else if let Some(c) = p.channels.iter_mut().find(|c| c.id == cid) {
                            c.muted = !c.muted;
                        }
                    });
                }
            } else if !solo && st.ui.editor == crate::state::Editor::Playlist {
                // Outside the rack the number keys are FL's playlist keys.
                match digit {
                    5 => zoom_command(st, "selection", ctx),
                    // FL 6 zooms on the performance zone; without one, the whole song.
                    6 => zoom_command(st, "all", ctx),
                    7 => {
                        execute(st, "edit.transpose_clip_down", ctx);
                    }
                    8 => {
                        execute(st, "edit.transpose_clip_up", ctx);
                    }
                    9 => {
                        execute(st, "edit.reverse_clips", ctx);
                    }
                    _ => {}
                }
            }
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
            // The editor under the mouse decides: the piano roll deletes its selected notes even while
            // playlist clips are selected too.
            let notes_first = st.ui.editor == Ed::PianoRoll && !st.ui.notes.is_empty();
            if !st.ui.clips.is_empty() && !notes_first {
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
        "transport.play" => {
            // FL: Space starts, and stops (back to the start); Ctrl+Space pauses.
            if st.transport().playing && !st.audio.is_recording() {
                st.stop();
            } else {
                st.toggle_play();
            }
        }
        "transport.pause" => st.toggle_play(),
        "transport.panic" => {
            st.audio.send(Cmd::Live(dragon_engine::render::LiveEvent::AllNotesOff));
            st.ui.status = "All sound stopped".into();
        }
        "transport.precount" => {
            let on = st.session.project.metronome.count_in_bars == 0;
            st.session.edit("Precount", |p| p.metronome.count_in_bars = if on { 1 } else { 0 });
            st.ui.status = format!("Recording precount {}", if on { "on" } else { "off" });
        }
        "view.picker" => st.ui.pl_picker = !st.ui.pl_picker,
        "edit.snap_toggle" => {
            if st.ui.snap > 0.0 {
                st.ui.snap_saved = st.ui.snap;
                st.ui.snap = 0.0;
            } else {
                st.ui.snap = if st.ui.snap_saved > 0.0 { st.ui.snap_saved } else { 0.25 };
            }
            st.ui.status = if st.ui.snap > 0.0 { format!("Snap {} beat", st.ui.snap) } else { "Snap off".into() };
        }
        "edit.quick_quantize" if st.ui.editor == crate::state::Editor::Playlist => {
            let (ids, grid) = (st.ui.clips.clone(), if st.ui.snap > 0.0 { st.ui.snap } else { 1.0 });
            st.session.edit("Quantize clip starts", |p| {
                for c in p.tracks.iter_mut().flat_map(|t| t.clips.iter_mut()).filter(|c| ids.is_empty() || ids.contains(&c.id)) {
                    c.start = (c.start / grid).round() * grid;
                }
            });
        }
        "edit.quick_quantize" => {
            let grid = (PPQ as f64 * st.ui.snap.max(0.25)) as u32;
            notes_apply(st, "Quick quantize", |v| dragon_engine::edit::quantize(v, grid.max(1), 1.0, false));
        }
        "edit.flam" => notes_edit(st, "Flam", |n, sel| {
            let v: Vec<Note> = sel.iter().map(|i| n[*i]).collect();
            let extra: Vec<Note> = dragon_engine::edit::flam(&v, (PPQ / 16).max(1)).into_iter().skip(v.len()).collect();
            n.extend(extra);
            sel.to_vec()
        }),
        "edit.glue" => notes_edit(st, "Glue notes", |n, sel| {
            // Joins selected notes of the same pitch that touch or overlap into one.
            let mut idx: Vec<usize> = sel.to_vec();
            idx.sort_by_key(|i| (n[*i].pitch, n[*i].start));
            let mut drop = Vec::new();
            let mut k = 0;
            while k < idx.len() {
                let a = idx[k];
                let mut j = k + 1;
                while j < idx.len() && n[idx[j]].pitch == n[a].pitch && n[idx[j]].start <= n[a].end() {
                    let end = n[a].end().max(n[idx[j]].end());
                    n[a].length = end - n[a].start;
                    drop.push(idx[j]);
                    j += 1;
                }
                k = j;
            }
            drop.sort_unstable();
            for i in drop.iter().rev() {
                n.remove(*i);
            }
            Vec::new()
        }),
        "edit.chop" => {
            let step = ((PPQ as f64 * if st.ui.snap > 0.0 { st.ui.snap } else { 0.25 }) as u32).max(1);
            notes_edit(st, "Chop notes", |n, sel| {
                let mut out = Vec::new();
                let mut keep: Vec<Note> = Vec::new();
                for (i, note) in n.iter().enumerate() {
                    if sel.contains(&i) {
                        let mut t = note.start;
                        while t < note.end() {
                            let len = step.min(note.end() - t);
                            out.push(Note { start: t, length: len, ..*note });
                            t += len;
                        }
                    } else {
                        keep.push(*note);
                    }
                }
                let first = keep.len();
                keep.extend(out);
                *n = keep;
                (first..n.len()).collect()
            });
        }
        "edit.unmute_sel" if st.ui.editor == Ed::PianoRoll && !st.ui.notes.is_empty() => {
            // FL binds Shift+M twice in the piano roll: unmute the selection, or – when nothing selected is
            // muted – select groups of notes at random.
            let notes = st.ui.piano_channel.and_then(|c| st.session.project.pattern(st.ui.pattern).and_then(|p| p.channels.get(&c))).map(|c| c.notes.clone()).unwrap_or_default();
            if st.ui.notes.iter().filter_map(|i| notes.get(*i)).any(|n| n.muted) {
                notes_apply(st, "Unmute notes", |v| v.iter_mut().for_each(|n| n.muted = false));
            } else {
                let mut rng = dragon_engine::dsp::Rng::new(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1));
                // Groups: notes starting together (chords) or explicit note groups.
                let key = |n: &Note| if n.group > 0 { u64::from(n.group) << 32 } else { u64::from(n.start) };
                let mut keys: Vec<u64> = notes.iter().map(key).collect();
                keys.sort_unstable();
                keys.dedup();
                let keep: Vec<u64> = keys.into_iter().filter(|_| rng.next_f32() < 0.5).collect();
                st.ui.notes = notes.iter().enumerate().filter(|(_, n)| keep.contains(&key(n))).map(|(i, _)| i).collect();
            }
        }
        "edit.mute_sel" | "edit.unmute_sel" => {
            let mute = id == "edit.mute_sel";
            if !st.ui.clips.is_empty() {
                let ids = st.ui.clips.clone();
                st.session.edit(if mute { "Mute clips" } else { "Unmute clips" }, |p| {
                    for c in p.tracks.iter_mut().flat_map(|t| t.clips.iter_mut()).filter(|c| ids.contains(&c.id)) {
                        c.muted = mute;
                    }
                });
            } else {
                notes_apply(st, if mute { "Mute notes" } else { "Unmute notes" }, |v| v.iter_mut().for_each(|n| n.muted = mute));
            }
        }
        "edit.invert_sel" if st.ui.editor == crate::state::Editor::Playlist => {
            let sel = std::mem::take(&mut st.ui.clips);
            st.ui.clips = st.session.project.tracks.iter().flat_map(|t| t.clips.iter().map(|c| c.id)).filter(|c| !sel.contains(c)).collect();
        }
        "edit.invert_sel" => {
            let n = st.ui.piano_channel.and_then(|c| st.session.project.pattern(st.ui.pattern).and_then(|p| p.channels.get(&c))).map(|x| x.notes.len()).unwrap_or(0);
            let sel = std::mem::take(&mut st.ui.notes);
            st.ui.notes = (0..n).filter(|i| !sel.contains(i)).collect();
        }
        "edit.move_left" | "edit.move_right" | "edit.move_up" | "edit.move_down" => move_selection(st, id),
        "transport.stop" => st.stop(),
        "transport.record" => {
            if st.transport().recording || st.audio.is_recording() {
                st.stop();
            } else if st.ui.waiting_record {
                st.ui.waiting_record = false;
                st.ui.status = "Recording cancelled".into();
            } else if st.ui.wait_input {
                st.ui.waiting_record = true;
                st.ui.status = "Waiting for input – play a note to start recording".into();
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
        "mixer.autobalance" => {
            st.start_auto_balance();
            st.ui.status = "Auto-balance: measuring every channel …".into();
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
/// Copies the selection into the app's clipboard; returns whether anything was copied.
fn copy_selection(st: &mut State) -> bool {
    let notes_first = st.ui.editor == crate::state::Editor::PianoRoll && !st.ui.notes.is_empty();
    if !st.ui.clips.is_empty() && !notes_first {
        let p = &st.session.project;
        let clips: Vec<(usize, Clip)> = p.tracks.iter().enumerate().flat_map(|(ti, t)| t.clips.iter().filter(|c| st.ui.clips.contains(&c.id)).map(move |c| (ti, c.clone()))).collect();
        st.ui.status = format!("Copied {} clip(s)", clips.len());
        let any = !clips.is_empty();
        st.ui.clipboard = Some(crate::state::Clipboard::Clips(clips));
        return any;
    } else if let Some(ch) = st.ui.piano_channel {
        let notes: Vec<Note> = st.session.project.pattern(st.ui.pattern).and_then(|p| p.channels.get(&ch)).map(|c| st.ui.notes.iter().filter_map(|i| c.notes.get(*i).copied()).collect()).unwrap_or_default();
        if !notes.is_empty() {
            st.ui.status = format!("Copied {} note(s)", notes.len());
            st.ui.clipboard = Some(crate::state::Clipboard::Notes(notes));
            return true;
        }
    }
    false
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

/// The selected rack channels (Ctrl+click), or the current one.
fn rack_selection(st: &State) -> Vec<Id> {
    if st.ui.rack_sel.is_empty() {
        st.ui.channel.into_iter().collect()
    } else {
        st.ui.rack_sel.clone()
    }
}

/// Channels the rack shows (the group filter applied).
pub fn visible_channels(st: &State) -> Vec<Id> {
    st.session.project.channels.iter().filter(|c| st.ui.rack_group.as_ref().is_none_or(|g| &c.group == g)).map(|c| c.id).collect()
}

fn rack_extend(st: &mut State, dir: i32) {
    let chans = visible_channels(st);
    let mut sel = rack_selection(st);
    let edge = if dir < 0 { sel.iter().filter_map(|c| chans.iter().position(|x| x == c)).min() } else { sel.iter().filter_map(|c| chans.iter().position(|x| x == c)).max() };
    if let Some(k) = edge {
        let n = (k as i32 + dir).clamp(0, chans.len() as i32 - 1) as usize;
        if !sel.contains(&chans[n]) {
            sel.push(chans[n]);
        }
        st.ui.rack_sel = sel;
    }
}

fn rack_group_step(st: &mut State, dir: i32) {
    let mut groups: Vec<Option<String>> = vec![None];
    for c in &st.session.project.channels {
        if !c.group.is_empty() && !groups.contains(&Some(c.group.clone())) {
            groups.push(Some(c.group.clone()));
        }
    }
    let k = groups.iter().position(|g| *g == st.ui.rack_group).unwrap_or(0) as i32;
    st.ui.rack_group = groups[(k + dir).rem_euclid(groups.len() as i32) as usize].clone();
    st.ui.status = format!("Rack group: {}", st.ui.rack_group.as_deref().unwrap_or("All"));
}

fn rack_clone(st: &mut State) {
    let sel = rack_selection(st);
    let new = st.session.edit("Clone channels", |p| {
        let mut out = Vec::new();
        for cid in &sel {
            let Some(mut c) = p.channels.iter().find(|c| c.id == *cid).cloned() else { continue };
            let id = p.next_id();
            c.id = id;
            c.name = format!("{} (2)", c.name);
            let at = p.channels.iter().position(|x| x.id == *cid).map(|k| k + 1).unwrap_or(p.channels.len());
            p.channels.insert(at, c);
            for pt in p.patterns.iter_mut() {
                if let Some(data) = pt.channels.get(cid).cloned() {
                    pt.channels.insert(id, data);
                }
            }
            out.push(id);
        }
        out
    });
    st.ui.rack_sel = new;
}

fn rack_zip(st: &mut State, zip: bool) {
    let sel = rack_selection(st);
    st.session.edit(if zip { "Zip channels" } else { "Unzip channels" }, |p| p.channels.iter_mut().filter(|c| sel.contains(&c.id)).for_each(|c| c.zipped = zip));
}

fn rack_move(st: &mut State, dir: i32) {
    let sel = rack_selection(st);
    st.session.edit("Move channels", |p| {
        let n = p.channels.len();
        let mut idx: Vec<usize> = p.channels.iter().enumerate().filter(|(_, c)| sel.contains(&c.id)).map(|(i, _)| i).collect();
        if (dir < 0 && idx.first() == Some(&0)) || (dir > 0 && idx.last() == Some(&(n - 1))) {
            return;
        }
        if dir > 0 {
            idx.reverse();
        }
        for i in idx {
            p.channels.swap(i, (i as i32 + dir) as usize);
        }
    });
}

/// FL Ctrl+L in the rack: each selected channel to its own free mixer track.
fn rack_route_free(st: &mut State) {
    let sel = rack_selection(st);
    st.session.edit("Route to free mixer tracks", |p| {
        for cid in sel {
            let used: Vec<Id> = p.channels.iter().map(|c| c.mixer).chain(p.tracks.iter().map(|t| t.mixer)).collect();
            let master = p.mixer.master.id;
            let free = p.mixer.channels.iter().find(|m| m.id != master && !used.contains(&m.id) && m.inserts.is_empty()).map(|m| m.id);
            let name = p.channels.iter().find(|c| c.id == cid).map(|c| c.name.clone()).unwrap_or_default();
            let target = free.unwrap_or_else(|| p.add_mixer_channel(&name));
            if let Some(m) = p.mixer.get_mut(target) {
                m.name = name;
            }
            if let Some(c) = p.channels.iter_mut().find(|c| c.id == cid) {
                c.mixer = target;
            }
        }
    });
}

/// FL's zoom keys for the playlist or piano roll (whichever was used last): PgUp / PgDn, Shift+0 centre
/// on the playhead, Shift+1…3 zoom levels, Shift+4 show all, Shift+5 / 5 zoom to the selection.
fn zoom_command(st: &mut State, what: &str, ctx: &egui::Context) {
    let piano = st.ui.editor == crate::state::Editor::PianoRoll;
    let (zoom, x, lo, hi) = if piano { (st.ui.piano_zoom, st.ui.pr_x, 12.0, 800.0) } else { (st.ui.playlist_zoom, st.ui.pl_x, 3.0, 400.0) };
    // Visible width in pixels, remembered by the views; a sensible default otherwise.
    let width = ctx.data(|d| d.get_temp::<egui::Rect>(egui::Id::new(if piano { "pr.grid" } else { "pl.timeline" }))).map(|r| r.width()).filter(|w| *w > 50.0).unwrap_or(1000.0);
    let now = st.beat_now();
    let (nz, nx) = match what {
        "in" => (zoom * 1.25, x),
        "out" => (zoom / 1.25, x),
        "center" => (zoom, (now - (width / zoom) as f64 * 0.5).max(0.0)),
        "level1" => (if piano { 24.0 } else { 6.0 }, x),
        "level2" => (if piano { 96.0 } else { 28.0 }, x),
        "level3" => (if piano { 300.0 } else { 120.0 }, x),
        "all" => {
            let end = if piano {
                st.session.project.pattern(st.ui.pattern).map(|p| p.length_beats()).unwrap_or(4.0)
            } else {
                st.session.project.tracks.iter().flat_map(|t| t.clips.iter().map(|c| c.end())).fold(16.0, f64::max)
            };
            (width / end as f32, 0.0)
        }
        _ => {
            // Selection: clips in the playlist, notes in the piano roll.
            let span = if piano {
                let notes = st.ui.piano_channel.and_then(|c| st.session.project.pattern(st.ui.pattern).and_then(|p| p.channels.get(&c))).map(|c| c.notes.clone()).unwrap_or_default();
                st.ui.notes.iter().filter_map(|i| notes.get(*i)).fold(None, |a: Option<(f64, f64)>, n| {
                    let (s, e) = (n.start as f64 / PPQ as f64, n.end() as f64 / PPQ as f64);
                    Some(a.map_or((s, e), |(x, y)| (x.min(s), y.max(e))))
                })
            } else {
                st.session
                    .project
                    .tracks
                    .iter()
                    .flat_map(|t| t.clips.iter())
                    .filter(|c| st.ui.clips.contains(&c.id))
                    .fold(None, |a: Option<(f64, f64)>, c| Some(a.map_or((c.start, c.end()), |(x, y)| (x.min(c.start), y.max(c.end())))))
            };
            match span {
                Some((s, e)) if e > s => (width / (e - s) as f32, s),
                _ => (zoom, x),
            }
        }
    };
    let nz = nz.clamp(lo, hi);
    if piano {
        st.ui.pr_anim = None;
        st.ui.piano_zoom = nz;
        st.ui.pr_x = nx;
    } else {
        st.ui.pl_anim = None;
        st.ui.playlist_zoom = nz;
        st.ui.pl_x = nx;
    }
}

/// FL's Shift+arrows: clips move by the snap (left/right) or to the neighbouring track (up/down);
/// notes move by the snap or a semitone.
fn move_selection(st: &mut State, id: &str) {
    let step = if st.ui.snap > 0.0 { st.ui.snap } else { 0.25 };
    if st.ui.editor == crate::state::Editor::Playlist && !st.ui.clips.is_empty() {
        let ids = st.ui.clips.clone();
        st.session.edit("Move clips", |p| match id {
            "edit.move_left" | "edit.move_right" => {
                let d = if id == "edit.move_left" { -step } else { step };
                for c in p.tracks.iter_mut().flat_map(|t| t.clips.iter_mut()).filter(|c| ids.contains(&c.id)) {
                    c.start = (c.start + d).max(0.0);
                }
            }
            _ => {
                let n = p.tracks.len();
                let up = id == "edit.move_up";
                // Refuse when any selected clip would leave the track list.
                let edge = p.tracks.iter().enumerate().any(|(k, t)| t.clips.iter().any(|c| ids.contains(&c.id)) && if up { k == 0 } else { k + 1 >= n });
                if edge {
                    return;
                }
                let mut moved: Vec<(usize, dragon_engine::model::Clip)> = Vec::new();
                for (k, t) in p.tracks.iter_mut().enumerate() {
                    let (go, stay): (Vec<_>, Vec<_>) = t.clips.drain(..).partition(|c| ids.contains(&c.id));
                    t.clips = stay;
                    moved.extend(go.into_iter().map(|c| (if up { k - 1 } else { k + 1 }, c)));
                }
                for (k, c) in moved {
                    p.tracks[k].clips.push(c);
                }
            }
        });
    } else {
        let ticks = ((PPQ as f64 * step) as i64).max(1);
        notes_apply(st, "Move notes", |v| {
            for n in v.iter_mut() {
                match id {
                    "edit.move_left" => n.start = (n.start as i64 - ticks).max(0) as u32,
                    "edit.move_right" => n.start += ticks as u32,
                    "edit.move_up" => n.pitch = (n.pitch + 1).min(127),
                    _ => n.pitch = n.pitch.saturating_sub(1),
                }
            }
        });
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
