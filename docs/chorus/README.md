# Coven (chorus) (chorus)

*Modulation*

Stereo chorus with quadrature LFOs.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Chorus](chorus/README.md) – Rate, Depth, Delay
- [Mix](mix/README.md) – Mix

## How it works

A modulated delay per channel; the two channels' LFOs run in quadrature (90° apart), which spreads the
detuned copies across the stereo field.

$$d_c[n] = \left(D_0 + 8\,\text{ms}\cdot\delta\cdot\tfrac{1 + \sin(2\pi(f n/f_s + \varphi_c))}{2}\right)10^{-3} f_s,\qquad \varphi_L = 0,\ \varphi_R = \tfrac14$$

$$w_c[n] = \text{line}_c(d_c[n]),\qquad y_c = x_c\left(1 - \tfrac{\mu}{2}\right) + \mu\,w_c$$

(fractional read with linear interpolation). The moving read position is a time-varying Doppler shift: the
instantaneous pitch ratio is $1 - \frac{d}{dn}d_c[n]$, at most $\pm\,8\,\text{ms}\cdot\delta\cdot\pi f$ — a gentle
detune of a few cents that sweeps up and down.

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
