# Dragon's Lair (reverb) (reverb)

*Space*

Room/hall/plate reverb (8-line FDN) with pre-delay, damping and width; early reflections and late reverb each have their own level and EQ.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Controls](controls/README.md) – Decay 2 time ×, Decay 3 time ×, Decay 4 time ×, Decay 5 time ×, Decay 6 time ×, Decay 2 freq, Decay 3 freq, Decay 4 freq, Decay 5 freq, Decay 6 freq, Decay 2 shape, Decay 3 shape, Decay 4 shape, Decay 5 shape, Decay 6 shape, Decay 2 Q, Decay 3 Q, Decay 4 Q, Decay 5 Q, Decay 6 Q
- [Decay Band](decay-band/README.md) – Decay 1 time ×, Decay 1 freq, Decay 1 shape, Decay 1 Q
- [Early Reflections](early-reflections/README.md) – Early level, Early size, Early feed, Early low cut, Early high cut, Early bell freq, Early bell gain
- [Late Reverb](late-reverb/README.md) – Late level, Late low cut, Late high cut, Late bell freq, Late bell gain
- [Mix](mix/README.md) – Mix
- [Space](space/README.md) – Mode, Size, Decay, Pre-delay, Width
- [Tone](tone/README.md) – Damping, Input low cut, Modulation

## How it works

An 8-line feedback delay network (FDN) with a Householder feedback matrix, frequency-dependent decay,
modulated line lengths and a tapped early-reflection section.

**Input.** $x \to$ one-pole high-pass (low cut) $\to$ pre-delay of $t_{pre}\,10^{-3} f_s$ samples (fractional,
linear interpolation).

**Early reflections.** Six taps at $\tau_k \in \{7, 11.3, 17.9, 23.1, 31.4, 41.7\}$ ms with gains
$g_k \in \{0.8, 0.65, 0.5, 0.42, 0.33, 0.25\}$, scaled by $(0.5 + S)\,E_s$ ($S$ = size, $E_s$ = early size).
Even taps stay on their side (the right one ×1.07 longer), odd taps cross to the other side at 0.7 gain
(×0.93): a decorrelated stereo pattern.

**Late field.** Line lengths $N_i = \tau_i\,10^{-3} f_s\,(0.3 + 1.7 S)\,m_{mode}$ with mutually prime-ish base
times $\tau_i \in \{29.7, 37.1, 41.1, 43.7, 53.3, 59.9, 67.1, 73.3\}$ ms. Each line output is

$$o_i = \text{DR}_i\!\left(\text{LP}_{damp}\!\left(z^{-(N_i + \delta_i[n])}\,u_i\right)\right),\qquad
\delta_i[n] = 0.0015\,f_s\,M\sin(2\pi f_i n/f_s),\ f_i = 0.3 + 0.07 i\ \text{Hz}$$

and the lines are fed back through the Householder matrix $A = I - \frac{2}{8}\mathbf{1}\mathbf{1}^T$
(lossless, maximally diffusing):

$$u_i = o_i - \tfrac{2}{8}\textstyle\sum_j o_j + 0.35\,x_{pre,(i \bmod 2)} + 0.4\,F_e\,e_{(i \bmod 2)}$$

**Decay.** For a reverb time $T_{60}$ (−60 dB), each pass through line $i$ must lose $60 N_i / (T_{60} f_s)$ dB:

$$g_i = 10^{-3 N_i / (T_{60} f_s)}$$

**Frequency-dependent decay (decay-rate EQ).** The tail is split per line by 11 one-pole crossovers into 12
log-spaced bands (centres $c_k = 30\cdot 600^{k/11}$ Hz, splits at $\sqrt{c_k c_{k+1}}$); band $k$ is fed back with
$g_i(T_{60}\,\mu(c_k))$, where the multiplier curve is

$$\mu(f) = 2^{\operatorname{clamp}(\sum_b w_b(f)\log_2 m_b,\ -2,\ 2)},\qquad x = \log_2(f/f_b)$$

with shapes $w$: bell $e^{-\frac12(1.5 Q x)^2}$, notch $e^{-\frac12(6Qx)^2}$, low shelf $\frac{1}{1 + e^{4Qx}}$,
high shelf $\frac{1}{1 + e^{-4Qx}}$ — so a band with $m_b = 2$ makes that region ring twice as long.

**Damping.** One-pole low-pass at $f_d\cdot\beta_{mode}$ inside the loop (absorption of high frequencies per reflection).

| mode | size ×$m$ | brightness $\beta$ |
|---|---|---|
| Room | 0.45 | 1 |
| Chamber | 0.7 | 0.9 |
| Hall | 1 | 1 |
| Plate | 0.6 | 1.3 |
| Cathedral | 1.6 | 0.75 |

**Output.** Even lines → left, odd → right, each $\times 0.5$, through the late EQ (RBJ high-pass, low-pass,
bell $Q = 0.8$); early reflections through the early EQ. $w = L_{late}\,\text{late} + L_{early}\,\text{early}$, then
mid/side width: $m = \frac{w_L + w_R}{2}$, $s = W\frac{w_L - w_R}{2}$, $y_{L,R} = (1-\mu)x + \mu(m \pm s)$.

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
