A gate / downward expander with hysteresis, hold and look-ahead; optionally an upward expander or a ducker.

**Detector.** Sidechain (own or external) → RBJ high-pass and low-pass ($Q = 0.707$) → $p = \max(|x_L|,|x_R|)$,
or RMS $e = \sqrt{2\,\overline{p^2}}$ with window $t_{rms}$ for the Clean (2 ms) and Vocal (10 ms) styles;
$X = 20\log_{10} e$.

**State with hysteresis.** The gate opens when $X > T$ and closes only when $X < T - H$ and the hold counter
($t_{hold}\,f_s$ samples, reloaded at every crossing) has run out.

**Target gain** $\Delta^*$ (dB):

- open: $0$ dB;
- closed: downward expansion with knee $K$, $u = X - T$ for $u < -K/2$, else $-\frac{(K/2 - u)^2}{2K}$;
  $\Delta^* = \max(u\,(R-1),\ \text{range})$ — a ratio of 100 behaves as a gate, small ratios as a gentle expander;
- Upward style, open: $\Delta^* = \text{clamp}((X - T)(1 - 1/R),\ 0,\ \min(-\text{range}, 24))$.

**Ballistics.** $\Delta[n] = \Delta^* + c(\Delta[n-1] - \Delta^*)$, attack coefficient when $\Delta^* > \Delta$
(opening), release otherwise; the style scales attack, release and hold:

| style | detector | attack × | release × | hold × |
|---|---|---|---|---|
| Classic | peak | 1 | 1 | 1 |
| Clean | RMS 2 ms | 1 | 1 | 1 |
| Vocal | RMS 10 ms | 2 | 1.5 | 1 |
| Guitar | peak | 0.5 | 2 | 2 |
| Upward | peak | 1 | 1 | 1 |

**Output.** Flip (ducking) uses $\Delta' = \text{range} - \Delta$. With the look-ahead-delayed input $x_D$:
$y = (x_D + \mu(x_D 10^{\Delta'/20} - x_D))\,10^{O/20}$.

<!-- knob: threshold -->
$T$: the gate opens when the detector exceeds it.

<!-- knob: range -->
The floor of the closed gain ($\Delta \ge$ range, e.g. −80 dB ≈ silence, −10 dB a gentle dip); in Upward
style $|\text{range}|$ (max 24 dB) is the most lift.

<!-- knob: attack -->
Opening time constant: $c_a = e^{-1/(t_a 10^{-3} f_s \sigma_a)}$.

<!-- knob: hold -->
Samples the gate stays open after the level has fallen below $T - H$: $t_h\,10^{-3} f_s\,\sigma_h$.

<!-- knob: release -->
Closing time constant: $c_r = e^{-1/(t_r 10^{-3} f_s \sigma_r)}$.

<!-- knob: sidechain -->
Detect on the external key input instead of the channel itself.

<!-- knob: ratio -->
$R$ of the expansion below $T$: the output falls $R$ dB per dB of input below the threshold (100 = gate).

<!-- knob: knee -->
$K$: quadratic transition width around $T$ for the closed state.

<!-- knob: hysteresis -->
$H$: closes at $T - H$ instead of $T$, so a level hovering at the threshold does not chatter.

<!-- knob: lookahead -->
Audio delayed by $\lfloor t\,10^{-3} f_s\rfloor$ samples (max 10 ms, reported as latency) so the gate is open
before the transient.

<!-- knob: style -->
Selects detector and timing multipliers (table above); Upward lifts instead of cutting.

<!-- knob: sc_hp -->
RBJ high-pass on the detector (12 dB/oct, $Q = 0.707$).

<!-- knob: sc_lp -->
RBJ low-pass on the detector.

<!-- knob: flip -->
Inverts the action: $\Delta' = \text{range} - \Delta$ (ducks when the key is loud).

<!-- knob: mix -->
$\mu$ (%) between the delayed input and the gated signal.

<!-- knob: listen -->
Outputs the filtered sidechain $(f_L, f_R)$ instead of the audio.

<!-- knob: output -->
Final gain $10^{O/20}$.
