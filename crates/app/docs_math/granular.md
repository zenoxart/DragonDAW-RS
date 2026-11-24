Live granular processing: short windowed grains are read from a recording of the last four seconds.

**Scheduling.** A grain starts every $f_s/\rho\cdot(0.75 + 0.5\,u)$ samples ($\rho$ = density, $u$ uniform: irregular
spacing avoids a buzz at the grain rate); up to 64 play at once.

**A grain** of length $L$ = size reads from $D + S\,u$ behind the write head (delay $D$, spray $S$), at speed
$\sigma = \pm 2^{(p + p_r v)/12}$ ($v$ uniform in −1 … 1; negative with the reverse probability), under a Hann window
$w = \tfrac12 - \tfrac12\cos(2\pi a/L)$, panned with the equal-power law at $\pi\,u$ within the spread. A grain that
reads faster than real time starts far enough back never to overtake the write head.

**Level.** Overlapping grains add up; the sum is scaled by $1/\sqrt{\max(1,\ \rho L)}$ (incoherent addition).
Feedback writes $\tanh(g\,y)$ back into the recording (bounded however high it is set); *Freeze* stops recording, so the
grains keep scattering the same moment.

<!-- knob: size -->
Grain length $L$ (10 – 500 ms): short grains buzz and blur, long ones are recognisable fragments.

<!-- knob: density -->
Grains per second $\rho$.

<!-- knob: position -->
How far in the past grains start reading, $D$.

<!-- knob: spray -->
Random extra delay $S$ per grain: spreads the grains over time.

<!-- knob: pitch -->
Transposition $p$ of every grain.

<!-- knob: pitch_rand -->
Random transposition per grain, $\pm p_r$ semitones.

<!-- knob: reverse -->
Probability that a grain plays backwards.

<!-- knob: spread -->
Stereo spread of the grains' pan positions.

<!-- knob: feedback -->
Share $g$ of the output written back into the recording.

<!-- knob: freeze -->
Stops recording: the grains keep reading the frozen past.

<!-- knob: mix -->
Wet share.

<!-- knob: output -->
Output gain.
