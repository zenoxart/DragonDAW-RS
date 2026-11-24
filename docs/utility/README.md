# Phial (utility) (utility)

*Utility*

Gain, balance, stereo width, polarity per side, swap, mono and DC filter.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Level](level/README.md) – Gain, Balance, Width
- [Polarity Routing](polarity-routing/README.md) – Invert left, Invert right, Swap L/R, Mono, DC filter

## How it works

Gain, balance, width and channel tools, in this order: swap → polarity → mid/side width (or mono) → DC
filter → gain × balance.

$$m = \tfrac12(a + b),\quad s = \tfrac12(a - b)\,W,\quad a' = m + s,\ b' = m - s$$

(mono sets $W = 0$). DC filter: one-pole high-pass at 8 Hz, $y = x - \text{LP}_{8\,\text{Hz}}(x)$.
Output $L = a'\,G\,g_L$, $R = b'\,G\,g_R$ with $G = 10^{g/20}$ and the balance law
$p \le 0$: $(1, 1+p)$, $p > 0$: $(1-p, 1)$.

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
