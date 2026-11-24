# Wyrmstack (amp & cabinet) (amp)

*Distortion*

Guitar and bass amplifier with speaker cabinet for direct input: five amps (clean, crunch, lead, bass, fuzz), gate, tone stack, presence, power-amp sag, four cabinets and three microphone positions.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Amp](amp/README.md) – Amp, Gate, Gain
- [Cabinet](cabinet/README.md) – Cabinet, Mic, Master
- [Tone](tone/README.md) – Bass, Middle, Treble, Presence, Sag

## How it works

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

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
