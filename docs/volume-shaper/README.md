# Tidecaller (volume shaper) (volume_shaper)

*Modulation*

Volume shaper and ducker. A tempo-synced gain curve (pump, duck, gate, saws, sine, swell) and / or ducking from a sidechain input.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Output](output/README.md) – Output
- [Shape](shape/README.md) – Mode, Shape, Rate, Depth, Tension, Phase, Smooth
- [Sidechain Duck](sidechain-duck/README.md) – Threshold, Attack, Hold, Release

## How it works

A gain modulator. The gain follows a curve that repeats with the song's tempo and/or dips when a sidechain signal
hits.

**Phase.** While the transport plays, the phase is read from the song position $b$ (beats) and the rate's length
$R$ in beats: $\varphi = \operatorname{frac}(b/R + o)$, so the curve stays locked to the bar however playback is
started or looped. Stopped, it free-runs at the tempo.

**Curves** $g(\varphi) \in [0, 1]$, bent by the tension $\tau$ as $g^{2^{2\tau}}$:

| shape | $g(\varphi)$ |
|---|---|
| Pump | $\frac{1 - e^{-6\varphi}}{1 - e^{-6}}$ (a sidechain-style exponential recovery) |
| Duck | smoothstep of $\varphi/0.25$ (back up within a quarter) |
| Gate | 1 for $\varphi < \tfrac12$, else 0 |
| Saw up / down | $\varphi$ / $1 - \varphi$ |
| Sine | $\tfrac12 - \tfrac12\cos 2\pi\varphi$ |
| Triangle | $1 - |2\varphi - 1|$ |
| Swell | $\varphi^2$ |

**Ducking.** A peak detector on the sidechain (or the input itself without one) with 0.5 ms attack and 30 ms
release, $e \leftarrow x + (e - x)\,c$; while $e$ exceeds the threshold the hold counter is reloaded. The duck gain
moves towards $1 - d$ while held and back to 1 afterwards, with one-pole coefficients
$c = e^{-1/(t\,10^{-3} f_s)}$ for the attack and release times.

**Gain.** $G^* = \big(1 - d\,(1 - g(\varphi))\big)\cdot G_{duck}$ (each factor only in the modes that use it), smoothed
by $G \leftarrow G^* + (G - G^*)\,c_s$ and multiplied by the output gain.

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
