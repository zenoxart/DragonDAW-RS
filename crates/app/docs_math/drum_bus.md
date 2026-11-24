A drum bus processor built from modules in a fixed order:

```text
input gain → drive (saturator, oversampled) → drive high-pass → harmonics → compression (parallel mix)
  → lift → harmonic EQ → + parallels (Sub, Low, Loud, Grit) → output rails
```

All filters are RBJ biquads per channel, retuned in place (state kept, so turning a knob never clicks).

**Drive.** The stock saturator runs on every block (its oversampling latency stays constant) with, for the knob value
$u = v/100$:

$$\text{drive} = 24u\ \text{dB},\qquad \text{output} = -10u\ \text{dB},\qquad \text{mix} = \min(20u,\ 1)$$

so the colour fades in over the first 5 % of travel instead of jumping in, and the output trim compensates roughly
for the gain the curve adds. Its output goes through the **Drive high-pass**.

**Harmonics.** The kick's fundamental region $b = BP_{f_c,\,Q=1.2}(x)$ (focus $f_c$ = 50 or 80 Hz) is shaped and
low-passed at $4f_c$ ($Q$ 0.7):

$$h = LP_{4f_c}\!\big(0.5\tanh(6b) + 0.3\tanh(3|b|)\big),\qquad y = x + 1.5\,\eta\,h$$

The $\tanh(6b)$ term adds odd harmonics, the rectified $|b|$ term even ones (octave, and a DC that the low-pass and the
following stages absorb); $\eta$ = Harmonics / 100.

**Lift** (amount $\lambda$): $y = x + 0.35\lambda\tanh\!\big(3\,HP_{2.5\,kHz}(x)\big) + 0.3\lambda\left(\frac{\tanh 1.5x}{1.5} - x\right)$
– an exciter on the top plus a little overall rounding.

**Harmonic EQ.** Five fixed bands (60, 180, 900 Hz, 3.5 and 10 kHz), each a band-pass $b_k$ with $Q$ 0.9. A band does not
boost linearly: it *adds* saturated content of its region,

$$y = x + \sum_k a_k\left(0.5\tanh(2b_k) + 0.5\,b_k\right),\qquad a_k^* = 10^{G_k/20} - 1$$

where $G_k$ is the knob in whole dB (0 … 10). $a_k$ glides towards $a_k^*$ with a 5 ms one-pole, so a 1 dB step is a
quick change, not a click. For small signals the band rises by $\approx 20\log_{10}(1 + a_k)= G_k$ dB at its centre;
louder hits saturate and gain harmonics instead of more level.

**Parallels.** Added to the main path after the Harmonic EQ, each scaled by its knob $p$:

| parallel | source | processing | added |
|---|---|---|---|
| Sub | $LP_{70}(x)$ | $\tanh(4s)$ | $0.5\,p$ |
| Low | $LP_{160}(x)$ | compressor: −30 dB, 8:1, 15 / 120 ms, +14 dB | $0.6\,p$ |
| Loud | $x$ | compressor: −36 dB, 20:1, 0.5 / 60 ms, +22 dB | $0.6\,p$ |
| Grit | $0.6\tanh(3\,HP_{1.2k}(x))$ | transient shaper: attack +80, sustain −40, 12 ms | $0.6\,p$ |

**Output.** $y = 16\tanh\!\big((x + \textstyle\sum\text{parallels})\,O/16\big)$, $O = 10^{o/20}$: rails near +24 dBFS,
linear at normal levels.

**Meters.** Input and output RMS over ≈ 300 ms, $\bar p[m] = p + (\bar p[m-1] - p)\,e^{-N/(0.3 f_s)}$ per block of $N$
samples; the target lamp lights when the block's input peak exceeds 3 × the input RMS (a crest above ≈ 9.5 dB: the
hits are still punchy).

<!-- knob: input -->
Input gain $10^{g/20}$ before everything else: it sets how hard the drive, the harmonics and the compressors are hit.

<!-- knob: drive -->
$u = v/100$ maps to the saturator's drive ($24u$ dB), its output trim ($-10u$ dB) and its mix ($\min(20u, 1)$): fully
wet from 5 % up.

<!-- knob: colour -->
The saturator's curve: *A (warm)* tube mode (12AX7 stage), *B (tape)* tape mode, *C (crunch)* tube mode with an EL34
power stage (harder, more odd harmonics).

<!-- knob: drive_hp -->
RBJ high-pass ($Q$ 0.7) after the drive, 10 … 120 Hz: removes the sub the saturation adds and keeps it out of the
compressor's detector.

<!-- knob: harmonics -->
Amount $\eta = v/100$ of the harmonics path, added as $1.5\,\eta\,h$ (see above).

<!-- knob: focus -->
Centre $f_c$ of the harmonics band-pass: 50 Hz (sub kicks, 808s) or 80 Hz (punchier acoustic kicks). The path's
low-pass follows at $4f_c$ (200 or 320 Hz).

<!-- knob: comp_mode -->
Settings of the bus compressor (stock compressor, auto make-up on, threshold −22 dB):

| mode | style | ratio | attack | release |
|---|---|---|---|---|
| A (glue) | Bus | 4:1 | 10 ms | 150 ms |
| B (punch) | Punch | 6:1 | 25 ms | 80 ms |

B's slower attack lets the transient through before it clamps down.

<!-- knob: comp_mix -->
Parallel blend $c = v/100$ of the compressed signal: $y = x + c\,(C(x) - x)$, with the dry and compressed copies taken
from the same block.

<!-- knob: lift -->
Amount $\lambda = v/100$ of the exciter (see **Lift** above).

<!-- knob: heq1 -->
Harmonic EQ band, in whole dB 0 … 10 (values are rounded to the step). Band $k$ adds
$a_k(0.5\tanh(2b_k) + 0.5b_k)$ with $a_k = 10^{G_k/20} - 1$, gliding over ≈ 5 ms; the five bands sit at 60, 180, 900 Hz,
3.5 and 10 kHz.

<!-- knob: sub -->
Parallel saturated sub: $0.5\,p\tanh(4\,LP_{70}(x))$ added to the output.

<!-- knob: low -->
Parallel squashed lows: the lows below 160 Hz through a fixed compressor (−30 dB, 8:1, 15 / 120 ms, +14 dB make-up),
added at $0.6\,p$.

<!-- knob: loud -->
Parallel crushed full band: the whole signal through a fixed compressor (−36 dB, 20:1, 0.5 / 60 ms, +22 dB make-up),
added at $0.6\,p$ – classic parallel ("New York") compression.

<!-- knob: grit -->
Parallel transients: the band above 1.2 kHz, saturated ($0.6\tanh(3x)$) and run through a transient shaper that keeps
the attack (+80) and cuts the sustain (−40, 12 ms attack window), added at $0.6\,p$.

<!-- knob: output -->
Output gain $O = 10^{o/20}$ into the rails $16\tanh(xO/16)$.
