# Serpent's Hush (de-esser) (deesser)

*Dynamics*

De-esser: single-vocal (relative) or allround detection, split- or wide-band processing, detection band, threshold, range, look-ahead, stereo link and listen.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Detection](detection/README.md) – Detection high-pass, Detection low-pass, Mode, Processing
- [Monitor](monitor/README.md) – Listen
- [Reduction](reduction/README.md) – Threshold, Range, Look-ahead, Stereo link

## How it works

A de-esser with a band-limited detector, relative ("single vocal") or absolute ("allround") detection and
split-band or wide-band reduction.

**Detector band.** $s = LP(HP(x))$: RBJ high-pass at $f_{hp}$ ($Q = 0.9$) and low-pass at
$f_{lp} = \max(f_{lp}, 1.2 f_{hp})$ ($Q = 0.707$).

**Envelopes** (per channel): sibilance $e_s$ with attack 0.5 ms / release 60 ms on $|s|$; full-band $e_f$
with a 30 ms one-pole on $|x|$. In dB: $S = 20\log_{10} e_s$, $F = 20\log_{10} e_f$.

**Overshoot.**

- Allround (absolute): $o = S - T$.
- Single vocal (relative): $o = \min(S - T,\ (S - F) + 6)$ — the reduction is limited by how strong the
  sibilance is *relative to the voice*, so a loud vowel does not trigger it.

Wanted reduction $w = \min(0.8\,\max(o, 0),\ \text{range})$ dB (a 1.25:1-like slope above the threshold).

**Stereo link** $\lambda$: $w_c \leftarrow w_c + \lambda(\max(w_L, w_R) - w_c)$, then ballistics 0.5 ms
attack / 60 ms release on $w_c$.

**Processing** on the look-ahead-delayed signal $x_D$:

- Split band: a high shelf at $0.8 f_{hp}$, $Q = 0.707$, gain $-w$ dB (retuned every 4 samples when $w$
  changes by more than 0.02 dB) — only the top end is turned down;
- Wide band: $y = x_D\,10^{-w/20}$.

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
