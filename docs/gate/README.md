# Portcullis (gate) (gate)

*Dynamics*

Gate / expander: threshold with hysteresis, range, ratio, knee, attack, hold, release, look-ahead, styles (Classic, Clean, Vocal, Guitar, Upward), sidechain filters, external sidechain, flip (ducking), mix.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Output](output/README.md) – Mix, Output
- [Sidechain](sidechain/README.md) – External sidechain, Sidechain high-pass, Sidechain low-pass, Listen to the sidechain
- [Style](style/README.md) – Style, Flip (duck)
- [Threshold](threshold/README.md) – Threshold, Range, Ratio, Knee, Hysteresis
- [Timing](timing/README.md) – Attack, Hold, Release, Look-ahead

## How it works

A gate / downward expander with hysteresis, hold and look-ahead; optionally an upward expander or a ducker.

**Detector.** Sidechain (own or external) → RBJ high-pass and low-pass ($Q = 0.707$) → $p = \max(|x_L|,|x_R|)$,
or RMS $e = \sqrt{2\,\overline{p^2}}$ with window $t_{rms}$ for the Clean (2 ms) and Vocal (10 ms) styles;
$X = 20\log_{10} e$.

**State with hysteresis.** The gate opens when $X > T$ and closes only when $X < T - H$ and the hold counter
($t_{hold}\,f_s$ samples, reloaded at every crossing) has run out.

**Target gain** $\Delta^*$ (dB):

- open: $0$ dB;
- closed: downward expansion with knee $K$, $u = X - T$ for $u < -K/2$, else $-\frac{(K/2 - u)^2}{2K}$;
  $\Delta^* = \max(u\,(R-1),\ \text{range})$ — a ratio of 100 behaves as a gate, small ratios as a gentle expander;
- Upward style, open: $\Delta^* = \text{clamp}((X - T)(1 - 1/R),\ 0,\ \min(-\text{range}, 24))$.

**Ballistics.** $\Delta[n] = \Delta^* + c(\Delta[n-1] - \Delta^*)$, attack coefficient when $\Delta^* > \Delta$
(opening), release otherwise; the style scales attack, release and hold:

| style | detector | attack × | release × | hold × |
|---|---|---|---|---|
| Classic | peak | 1 | 1 | 1 |
| Clean | RMS 2 ms | 1 | 1 | 1 |
| Vocal | RMS 10 ms | 2 | 1.5 | 1 |
| Guitar | peak | 0.5 | 2 | 2 |
| Upward | peak | 1 | 1 | 1 |

**Output.** Flip (ducking) uses $\Delta' = \text{range} - \Delta$. With the look-ahead-delayed input $x_D$:
$y = (x_D + \mu(x_D 10^{\Delta'/20} - x_D))\,10^{O/20}$.

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
