# Dragon DAW manual

Every feature of Dragon DAW, by section. In the app, **Help › Documentation** opens this page, and every knob's right-click menu has a **Help** entry that opens the knob's own page.

## Sections

- [Getting started](getting-started/README.md) – Audio setup, the first beat, the window layout
- [Transport](transport/README.md) – Play, stop, record, loop and punch, tempo, metronome, markers
- [Playlist](playlist/README.md) – Arranging clips and patterns, tools, zoom, splitting, markers
- [Channel rack](channel-rack/README.md) – Channels, the step sequencer, patterns, drum pads
- [Piano roll](piano-roll/README.md) – Notes, velocity and pan lanes, scales, stamps, quantize and the edit tools
- [Mixer](mixer/README.md) – Channels, meters and faders, inserts, sends, routing, the channel menu
- [Browser](browser/README.md) – Finding sounds, previewing, adding them to the rack
- [Recording](recording/README.md) – Audio and MIDI recording, count-in, takes, the recording template
- [Automation](automation/README.md) – Automation lanes, MIDI learn, modulation
- [Instruments](instruments/README.md) – Sampler, synths, the Transmuter, piano, guitar, drum pads
- [Effects](effects/README.md) – The stock effects, presets, the preset bar and the knob menu
- [Pitch correction](pitch-correction/README.md) – Correcting the pitch of recorded audio and one-shots
- [Audio repair](audio-repair/README.md) – Removing noise, clicks, hum and clipping
- [Mastering](mastering/README.md) – Loudness, reference tracks, the master chain
- [Export](export/README.md) – Rendering songs and stems
- [Templates](templates/README.md) – Starting projects from templates
- [Controllers](controllers/README.md) – MIDI keyboards, controller buttons, control surfaces
- [Plugins](plugins/README.md) – Hosting VST3 and CLAP plugins, scanning, presets
- [Options](options/README.md) – Settings, shortcuts, performance

## Stock effects

