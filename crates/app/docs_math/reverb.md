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

<!-- knob: mode -->
Scales the line lengths ($m$) and the damping frequency ($\beta$) — see the mode table.

<!-- knob: size -->
$S$: line lengths $\propto (0.3 + 1.7S)$ and early taps $\propto (0.5 + S)$. Larger sizes lower the modal
density per second (more distinct echoes) and need a longer $T_{60}$ to sound equally long.

<!-- knob: decay -->
$T_{60}$ (s): loop gain $g_i = 10^{-3N_i/(T_{60} f_s)}$, the time for the tail to fall by 60 dB.

<!-- knob: predelay -->
Delay before the reverb (and early reflections): $t\,10^{-3} f_s$ samples.

<!-- knob: damping -->
One-pole low-pass at $f_d\beta$ in every feedback loop: highs decay faster than lows,
$a = e^{-2\pi f_d\beta/f_s}$ per pass.

<!-- knob: lowcut -->
One-pole high-pass on the input: keeps low end out of the tail.

<!-- knob: mod -->
$M$: line-length modulation depth $0.0015 f_s M$ samples (up to 1.5 ms), eight LFOs at $0.3 + 0.07i$ Hz with
spread phases — breaks up metallic ringing.

<!-- knob: width -->
$W$: side gain of the wet signal (0 = mono reverb, 1 = as generated, 1.5 = wider).

<!-- knob: mix -->
$\mu$: dry/wet blend inside the unit.

<!-- knob: dr_1 -->
Decay-time multiplier $m_b$ of the band (×¼ … ×4); see $\mu(f)$.

<!-- knob: dr_f1 -->
Centre or corner frequency $f_b$ of the decay-rate band.

<!-- knob: dr_t1 -->
Shape $w_b$: bell, low shelf, high shelf or notch (formulas above).

<!-- knob: dr_q1 -->
$Q$: steepness / width of the decay-rate band's shape in octaves.

<!-- knob: early -->
$L_{early}$: level of the early reflections in the output.

<!-- knob: er_size -->
$E_s$: scales the six early tap times.

<!-- knob: er_feed -->
$F_e$: how much of the early reflections feeds the late field ($0.4F_e$).

<!-- knob: er_lo -->
RBJ high-pass on the early reflections (bypassed at 20 Hz).

<!-- knob: er_hi -->
RBJ low-pass on the early reflections (bypassed above 19.9 kHz).

<!-- knob: er_freq -->
Centre of the early reflections' bell ($Q = 0.8$).

<!-- knob: er_gain -->
Gain of the early reflections' bell.

<!-- knob: late -->
$L_{late}$: level of the late tail.

<!-- knob: late_lo -->
RBJ high-pass on the late tail.

<!-- knob: late_hi -->
RBJ low-pass on the late tail.

<!-- knob: late_freq -->
Centre of the late tail's bell ($Q = 0.8$).

<!-- knob: late_gain -->
Gain of the late tail's bell.
