# Hex (stutter / glitch) (stutter)

*Time*

Tempo-synced stutter: catches a slice of the beat and repeats it, always or by chance, with gating, a pitch step per repeat and reversed slices.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Hex](hex/README.md) – Mode, Slice, Repeats, Chance
- [Output](output/README.md) – Mix
- [Slices](slices/README.md) – Gate, Pitch step, Reverse

## How it works

A tempo-synced stutter. The song is cut into slices of the chosen note value; $H$ consecutive slices form a group.
At the start of a group the effect decides whether it stutters (always, or with the chance $P$ per group); if it
does, the group's first slice is played as it comes and recorded, and the remaining $H - 1$ slices replay it.

**Timing.** At song position $b$ (beats) with slice length $\lambda$ beats: slice $s = \lfloor b/\lambda\rfloor$,
group $\lfloor s/H\rfloor$, repeat number $k = s \bmod H$, position in the slice
$\tau = (b/\lambda - \lfloor b/\lambda\rfloor)\cdot\lambda\cdot\frac{60 f_s}{\text{bpm}}$ samples.

**Repeat $k$** reads the recording at speed $\sigma_k = 2^{k\,p/12}$ (the pitch step $p$ accumulates: a falling or
rising stutter), reversed with probability $R$ per slice, under a gate that is open for the first $\gamma$ of the
slice with 2 ms ramps. Switching between the live input and the repeats glides over a few milliseconds.

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
