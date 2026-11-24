Slows the sound to a halt like a tape machine whose motor stops, and spins it back up.

**Reading the tape.** The input is recorded continuously. When a stop begins, a read head starts at the live
position and advances by the current speed $s$ per sample, $r \leftarrow r + s$, reading with linear interpolation –
so the pitch falls with the speed. Stopped, the output is silent.

**Speed ramps** with progress $u \in [0, 1]$ over the stop or start time, bent by the curve $c$:

$$s_{stop}(u) = (1 - u)^{2^{2c}},\qquad s_{start}(u) = 1 - (1 - u)^{2^{2c}}$$

(positive curves brake early like a heavy reel, negative ones late). After a spin-up the read head is behind the live
input; a 12 ms crossfade rejoins it.

**Darkening.** A one-pole low-pass follows the speed, $f_c = 400 + 19000\,s^{1 + 2d}$ Hz, as a slowing tape loses its
top.

**Automatic triggers** end a stop exactly on the bar line: with $B$ beats per cycle (4, 8 or 16) the tape stops when
$b \bmod B \ge B - t_{stop}\cdot\text{bpm}/60$.

<!-- knob: stop -->
Stops the tape while on (automate it); off spins it back up.

<!-- knob: mode -->
*Manual* (the switch) or automatic at the end of every bar, 2 or 4 bars.

<!-- knob: stop_time -->
How long the tape takes to stop.

<!-- knob: start_time -->
How long it takes to come back up to speed.

<!-- knob: curve -->
Bend $c$ of both ramps (see above).

<!-- knob: darken -->
How much the sound dulls as it slows ($d$ in the cutoff formula).

<!-- knob: mix -->
Wet share.
