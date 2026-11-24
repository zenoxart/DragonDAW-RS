Ageing and wear, from tape to vinyl to early digital.

**Wow and flutter.** The signal runs through a delay around 6 ms whose length is modulated:
$d(t) = 6\,\text{ms} + W\cdot 2.4\,\text{ms}\,(\sin 2\pi\,0.6t + \delta(t)) + F\cdot 0.25\,\text{ms}\,\sin 2\pi\,9t$, with $\delta$ a
slow random drift. A changing delay is a changing pitch: the deviation is $-\dot d$, so wow at full depth sways the
pitch by about ±0.9 %.

**Age** narrows the band: an RBJ high-pass at $20\cdot 11^{a}$ Hz and two low-passes at
$20\,000\,(3500/20\,000)^{a}$ Hz (and 1.2× that), $a$ = age (0 … 1).

**Drive**: $y = \tanh(kx)/\sqrt k$, $k = 1 + 6\,\text{drive}$.

**Digital**: quantisation to $b$ bits, $y = \operatorname{round}(x\,2^{b-1})/2^{b-1}$, and sample-rate reduction by holding
each value for $f_s/f_r$ samples (no anti-alias filter: the aliasing is the point).

**Noise**: white hiss at the set level; crackle as random clicks (probability $40\,c^2/f_s$ per sample), each a burst
of noise decaying by 0.93 per sample.

The modulated delay's centre (6 ms) is reported as latency, so the track stays in time.

<!-- knob: wow -->
Depth $W$ of the slow (0.6 Hz) pitch sway and the random drift.

<!-- knob: flutter -->
Depth $F$ of the fast (9 Hz) flutter.

<!-- knob: age -->
How worn the band is (high-pass 20 → 220 Hz, low-pass 20 → 3.5 kHz).

<!-- knob: drive -->
Tape saturation.

<!-- knob: hiss -->
Level of the hiss; −90 dB is off.

<!-- knob: crackle -->
Density $c$ of the vinyl crackle.

<!-- knob: bits -->
Bit depth $b$ (24 = off).

<!-- knob: sample_rate -->
The reduced sample rate $f_r$ (48 kHz = off).

<!-- knob: mix -->
Wet share.

<!-- knob: output -->
Output gain.
