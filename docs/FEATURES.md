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
| Sampler, synth, drum pads | ✅ | `engine/instruments` (plus wavetable, guitar, piano, FM, organ, 808, ensemble and granular – see P2) |
| Built-in sounds | ✅ | Dragon Kit, synthesised on first run |
| Audio recording of armed tracks with latency compensation | ✅ | `device.rs` (recorder thread), Transport › Record |
| MIDI input (controllers, keyboards) and typing keyboard | ✅ | `device.rs` (`MidiInputs`), `views/keyboard.rs` |
| Mixer with volume, pan, mute, solo, inserts, meters | ✅ | `views/mixer.rs`: classic layout – insert strips with route arrows (click: route the selected insert, right-click: route to this track only, send-level knob on routed arrows), effect slots of the selected insert on the right (used slots + one empty, up to 10; bypass LED, mix, drag to reorder, replace / sidechain / remove) |
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
| Clip gain, fades, split, trim, duplicate | ✅ | playlist, clip window; dB handle on every audio clip (drag up/down, Shift fine, double-click = 0 dB, selected clips move together) |
| Drag & drop from the file manager | ✅ | audio dropped on the playlist lands on the track and grid beat under the mouse (a ghost shows where), several files on consecutive tracks, below the last track a new track; on the channel rack it loads a sampler |
| Designed plugin panels | ✅ | every stock effect has its own panel instead of a generic knob grid (`views/fx_panel.rs`): titled sections, a skin per device family (modern rings, brushed metal, chicken-head console knobs, cream valve caps), a large main knob, faders, LED switches, segmented selectors, band pickers inside the band's section; labels fit their cells. Every effect's display is also a control (drag nodes, lines, drawn knobs and bars, or an XY pad over the two main parameters). Tests check that each panel shows every parameter and each display reacts to the mouse |
| One-fader noise suppressor | ✅ | first stage of the Class-A Strip: one fader (0 … 100 %), four bands each tracking its own noise floor (2 s sliding minimum, -96 … -40 dBFS) and pulled down near it; bit-transparent at 0 %, zero latency; lamp shows live suppression |
| Auto-panner | ✅ | "Dragon Orbit": Sync (locked to the song position), Free or Trigger (one step per hit, with glide); 7 shapes incl. random; width, centre, phase, smoothing; equal-power or balance law; binaural mode (far-ear delay and head shadow). Its display is the control: drag the stereo field, click shapes and modes |
| Character EQ (Jadefang) | ✅ | Four bands (LF / HF shelves, LMF / HMF bells) in six voicings – Clean scale, Hoard (iron), Ember (valve, shelf dips), Talon (proportional Q), Skyfire (extra broad), Ancient (passive) – a driven input stage, a very broad Sub band and a boost-only Air band (2.5 – 40 kHz); laid out like a classic channel-strip EQ module: a black panel with backlit voicing keys and a big input knob, a row per band (gain, frequency) on a jade plate; no graph |
| Mixer auto-balance | ✅ | Mixer › Tools › Auto-balance: measures every channel pre-fader while it plays (loud-part loudness, peak, spectrum), recognises its role from the names of the channel, tracks, clips, instruments and samples (or from its sound), sets it a fixed number of dB under the lead (vocal 0, kick −1, snare / 808 −2, bass −3, lead −4, chords −7, backing vocals −8, percussion −10, cymbals −11, pads −12, FX −13, room −14), lets layers of one role share its level, pulls low-heavy non-bass sources back 1.5 dB, then sums the result once more and moves every fader alike for −16 LUFS / −6 dBFS into the master. Faders −40 … +10 dB, with sources that need more reported. Muted channels, buses, returns, VCAs, pan, sends and effects untouched; one undo step; a report where each role can be corrected (remembered per channel, re-balances instantly) |
| Real-time safety | ✅ | no allocation on the audio thread (`tests/realtime.rs`), old songs freed on a GC thread, constant-cost loudness meter, insert chains spread over all cores; 200 stock plugins run in real time on a 4-core machine; knob moves reach the effects every frame and effects retune without resetting their filters (no zipper or "downsampled" sound) |
| Audio output watchdog | ✅ | a stream error or a callback that stops for 2 s restarts the device and resumes playback at the same spot; a lost device is retried every 5 s |
| Takes and comping | ✅ | take lanes, `edit::comp_promote` |
| Loop recording into takes | ✅ | recording across an active loop splits the file into its passes: each pass is a take ("Pass 1", "Pass 2" …), the last whole pass plays on the main lane; over existing audio everything goes onto a new grouped track (`edit::loop_passes`, `state.rs`) |
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
| classic project folder on save (Samples, Recordings, Renders, Backup) | ✅ | `project_io::save_in_folder` |
| Mixer and Mastering as default tabs next to the playlist | ✅ | `docking::default_layout` (layout version 2) |
| Quests, XP, levels, daily streak, celebrations | ✅ | `views/quests.rs` |
| Full shortcut set (duplicate, undo toggle, clipboard, tools, windows, export) | ✅ | `commands.rs` |
| BPM display: integer / decimal drag zones, double-click to type, right-click tempo presets 70–160 | ✅ | `widgets::bpm_display` |
| Multi-select and drag of patterns / audio / automation clips from the picker | ✅ | `views/picker.rs` (`place_many`) |
| Playlist: Ctrl+drag selection across tracks, Shift+drag clone, multi-track moves | ✅ | `views/playlist.rs` |
| classic slice tool (playlist across tracks, piano roll across notes; Alt = no snap) | ✅ | `views/playlist.rs`, `views/piano_roll.rs` |
| Sampler volume envelope editor with bend handles, over the waveform, with presets | ✅ | `views/envelope.rs` |
| Browser waveform preview (cut when playback starts) | ✅ | `views/browser.rs`, `Renderer::play` |
| Export tempo matches playback at every device rate | ✅ | `Renderer::set_song` (the clock follows the song's rate) |
| Stock effects with pro-grade handling: EQ analyzer, band add / remove / shape menu, compressor level history, typed values, Ctrl+click reset, full screen | ✅ | `views/fx_ui.rs`, `dsp/spectrum.rs` |
| Native plugin editors resize with the plugin and can go full screen | ✅ Linux (X11) / Windows | `plugins/window.rs`, `clap.rs`, `vst3.rs` |
| Clean Voice, Class-A Strip, Drum Bus, Alter, tube saturator, transient designer, phase meter | ✅ | `effects/voice.rs`, `strips.rs`, `alter.rs`, `dsp/tube.rs`, `dynamics.rs`, `color.rs` |
| Convolution reverb, volume shaper, analyzer, tuner, amp & cabinet, vocoder, frequency shifter, stutter, tape stop, granular, lo-fi, exciter, auto-filter, shimmer reverb, spectral freeze | ✅ | Reliquary (uniformly partitioned convolution, 10 built-in spaces or your own IR file, swapped without a click), Tidecaller, Scrying Glass, Siren's Fork, Wyrmstack, Mandrake, Mercury Wheel, Hex, Petrify, Sandglass, Codex, Wisp, Familiar, Seraph, Rime – each with its themed panel, live display, emblem, 200 factory presets and manual pages (`effects/convolver.rs`, `shaper.rs`, `meters.rs`, `amp.rs`, `voxfx.rs`, `timefx.rs`, `colorfx.rs`, `spacefx.rs`) |
| Mid / side compression | ✅ | the compressor's Channels switch: Stereo, Mid or Side (the other part delayed to match the look-ahead) |
| MIDI effects | ✅ | a chain per channel in its instrument window: arpeggiator (synced, up / down / random / chords, octaves, gate, swing, latch), chord, note repeat, scale; allocation-free (`midifx.rs`) |
| More instruments | ✅ | Hearthfire (4-operator FM, 8 algorithms), Basilica (tonewheel organ: drawbars, percussion, scanner vibrato, rotary speaker), Wyrmbass (808 bass with glide), Chorale (strings, choirs, pads, brass), Sandstorm (granular instrument on any sample); themed panels, live displays, factory presets (`instruments/native.rs`). The slicer is the existing Shardcutter |

## Comparison with the shortcuts

Shortcuts are rebindable in Options; a test makes sure no two commands share a default key.

| Feature | Dragon DAW |
|---|---|
| Space play / stop, Ctrl+Space play / pause, R record, L pattern / song, Ctrl+M metronome, Ctrl+P precount, Ctrl+H stop sound, Ctrl+T typing keyboard | ✅ same |
| Ctrl+Z undo / redo last, Ctrl+Alt+Z undo step by step | ✅ same (plus Ctrl+Shift+Z redo) |
| Ctrl+A / Ctrl+D select all / deselect, Shift+I invert, Ctrl+B duplicate, Ctrl+C / X / V, Del | ✅ same (playlist and piano roll; Ctrl+X deletes every selected clip, note, or – in the channel rack – channel at once) |
| Tools P draw, B paint, D delete, T mute, C slice, E select, Y playback, Z zoom, S slip edit (playlist) / slide (piano roll); Alt bypasses snap; Backspace toggles snap | ✅ same |
| Shift+arrows move the selection, Alt+M / Shift+M mute / unmute, Insert slices at the cursor, 7 / 8 transpose and 9 reverse clips | ✅ same |
| PgUp / PgDn zoom, Shift+0 centre on the playhead, Shift+1…3 zoom levels, Shift+4 show all, Shift+5 and 5 zoom to selection | ✅ same |
| Ctrl+Insert / Ctrl+Delete insert / delete space (the ruler selection) | ✅ same (markers follow) |
| Channel rack 1…0 mute, Ctrl+1…0 solo the first ten channels | ✅ same (number keys follow the editor under the mouse) |
| Piano roll: Ctrl+↑/↓ octave, Ctrl+L legato, Ctrl+G glue, Ctrl+U quick chop, Alt+Q quantize, Alt+F flam | ✅ same (Shift+Q quick quantize instead of Ctrl+Alt+Q) |
| Piano-roll tools Alt+A arpeggiator, Alt+E riff machine, Alt+K limit, Alt+L articulate, Alt+O LFO, Alt+R randomize, Alt+S strum, Alt+U chop, Alt+W claw machine, Alt+X scale levels, Alt+Y score flipper | ✅ one tool window with a tab each (simpler than 's: no per-tool presets) |
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
| Shift+M in the piano roll: unmute the selection, or – when nothing selected is muted – select groups of notes at random | ✅ |

## the slice tool, compared

| Playlist and piano roll | Dragon DAW |
|---|---|
| C selects the slice tool | ✅ |
| Click on a clip / note slices it at the pointer (snapped) | ✅ |
| Drag a line (any angle) – every clip / note it crosses is cut where the line passes its track / row | ✅ |
| Alt while slicing: no snap | ✅ |
| Right-drag with the slice tool: slice and delete the smaller piece (Right-Shift+right-click in the piano roll) | ✅ |
| Right-Shift+left-click with any tool slices; Right-Shift+right-click slices and deletes the smaller piece (playlist) / keeps the longer one (piano roll); Right-Shift+drag cuts a line | ✅ |
| Right-Alt+click mutes, Right-Alt+right-click quantizes; Left-Shift+right-drag pans; Left-Shift+resize ripples the following clips; holding the right button on empty space starts the slice line; middle+right click opens the tool menu | ✅ |
| Which Shift / Alt is held (left or right) | read from the OS key state on X11 and Windows; on Wayland and macOS both sides act as left (use the slice tool there) |
| Double right-click activates the mute tool (playlist); Alt+Right-Shift slices with snap off | ✅ |
| Ctrl+U quick chop / Alt+U chop tool (piano roll) | ✅ |

## Comparison with the bundle

What the stock effects share with handling. Every listed feature is there; the one caveat: the compressor and limiter styles are modelled after names, not measured against the originals.

| | Dragon DAW stock effects |
|---|---|
| EQ: 24 bands, live pre / post analyzer, double-click to add a band, spectrum grab (drag a peak of the analyzer), drag nodes, wheel = Q, right-click shape menu, hover readout with note names, piano scale, remove a band, band solo, auto gain, one band's controls at a time | ✅ |
| EQ: dynamic EQ per band, stereo placement per band (stereo, mid, side, left, right), zero-latency, natural-phase (filters at 2× rate, ~0.5 ms) and linear-phase processing | ✅ (in linear phase too: placement through a mid/side kernel matrix, dynamic bands re-design the kernels every ≥ 512 samples) |
| EQ: EQ match against another EQ in the project, the sidechain input or an audio file (learn the spectra, fill free bands), collision display with another EQ's spectrum | ✅ |
| Compressor: styles (Clean, Classic, Opto, Vocal, Mastering, Bus, Punch, Pumping), look-ahead, hold, sidechain high- and low-pass, external sidechain, transfer curve with threshold drag, scrolling level / gain-reduction history, mix, auto makeup | ✅ (styles designed after pro-grade tools's names, not measured against it) |
| Limiter: true-peak limiting, styles (Allround, Transparent, Punchy, Dynamic, Aggressive, Modern, Bus, Safe), 2× / 4× oversampling, scrolling level / gain-reduction display, unity-gain listening, loudness meters | ✅ (LUFS in the Mastering view) |
| Multiband saturation – up to four bands with draggable crossovers, eight styles, drive, dynamics, tone, level, mix and a resonant feedback loop per band | ✅ "Multiband Saturator" |
| Reverb: decay-rate EQ, drawn as a curve with free nodes | ✅ on the reverb: six bands, each bell, low / high shelf or notch (right-click), free frequency 20 Hz … 20 kHz, ×¼ … ×4 of the decay time, Q on the wheel |
| Modulation: LFOs (five shapes, free or tempo-synced), envelope followers and an XY pad on any stock-effect parameter, with depth; the XY pad is MIDI-learnable and automatable (MIDI as a source) | ✅ plus MIDI note and MIDI velocity sources (the last note played – patterns, piano roll or live MIDI) and slew per route |
| Single-vocal (relative) / allround detection, split band (dynamic high shelf) / wide band, detection band high- and low-pass, threshold, range, look-ahead 0–15 ms, stereo link, listen | ✅ De-esser |
| Multiband dynamics: up to four bands, three crossovers, per band compress / expand / upward compress / upward expand, threshold, range, ratio, knee, attack, release, gain, solo, bypass; mix, output | ✅ Multiband Dynamics (per-band external sidechain; two processing modes: crossover bands with all-pass compensation, or dynamic phase – each band a dynamic shelf / bell on the full signal, untouched while idle) |
| Tempo-synced stereo delay with feedback, cross feedback, drive in the loop, filter with resonance, diffusion, ducking, freeze, width and pitch shift in the loop | ✅ Delay, with its own modulation matrix: two LFOs (free or synced, four shapes) and an envelope follower routed through four slots to time, feedback, low / high cut, drive, pitch, width or mix |
| Dual filter: two filters (LP, HP, BP, notch, peak, shelves) at 12 / 24 / 48 dB/oct, styles (Clean, Smooth, Ram, Hack), drive, serial / parallel routing, B following A, stereo spread | ✅ Filter |
| Gate: threshold with hysteresis, range, ratio (gate … expander), knee, attack, hold, release, look-ahead, styles (Classic, Clean, Vocal, Guitar, Upward), sidechain filters, external sidechain, flip (ducking), mix, listen | ✅ Gate |
| Every plugin: A/B compare, presets, resizable / full screen, typed values, Ctrl+click reset, fine drag | ✅ (stock editors: fitted window, never under 480 px, full screen in a window of its own; CLAP / VST3 editors: the window takes the editor's requested size, also under window managers, and full screen lifts the size lock – tested with openbox on Xvfb) |

## The new plugins against their published references

Checked against the manufacturers' manuals and spec sheets (tests in `effects/strips.rs`, `alter.rs`, `dynamics.rs`). None of them has been measured against recordings of the real units – that needs the hardware (or recordings of it) to compare with, which this project does not have; the sound is our model.

| Reference | Matches | Differs |
|---|---|---|
| Class-A preamp and EQ | HPF 50 / 80 / 160 / 300 Hz at 18 dB/oct, low shelf 35 / 60 / 110 / 220 Hz ±16 dB, mid bell 360 Hz … 7.2 kHz ±18 dB, high shelf 12 kHz ±16 dB, transformer-coupled class-A drive | no mic / line impedance switch, no stepped 5 dB gain switch (continuous drive) |
| Opto compressor | ratio 2–10, attack 0.5–300 ms, release 0.05–10 s, Fixed (1 ms / 50 ms), Fix/Man (fast release for short peaks, manual for long), Manual, gain up to 30 dB, opto cell | threshold −40…0 dBFS instead of +20…−40 dBm, no stereo link bus |
| Drum bus | modules Input (target meter), Drive (3 colours, high-pass after), Harmonics (50 / 80 Hz focus), Compression (2 modes, mix), Lift, Harmonic EQ (5 bands, 1 dB steps), Parallels Sub / Low / Loud / Grit, Output | band frequencies and algorithms are our own |
| Pitch and formant shifter | Pitch ±12 st, Formant, Link, modes Transpose / Quantize / Robot (Pitch sets the robot note, 0 = C5), tube Drive, Mix, Output | no MIDI control of Pitch |
| Transient designer | attack and sustain level, sensitivity, duration, shape (Needle / Nail / Blunt; Linear / Non-linear / Soft linear), Mix, Output before Guard, Guard Off / Clip / Limit at −0.1 dBFS, listen modes | an extra Soft clip guard |

## P3 – advanced

| Feature | Status | Where |
|---|---|---|
| Pitch correction | ✅ | "Dragon Tune" effect: real-time and low latency for tracking (pitch-synchronous shifting, 2.7 ms hop), YIN detection, key + scale, retune speed, humanize, flex-tune, vibrato; note stabilizer, doubler, heat map on the note keys, big note readout with a ±50 ct meter and a pitch graph over note lanes (`effects/pitch.rs`); negative speed and grouped instances are not there |
| MP3 / Ogg Vorbis export | ✅ | LAME and libvorbis (aoTuV) encoders, export dialog and `dragon render --format mp3/ogg` |
| Native plugin editors embedded in a host window | ✅ Linux (X11) / Windows | `plugins/window.rs`; the plugin's own window opens automatically with its effect / instrument window (generic controls as a fallback, with the reason when a GUI cannot open). VST3: `IPlugFrame`, `IRunLoop` (Linux), `IComponentHandler` (GUI edits reach the processor and the project, one undo step per knob turn), host-created `IMessage` / `IAttributeList`, content scale for HiDPI. CLAP: `set_parent`, `set_scale`, timer / fd host extensions, GUI edits from output events. Tested on Linux and Windows CI with real windows, and with 40 real third-party builds (Dragonfly Reverb, DISTRHO collection incl. the OpenGL ProM visualiser; VST3 and CLAP): all 28 with a GUI embed, the 10 without one fall back to generic controls – re-run with `DRAGON_THIRD_PARTY=<folders> cargo test -p dragon-test-plugins --test third_party -- --ignored --nocapture`. CLAP plugins shipped as folders (`Name.clap/Name.clap`) load. macOS ❌ |
| Sidechain into plugins | ✅ | CLAP second input port, VST3 aux input bus |
| Clip launcher / performance mode | ✅ | View › Performance (F12): tracks × patterns, launch quantised to bar / beat, stop lanes, `LiveEvent::Launch` in the renderer |
| Audio-to-MIDI | ✅ | Tools › Convert selected clip to MIDI (melody: YIN pitch tracking; drums: onsets sorted kick / snare / hat) |
| Stem separation | ✅ (DSP, not AI) | Tools › Separate: harmonic/percussive median filtering + centre extraction → drums, harmonic, centre stems |
| Control surfaces (MCU protocol) | ✅ | Options › Control surface: faders, V-pots, mute / solo / arm / select, transport, banks; motor faders and LEDs |
| Video track | ✅ (needs ffmpeg) | File › Import video, View › Video (frame at the playhead with timecode), soundtrack as a clip, export the video with the mix |
| Surround 5.1 / 7.1 | ✅ (export) | Project › Surround; per-channel XY panner with spread and LFE send (2D VBAP); multichannel WAV export (`surround.rs`). Monitoring is stereo; master inserts are not applied to the surround mix |
| Dolby Atmos (objects, ADM BWF) | ✅ | 7.1.4 bed with height panning; every top-level channel exported as two positioned objects in an ADM BWF (BS.2076 axml + BS.2088 chna) for the Dolby Atmos Renderer and other ADM tools. No in-app Atmos binaural monitoring (Dolby-licensed); files over 4 GB (BW64) not supported |
| Start screen: make a beat, record, mix, master, stem master, album master, film sound design | ✅ | `modes.rs`: each builds a fitting project and opens its views; File › Start screen…; Options › Show this at start |
| Album master | ✅ | Mastering tab (album projects): songs one after another on their own channels through an album bus, group buses, "Match loudness" (one offline measurement, every song to the median loudness before the shared chain) – `views/album.rs` |
| Stem master | ✅ | start screen: Drums, Melody, All vocals, SFX group buses and a mastering chain |
| Master chain in the Mastering tab | ✅ | signal flow (IN → effects with their gain reduction → OUT with true peak; click opens, the switch bypasses) and the full slot list |
| Audio Repair | ✅ | double-click an audio clip › Audio repair…: spectrogram with time and time × frequency selection; de-noise (learned profile, decision-directed Wiener), de-hum (auto 50 / 60 Hz, zero-phase notches), de-click (LPC interpolation), de-clip, distortion repair (soft-clipped peaks), de-reverb (Lebart), spectral attenuate, gain; every repair renders to a new file, Ctrl+Z goes back – `repair.rs`, `views/repair.rs` |
| Note editing  | ✅ (monophonic) | Audio repair › Notes: detected notes as blobs with pitch curves, drag to move (semitones / free), pitch centre, flatten drift and vibrato; Timing tool and Shape controls move a note in time, change its length (the gaps around it stretch) and shift its formants, by offline pitch-synchronous resynthesis (`repair::placement`, `resynth_note`). Polyphonic editing is not there |
| Hammerstein graph of an effect | ✅ | every effect editor › Hammerstein: kernels G1–G5, THD and the static curve from a synchronised sweep (Novak), re-measured on every change; time-varying and adaptive effects are flagged – `hammerstein.rs` |
| Drag a cut clip into a new grouped track | ✅ | drag clips below the last track: a new track grouped with theirs, same mixer channel; grouped tracks mute / solo together |
| Hint bar (control and value at the top left) | ✅ | stock panels and displays, plugin parameter lists, plugins' own windows, mixer faders |

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
