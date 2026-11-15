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

Checked against the FL Studio manual's shortcut page (*basics_shortcuts*). Shortcuts are rebindable in Options; a test makes sure no two commands share a default key.

| FL Studio | Dragon DAW |
|---|---|
| Space play / stop, Ctrl+Space play / pause, R record, L pattern / song, Ctrl+M metronome, Ctrl+P precount, Ctrl+H stop sound, Ctrl+T typing keyboard | ✅ same |
| Ctrl+Z undo / redo last, Ctrl+Alt+Z undo step by step | ✅ same (plus Ctrl+Shift+Z redo) |
| Ctrl+A / Ctrl+D select all / deselect, Shift+I invert, Ctrl+B duplicate, Ctrl+C / X / V, Del | ✅ same (playlist and piano roll) |
| Tools P draw, B paint, D delete, T mute, C slice, E select, Y playback, Z zoom, S slip edit (playlist) / slide (piano roll); Alt bypasses snap; Backspace toggles snap | ✅ same |
| Shift+arrows move the selection, Alt+M / Shift+M mute / unmute, Insert slices at the cursor, 7 / 8 transpose and 9 reverse clips | ✅ same |
| PgUp / PgDn zoom, Shift+0 centre on the playhead, Shift+1…3 zoom levels, Shift+4 show all, Shift+5 and 5 zoom to selection | ✅ same |
| Ctrl+Insert / Ctrl+Delete insert / delete space (the ruler selection) | ✅ same (markers follow) |
| Channel rack 1…0 mute, Ctrl+1…0 solo the first ten channels | ✅ same (number keys follow the editor under the mouse) |
| Piano roll: Ctrl+↑/↓ octave, Ctrl+L legato, Ctrl+G glue, Ctrl+U quick chop, Alt+Q quantize, Alt+F flam | ✅ same (Shift+Q quick quantize instead of Ctrl+Alt+Q) |
| Piano-roll tools Alt+A arpeggiator, Alt+E riff machine, Alt+K limit, Alt+L articulate, Alt+O LFO, Alt+R randomize, Alt+S strum, Alt+U chop, Alt+W claw machine, Alt+X scale levels, Alt+Y score flipper | ✅ one tool window with a tab each (simpler than FL's: no per-tool presets) |
| Playlist: Shift+Q quick-quantize clip starts, Alt+T time marker, Alt+P picker panel, Shift+drag clone, Ctrl+drag select | ✅ same |
| F4 next empty pattern, F5 playlist, F6 channel rack, F7 piano roll, F8 plugin picker, F9 mixer, F10 settings, F11 song info, F12 close windows, Alt+F8 browser | ✅ same |
| File: Ctrl+O, Ctrl+S, Ctrl+Shift+S, Ctrl+N save new version, Ctrl+R export WAV, Ctrl+Shift+R MP3, Ctrl+Shift+M MIDI | ✅ same |
| Channel rack: Ctrl+click multi-select, Alt+C clone, Alt+Del delete, Alt+G group, PgUp / PgDn group view, Alt+Z / Alt+U zip / unzip, Alt+↑/↓ move, Ctrl+L route to free mixer tracks, Alt+M mixer selectors, ↑/↓ select, Shift+↑/↓ extend, Ctrl+Enter focus playlist | ✅ |
| Piano roll: G / K and H / J channel stepping, Shift+H / Shift+K, M note names, O portamento, F / Shift+F event lane (velocity, pan, release), Shift+D discard lengths, Shift+N chord off, Shift+R select at random, Shift+C select by colour, Alt+C colour, Shift+G / Alt+G groups, Alt+B helpers, Alt+V ghosts, Alt+arrows nudge, Ctrl+Alt+Q quick quantize, Ctrl+M import MIDI | ✅ |
| Playlist: Shift+G / Alt+G clip groups, Shift+C select by source, Ctrl+Alt+C consolidate, Ctrl+P performance, 6 zoom, Ctrl+← / → / Enter time selection, Shift+Alt+T time signature, Ctrl+F8 project picker | ✅ (6 shows the whole song – there is no separate performance zone) |
| Piano roll Alt+N waveform helper (the song's audio under the notes), Shift+Alt+wheel nudge (playlist and piano roll), Ctrl+Alt+Home resizing from the left | ✅ |
| Numeric-pad 0 fast forward | ✅ held numeric-pad 0 seeks forward at 4× (read from the OS key state – X11 and Windows; not on Wayland or macOS) |
| Numeric-pad * and / (next / previous bar in song mode; with Alt next / previous marker) | ✅ (read as typed characters, so the main-keyboard * and / work too; Alt+/ and Alt++ also jump markers) |
| Ctrl+I: insert controller value (piano roll – the last touched parameter goes into its automation lane at the playhead) / wait for input to start recording (elsewhere – the first played note starts the armed recording) | ✅ |
| Shift+M in the piano roll: unmute the selection, or – when nothing selected is muted – select groups of notes at random (FL binds both) | ✅ |

## FL's slice tool, compared

| FL Studio (playlist and piano roll) | Dragon DAW |
|---|---|
| C selects the slice tool | ✅ |
| Click on a clip / note slices it at the pointer (snapped) | ✅ |
| Drag a line (any angle) – every clip / note it crosses is cut where the line passes its track / row | ✅ |
| Alt while slicing: no snap | ✅ |
| Right-drag with the slice tool: slice and delete the smaller piece (FL Right-Shift+right-click in the piano roll) | ✅ |
| Right-Shift+left-click with any tool slices; Right-Shift+right-click slices and deletes the smaller piece (playlist) / keeps the longer one (piano roll); Right-Shift+drag cuts a line | ✅ |
| Right-Alt+click mutes, Right-Alt+right-click quantizes; Left-Shift+right-drag pans; Left-Shift+resize ripples the following clips; holding the right button on empty space starts the slice line; middle+right click opens the tool menu | ✅ |
| Which Shift / Alt is held (left or right) | read from the OS key state on X11 and Windows; on Wayland and macOS both sides act as left (use the slice tool there) |
| Double right-click activates the mute tool (playlist); Alt+Right-Shift slices with snap off | ✅ |
| Ctrl+U quick chop / Alt+U chop tool (piano roll) | ✅ |

## Comparison with the FabFilter bundle

What the stock effects share with FabFilter's handling, and what is still missing.

| FabFilter | Dragon DAW stock effects |
|---|---|
| Pro-Q: 24 bands, live pre / post analyzer, double-click to add a band, spectrum grab (drag a peak of the analyzer), drag nodes, wheel = Q, right-click shape menu, hover readout with note names, piano scale, remove a band, band solo, auto gain, one band's controls at a time | ✅ |
| Pro-Q: dynamic EQ per band, stereo placement per band (stereo, mid, side, left, right), zero-latency, natural-phase (filters at 2× rate, ~0.5 ms) and linear-phase processing | ✅ (linear phase: bands are stereo and static) |
| Pro-Q: EQ match against another EQ in the project, the sidechain input or an audio file (learn the spectra, fill free bands), collision display with another EQ's spectrum | ✅ |
| Pro-C: styles (Clean, Classic, Opto, Vocal, Mastering, Bus, Punch, Pumping), look-ahead, hold, sidechain high- and low-pass, external sidechain, transfer curve with threshold drag, scrolling level / gain-reduction history, mix, auto makeup | ✅ (styles designed after Pro-C's names, not measured against it) |
| Pro-L: true-peak limiting, styles (Allround, Transparent, Punchy, Dynamic, Aggressive, Modern, Bus, Safe), 2× / 4× oversampling, scrolling level / gain-reduction display, unity-gain listening, loudness meters | ✅ (LUFS in the Mastering view) |
| Saturn: multiband saturation – up to four bands with draggable crossovers, eight styles, drive, dynamics, tone, level, mix and a resonant feedback loop per band | ✅ "Multiband Saturator" |
| Pro-R: decay-rate EQ, drawn as a curve | ✅ on the reverb: five draggable points (80 Hz … 12 kHz, ×¼ … ×4) – Pro-R's curve has free nodes |
| Modulation: LFOs (five shapes, free or tempo-synced), envelope followers and an XY pad on any stock-effect parameter, with depth; the XY pad is MIDI-learnable and automatable (MIDI as a source) | ✅ (no slew or per-source MIDI note / velocity sources) |
| Pro-DS: single-vocal (relative) / allround detection, split band (dynamic high shelf) / wide band, detection band high- and low-pass, threshold, range, look-ahead 0–15 ms, stereo link, listen | ✅ De-esser |
| Pro-MB: up to four bands, three crossovers, per band compress / expand / upward compress / upward expand, threshold, range, ratio, knee, attack, release, gain, solo, bypass; mix, output | ✅ Multiband Dynamics (crossover bands, not Pro-MB's dynamic phase mode; no per-band sidechain) |
| Timeless: tempo-synced stereo delay with feedback, cross feedback, drive in the loop, filter with resonance, diffusion, ducking, freeze, width and pitch shift in the loop | ✅ Delay (no modulation matrix beyond the shared LFO / envelope / XY sources) |
| Volcano: two filters (LP, HP, BP, notch, peak, shelves) at 12 / 24 / 48 dB/oct, styles (Clean, Smooth, Ram, Hack), drive, serial / parallel routing, B following A, stereo spread | ✅ Filter |
| Pro-G: threshold with hysteresis, range, ratio (gate … expander), knee, attack, hold, release, look-ahead, styles (Classic, Clean, Vocal, Guitar, Upward), sidechain filters, external sidechain, flip (ducking), mix, listen | ✅ Gate |
| Every plugin: A/B compare, presets, resizable / full screen, typed values, Ctrl+click reset, fine drag | ✅ |

## The new plugins against their published references

Checked against the manufacturers' manuals and spec sheets (tests in `effects/strips.rs`, `alter.rs`, `dynamics.rs`). None of them has been measured against recordings of the real units – that needs the hardware (or recordings of it) to compare with, which this project does not have; the sound is our model.

| Reference | Matches | Differs |
|---|---|---|
| Neve 1073 (preamp + EQ) | HPF 50 / 80 / 160 / 300 Hz at 18 dB/oct, low shelf 35 / 60 / 110 / 220 Hz ±16 dB, mid bell 360 Hz … 7.2 kHz ±18 dB, high shelf 12 kHz ±16 dB, transformer-coupled class-A drive | no mic / line impedance switch, no stepped 5 dB gain switch (continuous drive) |
| Tube-Tech CL 1B | ratio 2–10, attack 0.5–300 ms, release 0.05–10 s, Fixed (1 ms / 50 ms), Fix/Man (fast release for short peaks, manual for long), Manual, gain up to 30 dB, opto cell | threshold −40…0 dBFS instead of +20…−40 dBm, no stereo link bus |
| Cradle Orion | modules Input (target meter), Drive (3 colours, high-pass after), Harmonics (50 / 80 Hz focus), Compression (2 modes, mix), Lift, Harmonic EQ (5 bands, 1 dB steps), Parallels Sub / Low / Loud / Grit, Output | Orion's band frequencies and algorithms are not published; ours are our own |
| Soundtoys Little AlterBoy | Pitch ±12 st, Formant, Link, modes Transpose / Quantize / Robot (Pitch sets the robot note, 0 = C5), tube Drive, Mix, Output | no MIDI control of Pitch |
| Waves Smack Attack | attack and sustain level, sensitivity, duration, shape (Needle / Nail / Blunt; Linear / Non-linear / Soft linear), Mix, Output before Guard, Guard Off / Clip / Limit at −0.1 dBFS, listen modes | an extra Soft clip guard |

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
