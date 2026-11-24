# Obsidian (soft clipper) (clipper)

*Distortion*

Soft clipper: clean below the threshold, a smooth shoulder up to the ceiling, post gain; plus input drive, soft / hybrid / hard shapes, up to 4× oversampling and a clip-only listen.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Character](character/README.md) – Shape, Oversampling
- [Clip](clip/README.md) – Input, Threshold, Ceiling
- [Output](output/README.md) – Post gain, Clip only

## How it works

A memoryless clipper with a smooth shoulder, oversampled.

**Curve** for $a = |x|$ (sign restored afterwards), threshold $t$, ceiling $c$ (both linear, $t \le c$),
$u = \frac{a - t}{c - t}$:

$$
y = \begin{cases} a & a \le t\\ t + (c - t)\tanh u & \text{Soft}\\ t + (c - t)\frac{u}{\sqrt[4]{1 + u^4}} & \text{Hybrid}\\ \min(a, c) & \text{Hard}\end{cases}
$$

Below the threshold the signal is untouched (bit-exact); above it the shoulder approaches the ceiling
asymptotically (Soft is the gentlest, Hybrid stays linear longer then bends sharply).

**Oversampling.** The curve runs at $k f_s$ ($k = 1, 2, 4$) behind 47-tap half-band stages: harmonics
above $f_s/2$ are filtered before they can fold back.

**Output.** $y\cdot 10^{P/20}$; *Clip only* outputs $(x_D - y)\,10^{P/20}$ — the part that was removed
(with $x_D$ the input delayed by the oversampler's latency). The meter shows
$20\log_{10}\frac{|x|_{max}}{\text{curve}(|x|_{max})}$.

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
