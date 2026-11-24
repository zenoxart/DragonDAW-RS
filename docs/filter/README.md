# Alembic (filter) (filter)

*Filter*

Filter: two filters (serial or parallel), low / high / band pass, notch, peak and shelves, 12 / 24 / 48 dB slopes, styles (clean, smooth, ram, hack), drive, stereo spread, LFO and envelope follower.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Filter A](filter-a/README.md) – Type, Cutoff, Resonance, Slope, Style, Drive
- [Filter B](filter-b/README.md) – Filter B on, Filter B type, Filter B cutoff, Filter B resonance, Filter B slope, Filter B follows A's modulation
- [Modulation](modulation/README.md) – LFO rate, LFO depth, Envelope depth
- [Output](output/README.md) – Mix, Peak / shelf gain, Stereo spread (cutoff L/R), Routing

## How it works

Two multimode filters (A and B) built from cascaded topology-preserving state-variable filters, with drive,
nonlinear styles, an LFO, an envelope follower and stereo spread.

**SVF stage** (see [DSP foundations](../dsp/README.md)): $g = \tan(\pi f/f_s)$, $k = 2 - 1.95\,r$, outputs LP, BP, HP.
Slope 12 / 24 / 48 dB/oct = 1 / 2 / 4 stages; only the last stage gets the resonance $r$, earlier ones $r = 0.3$.

| type | output per stage |
|---|---|
| Low / High / Band pass | LP / HP / BP |
| Notch | LP + HP |
| Peak | $u + (G - 1)\,\text{BP}$ (one stage) |
| Low / High shelf | $u + (G - 1)\,\text{LP}$ / $u + (G-1)\,\text{HP}$ (one stage) |

with $G = 10^{g_{dB}/20}$.

**Styles** shape each stage's input $u$: Clean $u$; Smooth $\tanh(0.9u)/0.9$; Ram $\tanh(1.6u)/1.2$;
Hack $\text{clamp}(u, \pm 0.9)$ — saturation inside the cascade makes the resonance growl instead of ringing.

**Drive.** $u = \tanh(d\,x)/\sqrt d$, $d = 10^{D/20}$.

**Cutoff modulation.** $f_A = f_0\,2^{m + e}$ with LFO $m = \lambda\sin(2\pi f_{lfo}t)$ (λ octaves) and envelope
$e = \epsilon\,\min(\text{env}, 1)$ (follower 5 ms / 200 ms on the peak); $f_B = f_{0,B}\,2^{(m+e)\phi}$ with the
follow amount $\phi$. Stereo spread $\sigma$ octaves: left $\times 2^{-\sigma/2}$, right $\times 2^{\sigma/2}$.
All cutoffs clamped to 20 Hz … 20 kHz.

**Routing.** Serial $y = B(A(u))$, parallel $y = \tfrac12(A(u) + B(u))$; output $(1-\mu)x + \mu y$.

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
