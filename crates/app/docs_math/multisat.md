Up to four bands (4th-order Linkwitz–Riley crossovers, $Q = 1/\sqrt2$ cascades, kept $\ge 1.2\times$ apart),
each with its own waveshaper, level-dependent drive, tone, level, mix and a resonant feedback loop. The bands
sum back without the all-pass compensation of the multiband compressor (the saturation colours the phase anyway).

**Per band** (signal $x_b$):

1. detector: peak hold → follower (5 ms / 120 ms), $E = 20\log_{10} e$;
2. drive $d = D + 12\,\delta\cdot\text{clamp}\left(\frac{E + 36}{36}, 0, 1\right)$ dB — with dynamics $\delta > 0$
   loud passages are driven harder, $\delta < 0$ softer;
3. feedback: $u = (x_b + \phi\,y[n - N])\,10^{d/20}$ with $\phi = 0.95\,\text{fb}$ and $N = f_s/f_{fb}$ samples (a comb
   tuned to $f_{fb}$ and its harmonics);
4. $y = f_{style}(u)/\sqrt{10^{d/20}}$ (partial level compensation);
5. DC blocker (5 Hz), then a high shelf (Q 0.6) at the band's centre (band 1: $x_1/2$; middle bands
   $\sqrt{x_{k}x_{k+1}}$; top: $2x_3$) with the tone gain;
6. $(x_b + \mu_b(y - x_b))\cdot 10^{L_b/20}$.

**Styles:**

| style | $f(u)$ |
|---|---|
| Clean tube | $\tanh u$ |
| Warm tube | $\tanh(u + 0.25u^2)$ |
| Tape | $\frac{1.1\tanh 0.8u}{1 + 0.08|u|}$ |
| Transformer | $\frac{u}{\sqrt{1 + |u|}}\left(1 + 0.1\sin(3u)\min(0.3|u|, 0.3)\right)$ |
| Soft clip | $1.5u - 0.5u^3$ for $|u| < 1$, else $\operatorname{sgn} u$ |
| Hard clip | $\text{clamp}(u, \pm1)$ |
| Foldback | $\sin(\tfrac\pi2 u)$ |
| Rectify | $\tfrac12(\tanh u + 0.8\tanh|u|)$ (adds even harmonics and an octave) |

Output: $(x + \mu(\sum_b y_b - x))\,10^{O/20}$.

<!-- knob: bands -->
Number of bands $N$ (1–4).

<!-- knob: x1 -->
First LR4 crossover.

<!-- knob: x2 -->
Second crossover ($\ge 1.2 x_1$).

<!-- knob: x3 -->
Third crossover ($\ge 1.2 x_2$).

<!-- knob: style -->
The band's waveshaper (table above).

<!-- knob: drive -->
$D$ (dB): gain into the shaper.

<!-- knob: dyn -->
$\delta$ (±100 %): adds up to $\pm 12$ dB of drive as the band level rises from −36 to 0 dBFS.

<!-- knob: tone -->
High-shelf gain after the shaper at the band's centre frequency.

<!-- knob: level -->
$L_b$ (dB): the band's output level.

<!-- knob: mix -->
$\mu_b$: dry/saturated blend inside the band.

<!-- knob: fb -->
$\phi = 0.95\,\text{fb}$: output of the shaper fed back into its input through a delay of $f_s/f_{fb}$ samples.

<!-- knob: fb_freq -->
$f_{fb}$: the feedback comb's fundamental (resonances at $k f_{fb}$).

<!-- knob: output -->
$10^{O/20}$ after the bands are summed.
