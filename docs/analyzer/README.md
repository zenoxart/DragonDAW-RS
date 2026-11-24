# Scrying Glass (analyzer) (analyzer)

*Utility*

Analyzer insert: spectrum, peak and RMS levels, momentary and short-term loudness, true peak, stereo correlation and a vectorscope. The audio passes unchanged.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [View](view/README.md) – Channel, Speed, Slope, Range, Peak hold

## How it works

A measuring insert: the audio passes through bit-identical.

**Spectrum.** A 4096-point FFT with a Hann window every 1024 samples, folded into 96 log-spaced bands from 20 Hz to
20 kHz ($f_b = 20\cdot 1000^{(b + 0.5)/96}$); each band takes the largest bin magnitude in its range, in dB with a
full-scale sine reading 0 dB (window gain ½, one-sided spectrum: $\text{norm} = 4/N$). The display is then tilted by
the slope $\sigma$ around 1 kHz, $L'_b = L_b + \sigma\log_2(f_b/1000)$ (pink noise reads flat at 3 dB/oct, typical
music at 4.5), and falls back at the chosen speed.

**Levels.** Per block of $n$ samples: peak with a fall of $e^{-n/(1.5 f_s)}$ per block, RMS as a one-pole average of
the block's mean square with $k = e^{-n/(0.3 f_s)}$ (≈ 300 ms).

**Loudness** (ITU-R BS.1770): K-weighted, 100 ms blocks; momentary = 400 ms, short-term = 3 s,
$L = -0.691 + 10\log_{10}\sum_c z_c$ LUFS. True peak from 4× oversampling (see [Mastering](../mastering/README.md)).

**Correlation** $\rho = \frac{E[lr]}{\sqrt{E[l^2]\,E[r^2]}}$ with the same 300 ms averages: +1 mono, 0 unrelated,
−1 out of phase (cancels in mono).

**Vectorscope.** Every 4th sample plotted at $x = (r - l)/\sqrt2$ (side), $y = (l + r)/\sqrt2$ (mid): mono is a
vertical line, wide material a round cloud, out-of-phase material a horizontal line.

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
