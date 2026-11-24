# Sandglass (granular) (granular)

*Time*

Live granular processor: scatters short grains of the last seconds of the input – size, density, delay and spray, pitch and its randomness, reversed grains, stereo spread, feedback and freeze.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Grains](grains/README.md) – Grain size, Density, Delay, Spray
- [Loop](loop/README.md) – Feedback, Freeze
- [Output](output/README.md) – Mix, Output
- [Pitch](pitch/README.md) – Pitch, Pitch random, Reverse, Spread

## How it works

Live granular processing: short windowed grains are read from a recording of the last four seconds.

**Scheduling.** A grain starts every $f_s/\rho\cdot(0.75 + 0.5\,u)$ samples ($\rho$ = density, $u$ uniform: irregular
spacing avoids a buzz at the grain rate); up to 64 play at once.

**A grain** of length $L$ = size reads from $D + S\,u$ behind the write head (delay $D$, spray $S$), at speed
$\sigma = \pm 2^{(p + p_r v)/12}$ ($v$ uniform in −1 … 1; negative with the reverse probability), under a Hann window
$w = \tfrac12 - \tfrac12\cos(2\pi a/L)$, panned with the equal-power law at $\pi\,u$ within the spread. A grain that
reads faster than real time starts far enough back never to overtake the write head.

**Level.** Overlapping grains add up; the sum is scaled by $1/\sqrt{\max(1,\ \rho L)}$ (incoherent addition).
Feedback writes $\tanh(g\,y)$ back into the recording (bounded however high it is set); *Freeze* stops recording, so the
grains keep scattering the same moment.

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
