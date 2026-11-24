# Wingbeat (tremolo / auto-pan) (tremolo)

*Modulation*

Tempo-synced volume or pan modulation.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Motion](motion/README.md) – Depth, Shape, Auto-pan
- [Rate](rate/README.md) – Sync, Rate (free)

## How it works

Amplitude (or pan) modulation by an LFO, smoothed so square waves do not click.

**Rate.** Free $f$ Hz or synced $f = \frac{\text{BPM}}{60\,b}$ ($b$ beats per cycle).

**Smoothing.** $\hat m \leftarrow \hat m + k(m - \hat m)$, $k = 1 - e^{-1/(0.0015 f_s)}$ (1.5 ms).

**Tremolo.** $g = 1 - \delta\left(\tfrac12 - \tfrac12\hat m\right)$, i.e. the gain swings between 1 and $1 - \delta$
($\delta = 1$: full chop, $20\log_{10}(1-\delta)$ dB at the bottom).

**Auto-pan.** $p = \delta\hat m$, $g_L = \min(1 - p, 1)$, $g_R = \min(1 + p, 1)$ — one side dips while the other
stays at unity.

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
