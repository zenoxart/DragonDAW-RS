A look-ahead brickwall limiter whose gain is computed from the peak of the next 3 ms, optionally on true
(inter-sample) peaks and at 2× / 4× the sample rate.

**Look-ahead.** $L = \max(8, \lfloor 3\,\text{ms}\cdot f_s' \rfloor)$ samples at the processing rate $f_s'$.
Each input frame (after input gain $10^{G/20}$) is written into a ring of length $L$ together with its peak
$p$; the output is the frame leaving the ring.

**Peak.** $p = \max(|a|,|b|)$, or with true peak on also the 4× interpolated sample values: four polyphase
12-tap windowed-sinc interpolators $h_\phi[i] = \operatorname{sinc}(i - 6 + \phi/4)\,(\tfrac12 + \tfrac12\cos\frac{\pi(i - 6 + \phi/4)}{6.5})$.

**Gain computer.** $P = \max$ of the peaks in the ring (everything that will leave in the next $L$ samples),
$g^* = \min\!\left(1, \frac{c\,m}{P}\right)$ with ceiling $c = 10^{C/20}$ and margin $m = 0.995$ (true peak) or 1.

- falling: $g \leftarrow g^* + (g - g^*)\,a$, $a = e^{-5k_a/L}$ — five time constants within the window, so the
  gain has arrived before the peak leaves;
- rising (release): fast $g_f = g^* + (g - g^*) r_f$, slow $g_s = g^* + (g_s - g^*) r_s$ with
  $r = e^{-1/(t_r k_r 10^{-3} f_s')}$ and $r_s$ six times slower; $g = (1 - \pi) g_f + \pi\,\text{clamp}(g_s, g, g_f)$.
  The program share $\pi$ keeps the gain down after sustained limiting and lets it recover fast after a single peak.

Aggressive and Modern styles soft-clip before the detector: above $c(1 - \kappa)$,
$x \to \operatorname{sgn}(x)\max\!\left(k\tanh\frac{|x|}{k},\ c(1-\kappa)\right)$, $k = c(1+\kappa)$.

| style | $k_r$ (release ×) | $k_a$ (attack sharpness) | soft clip $\kappa$ | program $\pi$ |
|---|---|---|---|---|
| Allround | 1 | 1 | 0 | 0.2 |
| Transparent | 2 | 0.6 | 0 | 0.6 |
| Punchy | 0.5 | 1.6 | 0 | 0 |
| Dynamic | 0.7 | 1 | 0 | 0.8 |
| Aggressive | 0.4 | 2 | 0.35 | 0 |
| Modern | 1 | 1.2 | 0.15 | 0.4 |
| Bus | 1.5 | 0.8 | 0 | 0.3 |
| Safe | 3 | 0.5 | 0 | 0.5 |

A final clamp to $\pm c$ catches anything the smoothing missed; after oversampling the down-filter output is
clamped again. Latency: $L/2^{k} + $ the half-band filters' delay, compensated by the mixer.

<!-- knob: gain -->
$G$ (dB) input drive before the limiter: every dB above the ceiling becomes gain reduction.

<!-- knob: ceiling -->
$C$ (dBTP): the output never exceeds $10^{C/20}$ (with true peak also between samples, within the 4×
estimate's accuracy and the 0.995 margin).

<!-- knob: release -->
$t_r$ (ms), scaled by the style's $k_r$: $r = e^{-1/(t_r k_r 10^{-3} f_s')}$ for the fast path, 6× longer for
the slow path.

<!-- knob: true_peak -->
Detects the 4× interpolated waveform (inter-sample peaks that a DAC would reconstruct) and keeps a 0.5 %
margin.

<!-- knob: style -->
Selects $k_r$, $k_a$, soft clip and program share (table above).

<!-- knob: oversampling -->
Runs the whole core at $2^k f_s$ behind 47-tap half-band filters ([DSP foundations]({DSP})):
fast gain changes alias less and inter-sample peaks are caught directly.

<!-- knob: unity -->
Divides the output by the input gain $10^{G/20}$, so only the effect of limiting (not the level gain) is heard.
