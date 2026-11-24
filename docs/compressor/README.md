# Dragontamer (compressor) (compressor)

*Dynamics*

Compressor with eight styles (Clean, Classic, Opto, Vocal, Mastering, Bus, Punch, Pumping), look-ahead, hold, sidechain filters and external sidechain, auto makeup and parallel mix.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Compression](compression/README.md) – Threshold, Ratio, Knee
- [Output](output/README.md) – Makeup, Auto makeup, Mix
- [Sidechain](sidechain/README.md) – External sidechain, Sidechain HPF, Sidechain LPF
- [Style](style/README.md) – Style
- [Timing](timing/README.md) – Attack, Release, Look-ahead, Hold

## How it works

A feed-forward / feedback compressor working in the decibel domain, per sample.

**Signal flow.** detector input (own signal, or the external sidechain) → sidechain high-pass and low-pass
(RBJ, $Q = 0.707$) → peak hold (steady detection of low frequencies) → peak or RMS detector (depending on
the style) → blend with the output level (feedback styles) → static curve → attack / release ballistics on the
gain reduction → gain applied to the look-ahead-delayed audio → makeup → parallel mix.

**Detector.** With $s[n] = \max(|x_L|, |x_R|)$ after the sidechain filters and peak hold, RMS styles use
$\overline{s^2}[n] = s^2 + a_r(\overline{s^2}[n-1] - s^2)$, $a_r = e^{-1/(t_{rms} f_s)}$ and read
$e = \sqrt{2\,\overline{s^2}}$ (the $\sqrt2$ makes a sine read as its peak). Feedback styles detect
$e' = (1-\beta)e + \beta\,|y_{prev}|$ with $\beta$ the style's feedback share.

**Static curve** (threshold $T$, ratio $R$, knee $K$ in dB, input level $X = 20\log_{10} e$):

$$
Y(X) = \begin{cases}
X & X < T - K/2\\
X + \left(\frac1R - 1\right)\frac{(X - T + K/2)^2}{2K} & |X - T| \le K/2\\
T + \frac{X - T}{R} & X > T + K/2
\end{cases}
$$

The target reduction is $G^* = X - Y(X) \ge 0$ dB.

**Ballistics** on the reduction (dB), with attack $t_a$ and release $t_r$ in ms:

$$G[n] = G^* + c\,(G[n-1] - G^*),\qquad c = \begin{cases} e^{-1/(t_a 10^{-3} f_s)} & G^* > G[n-1]\\ e^{-1/(t_r 10^{-3} f_s)} & \text{otherwise}\end{cases}$$

Program-dependent styles lengthen the release with the current reduction:
$t_r' = t_r\,\sigma_r\,(1 + \pi\,G/6)$ ($\pi$ = the style's program factor, $\sigma_r$ its release scale).

**Output.** $y = (1-\mu)\,x_D + \mu\,x_D\,10^{-G/20}\,10^{M/20}$ where $x_D$ is the input delayed by the
look-ahead, $M$ the makeup and $\mu$ the mix.

**Styles** (RMS window, feedback, program factor, extra knee, attack scale, release scale):

| style | $t_{rms}$ | $\beta$ | $\pi$ | $+K$ | $\sigma_a$ | $\sigma_r$ |
|---|---|---|---|---|---|---|
| Clean | peak | 0 | 0 | 0 dB | 1 | 1 |
| Classic | peak | 1 | 0.3 | 6 dB | 1 | 1 |
| Opto | 5 ms | 0 | 1.5 | 10 dB | 3 | 1.5 |
| Vocal | 3 ms | 0 | 0.4 | 10 dB | 0.6 | 0.7 |
| Mastering | 30 ms | 0 | 0.5 | 18 dB | 0.7 | 1 |
| Bus | 8 ms | 0.3 | 0.3 | 6 dB | 1 | 1 |
| Punch | peak | 0 | 0 | 2 dB | 2.5 | 0.8 |
| Pumping | peak | 0 | 0 | 0 dB | 0.5 | 0.35 |

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
