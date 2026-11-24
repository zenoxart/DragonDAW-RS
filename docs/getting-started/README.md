# Getting started

1. **Audio:** Options › Audio picks the driver (ASIO on Windows), the device and the buffer size. Smaller buffers lower the latency; raise the buffer if you hear crackles. **Help › Set up audio** walks you through it.
2. **A first beat:** File › New gives you an empty project, File › New from template a mixing, beats or recording setup. The channel rack (F6) holds the sounds, click its steps to program a pattern, press Space to play.
3. **The layout:** the playlist, mixer, channel rack and piano roll are tabs; drag a tab out to float it, **View › Reset layout** puts every view back. **Help › Lost a window?** explains the rest.
4. **Help:** F1 opens the manual page of the view you are in, and every knob's right-click menu has a **Help** entry for that knob.

## How it works

**Latency.** The audio device calls the engine for one buffer of $B$ frames at a time. One buffer lasts
$B/f_s$ (128 frames at 44.1 kHz ≈ 2.9 ms, 512 at 48 kHz ≈ 10.7 ms); that is the delay from a knob or a played note to
the speakers, plus the driver's own buffers. Smaller buffers respond faster and cost more CPU per second (the fixed
work per callback is paid more often).

**Inside a buffer** the engine works in blocks of at most 128 samples: automation, modulation and parameter changes are
read once per block and ramped linearly across it ($v_k = v_0 + (v_1 - v_0)\frac{k+1}{n}$), so nothing steps audibly.

**Levels.** Everything inside the engine is 32-bit floating point: a signal above 0 dBFS is not clipped until it
leaves the program (export to an integer format, or the device). Decibels are $20\log_{10}|g|$ throughout.

See also: [Transport](../transport/README.md) · [Channel rack](../channel-rack/README.md) · [Manual](../README.md)
