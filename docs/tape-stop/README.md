# Petrify (tape stop) (tape_stop)

*Time*

Tape stop and spin-up: the sound slows to a halt (pitch falling with the speed) and starts again. Triggered by its switch (automate it) or at the end of every bar, 2 or 4 bars.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Character](character/README.md) – Curve, Darken
- [Output](output/README.md) – Mix
- [Tape](tape/README.md) – Stop, Trigger, Stop time, Start time

## How it works

Slows the sound to a halt like a tape machine whose motor stops, and spins it back up.

**Reading the tape.** The input is recorded continuously. When a stop begins, a read head starts at the live
position and advances by the current speed $s$ per sample, $r \leftarrow r + s$, reading with linear interpolation –
so the pitch falls with the speed. Stopped, the output is silent.

**Speed ramps** with progress $u \in [0, 1]$ over the stop or start time, bent by the curve $c$:

$$s_{stop}(u) = (1 - u)^{2^{2c}},\qquad s_{start}(u) = 1 - (1 - u)^{2^{2c}}$$

(positive curves brake early like a heavy reel, negative ones late). After a spin-up the read head is behind the live
input; a 12 ms crossfade rejoins it.

**Darkening.** A one-pole low-pass follows the speed, $f_c = 400 + 19000\,s^{1 + 2d}$ Hz, as a slowing tape loses its
top.

**Automatic triggers** end a stop exactly on the bar line: with $B$ beats per cycle (4, 8 or 16) the tape stops when
$b \bmod B \ge B - t_{stop}\cdot\text{bpm}/60$.

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
