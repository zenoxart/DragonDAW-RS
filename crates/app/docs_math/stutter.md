A tempo-synced stutter. The song is cut into slices of the chosen note value; $H$ consecutive slices form a group.
At the start of a group the effect decides whether it stutters (always, or with the chance $P$ per group); if it
does, the group's first slice is played as it comes and recorded, and the remaining $H - 1$ slices replay it.

**Timing.** At song position $b$ (beats) with slice length $\lambda$ beats: slice $s = \lfloor b/\lambda\rfloor$,
group $\lfloor s/H\rfloor$, repeat number $k = s \bmod H$, position in the slice
$\tau = (b/\lambda - \lfloor b/\lambda\rfloor)\cdot\lambda\cdot\frac{60 f_s}{\text{bpm}}$ samples.

**Repeat $k$** reads the recording at speed $\sigma_k = 2^{k\,p/12}$ (the pitch step $p$ accumulates: a falling or
rising stutter), reversed with probability $R$ per slice, under a gate that is open for the first $\gamma$ of the
slice with 2 ms ramps. Switching between the live input and the repeats glides over a few milliseconds.

<!-- knob: mode -->
*Off*, *Always* (every group stutters) or *Random* (each group with the chance).

<!-- knob: division -->
The slice length $\lambda$: a quarter note down to a 1/64 note.

<!-- knob: hold -->
Slices per group $H$ (2, 4, 8, 16): the captured slice plays $H - 1$ more times.

<!-- knob: chance -->
Probability $P$ that a group stutters in *Random* mode.

<!-- knob: gate -->
The open share $\gamma$ of each slice: below 100 % the repeats become choppy.

<!-- knob: pitch -->
Pitch step $p$ per repeat in semitones (speed $2^{kp/12}$ for repeat $k$).

<!-- knob: reverse -->
Probability $R$ that a slice plays backwards.

<!-- knob: mix -->
Wet share.
