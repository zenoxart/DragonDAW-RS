A shimmer reverb: on every round through the reverb the tail is pitch-shifted, so it climbs (an octave per round by
default) into a halo.

**Network.** Pre-delay, four series all-pass diffusers (4.7 – 11.3 ms, $g$ 0.6), then a feedback delay network of
four lines (47 – 90 ms × size) mixed with a Householder matrix $y_i = o_i - \tfrac{2}{N}\sum_j o_j$ (lossless, every line
feeds every other). Each line's loop gain sets the decay, $g_i = 10^{-3L_i/(T_{60} f_s)}$ (−60 dB after the decay
time), after a one-pole low-pass (damping). Two lines are modulated slowly (0.31 Hz) against metallic ringing.

**The shimmer.** The lines' sum goes through a pitch shifter – two taps sweeping through a 60 ms delay in opposite
halves of a cycle, crossfaded with $\sin^2$ (the taps' drift $1 - \rho$ per sample shifts by $\rho = 2^{s/12}$) – and is
fed back:

$$\text{line}_i \leftarrow x/2 + g_i\,\mathrm{LP}\big((1 - \mu)\,y_i + \tfrac{\mu}{2}\,\mathrm{shift}(\textstyle\sum o)\big)$$

with $\mu$ the shimmer amount. A soft ceiling in each loop keeps it bounded at any setting.

<!-- knob: shift -->
Pitch step per round, $s$ semitones (+12 = an octave up).

<!-- knob: shimmer -->
Share $\mu$ of the feedback that goes through the shifter.

<!-- knob: decay -->
$T_{60}$: time to fall by 60 dB.

<!-- knob: size -->
Scales the four line lengths.

<!-- knob: damping -->
Cutoff of the loop low-pass: lower values darken the tail as it decays.

<!-- knob: predelay -->
Delay before the reverb.

<!-- knob: modulation -->
Depth of the line modulation (up to ±1.2 ms).

<!-- knob: mix -->
Wet share.

<!-- knob: output -->
Output gain.
