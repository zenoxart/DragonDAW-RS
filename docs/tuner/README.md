# Siren's Fork (tuner) (tuner)

*Utility*

Chromatic tuner: the played note, how many cents it is off, a needle or a strobe. A4 reference 430 – 450 Hz; mute silences the output while you tune.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Tuner](tuner/README.md) – A4, Range, Display, Mute

## How it works

A chromatic tuner. The audio passes through unless *Mute* is on.

**Pitch.** Every 1024 samples the YIN detector runs on the newest 2048 samples (4096 for the bass and wide ranges,
which reach down to 28 Hz): difference function, cumulative-mean normalisation and the first dip below the
threshold (see [Pitch correction](../pitch-correction/README.md)). Readings with a confidence below 0.6 are ignored;
a reading within 4 % of the last one glides halfway to it (a steady needle), a new note jumps.

**Note and cents** against the reference $A_4 = f_{ref}$:

$$m = 69 + 12\log_2\frac{f}{f_{ref}},\qquad \text{note} = \operatorname{round}(m),\qquad c = 100\,(m - \operatorname{round}(m))$$

The display shows the note while the confidence is above 0.3 and the input above −60 dBFS; it turns green within
±3 cents.

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
