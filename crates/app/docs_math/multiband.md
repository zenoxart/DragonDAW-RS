Up to four bands, each with its own up- or downward compressor or expander, in one of two topologies.

**Crossover mode.** Each split is a 4th-order Linkwitz–Riley crossover: two cascaded RBJ Butterworth
sections ($Q = 1/\sqrt2$) for the low-pass and for the high-pass,

$$H_{LP}(s) = \left(\frac{\omega_c^2}{s^2 + \sqrt2\,\omega_c s + \omega_c^2}\right)^2,\qquad H_{LP} + H_{HP} = \text{all-pass}.$$

The signal is split low → high: band 1 = LP₁(x), the rest = HP₁(x) is split again at the next crossover, and so on.
Each lower band is then passed through the all-pass ($LP + HP$) of every crossover above it, so all bands share
the same phase and $\sum_b y_b$ is flat in magnitude (unity gain at every frequency when nothing is processed).
Crossovers are kept at least $\times1.25$ apart: $f_2 \ge 1.25 f_1$, $f_3 \ge 1.25 f_2$.

**Dynamic-phase mode.** No band split at all: each band is a filter on the full signal whose gain is the
band's current gain change — a low shelf at $f_1$ (band 1), a high shelf at $f_{N-1}$ (the top band) and bells
between, centred at $f_c = \sqrt{f_{lo} f_{hi}}$ with $Q = \text{clamp}\!\left(\frac{f_c}{f_{hi}-f_{lo}}, 0.3, 4\right)$.
While no band acts, every filter is at 0 dB and the signal is bit-transparent (no phase shift). The filters are
retuned every 8 samples when a gain moved by more than 0.01 dB.

**Detector per band.** $e_b = \text{peakhold}(\max(|x_{L,b}|, |x_{R,b}|))$ with a hold of
$\min(600/f_{low,b}, 30)$ ms (half a period of the band's lowest frequency), $X_b = 20\log_{10} e_b$.

**Gain law** (threshold $T$, ratio $R$, knee $K$, overshoot $o = X - T$). With the knee functions

$$s(o) = \begin{cases}0 & o < -K/2\\ \frac{(o + K/2)^2}{2K} & |o| \le K/2\\ o & o > K/2\end{cases}\qquad
u(o) = \begin{cases}o & o < -K/2\\ -\frac{(K/2 - o)^2}{2K} & |o|\le K/2\\ 0 & o > K/2\end{cases}$$

| mode | gain change $\Delta$ (dB) |
|---|---|
| Compress | $-s(o)\,(1 - 1/R)$ |
| Expand (downward) | $u(o)\,(R - 1)$ |
| Upward compress | $-u(o)\,(1 - 1/R)$ (lifts material below $T$) |
| Upward expand | $\tfrac12\,s(o)\,(R - 1)$ (lifts material above $T$) |

then $\Delta \leftarrow \text{clamp}(\Delta, -\text{range}, +\text{range})$, followed by attack/release
ballistics: $\Delta[n] = \Delta^* + c(\Delta[n-1] - \Delta^*)$ with $c$ = attack coefficient while
$|\Delta^*| > |\Delta|$, otherwise release.

**Output.** $y = \left(x + \mu\,(\sum_b 10^{(\Delta_b + G_b)/20} x_b - x\right)) \cdot 10^{O/20}$ with the
mix $\mu$, band gain $G_b$ and output $O$.

<!-- knob: bands -->
$N \in \{1,\dots,4\}$: the number of bands; $N-1$ crossovers are active.

<!-- knob: x1 -->
First crossover $f_1$ (LR4, −6 dB point of both halves). Band 1 is everything below it.

<!-- knob: x2 -->
Second crossover $f_2 \ge 1.25 f_1$.

<!-- knob: x3 -->
Third crossover $f_3 \ge 1.25 f_2$.

<!-- knob: mix -->
$\mu$ (0–100 %) in $y = x + \mu(y_{proc} - x)$.

<!-- knob: output -->
$O$ (dB): final gain $10^{O/20}$.

<!-- knob: phase -->
Crossover (LR4 split, all-pass compensated) or Dynamic phase (shelf/bell filters on the whole signal, no
phase shift while idle). Solo always uses the split.

<!-- knob: mode -->
Selects the row of the gain-law table: compress / expand, downward / upward.

<!-- knob: thr -->
$T$: the band level (dBFS, peak with hold) the gain law is centred on.

<!-- knob: range -->
Limits $|\Delta| \le$ range: the most the band can be cut or lifted.

<!-- knob: ratio -->
$R$: slope factor of the gain law; for compression the output slope above $T$ is $1/R$, for expansion the
input below $T$ falls $R$ dB per dB.

<!-- knob: knee -->
$K$: width of the quadratic knee functions $s(o)$ and $u(o)$.

<!-- knob: attack -->
Time constant (ms) while $|\Delta|$ grows: $c = e^{-1/(t\,10^{-3} f_s)}$.

<!-- knob: release -->
Time constant (ms) while $|\Delta|$ shrinks.

<!-- knob: gain -->
$G_b$ (dB): a static gain of the band, added to $\Delta_b$.

<!-- knob: solo -->
Only soloed bands are summed (crossover mode); the others are muted.

<!-- knob: bypass -->
The band passes with gain 1 (its detector keeps running for the display).

<!-- knob: sc -->
The band's detector reads the same band of the external sidechain (split with its own crossovers) instead of
its own signal.
