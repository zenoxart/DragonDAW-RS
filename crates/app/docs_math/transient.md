A transient designer: detected onsets open an *attack window* and a *sustain window*, each with its own gain
curve; the gain is applied per sample, followed by a guard against overs.

**Onset detection.** Two envelope followers on $p = \max(|x_L|,|x_R|)$: fast (0.2 ms / 8 ms) and slow
(15 ms / 120 ms). The jump $J = 20\log_{10}\frac{e_{fast}}{e_{slow}}$ triggers an onset when
$J > \vartheta$ with $\vartheta = 14 - 12.5\,\sigma_a$ dB ($\sigma_a$ = attack sensitivity 0…1) and re-arms
when $J < 0.4\vartheta$ (and at least 10 ms passed). $t$ = time since the onset.

**Window weights.** Attack, duration $d_a$, shape $\phi_a(x)$ with $x = t/d_a$:

| attack shape | $\phi_a(x)$ |
|---|---|
| Needle | $(1-x)^4$ |
| Nail | $(1-x)^{1.5}$ |
| Blunt | $r^2(3-2r)\cdot f$, $r = \min(x/0.25, 1)$, $f = 1$ for $x < 0.25$, else $1 - \left(\frac{x-0.25}{0.75}\right)^2$ |

Sustain starts at $t_0 = 0.6 d_a$, lasts $d_s$, fades in over 5 ms: $w_s = \min(\frac{t - t_0}{5\,\text{ms}}, 1)\,\phi_s\!\left(\frac{t - t_0}{d_s}\right)$:

| sustain shape | $\phi_s(x)$ |
|---|---|
| Linear | $1 - x$ |
| Non-linear | $(1-x)^{1 + 2|A_s|}$ |
| Soft linear | $\tfrac12 + \tfrac12\cos\pi x$ |

The sustain is gated off when the slow envelope is more than $10 + 40\sigma_s$ dB below the last peak
(decaying at a 0.4 s time constant), so room noise between hits is not lifted.

**Gain.** $G = 18\,(A_a w_a + A_s w_s)$ dB with the amounts $A \in [-1, 1]$: ±100 % = ±18 dB at the window's peak.
$y = (x(1-\mu) + x\,10^{G/20}\mu)\cdot 10^{O/20}$.

**Guard** (ceiling $c = -0.1$ dBFS): Clip $y \to \text{clamp}(y, \pm c)$; Limit: gain $\lambda$ falls instantly to
$c/|y|_{max}$ and recovers with a 50 ms time constant; Soft clip above 0.8: $0.8 + 0.2\tanh\frac{|y| - 0.8}{0.2}$.

<!-- knob: attack -->
$A_a$ (−100…+100 %): attack gain at the window peak $= 18 A_a$ dB.

<!-- knob: att_sens -->
$\sigma_a$: onset threshold $\vartheta = 14 - 12.5\sigma_a$ dB of fast-over-slow envelope jump.

<!-- knob: att_dur -->
$d_a$: length of the attack window; the sustain window starts at $0.6 d_a$.

<!-- knob: att_shape -->
$\phi_a$: Needle, Nail or Blunt (table above).

<!-- knob: sustain -->
$A_s$: sustain gain at the window peak $= 18 A_s$ dB.

<!-- knob: sus_sens -->
$\sigma_s$: the sustain is gated when the level is $10 + 40\sigma_s$ dB below the last peak.

<!-- knob: sus_dur -->
$d_s$: length of the sustain window.

<!-- knob: sus_shape -->
$\phi_s$: Linear, Non-linear or Soft linear (table above).

<!-- knob: guard -->
Off, Clip, Limit (instant attack, 50 ms recovery) or Soft clip (tanh above 0.8) at −0.1 dBFS.

<!-- knob: listen -->
Attack only: $x(10^{G_a/20} - 1)$; Sustain only: $x(10^{G_s/20} - 1)$ — just what each section adds or removes.

<!-- knob: mix -->
$\mu$ between dry and shaped signal.

<!-- knob: output -->
$10^{O/20}$ before the guard.
