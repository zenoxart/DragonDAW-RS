# Familiar (auto-filter) (autofilter)

*Filter*

Auto-filter: a resonant low-, band-, high-pass or notch whose cutoff an LFO (free or synced, six shapes, stereo phase) and an envelope follower (positive or negative, from the input or the sidechain) move.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Envelope](envelope/README.md) – Envelope, Env attack, Env release
- [Filter](filter/README.md) – Type, Cutoff, Resonance, Drive
- [Lfo](lfo/README.md) – LFO sync, LFO rate, LFO shape, LFO depth, Stereo phase
- [Output](output/README.md) – Mix, Output

## How it works

A resonant filter whose cutoff moves by itself.

**Filter**: the TPT state-variable filter (see the [DSP notes](../dsp/README.md)), $g = \tan(\pi f_c/f_s)$,
$k = 2 - 1.95\,r$; low-pass, band-pass, high-pass or notch (low + high). Drive feeds the filter
$\tanh(kx)/\sqrt k$, $k = 1 + 8\,\text{drive}$.

**Cutoff** in octaves around the knob:

$$f_c = f_0\cdot 2^{\,\frac{D}{2}\,\ell(\varphi) + E\cdot\min(4e,\,1)}$$

with the LFO $\ell \in [-1, 1]$ (depth $D$ octaves peak-to-peak) and the envelope $e$ of the input or the sidechain
(follower with its own attack and release), scaled by the envelope amount $E$ (±5 octaves; negative closes the filter
on loud notes).

**LFO.** Synced, the phase is the song position over the note length, $\varphi = b/\lambda$ (locked to the bar); free, it
runs at the rate. The right side runs ahead by the stereo phase. Shapes: sine, triangle, saw up and down, square, and
random (a new value each cycle, held).

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
