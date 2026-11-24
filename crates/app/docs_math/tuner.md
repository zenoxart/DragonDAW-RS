A chromatic tuner. The audio passes through unless *Mute* is on.

**Pitch.** Every 1024 samples the YIN detector runs on the newest 2048 samples (4096 for the bass and wide ranges,
which reach down to 28 Hz): difference function, cumulative-mean normalisation and the first dip below the
threshold (see [Pitch correction](../pitch-correction/README.md)). Readings with a confidence below 0.6 are ignored;
a reading within 4 % of the last one glides halfway to it (a steady needle), a new note jumps.

**Note and cents** against the reference $A_4 = f_{ref}$:

$$m = 69 + 12\log_2\frac{f}{f_{ref}},\qquad \text{note} = \operatorname{round}(m),\qquad c = 100\,(m - \operatorname{round}(m))$$

The display shows the note while the confidence is above 0.3 and the input above −60 dBFS; it turns green within
±3 cents.

<!-- knob: reference -->
The frequency of A4, $f_{ref}$ (430 … 450 Hz). Every note is measured against it.

<!-- knob: range -->
The search range of the detector: guitar 70 – 1000 Hz, bass 28 – 400 Hz, voice 70 – 1200 Hz, wide 28 – 2000 Hz. A
narrower range is faster and avoids octave errors.

<!-- knob: display -->
*Needle*: a needle over ±50 cents. *Strobe*: bands that drift at a speed proportional to the cents off and stand
still in tune – finer than a needle.

<!-- knob: mute -->
Silences the output (the tuner keeps measuring), for tuning on stage.
