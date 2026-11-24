A memoryless clipper with a smooth shoulder, oversampled.

**Curve** for $a = |x|$ (sign restored afterwards), threshold $t$, ceiling $c$ (both linear, $t \le c$),
$u = \frac{a - t}{c - t}$:

$$
y = \begin{cases} a & a \le t\\ t + (c - t)\tanh u & \text{Soft}\\ t + (c - t)\frac{u}{\sqrt[4]{1 + u^4}} & \text{Hybrid}\\ \min(a, c) & \text{Hard}\end{cases}
$$

Below the threshold the signal is untouched (bit-exact); above it the shoulder approaches the ceiling
asymptotically (Soft is the gentlest, Hybrid stays linear longer then bends sharply).

**Oversampling.** The curve runs at $k f_s$ ($k = 1, 2, 4$) behind 47-tap half-band stages: harmonics
above $f_s/2$ are filtered before they can fold back.

**Output.** $y\cdot 10^{P/20}$; *Clip only* outputs $(x_D - y)\,10^{P/20}$ — the part that was removed
(with $x_D$ the input delayed by the oversampler's latency). The meter shows
$20\log_{10}\frac{|x|_{max}}{\text{curve}(|x|_{max})}$.

<!-- knob: input -->
Drive $10^{I/20}$ into the curve.

<!-- knob: threshold -->
$t = 10^{T/20}$: where the shoulder starts (clamped to the ceiling).

<!-- knob: ceiling -->
$c = 10^{C/20}$: the level the shoulder approaches (Hard: never exceeds).

<!-- knob: shape -->
Soft ($\tanh$), Hybrid ($u/\sqrt[4]{1+u^4}$) or Hard (clamp).

<!-- knob: oversampling -->
$k = 1, 2, 4$: curve evaluated at $k f_s$.

<!-- knob: post -->
$10^{P/20}$ after the curve.

<!-- knob: listen -->
Outputs only what the clipper removes, $x_D - y$.
