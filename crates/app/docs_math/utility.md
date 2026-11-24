Gain, balance, width and channel tools, in this order: swap → polarity → mid/side width (or mono) → DC
filter → gain × balance.

$$m = \tfrac12(a + b),\quad s = \tfrac12(a - b)\,W,\quad a' = m + s,\ b' = m - s$$

(mono sets $W = 0$). DC filter: one-pole high-pass at 8 Hz, $y = x - \text{LP}_{8\,\text{Hz}}(x)$.
Output $L = a'\,G\,g_L$, $R = b'\,G\,g_R$ with $G = 10^{g/20}$ and the balance law
$p \le 0$: $(1, 1+p)$, $p > 0$: $(1-p, 1)$.

<!-- knob: gain -->
$G = 10^{g/20}$, ±36 dB.

<!-- knob: pan -->
Balance $p$: attenuates the opposite side linearly (stereo is preserved, no centre boost).

<!-- knob: width -->
$W$: side gain; 0 = mono, 1 = unchanged, 2 = side doubled (+6 dB).

<!-- knob: invert_l -->
$a \to -a$ (polarity flip of the left channel).

<!-- knob: invert_r -->
$b \to -b$.

<!-- knob: swap -->
Exchanges left and right.

<!-- knob: mono -->
$W = 0$: both channels carry $m = (L + R)/2$.

<!-- knob: dc -->
One-pole high-pass at 8 Hz (removes DC offset).