| Effect | Category | What it does |
|---|---|---|
| [Prism (parametric EQ)](eq/README.md) | EQ | 7-band parametric EQ with shelves, cuts and notch per band. |
| [Jadefang (character EQ)](jadefang/README.md) | EQ | Six voicings of a four-band EQ with a driven input stage, a very broad Sub band and a boost-only Air band. |
| [Dragontamer (compressor)](compressor/README.md) | Dynamics | Compressor with eight styles (Clean, Classic, Opto, Vocal, Mastering, Bus, Punch, Pumping), look-ahead, hold, sidechain filters and external sidechain, auto makeup and parallel mix. |
| [Quintessence (multiband dynamics)](multiband/README.md) | Dynamics | Multiband dynamics: up to four bands, each compressing or expanding (downward or upward) with threshold, range, ratio, knee, attack, release, gain, solo, bypass and per-band external sidechain. |
| [Portcullis (gate)](gate/README.md) | Dynamics | Gate / expander: threshold with hysteresis, range, ratio, knee, attack, hold, release, look-ahead, styles (Classic, Clean, Vocal, Guitar, Upward), sidechain filters, external sidechain, flip (ducking), mix. |
| [Lodestone (limiter)](limiter/README.md) | Dynamics | Look-ahead brickwall limiter with true-peak detection, eight styles (Allround, Transparent, Punchy, Dynamic, Aggressive, Modern, Bus, Safe) and 2× / 4× oversampling for mastering. |
| [Serpent's Hush (de-esser)](deesser/README.md) | Dynamics | De-esser after Pro-DS: single-vocal (relative) or allround detection, split- or wide-band processing, detection band, threshold, range, look-ahead, stereo link and listen. |
| [Talon (transient shaper)](transient/README.md) | Dynamics | Transient designer: attack (Needle / Nail / Blunt) and sustain (Linear / Non-linear / Soft linear) sections with level, sensitivity, duration and shape, mix, output, and a guard (Off / Clip / Limit / Soft clip) after the output. |
| [Draconic Ghost (resonance suppressor)](ghost/README.md) | Dynamics | Dynamic resonance suppressor: finds the frequencies that stick out right now and pulls only those down, only while they're there |
| [Dragon's Lair (reverb)](reverb/README.md) | Space | Room/hall/plate reverb (8-line FDN) with pre-delay, damping and width; early reflections and late reverb each have their own level and EQ. |
| [Echo Cavern (delay)](delay/README.md) | Delay | Delay: tempo-synced or free, stereo / ping-pong / tape, cross feedback, resonant filters, drive and diffusion in the loop, pitch-shifted repeats, ducking, freeze, width, and a modulation matrix (two LFOs and an envelope follower routed through four slots to time, feedback, filters, drive, pitch, width or mix). |
| [Coven (chorus)](chorus/README.md) | Modulation | Stereo chorus with quadrature LFOs. |
| [Quicksilver (flanger)](flanger/README.md) | Modulation | Through-zero style flanger with positive or negative feedback. |
| [Moonphase (phaser)](phaser/README.md) | Modulation | 2–12 stage all-pass phaser with feedback. |
| [Wingbeat (tremolo / auto-pan)](tremolo/README.md) | Modulation | Tempo-synced volume or pan modulation. |
| [Ouroboros (auto-panner)](orbit/README.md) | Modulation | Auto-panner: moves the sound across the stereo field locked to the bars, at a free rate, or one step per hit (Trigger, with glide) |
| [Crucible (saturator)](saturator/README.md) | Distortion | Tube (12AX7, 12AT7, 12AU7, 6SN7, EF86, EL34, 6L6, KT88, 300B models with bias, blocking and sag), tape, soft/hard clip, foldback and asymmetric waveshaping with tone and parallel mix, plus three parallel tube ingredients to blend into the brew: a warm 6SN7, a hot 12AX7 and a crisp EF86 pentode. |
| [Athanor (multiband saturator)](multisat/README.md) | Distortion | Multiband saturation: up to four bands (Linkwitz–Riley crossovers), each with a style (clean / warm tube, tape, transformer, soft / hard clip, foldback, rectify), drive, dynamics, tone, level, mix and a resonant feedback loop. |
| [Corroder (bit crusher)](bitcrusher/README.md) | Distortion | Bit depth and sample-rate reduction. |
| [Obsidian (soft clipper)](clipper/README.md) | Distortion | Soft clipper with the sound of the Fruity Soft Clipper: clean below the threshold, a smooth shoulder up to the ceiling, post gain; plus input drive, soft / hybrid / hard shapes, up to 4× oversampling and a clip-only listen. |
| [Alembic (filter)](filter/README.md) | Filter | Filter: two filters (serial or parallel), low / high / band pass, notch, peak and shelves, 12 / 24 / 48 dB slopes, styles (clean, smooth, ram, hack), drive, stereo spread, LFO and envelope follower. |
| [Phial (utility)](utility/README.md) | Utility | Gain, balance, stereo width, polarity per side, swap, mono and DC filter. |
| [Wingspan (stereo imager)](imager/README.md) | Utility | Widens the stereo image and keeps the bass mono; a goniometer and correlation meter show the phase relation of the result. |
| [Philosopher's Stone (one-knob mastering)](dragon-particle/README.md) | Mastering | One-knob mastering: tone EQ with character, transient punch, glue compression, tube colour, widening, a soft clipper and a true-peak limiter, with auto gain for honest A/B. |
| [Aurum Tune (pitch correction)](tune/README.md) | Pitch | Real-time, zero-latency pitch correction for tracking: snaps a monophonic voice to a key (retune speed, humanize, flex-tune, natural vibrato) |
| [Purify (voice cleanup)](clean-voice/README.md) | Vocal | Post-production vocal chain in one plugin (after zenoxart/CleanVoice): valve vari-mu, leveller, vari-mu, de-esser, two 4-band multibands, parallel presence, oversampled saturation and a resonance suppressor. |
| [Golem Strip (class-A channel strip)](class-a-strip/README.md) | Channel strip | Recording channel: a one-fader noise suppressor, Class-A transformer mic preamp with stepped inductor-style EQ into a tube opto compressor (program-dependent opto attack, two-stage release with memory, fixed or manual timing) |
| [Dragonforge (drum bus)](drum-bus/README.md) | Drums | Drum bus processor: input with target meter, drive (three colours, high-pass), harmonics (50 / 80 Hz focus), compression (glue / punch, mix), lift, five-band harmonic EQ in 1 dB steps, parallel Sub / Low / Loud / Grit, output. |
| [Shapeshifter (pitch & formant)](alter/README.md) | Pitch | Real-time voice pitch and formant shifter: pitch ±12 semitones, formant, link, Transpose / Quantize / Robot modes, tube drive, mix and output. |
