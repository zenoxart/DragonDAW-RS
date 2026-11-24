Mid/side width with mono bass, and a phase scope.

$$m = \tfrac{L+R}{2},\quad s = \tfrac{L-R}{2},\quad s' = W\,(s - \text{LP}_{f_m}(s)),\quad L' = (m + s')G,\ R' = (m - s')G$$

The side signal's lows (RBJ low-pass at $f_m$, $Q = 0.707$) are removed, so everything below $f_m$ becomes mono;
the rest of the side is scaled by $W$.

**Scope.** With a 300 ms exponential average $\langle\cdot\rangle$: correlation
$\rho = \frac{\langle LR\rangle}{\sqrt{\langle L^2\rangle\langle R^2\rangle}}$ (+1 mono, 0 unrelated, −1 out of phase),
balance $\frac{\langle R^2\rangle - \langle L^2\rangle}{\langle L^2\rangle + \langle R^2\rangle}$ and side/mid ratio
$\sqrt{\langle s^2\rangle/\langle m^2\rangle}$; the goniometer plots $(L, R)$ points decimated to ~4 kHz.

<!-- knob: width -->
$W$: gain of the side signal above $f_m$.

<!-- knob: mono_below -->
$f_m$: side content below it is removed (RBJ low-pass subtracted from the side).

<!-- knob: output -->
$G = 10^{O/20}$.
