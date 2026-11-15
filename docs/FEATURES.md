# Feature coverage

This page compares Dragon DAW 2.0 with the prioritised feature roadmap (P0 = needed to record, make beats, mix and
master at all; P4 = nice to have). ✅ = implemented and covered by tests or checked in the running app,
🟡 = partly, ❌ = not yet.

## P0 – core

| Feature | Status | Where |
|---|---|---|
| Audio engine with low-latency output, driver / device / buffer choice | ✅ | `engine/device.rs`, Options › Audio (ASIO would need cpal's `asio` feature, which is off) |
| Transport: play, pause, stop, seek, loop, song / pattern mode | ✅ | `render.rs`, transport bar |
| Tempo, time signature, metronome, count-in | ✅ | transport bar, Project view |
| Playlist with audio and pattern clips, snap, zoom | ✅ | `views/playlist.rs`; Ctrl + right-drag zooms into a box (time and track height), Ctrl + right-click zooms out, animated; middle-drag pans; Alt + wheel track height |
| Channel rack and step sequencer | ✅ | `views/rack.rs`; drop a sample (browser or desktop) on a channel to replace its sound |
| Patterns: clone, make unique, split by channel, split at a bar, source pattern, delete | ✅ | the rack's Pattern menu and the ▼ menu of pattern clips (`edit::make_unique`, `split_by_channel`, `split_pattern_at`) |
| Piano roll | ✅ | `views/piano_roll.rs`; drag selected notes (grabbed note snaps, Shift = copy, Alt = no snap), draw-and-drag, arrow-key nudge, Ctrl + right-drag zoom |
| Sampler, synth, drum pads | ✅ | `engine/instruments` |
| Built-in sounds | ✅ | Dragon Kit, synthesised on first run |
| Audio recording of armed tracks with latency compensation | ✅ | `device.rs` (recorder thread), Transport › Record |
| MIDI input (controllers, keyboards) and typing keyboard | ✅ | `device.rs` (`MidiInputs`), `views/keyboard.rs` |
| Mixer with volume, pan, mute, solo, inserts, meters | ✅ | `views/mixer.rs`: FL Studio layout – insert strips with route arrows (click: route the selected insert, right-click: route to this track only, send-level knob on routed arrows), effect slots of the selected insert on the right (used slots + one empty, up to 10; bypass LED, mix, drag to reorder, replace / sidechain / remove) |
| Click-free parameter changes | ✅ | effect parameters glide (`effects::Smoothed`), faders / pan / sends / mix ramp per block, synth cutoff / resonance / gain smoothed |
| Stock EQ, compressor, limiter, reverb, delay | ✅ | `engine/effects`; the reverb has separate early-reflection and late-reverb levels, early size and feed, and an EQ per part (low cut, high cut, bell) |
| Undo / redo | ✅ | `history.rs`, undo history view |
| Save / open projects | ✅ | `project_io.rs` |
| Export WAV | ✅ | `export.rs`, export dialog, `dragon render` |

## P1 – essential production

| Feature | Status | Where |
|---|---|---|
| VST3 plugin hosting | ✅ | `plugins/vst3.rs` (generic editor + embedded native editor) |
| CLAP plugin hosting | ✅ | `plugins/clap.rs` (embedded native editor, floating as fallback) |
| Crash-safe plugin scan | ✅ | `scan_isolated` + `dragon scan-plugin` |
| Plugin delay compensation | ✅ | per-edge delay lines in `Song::compile` |
| Sends, buses, output routing | ✅ | mixer |
| Sidechain | ✅ | stock compressor / gate and other sidechain effects; CLAP second input port and VST3 aux bus |
| Automation lanes and modes | ✅ | `views/automation.rs`, `render.rs` |
| Clip gain, fades, split, trim, duplicate | ✅ | playlist, clip window |
| Takes and comping | ✅ | take lanes, `edit::comp_promote` |
| Loop recording into takes | 🟡 | each new recording pass moves the existing clips into a take lane; several loop cycles in one pass are kept as one long file |
| Punch in / out | ✅ | ruler `Shift`+drag, transport |
| Quantize, legato, transpose, humanize | ✅ | piano roll |
| Scale highlighting and snap | ✅ | piano roll: key (C … B) and scale (26 scales and modes) picked separately; in-scale rows lit, roots marked |
| Loudness metering (LUFS, true peak) | ✅ | Mastering view, `dsp/loudness.rs` |
| Reference track comparison | ✅ | Mastering › Reference: load a song, drag to select a region, compare with the mix (song or loop): LUFS, short-term max, LRA, RMS, peak, true peak, crest, PLR, side/mid, correlation, third-octave tonal balance relative to loudness, suggestions, loudness-matched A/B (`reference.rs`) |
| Export FLAC, stems, normalisation | ✅ | export dialog |
| Autosave and crash recovery | ✅ | `project_io::Autosave`, recovery dialog |
| Browser with preview and drag & drop | ✅ | `views/browser.rs` |
| Keyboard shortcuts, rebindable | ✅ | Options › Keyboard shortcuts |

## P2 – workflow

| Feature | Status | Where |
|---|---|---|
| Time-stretch / pitch-shift of clips, fit to tempo | ✅ | WSOLA in `dsp/stretch.rs`, clip window |
| BPM and key detection | ✅ | `analysis.rs`, Tools menu, browser |
| Chord stamps, progressions, arpeggiator, strum, flam | ✅ | piano roll |
| Slicing a sample to the rack | ✅ | `edit::slice_to_rack`, Tools › Slice |
| Retro MIDI capture | ✅ | `RetroBuffer`, Edit › Notes › Capture |
| MIDI learn | ✅ | right-click a knob or fader |
| VCA faders, solo-safe, polarity, width | ✅ | mixer |
| Multiband compressor, de-esser, transient shaper, imager, saturation and modulation effects | ✅ | `engine/effects` |
| Effect presets | ✅ | effect window (JSON in the config folder) |
| Bounce in place | ✅ | Tools › Bounce |
| MIDI file import / export | ✅ | File › Import / Export |
| Markers, tempo changes, time-signature changes | ✅ | ruler, Project view |
| Command palette | ✅ | `Ctrl+Shift+P` |
| Collect files, relink missing files, backups | ✅ | File menu, Project view |
| Spectrum analyser | ✅ | Mastering view |
| Picker panel: drag patterns (split ones too), audio files and automation clips into the playlist | ✅ | `views/picker.rs` |
| Automation clips in the playlist (the lane plays relative to each clip) | ✅ | `ClipKind::Automation`, `AutomationLane::value_in_clips` |
| Visual routing cables in the mixer, drag a cable to route | ✅ | `views/mixer.rs` (`cables`) |
| Large effect slots, animated interactive effect / synth displays | ✅ | `views/mixer.rs`, `views/fx_ui.rs` |
| Effect editor A/B compare, effect gallery | ✅ | `views/windows.rs`, `views/fx_gallery.rs` |
| Design after Higgsfield references (effect suite sheet, level-up animation) | ✅ | `docs/images/design/` |
| FL-style project folder on save (Samples, Recordings, Renders, Backup) | ✅ | `project_io::save_in_folder` |
| Mixer and Mastering as default tabs next to the playlist | ✅ | `docking::default_layout` (layout version 2) |
| Quests, XP, levels, daily streak, celebrations | ✅ | `views/quests.rs` |
| FL Studio shortcut set (duplicate, undo toggle, clipboard, tools, windows, export) | ✅ | `commands.rs` |
| BPM display: integer / decimal drag zones, double-click to type, right-click tempo presets 70–160 | ✅ | `widgets::bpm_display` |
| Multi-select and drag of patterns / audio / automation clips from the picker | ✅ | `views/picker.rs` (`place_many`) |
| Playlist: Ctrl+drag selection across tracks, Shift+drag clone, multi-track moves | ✅ | `views/playlist.rs` |
| FL-style slice tool (playlist across tracks, piano roll across notes; Alt = no snap) | ✅ | `views/playlist.rs`, `views/piano_roll.rs` |
| Sampler volume envelope editor with bend handles, over the waveform, with presets | ✅ | `views/envelope.rs` |
| Browser waveform preview (cut when playback starts) | ✅ | `views/browser.rs`, `Renderer::play` |
| Export tempo matches playback at every device rate | ✅ | `Renderer::set_song` (the clock follows the song's rate) |
| Stock effects with FabFilter-style handling: EQ analyzer, band add / remove / shape menu, compressor level history, typed values, Ctrl+click reset, full screen | ✅ | `views/fx_ui.rs`, `dsp/spectrum.rs` |
| Native plugin editors resize with the plugin and can go full screen | ✅ Linux (X11) / Windows | `plugins/window.rs`, `clap.rs`, `vst3.rs` |
| Clean Voice, Class-A Strip, Drum Bus, Alter, tube saturator, transient designer, phase meter | ✅ | `effects/voice.rs`, `strips.rs`, `alter.rs`, `dsp/tube.rs`, `dynamics.rs`, `color.rs` |

## Comparison with FL Studio's shortcuts

Checked against the FL Studio manual's shortcut page (*basics_shortcuts*). Shortcuts are rebindable in Options.

| FL Studio | Dragon DAW |
|---|---|
| Space play / stop, Ctrl+Space play / pause, R record, L pattern / song, Ctrl+M metronome, Ctrl+P precount, Ctrl+H stop sound, Ctrl+T typing keyboard | ✅ same |
| Ctrl+Z undo / redo last, Ctrl+Alt+Z undo step by step | ✅ same (plus Ctrl+Shift+Z redo) |
| Ctrl+A / Ctrl+D select all / deselect, Shift+I invert, Ctrl+B duplicate, Ctrl+C / X / V, Del | ✅ same (playlist and piano roll) |
| Tools P draw, B paint, D delete, T mute, C slice, E select; Alt bypasses snap; Backspace toggles snap | ✅ same |
| Shift+arrows move the selection, Alt+M / Shift+M mute / unmute, Insert slices at the cursor | ✅ same |
| Piano roll: Ctrl+↑/↓ octave, Ctrl+L legato, Ctrl+G glue, Ctrl+U chop, Alt+Q quantize, Alt+R randomize, Alt+S strum, Alt+F flam | ✅ same (Shift+Q quick quantize instead of Ctrl+Alt+Q) |
| Playlist: Shift+Q quick-quantize clip starts, Alt+T time marker, Alt+P picker panel, Shift+drag clone, Ctrl+drag select | ✅ same |
| F4 next empty pattern, F5 playlist, F6 channel rack, F7 piano roll, F8 plugin picker, F9 mixer, F10 settings, F11 song info, F12 close windows, Alt+F8 browser | ✅ same |
| File: Ctrl+O, Ctrl+S, Ctrl+Shift+S, Ctrl+N save new version, Ctrl+R export WAV, Ctrl+Shift+R MP3, Ctrl+Shift+M MIDI | ✅ same |
| Y playback tool, Z zoom tool, S slip tool, 5/6 zoom keys, Shift+0…6 zoom levels, PgUp / PgDn zoom, 7/8 transpose and 9 reverse clips, Ctrl+Ins / Ctrl+Del insert / delete time, Alt+A arpeggiator / Alt+E riff machine / Alt+K… tool windows, channel-rack number keys (mute / solo) | ❌ not yet |

## Comparison with the FabFilter bundle

What the stock effects share with FabFilter's handling, and what is still missing.

| FabFilter | Dragon DAW stock effects |
|---|---|
| Pro-Q: live pre / post analyzer behind the curve | ✅ (`dsp/spectrum.rs`, tilted 4.5 dB/oct) |
| Pro-Q: double-click to add a band, drag nodes, wheel = Q, right-click shape menu, hover readout, remove a band | ✅ (Alt+click removes) |
| Pro-Q: solo a band to listen to it, auto gain | ✅ (band menu → Solo; Auto gain parameter) |
| Pro-Q: dynamic EQ, mid / side and L / R per band, linear / natural phase, EQ match, collision detection, piano scale, 24 bands | ❌ (7 bands, stereo, minimum phase) |
| Pro-C: transfer curve, threshold drag, scrolling input / output / gain-reduction history, sidechain HPF and external sidechain, mix, auto makeup | ✅ |
| Pro-C: styles (Clean, Punch, Opto, Vocal…), lookahead, sidechain EQ, hold | ❌ (the limiter has lookahead) |
| Pro-L: true-peak limiting, loudness meters | ✅ (limiter true peak; LUFS in the Mastering view) |
| Pro-L: limiting styles, oversampling choice, scrolling waveform | ❌ |
| Pro-R / Pro-DS / Pro-MB / Saturn / Timeless / Volcano | partial: reverb with early / late EQ, de-esser, multiband, saturator with nine tubes, delay, filter – without FabFilter's modulation matrix, per-band saturation or decay-rate EQ |
| Every plugin: A/B compare, presets, resizable / full screen, typed values, Ctrl+click reset, fine drag | ✅ (no undo history inside a plugin; the project undo covers parameter edits) |

## P3 – advanced

| Feature | Status | Where |
|---|---|---|
| Pitch correction (Auto-Tune / Newtone style) | ✅ | "Dragon Tune" effect: real-time and low latency for tracking (pitch-synchronous shifting, 2.7 ms hop), YIN detection, key + scale, retune speed, humanize, flex-tune, vibrato (`effects/pitch.rs`) |
| MP3 / Ogg Vorbis export | ✅ | LAME and libvorbis (aoTuV) encoders, export dialog and `dragon render --format mp3/ogg` |
| Native plugin editors embedded in a host window | ✅ Linux (X11) / Windows | `plugins/window.rs`; the plugin's own window opens automatically with its effect / instrument window (generic controls as a fallback, with the reason when a GUI cannot open). VST3: `IPlugFrame`, `IRunLoop` (Linux), `IComponentHandler` (GUI edits reach the processor and the project, one undo step per knob turn), host-created `IMessage` / `IAttributeList`, content scale for HiDPI. CLAP: `set_parent`, `set_scale`, timer / fd host extensions, GUI edits from output events. Tested on Linux and Windows CI with real windows, and with 40 real third-party builds (Dragonfly Reverb, DISTRHO collection incl. the OpenGL ProM visualiser; VST3 and CLAP): all 28 with a GUI embed, the 10 without one fall back to generic controls – re-run with `DRAGON_THIRD_PARTY=<folders> cargo test -p dragon-test-plugins --test third_party -- --ignored --nocapture`. CLAP plugins shipped as folders (`Name.clap/Name.clap`) load. macOS ❌ |
| Sidechain into plugins | ✅ | CLAP second input port, VST3 aux input bus |
| Clip launcher / performance mode | ✅ | View › Performance (F12): tracks × patterns, launch quantised to bar / beat, stop lanes, `LiveEvent::Launch` in the renderer |
| Audio-to-MIDI | ✅ | Tools › Convert selected clip to MIDI (melody: YIN pitch tracking; drums: onsets sorted kick / snare / hat) |
| Stem separation | ✅ (DSP, not AI) | Tools › Separate: harmonic/percussive median filtering + centre extraction → drums, harmonic, centre stems |
| Control surfaces (Mackie Control) | ✅ | Options › Control surface: faders, V-pots, mute / solo / arm / select, transport, banks; motor faders and LEDs |
| Video track | ✅ (needs ffmpeg) | File › Import video, View › Video (frame at the playhead with timecode), soundtrack as a clip, export the video with the mix |
| Surround 5.1 / 7.1 | ✅ (export) | Project › Surround; per-channel XY panner with spread and LFE send (2D VBAP); multichannel WAV export (`surround.rs`). Monitoring is stereo; master inserts are not applied to the surround mix |
| Dolby Atmos (objects, ADM BWF) | ✅ | 7.1.4 bed with height panning; every top-level channel exported as two positioned objects in an ADM BWF (BS.2076 axml + BS.2088 chna) for the Dolby Atmos Renderer, Resolve, Nuendo. No in-app Atmos binaural monitoring (Dolby-licensed); files over 4 GB (BW64) not supported |

## P4 – out of scope for an offline app

| Feature | Status | Notes |
|---|---|---|
| Cloud collaboration, sharing, accounts | ❌ by design | you asked for an offline app without a backend |
| AI (neural) stem separation / generation | ❌ | the DSP separation above runs offline; the splash animation was made with Higgsfield AI |

## Not verified on real hardware

The development environment has no sound card, no MIDI ports and no control surface; plugin editors were tested
against the in-repo test plugins on an X11 server (Xvfb), not against commercial plugins. The device layer (`cpal` output and input
streams, the recorder thread, `midir` inputs) compiles and is exercised through the same `Renderer` the tests
cover. Without a device the app falls back to a silent clock. Before relying on recording, test it on your
machine.
