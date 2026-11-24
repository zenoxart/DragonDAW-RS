# Purify (voice cleanup) (clean_voice)

*Vocal*

Post-production vocal chain in one plugin (after zenoxart/CleanVoice): valve vari-mu, leveller, vari-mu, de-esser, two 4-band multibands, parallel presence, oversampled saturation and a resonance suppressor.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [De Esser](de-esser/README.md) – Deesser threshold, Deesser range, Deesser detection, Deesser band
- [Finish](finish/README.md) – Presence, Saturation, Resonance tamer mix
- [Leveling](leveling/README.md) – Tube, Comp, Vari Mu
- [Multiband I](multiband-i/README.md) – Multiband 1 lowmids, Multiband 1 highmids
- [Multiband Ii](multiband-ii/README.md) – Multiband 2 lowmids, Multiband 2 highmids

## How it works

A fixed post-production vocal chain. Every stage is a block of its own; the knobs set how hard each one works, the
rest of its settings are baked in.

```text
in → tube vari-mu → leveller → vari-mu → de-esser → multiband 1 → presence (parallel)
   → multiband 2 → saturation (2× oversampled) → resonance suppressor → out
```

**Shared helpers.** Levels are $X = 20\log_{10}|x|$ (floored at −120 dB). Envelopes are one-pole followers
$e[n] = x + c\,(e[n-1] - x)$ with $c = e^{-1/(t\,10^{-3} f_s)}$, the attack coefficient while rising and the release
coefficient while falling, after a 30 ms peak hold (so a bass wave's cycle does not modulate the gain). Every gain
computer uses the same soft knee (over-threshold $o$, knee $K$, ratio $R$, slope $s = 1/R - 1$):

$$
g(o) = \begin{cases}
0 & 2o < -K\\
s\,\frac{(o + K/2)^2}{2K} & 2|o| \le K\\
s\,o & 2o > K
\end{cases}\quad\text{(dB, } \le 0\text{)}
$$

Applied gains glide with a one-pole of 0.6 – 1.5 ms (per stage) so the reduction never steps.

**Vari-mu stages** (the tube at the input and the Vari Mu after the leveller). The ratio grows with how far the
signal is over threshold, $R(o) = R_{min} + (R_{max} - R_{min})\frac{o}{o + 12}$, so quiet passages see
$R_{min}$ and loud ones approach $R_{max}$. A fast and a slow detector run in parallel (the slow one with 4× the
attack), the detector input is high-passed (60 / 90 Hz). Make-up is the reduction a −14 dBFS signal would get,
$M = -g(-14 - T)$. After the gain, a valve curve adds harmonics that grow with the reduction:

$$v(x, c) = x + c\left(\frac{\tanh(d + \tfrac{c}{4}d^2)}{1 + 2c} - x\right),\quad d = x(1 + 2c),\quad c = h\,(0.35 + 0.65\,(1 - g_{lin}))$$

followed by a transformer high-shelf and a 12 Hz DC blocker.

| stage | $T$ | $R_{min}$ – $R_{max}$ | knee | attack | release fast / slow | sidechain HP | harmonics $h$ | shelf |
|---|---|---|---|---|---|---|---|---|
| Tube | −20 dB | 1.6 – 4 | 16 dB | 0.2 ms | 300 / 800 ms | 60 Hz | 0.30 | 12 kHz, −1 dB |
| Vari Mu | $-14 - 14\,u$ dB | 1.4 – 3 | 18 dB | 8 ms | 250 / 1200 ms | 90 Hz | 0.22 | 14 kHz, −0.7 dB |

**Multiband stages.** Linkwitz–Riley 4th-order crossovers (two cascaded RBJ Butterworth sections, $Q = 1/\sqrt2$) at
100 Hz, 1 kHz and 10 kHz, with all-pass compensation so the four bands sum flat. Each band detects
$\max(e_{fast},\ e_{slow}(1 - \tfrac12\kappa))$ where $\kappa$ is the band's crest (peak against a rectified
ripple average), so steady tones are treated less harshly than transients; the slow detector has 2× the attack and
4× the release. The band's reduction is $\max(g(X - T_b),\ r_b)$ with knee 6 dB and ratio 3, $r_b$ the band's
range.

| band | range | threshold $T_b$ | attack | release |
|---|---|---|---|---|
| < 100 Hz | 0 dB (off) | −27 dB | 35 ms | 40 ms |
| 100 Hz – 1 kHz | *lowmids* knob | −19.2 dB | 35 ms | 15 ms |
| 1 – 10 kHz | *highmids* knob | −30.1 dB | 40 ms | 10 ms |
| > 10 kHz | 0 dB (off) | −35.3 dB | 2 ms | 5 ms |

**Presence** (parallel): $y = x + m\,(P(x) - x)$ with $P$ = low shelf 40 Hz +1.5 dB ($Q$ 0.7) → peak 2.5 kHz +1.5 dB
($Q$ 0.8) → high shelf 10 kHz +12 dB ($Q$ 0.4).

**Saturation** at $2f_s$: zero-stuffed upsampling (×2 gain) through a 4th-order Butterworth low-pass at
$0.45 f_s$ (sections $Q$ 0.5412 and 1.3066), pre-emphasis (high shelf 3 kHz +4 dB, input ×1.7), the curve

$$c(x) = 0.72\,\tanh\!\big(1.6\,(x + 0.12x^2)\big)$$

(the $x^2$ term makes it asymmetric: even harmonics), de-emphasis (3 kHz −4 dB), a tone shelf (9 kHz −1.5 dB), a
firm low shelf (80 Hz −1 dB), a 15 Hz DC blocker, ×1.25, the same anti-alias filter, and decimation by 2. The
result is blended: $y = x + m\,(c - x)$.

**Resonance suppressor.** STFT, $N = 1024$, hop $N/4$, periodic Hann window, overlap-add normalised by
$\sum w^2$. Per bin $k$ the magnitude in dB, $M_k = 20\log_{10}\frac{2|X_k|}{N}$, is smoothed in both directions with
a one-pole whose length grows with frequency (about $\frac{k}{2}(2^{o} - 1)$ bins, $o = 1.105$ octaves), giving a
baseline $B_k$. What sticks out above it plus a frequency-dependent sensitivity offset $S(f)$ (a smoothstep-interpolated
curve) is pulled down:

$$e_k = M_k - B_k - S(f_k) - 5.32,\qquad g_k = 10^{-\min(0.9\,e_k,\ 15)/20}\ \ (e_k > 0)$$

with 5 ms attack and 20 ms release per frame. The output blends the cleaned signal with the latency-aligned dry by the
mix.

The plugin reports the de-esser's look-ahead (5 ms) plus the suppressor's frame ($N$) as latency, so the dry path in the
mixer stays aligned.

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
