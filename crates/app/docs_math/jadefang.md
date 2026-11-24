A character equaliser: a driven input stage followed by a voicing-dependent set of RBJ filters
(see [DSP foundations]({DSP})).

**Input stage.** $u = x\cdot 10^{I/20}$, then for voicings with saturation $\sigma > 0$:

$$s(u) = u + \sigma\left(\tanh(u + 0.3\,\epsilon\,u^2) - u\right)$$

where $\epsilon$ (the voicing's "evenness") adds an asymmetric square term — even harmonics. Asymmetric
shaping creates a DC offset, removed by a DC blocker $y[n] = s[n] - s[n-1] + \rho\,y[n-1]$,
$\rho = 1 - 2\pi\cdot 8/f_s$ (an 8 Hz high-pass).

**Filters** (bands at 0 dB are left out, all frequencies clamped to $[10, 0.45 f_s]$):

| band | filter |
|---|---|
| Sub | low shelf at 18 Hz, $Q = 0.5$, gain = Sub |
| LF | low shelf at LF freq, $Q_{shelf}$; *Ember* adds a dip bell at $2.6 f$, $Q = 1$, $-0.35 G$ |
| LMF, HMF | bells, $Q_{bell}$ (*Talon*: proportional $Q = Q_{bell} + 1.6|G|/12$) |
| HF | high shelf at HF freq, $Q_{shelf}$; *Ember* adds a dip at $0.45 f$, $Q = 1$, $-0.3 G$ |
| Air | high shelf, $Q = 0.35$, gain $A\,k_{air}\,\rho$; above $0.45 f_s$ it moves to $0.55 f$ with $\rho = \sqrt{0.45 f_s / f}$ |
| colour | the voicing's fixed bands |

| voicing | $Q_{bell}$ | $Q_{shelf}$ | $\sigma$ | $\epsilon$ | colour bands |
|---|---|---|---|---|---|
| Clean scale | 0.9 | 0.7 | 0 | 0 | — |
| Hoard | 0.7 | 0.9 | 0.30 | 0.35 | bell 50 Hz $Q$ 0.7 +0.6 dB, shelf 16 kHz −0.6 dB |
| Ember | 0.8 | 0.7 | 0.35 | 0.5 | bell 120 Hz $Q$ 0.6 +0.3 dB; shelf dips |
| Talon | 0.7 | 1.2 | 0.15 | 0 | proportional Q |
| Skyfire | 0.45 | 0.45 | 0 | 0 | air ×1.2 |
| Ancient | 0.6 | 0.6 | 0.22 | 0.25 | shelf 14 kHz −1 dB |

**Output.** $y = H(s(u))\cdot 10^{(O - 0.85 I)/20}$ for saturating voicings (the input drive is mostly
compensated, so turning Input changes colour, not level), $10^{(O - I)/20}$ otherwise.

<!-- knob: model -->
Selects the row of the voicing table: Q values, dips, saturation $\sigma$, evenness $\epsilon$, colour bands.

<!-- knob: input -->
$I$ (dB): drive into $s(u)$; compensated by $-0.85 I$ (saturating voicings) or $-I$ at the output.

<!-- knob: sub -->
Low shelf at 18 Hz, $Q = 0.5$ — a very broad, gentle low-end tilt.

<!-- knob: lf_gain -->
Low-shelf gain $G$ ($A = 10^{G/40}$).

<!-- knob: lf_freq -->
Low-shelf corner frequency (midpoint of the shelf transition).

<!-- knob: lmf_gain -->
Bell gain at the LMF frequency.

<!-- knob: lmf_freq -->
Bell centre frequency.

<!-- knob: hmf_gain -->
Bell gain at the HMF frequency.

<!-- knob: hmf_freq -->
Bell centre frequency.

<!-- knob: hf_gain -->
High-shelf gain.

<!-- knob: hf_freq -->
High-shelf corner frequency.

<!-- knob: air -->
Boost-only very wide high shelf ($Q = 0.35$), gain $A\,k_{air}$.

<!-- knob: air_freq -->
Air shelf frequency: 2.5, 5, 10, 20 or 40 kHz; frequencies above $0.45 f_s$ are approximated by a lower shelf
with reduced gain so the audible part of the curve matches.

<!-- knob: output -->
$O$ (dB) after the filters.
