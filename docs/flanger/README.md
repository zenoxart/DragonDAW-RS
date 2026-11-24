# Quicksilver (flanger) (flanger)

*Modulation*

Through-zero style flanger with positive or negative feedback.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Flanger](flanger/README.md) – Rate, Depth, Feedback, Delay
- [Mix](mix/README.md) – Mix

## How it works

A short modulated delay with feedback: the comb filter $1 \pm g z^{-d}$ whose notches sweep.

$$d_c[n] = \left(D_0 + 5\,\text{ms}\cdot\delta\cdot\tfrac{1 + \sin(2\pi(f n/f_s + \varphi_c))}{2}\right)10^{-3} f_s,\quad \varphi_L = 0,\ \varphi_R = \tfrac14$$

$$w = \text{line}(d),\qquad \text{line} \leftarrow x + g\,w,\qquad y = x\left(1 - \tfrac\mu2\right) + \mu w$$

The feedback comb $H(z) = \frac{z^{-d}}{1 - g z^{-d}}$ has peaks at multiples of $f_s/d$ (positive $g$) or
halfway between them (negative $g$); as $d$ sweeps from $D_0$ to $D_0 + 5\delta$ ms the peaks and notches
glide — the jet sound. Their height grows as $\frac{1}{1 - |g|}$.

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
