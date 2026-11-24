# Talon (transient shaper) (transient)

*Dynamics*

Transient designer: attack (Needle / Nail / Blunt) and sustain (Linear / Non-linear / Soft linear) sections with level, sensitivity, duration and shape, mix, output, and a guard (Off / Clip / Limit / Soft clip) after the output.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Attack](attack/README.md) – Attack amount, Attack sensitivity, Attack duration, Attack shape
- [Output](output/README.md) – Guard, Listen, Mix, Output
- [Sustain](sustain/README.md) – Sustain amount, Sustain sensitivity, Sustain duration, Sustain shape

## How it works

A transient designer: detected onsets open an *attack window* and a *sustain window*, each with its own gain
curve; the gain is applied per sample, followed by a guard against overs.

**Onset detection.** Two envelope followers on $p = \max(|x_L|,|x_R|)$: fast (0.2 ms / 8 ms) and slow
(15 ms / 120 ms). The jump $J = 20\log_{10}\frac{e_{fast}}{e_{slow}}$ triggers an onset when
$J > \vartheta$ with $\vartheta = 14 - 12.5\,\sigma_a$ dB ($\sigma_a$ = attack sensitivity 0…1) and re-arms
when $J < 0.4\vartheta$ (and at least 10 ms passed). $t$ = time since the onset.

**Window weights.** Attack, duration $d_a$, shape $\phi_a(x)$ with $x = t/d_a$:

| attack shape | $\phi_a(x)$ |
|---|---|
| Needle | $(1-x)^4$ |
| Nail | $(1-x)^{1.5}$ |
| Blunt | $r^2(3-2r)\cdot f$, $r = \min(x/0.25, 1)$, $f = 1$ for $x < 0.25$, else $1 - \left(\frac{x-0.25}{0.75}\right)^2$ |

Sustain starts at $t_0 = 0.6 d_a$, lasts $d_s$, fades in over 5 ms: $w_s = \min(\frac{t - t_0}{5\,\text{ms}}, 1)\,\phi_s\!\left(\frac{t - t_0}{d_s}\right)$:

| sustain shape | $\phi_s(x)$ |
|---|---|
| Linear | $1 - x$ |
| Non-linear | $(1-x)^{1 + 2|A_s|}$ |
| Soft linear | $\tfrac12 + \tfrac12\cos\pi x$ |

The sustain is gated off when the slow envelope is more than $10 + 40\sigma_s$ dB below the last peak
(decaying at a 0.4 s time constant), so room noise between hits is not lifted.

**Gain.** $G = 18\,(A_a w_a + A_s w_s)$ dB with the amounts $A \in [-1, 1]$: ±100 % = ±18 dB at the window's peak.
$y = (x(1-\mu) + x\,10^{G/20}\mu)\cdot 10^{O/20}$.

**Guard** (ceiling $c = -0.1$ dBFS): Clip $y \to \text{clamp}(y, \pm c)$; Limit: gain $\lambda$ falls instantly to
$c/|y|_{max}$ and recovers with a 50 ms time constant; Soft clip above 0.8: $0.8 + 0.2\tanh\frac{|y| - 0.8}{0.2}$.

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
