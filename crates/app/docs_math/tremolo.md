Amplitude (or pan) modulation by an LFO, smoothed so square waves do not click.

**Rate.** Free $f$ Hz or synced $f = \frac{\text{BPM}}{60\,b}$ ($b$ beats per cycle).

**Smoothing.** $\hat m \leftarrow \hat m + k(m - \hat m)$, $k = 1 - e^{-1/(0.0015 f_s)}$ (1.5 ms).

**Tremolo.** $g = 1 - \delta\left(\tfrac12 - \tfrac12\hat m\right)$, i.e. the gain swings between 1 and $1 - \delta$
($\delta = 1$: full chop, $20\log_{10}(1-\delta)$ dB at the bottom).

**Auto-pan.** $p = \delta\hat m$, $g_L = \min(1 - p, 1)$, $g_R = \min(1 + p, 1)$ — one side dips while the other
stays at unity.

<!-- knob: sync -->
Free or a note value: $f = \text{BPM}/(60 b)$.

<!-- knob: rate -->
Free LFO frequency.

<!-- knob: depth -->
$\delta$: the dip at the bottom of each cycle, $1 - \delta$ of the level.

<!-- knob: shape -->
Sine, triangle or square LFO (smoothed over 1.5 ms).

<!-- knob: pan -->
Modulates the balance instead of the level.
