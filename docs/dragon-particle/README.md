# Philosopher's Stone (one-knob mastering) (dragon_particle)

*Mastering*

One-knob mastering: tone EQ with character, transient punch, glue compression, tube colour, widening, a soft clipper and a true-peak limiter, with auto gain for honest A/B.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Drive](drive/README.md) – Amount, Input, Character, Punch, Clipper
- [Output](output/README.md) – Ceiling, Width, Auto gain (A/B at equal loudness)
- [Tone](tone/README.md) – Low, Mid, High

## How it works

One-knob mastering: the amount $a \in [0, 1]$ drives a whole chain whose stages are the stock units
documented elsewhere (compressor, transient shaper, saturator, imager, limiter).

**Chain.** input gain → three-band tone EQ → transient punch → glue compressor → tube saturator → imager →
soft clipper → true-peak limiter → auto gain.

**Tone EQ** (RBJ, $Q = 0.7$), with the character offsets $(c_l, c_m, c_h)$:

- low shelf at 90 Hz (70 Hz for Punchy): $L + 1.5a + c_l a$ dB
- bell at 1.2 kHz: $M - 0.8a + c_m a$ dB
- high shelf at 9 kHz: $H + 2a + c_h a$ dB

| character | $c_l$ | $c_m$ | $c_h$ | saturator tube |
|---|---|---|---|---|
| Clean | 0 | 0 | 0 | 12AU7 |
| Warm | 0.8 | −0.3 | 0.3 | 6SN7 |
| Bright | −0.5 | 0.3 | 1.5 | 12AU7 |
| Punchy | 1.4 | 0.6 | 0.8 | 6SN7 |

**Stage settings** as functions of $a$:

| stage | parameters |
|---|---|
| punch (transient shaper) | attack $= 60\,P\,(0.5 + a)$ %, duration 18 ms, Nail |
| glue (compressor) | threshold $-6 - 18a$ dB, ratio $1.5 + 2.5a$, attack 30 ms, release 200 ms, knee 8 dB, makeup $10a$ dB |
| saturator | tube mode, drive $9a$ dB, output $-6a$ dB, mix $0.1 + 0.15a$ (Clean) or $0.25 + 0.35a$ |
| imager | width $1 + 0.6\,w\,(0.4 + a)$, mono below 120 Hz |
| limiter | input gain $8a$ dB, ceiling $C$, release $60 + 120(1 - a)$ ms |

**Soft clipper** with knee $k = 10^{C/20}(1 - 0.35\kappa)$: above $k$,
$|y| = k + \frac{o}{1 + o/(0.5k)}$ with $o = |x| - k$ (a rational knee that approaches $k + 0.5k$).

**Auto gain.** Running mean powers $P_{in}, P_{out}$ with a 3 s time constant; output $\times\sqrt{P_{in}/P_{out}}$
(clamped to 0.1 … 2), ramped across each block — compare at equal loudness.

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
