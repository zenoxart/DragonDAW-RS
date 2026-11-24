An amplifier and speaker cabinet for a guitar or bass plugged in directly.

**Chain** (per side): gate → input high-pass (70 – 110 Hz, 30 Hz for bass) and, on the lead amp, a +6 dB bell at
800 Hz into the preamp → gain $G$ → preamp at $2f_s$ → tone stack → power amp → presence → cabinet → microphone →
master.

**Preamp stages.** At twice the sample rate (47-tap half-band filters, see the [DSP notes](../dsp/README.md)), one to
three valve stages

$$v(x) = \tanh(x + \beta) - \tanh\beta$$

each followed by a coupling high-pass (25 / 40 / 55 Hz) and a low-pass (9 / 7.5 / 6 kHz), with ×4 between stages.
The bias $\beta$ makes a stage asymmetric (even harmonics); subtracting $\tanh\beta$ keeps silence at zero. The fuzz
uses a hard, asymmetric curve instead: $v(u) = \operatorname{sign}(u)(1 - e^{-|u|})$ with positive inputs pushed 1.6×.

| amp | gain range | stages | $\beta$ |
|---|---|---|---|
| Clean | 0 – 26 dB | 1 | 0.10 |
| Crunch | 8 – 38 dB | 2 | 0.20 |
| Lead | 16 – 56 dB | 3 | 0.25 |
| Bass | 0 – 30 dB | 1 | 0.15 (plus half the clean lows below 180 Hz) |
| Fuzz | 24 – 52 dB | 1 | 0.30 |

**Power amp.** The supply sags under load: a peak envelope $e$ (50 ms release) lowers the gain to
$1/(1 + 1.5\,s\,e)$, then $y = \tanh(1.4\,x)/1.4$ – the softer compression of a valve power stage.

**Cabinets** are filter models: the box's high-pass, the cone's low resonance (90 – 140 Hz), the presence region
(1.8 – 2.6 kHz), break-up dips and two steep low-passes (4.5 – 6.6 kHz). The microphone adds a 3.5 kHz bell (on axis),
a −6 dB shelf above 3 kHz (off axis) or a 1.2 kHz dip (edge).

**Level.** The output is trimmed by $-6 - 0.25\,G_{dB}$ dB so higher gain settings come out at a similar loudness,
then multiplied by the master.

<!-- knob: model -->
Which amp (see the table above): the gain range, number of stages, bias, input high-pass and, for the bass amp, the
clean low end blended under the distortion.

<!-- knob: gate -->
Threshold of the noise gate before the amp. Its envelope rises at once and falls over 80 ms; above the threshold
the gain opens in ~1 ms, below it closes over 80 ms. At −90 dB the gate is off.

<!-- knob: drive -->
Position $u$ in the amp's gain range: $G = 10^{(G_{lo} + u(G_{hi} - G_{lo}))/20}$ into the preamp.

<!-- knob: bass -->
Low shelf at 110 Hz (80 Hz on the bass amp), $Q$ 0.6: $(v - 5)\cdot 2.4$ dB (±12 dB, flat at 5).

<!-- knob: mid -->
Bell at 700 Hz (500 Hz on the bass amp), $Q$ 0.7: $(v - 5)\cdot 2.4$ dB.

<!-- knob: treble -->
High shelf at 3.2 kHz, $Q$ 0.6: $(v - 5)\cdot 2.4$ dB.

<!-- knob: presence -->
High shelf at 5.5 kHz after the power amp: $(v - 5)\cdot 1.6$ dB (±8 dB).

<!-- knob: sag -->
Depth $s$ of the supply sag: the power-amp gain is $1/(1 + 1.5\,s\,e)$ for the load envelope $e$. More sag, more
compression and "bloom" on hard chords.

<!-- knob: cab -->
The speaker cabinet: off (amp straight), an open-back 1×12, a 2×12, a closed 4×12, a bass 4×10.

<!-- knob: mic -->
Microphone placement on the speaker: on axis (bright), off axis (darker), at the cone's edge (less mid bite).

<!-- knob: master -->
Output gain $10^{m/20}$.
