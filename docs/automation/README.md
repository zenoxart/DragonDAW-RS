# Automation

- Right-click any knob › **Create automation lane** draws its movement in the playlist.
- **MIDI learn** in the same menu ties a knob to a controller knob or fader.
- Every effect has a modulation section (LFOs and envelope followers) under its panel.

## How it works

**Curves.** Between points $(b_a, v_a)$ and $(b_b, v_b)$ the value is

$$v(b) = v_a + (v_b - v_a)\,\varphi(t),\qquad t = \frac{b - b_a}{b_b - b_a},\qquad \varphi(t) = \begin{cases} t & \tau = 0\\ \frac{e^{6\tau t} - 1}{e^{6\tau} - 1} & \tau \ne 0\end{cases}$$

where $\tau \in [-1, 1]$ is the segment's tension (positive: slow start, negative: fast start). Before the first point
the first value holds, after the last the last.

**Evaluation.** Lanes are read once per engine block (≤ 128 samples) and the parameter is ramped linearly across the
block. Effect parameters additionally pass the effect's own smoothing (see the [DSP notes](../dsp/README.md)), so even
a fast automation step arrives as a short glide.

**MIDI learn.** A controller value $c \in 0 … 127$ maps linearly onto the learned range,
$v = v_{min} + \frac{c}{127}(v_{max} - v_{min})$; notes map their velocity the same way. 14-bit pitch bend is decoded
as $(d_2 \ll 7 \,|\, d_1) - 8192$.

[Manual](../README.md)
