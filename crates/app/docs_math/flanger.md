A short modulated delay with feedback: the comb filter $1 \pm g z^{-d}$ whose notches sweep.

$$d_c[n] = \left(D_0 + 5\,\text{ms}\cdot\delta\cdot\tfrac{1 + \sin(2\pi(f n/f_s + \varphi_c))}{2}\right)10^{-3} f_s,\quad \varphi_L = 0,\ \varphi_R = \tfrac14$$

$$w = \text{line}(d),\qquad \text{line} \leftarrow x + g\,w,\qquad y = x\left(1 - \tfrac\mu2\right) + \mu w$$

The feedback comb $H(z) = \frac{z^{-d}}{1 - g z^{-d}}$ has peaks at multiples of $f_s/d$ (positive $g$) or
halfway between them (negative $g$); as $d$ sweeps from $D_0$ to $D_0 + 5\delta$ ms the peaks and notches
glide — the jet sound. Their height grows as $\frac{1}{1 - |g|}$.

<!-- knob: rate -->
$f$ (Hz): sweep frequency.

<!-- knob: depth -->
$\delta$: sweep range $5\delta$ ms.

<!-- knob: feedback -->
$g$ (−0.9 … 0.9): comb resonance; the sign shifts the comb by half a tooth (hollow vs. metallic).

<!-- knob: delay -->
$D_0$ (ms): the shortest delay — sets the highest comb spacing $f_s/d$.

<!-- knob: mix -->
$\mu$: wet level (dry at $1 - \mu/2$).
