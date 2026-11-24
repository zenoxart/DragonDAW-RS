# Ouroboros (auto-panner) (orbit)

*Modulation*

Auto-panner: moves the sound across the stereo field locked to the bars, at a free rate, or one step per hit (Trigger, with glide). Seven shapes incl. random, width / centre / phase, smoothing, constant-power or balance pan law, binaural mode for headphones.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Field](field/README.md) – Width, Centre
- [Motion](motion/README.md) – Mode, Sync rate, Free rate, Shape, Phase, Smooth
- [Output](output/README.md) – Pan law, Binaural
- [Trigger](trigger/README.md) – Trigger threshold, Trigger glide

## How it works

An auto-panner: a position $p \in [-1, 1]$ follows a shape over cycles (bar-locked, free or one step per hit),
is smoothed, and pans the signal with one of three laws.

**Cycle position.** Sync: $c = \frac{\text{beat}}{b} + \varphi_0$ from the song position ($b$ beats per cycle),
so the movement is locked to the bars; Free: $c$ advances $f/f_s$ per sample; $\phi = c - \lfloor c\rfloor$.

**Shapes** $s(\phi, n)$ for cycle $n$: sine $\sin 2\pi\phi$; triangle $1 - 4\left|((\phi + \tfrac14) \bmod 1) - \tfrac12\right|$;
square $\pm1$; saw up $2\phi - 1$; saw down $1 - 2\phi$; random steps $h(n)$; smooth random
$h(n) + (h(n+1) - h(n))\frac{1 - \cos\pi\phi}{2}$, with $h$ a SplitMix64 hash of the cycle number mapped to
$[-1, 1]$ (the same cycle always gives the same value, so playback is repeatable).

**Target.** $p^* = \text{clamp}(C + W s, -1, 1)$ (centre $C$, width $W$).
Trigger mode: an envelope (0.5 ms / 40 ms) crossing the threshold (re-armed 3 dB below, at least 30 ms
apart) advances one step: $p^* = C + W s(\varphi_0 + k\Delta, k)$ with $\Delta = \tfrac18$ ($\tfrac12$ for square).

**Smoothing.** $p \leftarrow p + \kappa(p^* - p)$, $\kappa = 1 - e^{-1/(\tau f_s)}$ with
$\tau = 2\,\text{ms} + 0.25\,\sigma\,T_{cycle}$ (or the glide time in Trigger mode).

**Pan laws.** Equal power on the mono sum $m = \frac{L+R}{2}$: $\theta = (p+1)\frac\pi4$,
$(L, R) = (\sqrt2\cos\theta\,m,\ \sqrt2\sin\theta\,m)$; Balance keeps stereo: $L\min(1-p, 1)$, $R\min(1+p, 1)$.

**Binaural.** The far ear gets $m(1 - 0.6|p|)$, delayed by the interaural time difference
$\text{ITD} = 0.65\,\text{ms}\cdot|p|$ (fractional, linear interpolation) and low-passed by a one-pole at
$f_c = 20\,\text{kHz}\cdot 0.12^{|p|}$ (head shadow: 2.4 kHz at the side) — the localisation cues of a real
source on headphones.

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
