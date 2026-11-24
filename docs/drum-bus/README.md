# Dragonforge (drum bus) (drum_bus)

*Drums*

Drum bus processor: input with target meter, drive (three colours, high-pass), harmonics (50 / 80 Hz focus), compression (glue / punch, mix), lift, five-band harmonic EQ in 1 dB steps, parallel Sub / Low / Loud / Grit, output.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Compression](compression/README.md) – Compression mode, Compression, Lift
- [Drive](drive/README.md) – Input, Drive, Drive colour, Drive high-pass
- [Harmonic Eq](harmonic-eq/README.md) – Harmonic EQ 60 Hz, Harmonic EQ 180 Hz, Harmonic EQ 900 Hz, Harmonic EQ 3.5 kHz, Harmonic EQ 10 kHz
- [Harmonics](harmonics/README.md) – Harmonics, Harmonics focus
- [Output](output/README.md) – Output
- [Parallels](parallels/README.md) – Parallel: Sub, Parallel: Low, Parallel: Loud, Parallel: Grit

## How it works

A drum bus processor built from modules in a fixed order:

```text
input gain → drive (saturator, oversampled) → drive high-pass → harmonics → compression (parallel mix)
  → lift → harmonic EQ → + parallels (Sub, Low, Loud, Grit) → output rails
```

All filters are RBJ biquads per channel, retuned in place (state kept, so turning a knob never clicks).

**Drive.** The stock saturator runs on every block (its oversampling latency stays constant) with, for the knob value
$u = v/100$:

$$\text{drive} = 24u\ \text{dB},\qquad \text{output} = -10u\ \text{dB},\qquad \text{mix} = \min(20u,\ 1)$$

so the colour fades in over the first 5 % of travel instead of jumping in, and the output trim compensates roughly
for the gain the curve adds. Its output goes through the **Drive high-pass**.

**Harmonics.** The kick's fundamental region $b = BP_{f_c,\,Q=1.2}(x)$ (focus $f_c$ = 50 or 80 Hz) is shaped and
low-passed at $4f_c$ ($Q$ 0.7):

$$h = LP_{4f_c}\!\big(0.5\tanh(6b) + 0.3\tanh(3|b|)\big),\qquad y = x + 1.5\,\eta\,h$$

The $\tanh(6b)$ term adds odd harmonics, the rectified $|b|$ term even ones (octave, and a DC that the low-pass and the
following stages absorb); $\eta$ = Harmonics / 100.

**Lift** (amount $\lambda$): $y = x + 0.35\lambda\tanh\!\big(3\,HP_{2.5\,kHz}(x)\big) + 0.3\lambda\left(\frac{\tanh 1.5x}{1.5} - x\right)$
– an exciter on the top plus a little overall rounding.

**Harmonic EQ.** Five fixed bands (60, 180, 900 Hz, 3.5 and 10 kHz), each a band-pass $b_k$ with $Q$ 0.9. A band does not
boost linearly: it *adds* saturated content of its region,

$$y = x + \sum_k a_k\left(0.5\tanh(2b_k) + 0.5\,b_k\right),\qquad a_k^* = 10^{G_k/20} - 1$$

where $G_k$ is the knob in whole dB (0 … 10). $a_k$ glides towards $a_k^*$ with a 5 ms one-pole, so a 1 dB step is a
quick change, not a click. For small signals the band rises by $\approx 20\log_{10}(1 + a_k)= G_k$ dB at its centre;
louder hits saturate and gain harmonics instead of more level.

**Parallels.** Added to the main path after the Harmonic EQ, each scaled by its knob $p$:

| parallel | source | processing | added |
|---|---|---|---|
| Sub | $LP_{70}(x)$ | $\tanh(4s)$ | $0.5\,p$ |
| Low | $LP_{160}(x)$ | compressor: −30 dB, 8:1, 15 / 120 ms, +14 dB | $0.6\,p$ |
| Loud | $x$ | compressor: −36 dB, 20:1, 0.5 / 60 ms, +22 dB | $0.6\,p$ |
| Grit | $0.6\tanh(3\,HP_{1.2k}(x))$ | transient shaper: attack +80, sustain −40, 12 ms | $0.6\,p$ |

**Output.** $y = 16\tanh\!\big((x + \textstyle\sum\text{parallels})\,O/16\big)$, $O = 10^{o/20}$: rails near +24 dBFS,
linear at normal levels.

**Meters.** Input and output RMS over ≈ 300 ms, $\bar p[m] = p + (\bar p[m-1] - p)\,e^{-N/(0.3 f_s)}$ per block of $N$
samples; the target lamp lights when the block's input peak exceeds 3 × the input RMS (a crest above ≈ 9.5 dB: the
hits are still punchy).

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
