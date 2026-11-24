A feed-forward / feedback compressor working in the decibel domain, per sample.

**Signal flow.** detector input (own signal, or the external sidechain) → sidechain high-pass and low-pass
(RBJ, $Q = 0.707$) → peak hold (steady detection of low frequencies) → peak or RMS detector (depending on
the style) → blend with the output level (feedback styles) → static curve → attack / release ballistics on the
gain reduction → gain applied to the look-ahead-delayed audio → makeup → parallel mix.

**Detector.** With $s[n] = \max(|x_L|, |x_R|)$ after the sidechain filters and peak hold, RMS styles use
$\overline{s^2}[n] = s^2 + a_r(\overline{s^2}[n-1] - s^2)$, $a_r = e^{-1/(t_{rms} f_s)}$ and read
$e = \sqrt{2\,\overline{s^2}}$ (the $\sqrt2$ makes a sine read as its peak). Feedback styles detect
$e' = (1-\beta)e + \beta\,|y_{prev}|$ with $\beta$ the style's feedback share.

**Static curve** (threshold $T$, ratio $R$, knee $K$ in dB, input level $X = 20\log_{10} e$):

$$
Y(X) = \begin{cases}
X & X < T - K/2\\
X + \left(\frac1R - 1\right)\frac{(X - T + K/2)^2}{2K} & |X - T| \le K/2\\
T + \frac{X - T}{R} & X > T + K/2
\end{cases}
$$

The target reduction is $G^* = X - Y(X) \ge 0$ dB.

**Ballistics** on the reduction (dB), with attack $t_a$ and release $t_r$ in ms:

$$G[n] = G^* + c\,(G[n-1] - G^*),\qquad c = \begin{cases} e^{-1/(t_a 10^{-3} f_s)} & G^* > G[n-1]\\ e^{-1/(t_r 10^{-3} f_s)} & \text{otherwise}\end{cases}$$

Program-dependent styles lengthen the release with the current reduction:
$t_r' = t_r\,\sigma_r\,(1 + \pi\,G/6)$ ($\pi$ = the style's program factor, $\sigma_r$ its release scale).

**Output.** $y = (1-\mu)\,x_D + \mu\,x_D\,10^{-G/20}\,10^{M/20}$ where $x_D$ is the input delayed by the
look-ahead, $M$ the makeup and $\mu$ the mix.

**Styles** (RMS window, feedback, program factor, extra knee, attack scale, release scale):

| style | $t_{rms}$ | $\beta$ | $\pi$ | $+K$ | $\sigma_a$ | $\sigma_r$ |
|---|---|---|---|---|---|---|
| Clean | peak | 0 | 0 | 0 dB | 1 | 1 |
| Classic | peak | 1 | 0.3 | 6 dB | 1 | 1 |
| Opto | 5 ms | 0 | 1.5 | 10 dB | 3 | 1.5 |
| Vocal | 3 ms | 0 | 0.4 | 10 dB | 0.6 | 0.7 |
| Mastering | 30 ms | 0 | 0.5 | 18 dB | 0.7 | 1 |
| Bus | 8 ms | 0.3 | 0.3 | 6 dB | 1 | 1 |
| Punch | peak | 0 | 0 | 2 dB | 2.5 | 0.8 |
| Pumping | peak | 0 | 0 | 0 dB | 0.5 | 0.35 |

<!-- knob: threshold -->
$T$ in the static curve: the level (dBFS of the detector) where compression begins. Every dB the detector
reads above $T$ (beyond the knee) comes out as $1/R$ dB.

<!-- knob: ratio -->
$R$ in $Y = T + (X - T)/R$. The slope of the curve above the threshold is $1/R$: $R = 1$ does nothing,
$R = 20$ is close to limiting. On a logarithmic knob ($R = 20^{p}$), the centre is $R \approx 4.5$.

<!-- knob: knee -->
$K$: the width (dB) of the quadratic transition centred on $T$. Inside it the slope changes continuously
from 1 to $1/R$: $\frac{dY}{dX} = 1 + (\frac1R - 1)\frac{X - T + K/2}{K}$. The style adds its own knee.

<!-- knob: attack -->
$t_a$ (ms): time constant while the reduction grows, $c_a = e^{-1/(t_a 10^{-3} f_s)}$; after $t_a$ the
reduction has reached 63 % of its target. Scaled by the style ($\sigma_a$).

<!-- knob: release -->
$t_r$ (ms): time constant while the reduction falls, $c_r = e^{-1/(t_r 10^{-3} f_s)}$, scaled by the style
and, in program-dependent styles, by $(1 + \pi G/6)$.

<!-- knob: makeup -->
$M$ (dB) gain after the reduction: $\times 10^{M/20}$. With **Auto makeup** on,
$M_{auto} = -\tfrac12\,(Y(0) - 0)$: half of the reduction the curve applies at 0 dBFS is added back.

<!-- knob: auto_makeup -->
Adds $M_{auto} = -\tfrac12 Y(0)$ dB to the makeup: half the static reduction of a full-scale signal.

<!-- knob: mix -->
$\mu$ in $y = (1-\mu)x_D + \mu\,x_D\,g$: parallel ("New York") compression inside the unit, sample-aligned
because both paths use the same look-ahead-delayed $x_D$.

<!-- knob: sidechain -->
Replaces the detector input by the external key signal (chosen on the insert); the gain still acts on this
channel's audio.

<!-- knob: sc_hp -->
2nd-order RBJ high-pass ($Q = 0.707$, 12 dB/oct) on the detector path only. Bypassed at 20 Hz. Lowers the
detector's sensitivity to bass so kicks and bass notes do not pump the whole mix.

<!-- knob: sc_lp -->
2nd-order RBJ low-pass on the detector path ($f \le 0.45 f_s$). Bypassed above 19.5 kHz.

<!-- knob: style -->
Selects the detector and ballistics row of the style table above.

<!-- knob: lookahead -->
The audio is delayed by $D = \lfloor t\,10^{-3} f_s\rfloor$ samples (max 20 ms) while the detector is not, so
the gain is already falling when a transient arrives. $D$ is reported as latency and compensated by the mixer.

<!-- knob: hold -->
After the reduction stops rising it is held for $h = t\,10^{-3} f_s$ samples before the release starts,
which avoids release ripple between closely spaced hits.

<!-- knob: channels -->
*Stereo*: both sides through one linked detector. *Mid* or *Side*: the signal is split, $m = \tfrac12(l + r)$,
$s = \tfrac12(l - r)$; only the chosen component is compressed (detected on itself, or on the external sidechain), the
other is delayed by the look-ahead to stay aligned, and $l = m + s$, $r = m - s$ are rebuilt. Side compression tames
wide, harsh reverbs and stereo cymbals without touching the centre; mid compression controls a vocal or bass in a
finished mix while the stereo field stays.
