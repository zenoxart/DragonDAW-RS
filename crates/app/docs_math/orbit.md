An auto-panner: a position $p \in [-1, 1]$ follows a shape over cycles (bar-locked, free or one step per hit),
is smoothed, and pans the signal with one of three laws.

**Cycle position.** Sync: $c = \frac{\text{beat}}{b} + \varphi_0$ from the song position ($b$ beats per cycle),
so the movement is locked to the bars; Free: $c$ advances $f/f_s$ per sample; $\phi = c - \lfloor c\rfloor$.

**Shapes** $s(\phi, n)$ for cycle $n$: sine $\sin 2\pi\phi$; triangle $1 - 4\left|((\phi + \tfrac14) \bmod 1) - \tfrac12\right|$;
square $\pm1$; saw up $2\phi - 1$; saw down $1 - 2\phi$; random steps $h(n)$; smooth random
$h(n) + (h(n+1) - h(n))\frac{1 - \cos\pi\phi}{2}$, with $h$ a SplitMix64 hash of the cycle number mapped to
$[-1, 1]$ (the same cycle always gives the same value, so playback is repeatable).

**Target.** $p^* = \text{clamp}(C + W s, -1, 1)$ (centre $C$, width $W$).
Trigger mode: an envelope (0.5 ms / 40 ms) crossing the threshold (re-armed 3 dB below, at least 30 ms
apart) advances one step: $p^* = C + W s(\varphi_0 + k\Delta, k)$ with $\Delta = \tfrac18$ ($\tfrac12$ for square).

**Smoothing.** $p \leftarrow p + \kappa(p^* - p)$, $\kappa = 1 - e^{-1/(\tau f_s)}$ with
$\tau = 2\,\text{ms} + 0.25\,\sigma\,T_{cycle}$ (or the glide time in Trigger mode).

**Pan laws.** Equal power on the mono sum $m = \frac{L+R}{2}$: $\theta = (p+1)\frac\pi4$,
$(L, R) = (\sqrt2\cos\theta\,m,\ \sqrt2\sin\theta\,m)$; Balance keeps stereo: $L\min(1-p, 1)$, $R\min(1+p, 1)$.

**Binaural.** The far ear gets $m(1 - 0.6|p|)$, delayed by the interaural time difference
$\text{ITD} = 0.65\,\text{ms}\cdot|p|$ (fractional, linear interpolation) and low-passed by a one-pole at
$f_c = 20\,\text{kHz}\cdot 0.12^{|p|}$ (head shadow: 2.4 kHz at the side) — the localisation cues of a real
source on headphones.

<!-- knob: mode -->
Sync (bar-locked), Free (Hz) or Trigger (one step per detected hit).

<!-- knob: sync -->
$b$: beats per cycle, 1/16 … 4 bars.

<!-- knob: rate -->
$f$ (Hz) in Free mode.

<!-- knob: shape -->
$s(\phi, n)$ (formulas above).

<!-- knob: width -->
$W$: amplitude of the movement around the centre.

<!-- knob: center -->
$C$: the middle of the movement (−100 % left … +100 % right).

<!-- knob: phase -->
$\varphi_0$: shifts the cycle by phase/360°.

<!-- knob: smooth -->
$\sigma$: smoothing time $2\,\text{ms} + 0.25\sigma T_{cycle}$ — 0 % keeps square edges, 100 % rounds the
movement over a quarter of the cycle.

<!-- knob: threshold -->
Trigger level (dB) of the 0.5/40 ms envelope; re-arms 3 dB below.

<!-- knob: glide -->
Trigger mode: time constant of the move to the next step.

<!-- knob: law -->
Equal power ($\sqrt2\cos\theta$, $\sqrt2\sin\theta$ on the mono sum) or Balance (keeps the stereo image).

<!-- knob: binaural -->
Interaural time and level difference plus head-shadow low-pass for headphones (formulas above).
