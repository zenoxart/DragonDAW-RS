<div align="center">

<img src="docs/images/dragon-logo.png" width="200" alt="Dragon DAW logo" />

# Dragon DAW

**A free, offline digital audio workstation written in Rust: make beats, record, repair, mix and master.
It's built on the Red Hive desktop framework and hosts CLAP and VST3 plugins.**

[![CI](https://github.com/zenoxart/DragonDAW-RS/actions/workflows/ci.yml/badge.svg)](https://github.com/zenoxart/DragonDAW-RS/actions/workflows/ci.yml)
[![Release](https://github.com/zenoxart/DragonDAW-RS/actions/workflows/release.yml/badge.svg)](https://github.com/zenoxart/DragonDAW-RS/actions/workflows/release.yml)
[![Nightly](https://img.shields.io/badge/download-nightly-E84A2C?logo=github)](https://github.com/zenoxart/DragonDAW-RS/releases/tag/nightly)
[![Rust](https://img.shields.io/badge/Rust-1.89%2B-B7410E?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![egui](https://img.shields.io/badge/egui-0.31-C62828)](https://github.com/emilk/egui)
[![CLAP](https://img.shields.io/badge/plugins-CLAP%20%7C%20VST3-E84A2C)](#-plugins)
[![Effects](https://img.shields.io/badge/stock%20effects-27-FF9A3C)](#-features)
[![Offline](https://img.shields.io/badge/offline-no%20server%2C%20no%20account-2E7D32)](#-features)
[![Platforms](https://img.shields.io/badge/Windows%20%C2%B7%20macOS%20%C2%B7%20Linux-555?logo=windows&logoColor=white)](#-quick-start)
[![License](https://img.shields.io/badge/License-Apache%202.0-E53935)](LICENSE)

[✨ Features](#-features) ·
[📸 Screenshots](#-screenshots) ·
[🚀 Quick start](#-quick-start) ·
[🏗️ Architecture](#%EF%B8%8F-architecture) ·
[🔌 Plugins](#-plugins) ·
[⌨️ Command line](#%EF%B8%8F-command-line) ·
[📋 Feature coverage](docs/FEATURES.md)

<br/>

<img src="docs/images/demo.gif" width="760" alt="Dragon DAW in action: playlist, channel rack, piano roll, mixer, effects, audio repair, pitch correction and mastering" />

</div>

---

Dragon DAW is a desktop DAW that runs entirely on your machine, with no server, no account and no network
(except an optional check for new versions). It brings together:

- 🥁 a **pattern-based beat workflow**: channel rack, step sequencer, pattern clips and piano roll;
- 🎙️ **Studio-grade recording and editing**: recording channels, count-in, punch, takes and comping, a sample-accurate playlist;
- 🩹 **Spectral audio repair and note-based pitch correction** built in;
- 🎛️ a **mixer with routing cables**, sends, sidechains, VCAs and plugin delay compensation;
- 🏁 **mastering**: BS.1770 loudness, a live 3D spectrum, the master chain, album and stem mastering, loudness-normalised export.

When it opens, it asks what you want to do (make a beat, record, mix, master, stem or album master, film sound)
and sets up the project and the views for it. Guided tours explain the app and how to record properly.

This repository started as the **Red Hive** framework. The enterprise parts (backend services, sign-in, users,
documents, invoices, reports, workflows and module hot-swap) have been removed. The desktop framework is kept:
docking, command pool, shortcuts, palette, navigation and i18n. Dragon DAW is now the one app it hosts.

## ✨ Features

| | |
|---|---|
| 🚀 **Start screen** | "What do you want to do?" – **Make a beat, Record, Mix, Master, Stem master, Album master or Film sound design**: each sets up the tracks, buses, chains and views for it. Recent projects, an empty project or the tour are one click away |
| 🌊 **Transmuter synth** | A wavetable synth built in: 3 wavetable oscillators (12 factory tables, or drop any audio file to make your own), up to 16-voice unison with detune / blend / stereo width, 8 warp modes (sync, bend, PWM, FM, fold, crush …), sub and noise oscillators, 2 filters (low / high / band pass, notch, comb, formant) in series or parallel, 4 envelopes, 4 LFOs (tempo sync, one-shot), a mod matrix, 4 macros, a 12-slot FX rack, mono / legato / glide. **Drag-and-drop modulation** (drag an envelope, LFO, macro, velocity or note onto any knob), a playable keyboard, and **2,167 presets** (bass, lead, pad, pluck, keys, bells, arps, stabs, atmospheres, FX, drums) at an even level, with a browser, search, favourites and your own presets — all automatable |
| 🎹 **Emberkeys** | A modelled piano – no samples: two or three detuned strings per key with real string stiffness (the upper partials run sharp, as on a piano), felt hammers that get brighter the harder you play, dampers, **sustain, sostenuto and soft pedal** (CC64/66/67), sympathetic resonance with the pedal down, stretch tuning and a soundboard. Ten instruments: concert, studio and bright grands, upright, felt piano, honky-tonk, tine and reed electric pianos, toy piano and celesta. An **arpeggiator**, velocity range, swell, and **2,010 presets** (concert, pop, upright, felt, cinematic, jazz, saloon, lo-fi, electric, celesta, arpeggios) at an even level. Artwork generated with Higgsfield |
| 🎸 **Drakestring** | A modelled plucked-string instrument: steel acoustic, nylon, 12-string, electric (pickup + amp), bass, banjo, mandolin, ukulele, sitar and harp. One note per string like a real player, **Solo / Strum / Chord / Pattern** modes (play one key for a full guitar chord; hold a chord for a rhythm in time with the song), palm mute, harmonics, dead notes and slides on key switches, alternate tunings and capo, a body or amp per instrument, the FX rack, and **2,010 presets** at an even level with a browser. The neck shows where every note is fretted; artwork generated with Higgsfield |
| 🥁 **Make beats** | Channel rack with step sequencer (per-step velocity, swing, up to 64 steps), **Fill** a channel every step, every 2 / 3 / 4 / 8 steps, off-beats or backbeat (plus invert, shift and clear), drop a sample on a channel to replace its sound, patterns as clips with **clone / make unique / split by channel / split at a bar**, pattern / song mode, a built-in synthesised **Dragon Kit** so a fresh install makes sound with no sample library, sampler (slices, glide, a **volume envelope** drawn over the waveform), subtractive synth (3 oscillators, unison, SVF filter, envelopes, LFO), drum pads with velocity layers and choke groups |
| 🎹 **Piano roll** | Big, labelled tool buttons (draw, paint, select, slice, delete, mute, slip, play, zoom), drag notes (Shift = copy), arrow-key nudge, snap, **key + scale** (26 scales) with snap-to-scale, chord stamps and progressions, arpeggiator, strum, flam, quantize, legato, humanize, slide notes, velocity lane, ghost notes, typing keyboard, retro MIDI capture |
| 🎙️ **Record** | **Recording channels in the mixer** (`+ record`): pick the interface **input** (mono 1, 2 … or stereo 1+2 …), **monitoring** (Auto / Always / Off) and the channel it records **into**; arm from the strip; metronome with count-in, loop and punch, automatic latency compensation, takes in lanes with **comping**, MIDI note recording. **Help › How to record** walks you through interface, input, level, monitoring and the take |
| ✂️ **Arrange & edit** | Playlist with audio, pattern and **automation clips**, a **picker panel**, **fade handles on every audio clip** (drag the top corners; the curve, the faded part and the waveform follow, the length shows in ms), clip gain handle, **compact mode** for many tracks, Ctrl + right-drag zoom to area, multi-select, Shift+drag to clone, a **slice tool** across tracks, grouped clips to new tracks (same mixer channel), reverse, pitch, varispeed and **time-stretch** (WSOLA), fit-to-tempo, markers, tempo and signature changes, unlimited undo with a history list |
| 🔊 **Audio clip window** | Double-click an audio clip: waveform with fades, play from any point, normalize / reverse / fit tempo, level, fades with three shapes, pitch and time, and big **Pitch correction** and **Audio repair** buttons |
| 🩹 **Audio repair** | Spectrogram you can zoom and select in (time range, or a time × frequency box), **"Check the recording"** finds hum, clicks, clipping and background noise and fixes each with one click; **de-noise** (learn the noise), **de-hum**, **de-click**, **de-clip**, **distortion repair**, **de-reverb**, **spectral attenuate**, gain – each with **Preview** before **Apply**, A/B against the original, Ctrl+Z to undo |
| 🎯 **Pitch correction**  | Detected notes as blobs with their pitch curves, **move** (semitone snap, Alt = free), **vibrato** (scale a note's drift 0–200 %), **split**, rubber-band and Ctrl+A selection, arrows / Ctrl+arrows / Alt+arrows for semitone / octave / 10 cents, **Correct pitch** macro, zoom in time and pitch, its own undo |
| 🎛️ **Mix** | Mixer with **hanging routing cables** (only for the selected channel), route arrows and send knobs, **large effect slots** (drag to reorder, bypass, mix, gain-reduction), pre/post-fader sends, buses, sidechains, VCAs, width, polarity, solo-safe, **compact strips**, **auto-balance** (by role under the lead, with headroom into the master and a correctable report), **plugin delay compensation**, automation lanes (read / write / touch / latch / trim), MIDI learn |
| 🧪 **Stock effects** | **28 effects, each built like an analog rack unit**: a faceplate in its own finish (brushed aluminium, black anodised, console enamel, cream valve enamel, gold, oxblood crackle, amethyst cave rock, wet slate, copper), rack ears with screws, photographed knob caps, a pilot lamp, a glowing valve on the valve units and **VU meters whose needles swing with the audio** (gain reduction and output on the dynamics). The modulation units add a lit emblem and a phosphor oscilloscope: Coven's three candle flames sway as its chorus voices, Quicksilver's mercury column rides the flanger's delay over its comb filter, Moonphase's moon runs through its phases with the phaser's sweep, Wingbeat's dragon wings beat with the tremolo, and Ouroboros pans inside a circling serpent. Every other unit's display is a phosphor screen in its finish's colour (green on the metal units, amber on the valve, console, gold and copper ones, violet in Dragon's Lair, teal in Echo Cavern), and its nameplate carries an animated emblem: Prism's crystal glints, Alembic and Phial bubble, Dragontamer's eye burns with gain reduction, Portcullis lifts as the gate opens, Dragonforge's anvil and Crucible throw sparks, Golem Strip's eyes glow with the level, and so on. Golem Strip, Dragonforge, Purify and Philosopher's Stone are the heavy, gnostic flagships: carved basalt, forged iron, lapis and obsidian plates with gold inlay and filigree, brass knobs, black-glass meters lit gold, an alchemical motto on each (EMETH, IGNE NATURA RENOVATUR INTEGRA, LUX IN TENEBRIS LUCET, SOLVE ET COAGULA) and a sacred-geometry sigil turning slowly behind the display. Their stages are laid out as a numbered signal chain (switches on top, the main control large, the rest on an even grid). Philosopher's Stone is worked by turning a real 3D stone (drag it round: one turn is 100 %), which glows brighter the further it turns; Purify shows a power valve whose heaters follow the input, two antique gain-reduction gauges, six phials filling with each stage's reduction and the real input and output levels; Golem Strip and Dragonforge read their gain reduction and levels on carved-stone and forged-iron gauges and phials of molten metal. Dragon's Lair and Echo Cavern are caves painted over Higgsfield scenes, and every graph in them is a control: in the lair, drag the PRE diamond (pre-delay), the EARLY bead (early level and spread), the LATE bead (tail level) and the DECAY bead along the glowing tail, move the dragon's eye across the chamber map (size and width) and shape the decay-rate EQ; in the cavern, each echo is a stalactite reaching down as far as it is loud, with drops falling into the water as the echoes sound: drag TIME (snapping to note values when synced), FEEDBACK and R OFFSET on the stalactites, and LOW CUT / HIGH CUT · RESO on the loop filter. Double-click a handle to reset it. Crucible is an alchemist's cauldron: three jars of ingredients, parallel tube saturations (a warm 6SN7, a hot 12AX7 and a crisp EF86 pentode), pour into the brew as far as their knobs go, the liquid takes their blended colour and boils with the real input level, the fire burns with the drive, and the Mix knob fills a phial from the cauldron; drag the jars, the cauldron (drive) and the phial directly, and watch the brew's transfer curve on its amber screen. All readings are live: with no audio they rest. All artwork generated with Higgsfield, animated in code. Each has engraved sections, a live display that's also a control, A/B, presets, full screen and a **hint bar** showing what you tweak, named after dragons and alchemy: **Prism** 24-band EQ, **Jadefang** character EQ (six voicings from clean to iron, valve and passive, a driven input, very broad Sub and boost-only Air bands; laid out like a classic channel-strip EQ module: backlit voicing keys and a big input knob beside a row of gain and frequency knobs per band), **Quintessence** 4-band multiband dynamics, **Obsidian** clipper, **Dragontamer** compressor with eight styles, **Lodestone** limiter, **Serpent's Hush** de-esser, **Portcullis** gate, **Talon** transient designer, **Crucible** saturator, **Athanor** multiband saturator, **Dragon's Lair** reverb, **Echo Cavern** delay, **Coven** chorus, **Quicksilver** flanger, **Moonphase** phaser, **Wingbeat** tremolo, **Ouroboros** auto-panner, **Wingspan** stereo imager, **Alembic** filter, **Corroder** bit crusher, **Phial** utility, **Shapeshifter** pitch & formant, **Aurum Tune** real-time pitch correction, **Golem Strip**, **Dragonforge** drum bus, **Purify** voice cleanup and **Philosopher's Stone** one-knob mastering. Every effect has a **Hammerstein tab**: the harmonic distortion its current settings produce, measured live |
| 🏁 **Master** | Momentary / short-term / integrated LUFS and true peak (ITU-R BS.1770), streaming / broadcast / club targets, a **real-time 3D heat-map spectrum** (frequency × level × the last 5 seconds), the **master chain** shown and edited as a signal flow, a **reference track** comparison, **album master** (matched loudness across songs) and **stem master** (group buses) |
| 💾 **Export** | A clear export dialog: WAV 16 / 24 / 32-float, FLAC, MP3, Ogg Vorbis, 44.1–96 kHz, song / loop / pattern, tail, dither, mono, **stems**, normalise to LUFS or peak, bounce-in-place, MIDI export |
| 🔌 **Plugins** | CLAP and VST3 instruments and effects, **crash-isolated scanning**, plugin GUIs embedded in native windows that resize and go full screen, sidechain, full plugin state saved, latency reported to PDC |
| 🎛️ **Performance & hardware** | Clip launcher (tracks × patterns, launch on the bar), MCU control surfaces with motor faders and LEDs |
| 🔊 **Surround & Atmos** | 5.1, 7.1 and 7.1.4 mixes with a surround panner, multichannel WAV export and Dolby Atmos ADM BWF |
| 🎬 **Video** | Import a video, watch it in sync with timecode, its soundtrack on a track, export the video with your mix. **ffmpeg** comes with the Windows download, or *Get ffmpeg* installs it |
| 🎓 **Tours & quests** | Click-through **tours** over the real interface (the basics, and how to record) and optional **Dragon Quests**: quests that teach the workflow, XP and dragon levels, a daily streak and celebrations (off with *Reduce motion*) |
| 🔄 **Updates** | Tells you when a new version is out, with **Download** and – on Windows – **Install and restart** (Help › Check for updates; can be turned off) |
| 🛟 **Safety** | Project folders with `Samples/`, `Recordings/`, `Renders/` and `Backup/`, autosave with crash recovery, numbered backups, missing-file relinking, crash reports and **safe mode** for projects with a crashing plugin |
| 🪟 **Desktop shell** | VS-style docking (tabs, splits, floating windows, saved layout), menus, a command palette (`Ctrl+Shift+P`), rebindable shortcuts, drag & drop from the file manager, the Dragon logo, icon and startup sound |

The full list, compared item by item with the prioritised roadmap, is in **[docs/FEATURES.md](docs/FEATURES.md)**.

## 📸 Screenshots

> Real screenshots of the app on Linux.

### 🚀 Start and make beats

<table>
  <tr>
    <td width="50%"><img src="docs/images/start-screen.png" alt="Start screen: what do you want to do?" /></td>
    <td width="50%"><img src="docs/images/playlist-fades.png" alt="Playlist with pattern clips and audio clips with fades" /></td>
  </tr>
  <tr>
    <td align="center"><b>Start screen</b> — pick a beat, a recording, a mix, a master …</td>
    <td align="center"><b>Playlist</b> — big tool buttons, audio clips with draggable fades</td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/images/rack-fill.png" alt="Channel rack with the Fill menu" /></td>
    <td width="50%"><img src="docs/images/screenshot-piano-roll.png" alt="Piano roll with a chord progression" /></td>
  </tr>
  <tr>
    <td align="center"><b>Channel rack</b> — fill every 2, 4, 8 steps, off-beats, backbeat</td>
    <td align="center"><b>Piano roll</b> — scales, chord stamps and progressions</td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/images/dragon-wave.png" alt="Transmuter wavetable synth: three oscillators with 3D wavetable views, sub, noise and two filters" /></td>
    <td width="50%"><img src="docs/images/dragon-wave-mod.png" alt="Transmuter preset browser with categories, search and favourites" /></td>
  </tr>
  <tr>
    <td align="center"><b>Transmuter</b> — oscillators, filters, envelopes and LFOs on one page; drag a mod source onto any knob</td>
    <td align="center"><b>Preset browser</b> — 2,167 presets in 12 categories, search, favourites, your own presets</td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/images/dragon-strings.png" alt="Drakestring: a guitar with the played chord lit on the neck, a leather knob strip and a keyboard with key switches" /></td>
    <td width="50%"><img src="docs/images/dragon-strings-strummer.png" alt="Drakestring strummer: play modes and 16 rhythm patterns" /></td>
  </tr>
  <tr>
    <td align="center"><b>Drakestring</b> — modelled guitars; the notes light up where they're fretted</td>
    <td align="center"><b>Strummer</b> — chords voiced like a guitarist, strummed or played in rhythm patterns</td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/images/emberkeys.png" alt="Emberkeys: a gold title over a painted stage with a piano and a dragon in the clouds, gold knobs on black lacquer and 88 keys" /></td>
    <td width="50%"><img src="docs/images/emberkeys-arp.png" alt="Emberkeys arpeggiator: patterns, rates, octaves, gate, swing and accent" /></td>
  </tr>
  <tr>
    <td align="center"><b>Emberkeys</b> — a modelled piano: stiff strings, felt hammers, dampers, pedals and soundboard</td>
    <td align="center"><b>Arpeggiator</b> — six patterns in time with the song</td>
  </tr>
</table>

### 🎙️ Record, repair and tune

<table>
  <tr>
    <td width="50%"><img src="docs/images/mixer-recording.png" alt="Mixer with a recording channel" /></td>
    <td width="50%"><img src="docs/images/recording-tour.png" alt="The recording tour" /></td>
  </tr>
  <tr>
    <td align="center"><b>Recording channels</b> — input, monitoring, where it records into</td>
    <td align="center"><b>How to record</b> — a guided tour over the real controls</td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/images/audio-repair.png" alt="Audio repair: spectrogram, check the recording, repair tools" /></td>
    <td width="50%"><img src="docs/images/pitch-correction.png" alt="Pitch correction with note blobs" /></td>
  </tr>
  <tr>
    <td align="center"><b>Audio repair</b> — finds hum, clicks, clipping and noise; fix with one click</td>
    <td align="center"><b>Pitch correction</b> — move notes, calm the vibrato, split</td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/images/clip-window.png" alt="Audio clip window" /></td>
    <td width="50%"><img src="docs/images/compact-mixer.png" alt="Compact mixer strips" /></td>
  </tr>
  <tr>
    <td align="center"><b>Audio clip window</b> — fades, level, pitch and the repair tools</td>
    <td align="center"><b>Compact mode</b> — every channel on one screen</td>
  </tr>
</table>

### 🎛️ Mix and master

<table>
  <tr>
    <td width="50%"><img src="docs/images/multiband.png" alt="4-band multiband dynamics device" /></td>
    <td width="50%"><img src="docs/images/hammerstein.png" alt="Hammerstein tab: harmonic distortion of the current settings" /></td>
  </tr>
  <tr>
    <td align="center"><b>Multiband dynamics</b> — four bands, one column each</td>
    <td align="center"><b>Hammerstein tab</b> — the harmonics each effect adds</td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/images/mastering-3d.png" alt="Mastering with the 3D heat-map spectrum and the master chain" /></td>
    <td width="50%"><img src="docs/images/mixer-cables.png" alt="Mixer routing cables and large effect slots" /></td>
  </tr>
  <tr>
    <td align="center"><b>Mastering</b> — LUFS, 3D heat-map spectrum, master chain</td>
    <td align="center"><b>Routing cables</b> — drag a cable between strips</td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/images/effect-gallery.png" alt="Effect gallery: EQ, dynamics, reverb, delay" /></td>
    <td width="50%"><img src="docs/images/level-up.png" alt="Quest celebration" /></td>
  </tr>
  <tr>
    <td align="center"><b>Effect gallery</b> — every stock effect with its live display</td>
    <td align="center"><b>Dragon Quests</b> — quest celebration with embers and XP bar</td>
  </tr>
</table>

The look of the effect editors and the quest celebration follows design references generated with
**Higgsfield AI**: an [effect-suite UI sheet](docs/images/design/higgsfield-effect-suite-reference.jpg) and an
[animated level-up GIF](docs/images/design/higgsfield-levelup-reference.jpg). Both are re-created procedurally in
egui so they scale, animate live and respect *Reduce motion*.

## 🚀 Quick start

**📦 Download a build:** the [latest nightly](https://github.com/zenoxart/DragonDAW-RS/releases/tag/nightly)
(built from `main` after every green CI run) or a [versioned release](https://github.com/zenoxart/DragonDAW-RS/releases)
– 🪟 Windows x64, 🐧 Linux x64, 🍎 macOS Apple Silicon and Intel. Unzip it to a folder of your own (not
*Program Files*) so the built-in updater can replace it. The Windows zip includes ffmpeg for video.

**🛠️ Or build it yourself:**

```bash
# Linux: ALSA headers for audio and MIDI
sudo apt install libasound2-dev

cargo run --release -p dragon-daw            # the DAW
cargo run --release -p dragon-daw -- song.dragon
cargo build --release -p dragon-cli          # the `dragon` tool (put it next to the app for isolated plugin scans)
```

Optional on Linux / macOS: install **ffmpeg** (with `ffprobe`) for the video features.

**🛟 If a project crashes the app** (usually a third-party plugin): the next start says which plugin was
loading and offers safe mode, with plugins off or without their saved settings. `dragon-daw --safe song.dragon`
does the same. Crash reports go to `%LOCALAPPDATA%\DragonDAW\Crash` on Windows.

### ⌨️ Useful keys

| Key | Action | Key | Action |
|---|---|---|---|
| `Space` | play / pause | `F5` `F6` `F7` `F9` | playlist, channel rack, piano roll, mixer |
| `R` | record | `L` | pattern / song mode |
| `Ctrl+Z` / `Ctrl+Y` | undo / redo | `Ctrl+T` | typing keyboard to piano |
| `Ctrl+S` | save | `Ctrl+R` | export |
| `Ctrl+Shift+P` | command palette | `F8` / `F10` | effect gallery / options |
| `Ctrl` + right-drag | zoom into the box | `Ctrl` + right-click | zoom out |
| middle-drag | pan | `Alt` + wheel | track / key height |
| `←` `→` `↑` `↓` | nudge selected notes (`Shift` = octave) | `Shift` + drag | copy notes |
| `Ctrl+A` in Audio repair | select all notes / the whole clip | `↑` `↓` in Pitch correction | move notes a semitone |

Every shortcut can be changed in **Options › Keyboard shortcuts**. **Help** has *Take the tour*,
*How to record* and *Check for updates*.

## 🏗️ Architecture

```mermaid
flowchart LR
    subgraph App["🪟 dragon-daw (egui)"]
        Views["Views<br/><sub>playlist · rack · piano roll · mixer · mastering · audio repair</sub>"]
        Session["Session<br/><sub>project · undo · sample cache · plugin pool</sub>"]
        Shell["Red Hive framework<br/><sub>docking · commands · shortcuts · palette</sub>"]
    end

    subgraph Engine["🐉 dragon-engine"]
        Model["model<br/><sub>plain-data project</sub>"]
        Compile["Song::compile<br/><sub>routing graph · PDC · events</sub>"]
        Render["Renderer<br/><sub>128-frame blocks</sub>"]
        DSP["effects · instruments · dsp"]
        Repair["repair<br/><sub>STFT de-noise · de-hum · de-click · notes</sub>"]
        Plug["plugins<br/><sub>CLAP · VST3</sub>"]
        IO["device<br/><sub>cpal · midir · recorder</sub>"]
        Exp["export · project_io · midi · analysis · video"]
    end

    Views -->|"edits"| Session
    Session -->|"Arc&lt;Song&gt;"| IO
    Session --> Compile
    Compile --> Render
    Render --> DSP & Plug
    Views --> Repair
    IO -->|"audio thread"| Render
    Exp --> Render
    CLI["dragon CLI"] --> Exp & Plug
```

- 📄 **The project is plain data.** Every edit snapshots the project for undo, and the session compiles it into an
  immutable `Song` (routing order, delay lines for plugin delay compensation, pattern events, clip timing). The
  audio thread swaps it in without locking the UI.
- 🔁 **One renderer for everything.** Real-time playback, export, stems, bounce-in-place and the CLI all run the
  same `Renderer`, so what you hear is what you export.
- 🧠 **Processors survive recompiles.** A pool keeps effect, instrument and plugin instances by id. Turning a knob
  recompiles the song but doesn't reset reverb tails or reload plugins.
- 🩹 **Repairs are non-destructive.** Audio repair and pitch correction render to a new file next to the
  recordings and point the clip at it; undo points it back.
- 🔇 **No device? Keep working.** If no audio device can be opened, a silent clock drives the same renderer. The
  transport, meters, editing and export all keep working.

More detail is in [ARCHITECTURE.md](ARCHITECTURE.md).

## 🔌 Plugins

Dragon DAW looks for plugins in the standard CLAP and VST3 folders of each platform, plus any folders you add in
**View › Plugin manager**. Scanning runs `dragon scan-plugin <format> <path>` in a child process for each file,
so a plugin that crashes while loading is just skipped. Instruments go into the channel rack; effects go into
any mixer insert slot. The host:

- 🎵 sends notes and sample-accurate parameter events,
- ⏱️ reports the plugin's latency to delay compensation,
- 💾 stores the plugin's full state in the project,
- 🔀 passes a sidechain signal (CLAP second input port, VST3 aux bus),
- 🪟 embeds the plugin's own GUI in a native window (X11 on Linux, Win32 on Windows; macOS falls back to CLAP
  floating windows and the generic editor).

The host is tested against small CLAP and VST3 plugins in [`crates/test-plugins`](crates/test-plugins): a gain
effect and a sine instrument written for this repository.

## ⌨️ Command line

```text
dragon render <project> <out.wav|out.flac|out.mp3|out.ogg|folder> [--format wav16|wav24|wav32|flac16|flac24|mp3|ogg]
              [--rate 48000] [--lufs -14] [--peak -1] [--stems] [--mono] [--no-plugins]
dragon info <project>          tracks, patterns, length, missing files
dragon new <project>           writes an empty project
dragon loudness <audio>        integrated LUFS, true peak, sample peak
dragon analyze <audio>         BPM and key detection
dragon scan [folder …]         plugin scan, each file in its own process
```

## 📁 Repository layout

```text
crates/
├── engine/             dragon-engine: model, renderer, DSP, effects, instruments, repair, plugins, device I/O, export
├── app/                dragon-daw: the desktop app (views, session, audio backend, tours, updater)
├── test-plugins/       CLAP + VST3 plugins used by the host tests
├── client/framework/   Red Hive desktop framework (docking, commands, shortcuts, i18n, widgets)
└── shared/
    ├── contracts/      command, shortcut and UI primitives
    └── monads/         Res<T>, Error, Validator
tools/dragon-cli/       the `dragon` command-line tool
docs/                   feature coverage, images
```

## 🧪 Development

```bash
cargo test --workspace                 # engine, host-with-real-plugins, framework and app tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all                        # rustfmt.toml: 220 columns
```

The engine tests render audio and assert on the result: sample-accurate clip and step timing, swing, routing and
solo, plugin delay compensation, automation, effect stability, limiter ceiling, LUFS, time-stretch, export, MIDI
round trips, BPM and key detection, comping, autosave, and the repair tools (noise, hum, clicks and clipping
measurably removed, notes detected and moved). The app tests drive the views with synthetic mouse and keyboard
input (dragging fades, filling steps, the tours, recording channels, compact mode …). The plugin tests load the
in-repo CLAP and VST3 plugins through the real host.

### 🔄 CI/CD

GitHub Actions runs on every push to `main` and every pull request – see the
[CI runs](https://github.com/zenoxart/DragonDAW-RS/actions/workflows/ci.yml),
[release runs](https://github.com/zenoxart/DragonDAW-RS/actions/workflows/release.yml) and
[releases](https://github.com/zenoxart/DragonDAW-RS/releases):

| Workflow | Jobs |
|---|---|
| [`ci.yml`](.github/workflows/ci.yml) | rustfmt and Clippy (`-D warnings`), the full test suite on Linux under Xvfb (including the CLAP/VST3 host tests), tests on Windows and macOS, and a build check on the minimum Rust version (1.89) |
| [`release.yml`](.github/workflows/release.yml) | after every green CI run on `main`, release builds for all four platforms are published as the rolling [`nightly`](https://github.com/zenoxart/DragonDAW-RS/releases/tag/nightly) pre-release; on a `v*` tag, a versioned GitHub release. Each build knows its commit and channel, so the app's update check offers the right download. The Windows zip bundles ffmpeg |

Dependabot opens weekly grouped updates for Cargo crates and the actions themselves.

To publish a release:

```bash
git tag v2.1.0
git push origin v2.1.0
```

## 🤝 Contributing

Issues and pull requests are welcome. Please keep code, comments and docs in English, run the tests and clippy,
and add a test for new engine behaviour that renders audio and checks it.

## 📄 License

Copyright © ZenoxArt. Licensed under the [Apache License, Version 2.0](LICENSE).
