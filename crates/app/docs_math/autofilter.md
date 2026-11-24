A resonant filter whose cutoff moves by itself.

**Filter**: the TPT state-variable filter (see the [DSP notes](../dsp/README.md)), $g = \tan(\pi f_c/f_s)$,
$k = 2 - 1.95\,r$; low-pass, band-pass, high-pass or notch (low + high). Drive feeds the filter
$\tanh(kx)/\sqrt k$, $k = 1 + 8\,\text{drive}$.

**Cutoff** in octaves around the knob:

$$f_c = f_0\cdot 2^{\,\frac{D}{2}\,\ell(\varphi) + E\cdot\min(4e,\,1)}$$

with the LFO $\ell \in [-1, 1]$ (depth $D$ octaves peak-to-peak) and the envelope $e$ of the input or the sidechain
(follower with its own attack and release), scaled by the envelope amount $E$ (±5 octaves; negative closes the filter
on loud notes).

**LFO.** Synced, the phase is the song position over the note length, $\varphi = b/\lambda$ (locked to the bar); free, it
runs at the rate. The right side runs ahead by the stereo phase. Shapes: sine, triangle, saw up and down, square, and
random (a new value each cycle, held).

<!-- knob: type -->
Low-pass, band-pass, high-pass or notch.

<!-- knob: cutoff -->
The centre $f_0$ the modulation moves around.

<!-- knob: resonance -->
Resonance $r$ (peak at the cutoff).

<!-- knob: drive -->
Saturation into the filter.

<!-- knob: sync -->
*Free* (Hz) or a note value from 1/16 to 4 bars.

<!-- knob: lfo_rate -->
LFO rate in Hz when free.

<!-- knob: lfo_shape -->
The LFO waveform.

<!-- knob: lfo_depth -->
Peak-to-peak sweep $D$ in octaves.

<!-- knob: stereo -->
Phase offset of the right side's LFO (0 … 180°): the filter moves in opposite directions at 180°.

<!-- knob: env_amount -->
Envelope sweep $E$ in octaves.

<!-- knob: env_attack -->
Attack of the envelope follower.

<!-- knob: env_release -->
Release of the envelope follower.

<!-- knob: mix -->
Wet share.

<!-- knob: output -->
Output gain.
