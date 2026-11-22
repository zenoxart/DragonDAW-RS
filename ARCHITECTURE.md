# Dragon DAW – architecture

## Principles

1. **The project is plain data.** `dragon_engine::model::Project` is a serde-serialisable tree: tracks with
   clips and take lanes, rack channels with their instruments, patterns with steps and notes, the mixer and
   automation lanes. It holds no handles, threads or caches. That makes undo a snapshot, makes saving a
   `serde_json` call and keeps every editor a plain function of the model.
2. **Compile, then render.** `Song::compile(&Project, …)` turns the model into an immutable `Song`. That
   includes the mixer strips in topological order (outputs, sends and sidechains), delay lines for plugin delay
   compensation, a tempo map, sample-accurate pattern events, audio clips with resolved timing and
   time-stretched audio. The audio thread receives an `Arc<Song>` and swaps it between blocks, so the UI never
   locks the audio thread.
3. **One renderer.** `Renderer` renders blocks of 128 frames (`AUTOMATION_BLOCK`). It is used for playback,
   export, stems, bounce-in-place and the CLI, so what you hear is what you export.
4. **Processors outlive compiles.** The `Pool` keeps effect, instrument and plugin instances keyed by model id
   (and by kind, so replacing an effect makes a new instance). Recompiling after a knob turn keeps reverb
   tails, voices and loaded plugins.
5. **Errors are values.** The engine returns `Result<T, EngineError>`; the shared crates keep Red Hive's
   `Res<T>` / `Error` for the framework.

## Crates

| Crate | Role |
|---|---|
| `dragon-engine` | Everything audio. No UI dependency. Features: `device` (cpal + midir), `plugins` (libloading + clap-sys + vst3). |
| `dragon-daw` | The egui app on top of the Red Hive framework. |
| `dragon-cli` | `dragon` tool: render, loudness, analyse, isolated plugin scans. |
| `dragon-test-plugins` | A CLAP gain effect, a CLAP sine instrument and a VST3 gain effect, built as one `cdylib` for the host tests. |
| `redhive-client-framework` | Docking, command pool, shortcuts, palette search, navigation, i18n, widgets. |
| `redhive-contracts`, `redhive-monads` | Command / shortcut / UI primitives; `Res`, `Error`, `Validator`. |

## Engine modules

| Module | Content |
|---|---|
| `model` | Project, tracks, clips, takes, rack channels, instruments, patterns, notes (96 PPQ), mixer, automation, MIDI mappings |
| `time` | Tempo map with tempo changes (beats ↔ seconds ↔ frames), bar/beat and timecode formatting |
| `samples` | Decoding (symphonia), resampling (rubato), a shared sample cache, waveform peaks, WAV writing with TPDF dither |
| `dsp` | dB helpers, pan law, smoothers, biquads, SVF, DAHDSR envelopes, delay lines, LFOs, envelope followers, BS.1770 loudness and true peak, WSOLA time-stretch / pitch-shift |
| `effects` | The 20 stock effects behind one `Effect` trait (parameters, latency, gain reduction, tempo, sidechain, plugin editor idle); `effects::pitch` has YIN pitch detection, the granular pitch shifter and Dragon Tune |
| `instruments` | Sampler, synth and drum pads behind `InstrumentVoice`, plus the built-in Dragon Kit |
| `render` | `Pool`, `Song::compile`, `Renderer`, meters, live events (notes, previews, clip-launcher launches), the offline renderer |
| `edit` | Split, duplicate, delete, shuffle, normalise, takes and comping, scales, quantize, legato, transpose, chords, progressions, arpeggiator, strum, flam, humanize, steps → notes, slicing to the rack |
| `history` | Snapshot undo / redo with labels and a dirty flag |
| `project_io` | Save (relative paths, numbered backups), load and repair, missing files, collect files, autosave with crash detection, recent projects |
| `export` | Mix, stems, WAV / FLAC / MP3 / Ogg, loudness / peak normalisation, bounce-in-place, all with a plugin-aware pool |
| `midi` | MIDI message parsing, CC mapping, SMF import / export, the retro-capture buffer, MCU decoding and feedback messages |
| `analysis` | BPM (onset autocorrelation) and key (chroma + key profiles) detection, audio-to-MIDI, HPSS + centre stem separation |
| `device` | Output stream with a command channel, input capture into a recorder thread, recorded takes with latency, shared transport, MIDI inputs |
| `plugins` | Folder scan, isolated scan, CLAP host, VST3 host, native editor windows (`window`), `Host` as the pool's plugin factory |
| `surround` | 5.1 / 7.1 / 7.1.4 speaker layouts, VBAP with a height layer, the surround mix from per-channel stems, multichannel WAV, ADM BWF (Atmos objects) |
| `video` | ffprobe / ffmpeg wrappers: probe, frame at a time, soundtrack extraction, muxing the mix into the video |

## Real-time path

```text
UI thread                                   audio thread (cpal callback)
─────────                                   ────────────────────────────
edit ─► Session (undo snapshot)
       └─► Song::compile ─► Arc<Song> ──►   Command::SetSong ─► Renderer::set_song
transport buttons ───────────────────►      Command::Play / Pause / Seek / Record
typing keyboard / MIDI ──────────────►      Command::Live(NoteOn …)
                                            Renderer::render(block)
meters / transport ◄─────────── Mutex snapshots (Meters, SharedTransport)
recorder thread ◄────────────── input blocks (armed tracks) ─► WAV files ─► takes
```

The app recompiles at most every 40 ms while you drag a control, and right away otherwise. Plugin parameters
edited in the host are re-applied on every compile. Plugin state is captured into the model on save and export.

## Plugin hosting

- **CLAP**: `clap_entry` → plugin factory → `create_plugin` / `init` / `activate`. Audio ports, params
  (plain ↔ normalised), latency, state and GUI extensions. Notes and parameter values go out as input events.
  The GUI is embedded into a native host window (`set_parent`); the host implements the gui, timer-support and
  posix-fd-support extensions and services them from the UI thread. Floating windows are the fallback.
- **VST3**: `GetPluginFactory` → `IComponent` + `IAudioProcessor`. The `IEditController` is either the same
  object or a separate class, connected through `IConnectionPoint`. The host implements `IHostApplication`,
  `IParameterChanges` / `IParamValueQueue`, `IEventList` and an `IBStream` memory stream. Component and
  controller state are packed into one blob in the project. Editors attach to a native host window through an
  `IPlugFrame` that also implements the Linux `IRunLoop` (fd handlers and timers, serviced from the UI thread).
- **Sidechain**: a second CLAP input port or a VST3 aux input bus receives the insert's sidechain signal.
- **Scanning** runs `dragon scan-plugin` in a child process per file, with a timeout. Loaded libraries are never
  unloaded while the app runs.

## The desktop shell

The Red Hive framework supplies the parts every desktop app needs:

- the `CommandPool`: every action is a command, and the menus and the palette are generated from it,
- the `ShortcutManager`: layered, chorded and rebindable, with reserved shell gestures,
- the `DockManager`: egui_dock with a saved layout,
- `Paths`: config, local data, projects and autosave folders,
- toasts, the log and i18n.

Dragon DAW registers its commands in `crates/app/src/commands.rs` and its views in `crates/app/src/views`.
