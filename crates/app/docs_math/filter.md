Two multimode filters (A and B) built from cascaded topology-preserving state-variable filters, with drive,
nonlinear styles, an LFO, an envelope follower and stereo spread.

**SVF stage** (see [DSP foundations]({DSP})): $g = \tan(\pi f/f_s)$, $k = 2 - 1.95\,r$, outputs LP, BP, HP.
Slope 12 / 24 / 48 dB/oct = 1 / 2 / 4 stages; only the last stage gets the resonance $r$, earlier ones $r = 0.3$.

| type | output per stage |
|---|---|
| Low / High / Band pass | LP / HP / BP |
| Notch | LP + HP |
| Peak | $u + (G - 1)\,\text{BP}$ (one stage) |
| Low / High shelf | $u + (G - 1)\,\text{LP}$ / $u + (G-1)\,\text{HP}$ (one stage) |

with $G = 10^{g_{dB}/20}$.

**Styles** shape each stage's input $u$: Clean $u$; Smooth $\tanh(0.9u)/0.9$; Ram $\tanh(1.6u)/1.2$;
Hack $\text{clamp}(u, \pm 0.9)$ — saturation inside the cascade makes the resonance growl instead of ringing.

**Drive.** $u = \tanh(d\,x)/\sqrt d$, $d = 10^{D/20}$.

**Cutoff modulation.** $f_A = f_0\,2^{m + e}$ with LFO $m = \lambda\sin(2\pi f_{lfo}t)$ (λ octaves) and envelope
$e = \epsilon\,\min(\text{env}, 1)$ (follower 5 ms / 200 ms on the peak); $f_B = f_{0,B}\,2^{(m+e)\phi}$ with the
follow amount $\phi$. Stereo spread $\sigma$ octaves: left $\times 2^{-\sigma/2}$, right $\times 2^{\sigma/2}$.
All cutoffs clamped to 20 Hz … 20 kHz.

**Routing.** Serial $y = B(A(u))$, parallel $y = \tfrac12(A(u) + B(u))$; output $(1-\mu)x + \mu y$.

<!-- knob: type -->
Response of filter A (table above).

<!-- knob: cutoff -->
$f_0$: cutoff of A (before modulation and spread).

<!-- knob: resonance -->
$r$: SVF damping $k = 2 - 1.95r$ of the last stage; the peak at $f_0$ grows towards self-oscillation as $k \to 0.05$.

<!-- knob: lfo_rate -->
Sine LFO frequency.

<!-- knob: lfo_depth -->
$\lambda$ (octaves): $f \times 2^{\lambda\sin}$.

<!-- knob: env_depth -->
$\epsilon$ (octaves, ±): envelope (5 ms / 200 ms) opens (+) or closes (−) the filter with the level.

<!-- knob: mix -->
$\mu$: dry/filtered blend.

<!-- knob: slope -->
Number of SVF stages: 1, 2 or 4 (12, 24, 48 dB/oct for the pass types).

<!-- knob: style -->
Nonlinearity in front of each stage (Clean, Smooth, Ram, Hack).

<!-- knob: drive -->
$D$ (dB): $\tanh(dx)/\sqrt d$ before the filters.

<!-- knob: gain -->
$g_{dB}$ of the peak and shelf types: $u + (10^{g/20} - 1)\cdot$band.

<!-- knob: pan -->
$\sigma$ (octaves): cutoffs spread apart, left $2^{-\sigma/2}$, right $2^{\sigma/2}$.

<!-- knob: b_on -->
Enables filter B.

<!-- knob: b_type -->
Response of B.

<!-- knob: b_cutoff -->
$f_{0,B}$.

<!-- knob: b_resonance -->
Resonance of B.

<!-- knob: b_slope -->
Stages of B.

<!-- knob: routing -->
Serial $B(A(u))$ or parallel $\tfrac12(A + B)$.

<!-- knob: b_follow -->
$\phi$: how much of A's modulation (LFO + envelope) B follows.
