# Wisp (exciter) (exciter)

*Distortion*

Harmonic exciter: generates new harmonics above a chosen frequency (odd or even), adds air on top and, if asked, sub harmonics below the low end. Listen plays only what it adds.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Extremes](extremes/README.md) – Air, Sub harmonics, Listen
- [Harmonics](harmonics/README.md) – Frequency, Drive, Odd / even, Amount
- [Output](output/README.md) – Output

## How it works

A harmonic exciter: it does not boost the highs that are there (an EQ would), it *generates* new harmonics from
the band above a frequency and adds a little of them.

**Path.** $h = \mathrm{HP}^2_{f}(x)$ (two RBJ high-passes, $Q$ 0.707), driven $u = g\,h$ with $g = 1 + 15\,\text{drive}$, shaped

$$v = \tfrac{1 - c}{2}\tanh u + \tfrac{1 + c}{2}\cdot\tfrac12\tanh(u^2)$$

(the odd function gives odd harmonics, the even one even harmonics; $c$ = character). The new content is
$a = \mathrm{HP}^2_{f}(v/\sqrt g - h/2)$ – the fundamental share removed and the shaper's low products filtered out –
added as $y = x + 2A\,a$.

**Sub harmonics.** Below 90 Hz a flip-flop toggles on every upward zero crossing of the low band, giving a square at
half its frequency; scaled by the low band's level, rounded by a 70 Hz low-pass and added – an octave below the kick
or bass.

**Air**: a high shelf at 12 kHz on the result.

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
