A gain modulator. The gain follows a curve that repeats with the song's tempo and/or dips when a sidechain signal
hits.

**Phase.** While the transport plays, the phase is read from the song position $b$ (beats) and the rate's length
$R$ in beats: $\varphi = \operatorname{frac}(b/R + o)$, so the curve stays locked to the bar however playback is
started or looped. Stopped, it free-runs at the tempo.

**Curves** $g(\varphi) \in [0, 1]$, bent by the tension $\tau$ as $g^{2^{2\tau}}$:

| shape | $g(\varphi)$ |
|---|---|
| Pump | $\frac{1 - e^{-6\varphi}}{1 - e^{-6}}$ (a sidechain-style exponential recovery) |
| Duck | smoothstep of $\varphi/0.25$ (back up within a quarter) |
| Gate | 1 for $\varphi < \tfrac12$, else 0 |
| Saw up / down | $\varphi$ / $1 - \varphi$ |
| Sine | $\tfrac12 - \tfrac12\cos 2\pi\varphi$ |
| Triangle | $1 - |2\varphi - 1|$ |
| Swell | $\varphi^2$ |

**Ducking.** A peak detector on the sidechain (or the input itself without one) with 0.5 ms attack and 30 ms
release, $e \leftarrow x + (e - x)\,c$; while $e$ exceeds the threshold the hold counter is reloaded. The duck gain
moves towards $1 - d$ while held and back to 1 afterwards, with one-pole coefficients
$c = e^{-1/(t\,10^{-3} f_s)}$ for the attack and release times.

**Gain.** $G^* = \big(1 - d\,(1 - g(\varphi))\big)\cdot G_{duck}$ (each factor only in the modes that use it), smoothed
by $G \leftarrow G^* + (G - G^*)\,c_s$ and multiplied by the output gain.

<!-- knob: mode -->
*Shape*: the tempo curve alone. *Sidechain duck*: only the ducker. *Both*: the product of the two.

<!-- knob: shape -->
The curve $g(\varphi)$ (see the table above).

<!-- knob: rate -->
Length $R$ of one cycle: 1/32 note (0.125 beat) … 4 bars (16 beats).

<!-- knob: depth -->
How far the gain goes down, $d$ = depth / 100: the curve's 0 becomes $1 - d$ (−∞ dB at 100 %, −6 dB at 50 %), and
the ducker dips to $1 - d$.

<!-- knob: tension -->
Bends the curve: $g \leftarrow g^{k}$, $k = 2^{2\tau}$ ($\tau$ = tension / 100). Positive values keep the gain low for
longer (a deeper pump), negative ones recover sooner.

<!-- knob: offset -->
Phase offset $o$ (fraction of a cycle) added to $\varphi$: moves the dip off the beat.

<!-- knob: smooth -->
Time constant of the final one-pole on the gain (0.1 … 20 ms): rounds the edges of the gate and the saws so they
do not click.

<!-- knob: threshold -->
Level (dBFS) the sidechain detector must exceed to duck.

<!-- knob: attack -->
Time constant of the duck going down.

<!-- knob: hold -->
How long the duck stays down after the detector falls below the threshold.

<!-- knob: release -->
Time constant of the duck coming back up.

<!-- knob: output -->
Output gain $10^{o/20}$.
