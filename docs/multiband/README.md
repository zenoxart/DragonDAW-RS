# Quintessence (multiband dynamics) (multiband)

*Dynamics*

Multiband dynamics: up to four bands, each compressing or expanding (downward or upward) with threshold, range, ratio, knee, attack, release, gain, solo, bypass and per-band external sidechain.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Band](band/README.md) – Mode, Threshold, Range, Ratio, Knee, Attack, Release, Gain, Solo, Bypass, External sidechain
- [Output](output/README.md) – Bands, Crossover 1, Crossover 2, Crossover 3, Mix, Output, Processing

## How it works

Up to four bands, each with its own up- or downward compressor or expander, in one of two topologies.

**Crossover mode.** Each split is a 4th-order Linkwitz–Riley crossover: two cascaded RBJ Butterworth
sections ($Q = 1/\sqrt2$) for the low-pass and for the high-pass,

$$H_{LP}(s) = \left(\frac{\omega_c^2}{s^2 + \sqrt2\,\omega_c s + \omega_c^2}\right)^2,\qquad H_{LP} + H_{HP} = \text{all-pass}.$$

The signal is split low → high: band 1 = LP₁(x), the rest = HP₁(x) is split again at the next crossover, and so on.
Each lower band is then passed through the all-pass ($LP + HP$) of every crossover above it, so all bands share
the same phase and $\sum_b y_b$ is flat in magnitude (unity gain at every frequency when nothing is processed).
Crossovers are kept at least $\times1.25$ apart: $f_2 \ge 1.25 f_1$, $f_3 \ge 1.25 f_2$.

**Dynamic-phase mode.** No band split at all: each band is a filter on the full signal whose gain is the
band's current gain change — a low shelf at $f_1$ (band 1), a high shelf at $f_{N-1}$ (the top band) and bells
between, centred at $f_c = \sqrt{f_{lo} f_{hi}}$ with $Q = \text{clamp}\!\left(\frac{f_c}{f_{hi}-f_{lo}}, 0.3, 4\right)$.
While no band acts, every filter is at 0 dB and the signal is bit-transparent (no phase shift). The filters are
retuned every 8 samples when a gain moved by more than 0.01 dB.

**Detector per band.** $e_b = \text{peakhold}(\max(|x_{L,b}|, |x_{R,b}|))$ with a hold of
$\min(600/f_{low,b}, 30)$ ms (half a period of the band's lowest frequency), $X_b = 20\log_{10} e_b$.

**Gain law** (threshold $T$, ratio $R$, knee $K$, overshoot $o = X - T$). With the knee functions

$$s(o) = \begin{cases}0 & o < -K/2\\ \frac{(o + K/2)^2}{2K} & |o| \le K/2\\ o & o > K/2\end{cases}\qquad
u(o) = \begin{cases}o & o < -K/2\\ -\frac{(K/2 - o)^2}{2K} & |o|\le K/2\\ 0 & o > K/2\end{cases}$$

| mode | gain change $\Delta$ (dB) |
|---|---|
| Compress | $-s(o)\,(1 - 1/R)$ |
| Expand (downward) | $u(o)\,(R - 1)$ |
| Upward compress | $-u(o)\,(1 - 1/R)$ (lifts material below $T$) |
| Upward expand | $\tfrac12\,s(o)\,(R - 1)$ (lifts material above $T$) |

then $\Delta \leftarrow \text{clamp}(\Delta, -\text{range}, +\text{range})$, followed by attack/release
ballistics: $\Delta[n] = \Delta^* + c(\Delta[n-1] - \Delta^*)$ with $c$ = attack coefficient while
$|\Delta^*| > |\Delta|$, otherwise release.

**Output.** $y = \left(x + \mu\,(\sum_b 10^{(\Delta_b + G_b)/20} x_b - x\right)) \cdot 10^{O/20}$ with the
mix $\mu$, band gain $G_b$ and output $O$.

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
