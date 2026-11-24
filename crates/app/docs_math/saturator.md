A waveshaper running at twice the sample rate, with tube models that have memory (grid blocking, supply
sag, Miller roll-off), plus three parallel tube "ingredients".

**Signal flow.** $u = x\cdot 10^{D/20}$ → 2× upsampling (47-tap half-band, see [DSP foundations]({DSP})) →
shaper at $2f_s$ → downsampling → one-pole low-pass (tone) → one-pole high-pass at 5 Hz (DC removal) →
$\times 10^{O/20}$ → mix with the dry signal delayed by the oversampler's latency:
$y = (1-\mu)x_D + \mu\,\text{wet}$.

**Static curves** (mode):

| mode | $f(u)$ |
|---|---|
| Tube | tube stage (below) |
| Tape | $1.05\tanh(0.9u) + 0.02\,|u|_{\le1}\sin 3u$ |
| Soft clip | $\frac{u}{1 + |u|}$ |
| Hard clip | $\text{clamp}(u, -1, 1)$ |
| Foldback | reflect at ±1 up to 4 times: $u > 1 \to 2 - u$, $u < -1 \to -2 - u$ |
| Asymmetric | $\tanh u$ for $u \ge 0$, $\frac{0.8}{0.6}\tanh 0.6u$ for $u < 0$ |

**Tube stage.** Miller low-pass $\bar u = u + a(\bar u_{prev} - u)$, $a = e^{-2\pi f_{M}/f_s}$. Curve with
bias $b = \alpha + 0.4\beta - \Delta$ ($\alpha$ = the tube's asymmetry, $\beta$ = Bias knob, $\Delta$ = blocking shift):

- triode: $v = k\bar u$, $\frac{1}{k}\left(\frac{\tanh 0.8v}{0.8} + b(1 - e^{-v^2}) - [v > 1.2]\,0.35\tanh(0.6(v - 1.2))\right)$ —
  the $b(1 - e^{-v^2})$ term is even (2nd harmonic), the last term grid-current compression;
- pentode: $\frac{v}{k\,(1 + |v|^{2.5})^{0.4}}$ — symmetric, odd harmonics, harder knee;

blended by the model's pentode share and scaled by 1.15.

**Memory.** Grid blocking: $o = \text{peakhold}(\max(k\bar u + \alpha - 0.6, 0))$,
$\Delta \leftarrow \Delta + (0.5o - \Delta)\cdot\{0.002\,\text{blocking},\ 3\cdot10^{-5}\}\cdot\frac{48000}{f_s}$
(fast attack, very slow release) — after loud passages the operating point shifts and the sound gets grittier.
Sag: envelope $e$ (attack $5\cdot10^{-4}$, release $5\cdot10^{-5}$ per 48 kHz sample) divides the input:
$\bar u / (1 + 1.5\,s\,e)$ — power tubes compress under load. A 5 Hz DC blocker follows.

| tube | $k$ | asym $\alpha$ | pentode | sag $s$ | Miller $f_M$ | blocking |
|---|---|---|---|---|---|---|
| 12AX7 | 1.7 | 0.34 | 0 | 0 | 9 kHz | 0.6 |
| 12AT7 | 1.25 | 0.26 | 0 | 0 | 14 kHz | 0.45 |
| 12AU7 | 0.85 | 0.18 | 0 | 0 | 19 kHz | 0.3 |
| 6SN7 | 0.95 | 0.24 | 0 | 0.05 | 12 kHz | 0.35 |
| EF86 | 1.45 | 0.12 | 0.6 | 0 | 15 kHz | 0.4 |
| EL34 | 1.25 | 0.06 | 0.85 | 0.45 | 10 kHz | 0.5 |
| 6L6 | 1.0 | 0.05 | 0.7 | 0.3 | 13 kHz | 0.4 |
| KT88 | 0.8 | 0.04 | 0.6 | 0.15 | 16 kHz | 0.3 |
| 300B | 1.05 | 0.45 | 0 | 0.35 | 11 kHz | 0.5 |

**Ingredients.** Three parallel tube stages: warm 6SN7 (+0 dB), hot 12AX7 (+9 dB), crisp EF86 (+4 dB). With
amounts $a_g$ and $w_0 = \max(1 - \sum a_g, 0)$:
$\text{wet} = \frac{w_0 f(u) + \sum_g a_g\,T_g(G_g u)/\sqrt{G_g}}{w_0 + \sum a_g}$ (the $1/\sqrt{G_g}$ partly
compensates each ingredient's extra drive).

<!-- knob: mode -->
Selects the transfer curve (table above).

<!-- knob: drive -->
$D$ (dB): input gain into the shaper; the harmonic content grows with $10^{D/20}$.

<!-- knob: tone -->
One-pole low-pass after the shaper: $a = e^{-2\pi f/f_s}$ (6 dB/oct).

<!-- knob: output -->
$O$ (dB) after the shaper.

<!-- knob: mix -->
$\mu$ between the latency-aligned dry and the wet signal.

<!-- knob: tube -->
Selects the tube model's parameters (table above).

<!-- knob: bias -->
$\beta$: shifts the operating point by $0.4\beta$ — more asymmetry (even harmonics) or less.

<!-- knob: ing_warm -->
$a_1$: blend of the 6SN7 stage (soft, even).

<!-- knob: ing_hot -->
$a_2$: blend of the 12AX7 stage driven +9 dB.

<!-- knob: ing_crisp -->
$a_3$: blend of the EF86 pentode stage driven +4 dB (bright, odd).
