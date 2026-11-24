A modulated delay per channel; the two channels' LFOs run in quadrature (90° apart), which spreads the
detuned copies across the stereo field.

$$d_c[n] = \left(D_0 + 8\,\text{ms}\cdot\delta\cdot\tfrac{1 + \sin(2\pi(f n/f_s + \varphi_c))}{2}\right)10^{-3} f_s,\qquad \varphi_L = 0,\ \varphi_R = \tfrac14$$

$$w_c[n] = \text{line}_c(d_c[n]),\qquad y_c = x_c\left(1 - \tfrac{\mu}{2}\right) + \mu\,w_c$$

(fractional read with linear interpolation). The moving read position is a time-varying Doppler shift: the
instantaneous pitch ratio is $1 - \frac{d}{dn}d_c[n]$, at most $\pm\,8\,\text{ms}\cdot\delta\cdot\pi f$ — a gentle
detune of a few cents that sweeps up and down.

<!-- knob: rate -->
$f$ (Hz): LFO frequency (sine, quadrature between left and right).

<!-- knob: depth -->
$\delta$: sweep width, $8\delta$ ms peak-to-peak of delay modulation.

<!-- knob: delay -->
$D_0$ (ms): the shortest delay; the sweep runs from $D_0$ to $D_0 + 8\delta$ ms.

<!-- knob: mix -->
$\mu$: wet level; the dry is lowered only to $1 - \mu/2$ so the sum stays full.
