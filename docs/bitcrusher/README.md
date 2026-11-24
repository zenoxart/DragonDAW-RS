# Corroder (bit crusher) (bitcrusher)

*Distortion*

Bit depth and sample-rate reduction.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Crush](crush/README.md) – Bits, Downsample
- [Mix](mix/README.md) – Mix

## How it works

Quantisation and sample-and-hold decimation.

**Bit depth.** With $B$ bits (fractional allowed) the step count per polarity is $q = 2^{B-1}$:
$\hat x = \frac{\operatorname{round}(q x)}{q}$. The quantisation error is roughly uniform with power
$\frac{\Delta^2}{12}$, $\Delta = 1/q$ — signal-to-noise $\approx 6.02B + 1.76$ dB for a full-scale sine.

**Downsampling.** A phase counter advances 1 per sample; whenever it reaches $R$ (fractional), the current
quantised sample is held: $y[n] = \hat x[n_k]$ for $n_k \le n < n_{k+1}$. No anti-alias filter is applied, so
content above $\frac{f_s}{2R}$ folds back (aliasing) — the intended lo-fi sound.

$y = (1-\mu)x + \mu\,y_{held}$.

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
