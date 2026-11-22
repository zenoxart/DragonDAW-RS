# Windows test checklist

What the automated tests cannot hear or click. About 15 minutes; tick what works, note what does not
(with the time and what you did) and send it back.

## Audio setup
- [ ] Options › Audio: driver **ASIO**, your interface as output and input; "Running:" shows it.
- [ ] Buffer 128: the line shows about 2.9 ms (44.1 kHz) / 2.7 ms (48 kHz); playback is clean.
- [ ] Buffer 64 and 256: switch while playing – no crash, sound comes back.
- [ ] Speakers + mic monitoring on purpose: within a second the status bar says
      "Feedback – input monitoring muted"; "Monitor again" turns it back on.

## Recording
- [ ] + Add › Recording channel, record 10 s while the click plays: the take sits on the beat
      (zoom in on a clap / count-in click: no visible offset).
- [ ] Record over the same bars again: the new take lands on a grouped track below, the old one stays.
- [ ] With punch on: recording replaces inside the punch range only.
- [ ] The waveform of a normal-level take fills most of the clip height.

## Closing (the old crash)
- [ ] Open two plugin windows (one VST3, one CLAP if you have one), play, close the app with the X.
      Repeat 5×. No crash, no crash report (`%LOCALAPPDATA%\DragonDAW\Crash\last-crash.txt`).
- [ ] Same with ASIO playing and a recording armed.

## Sound (your ears)
- [ ] Aurum Tune, retune speed 0, key of the song, on a sung line: the hard "T-Pain" effect, no
      warble or chorus. At 20–50 ms: natural correction.
- [ ] Philosopher's Stone on a finished mix, default settings: loud, tight low end, open top; compare
      with God Particle at the same loudness.
- [ ] Prism EQ high-pass at 96 dB/oct and 6 dB/oct: audibly different; linear-phase mode has no
      pre-ringing clicks on drums.

## Plugins
- [ ] A VST3 with factory presets: the Preset list shows them; ◀ ▶ step through; the sound changes.
- [ ] Save preset… then load it again (and in another DAW, it shows in its VST3 preset browser).
- [ ] Freeze (❄) a synth track with heavy plugins: CPU drops, it sounds the same; unfreeze restores.
