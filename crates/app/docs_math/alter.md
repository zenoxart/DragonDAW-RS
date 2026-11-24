Pitch and formant shifting by pitch-synchronous overlap-add (TD-PSOLA), so the two move independently and a voice
keeps sounding like a voice.

**Pitch detection.** Every 128 samples the last 1024 samples (summed to mono) are analysed with YIN between 70 and
1000 Hz. A detection with confidence above 0.75 sets the input period $P = f_s / f_0$; otherwise the last period is
kept.

**Analysis.** Analysis marks $t_a$ advance through the input in whole periods $P$. Around each mark a grain of two
periods is taken with a Hann window, $w(\tau) = \tfrac12\big(1 + \cos\frac{\pi\tau}{P}\big)$ for $|\tau| \le P$.

**Synthesis.** Grains are laid down at synthesis marks $t_s$ spaced by the output period $P_{out}$ (at least 16
samples), each centred where its source sits in the input. Changing the spacing changes the pitch:
$f_{out} = f_s / P_{out}$. Each grain is *resampled* while it is laid down: read at a rate $\phi$ (formant factor) it
becomes $1/\phi$ as long, which scales every resonance inside it by $\phi$ without touching the spacing. Reads between
samples are linearly interpolated.

**Output period** by mode, with $\rho = 2^{p/12}$ for Pitch $p$ and $m(f) = 69 + 12\log_2(f/440)$:

| mode | $P_{out}$ |
|---|---|
| Transpose | $P / \rho$ |
| Quantize | $f_s / 440\cdot 2^{-(\lfloor m(f_0)\rceil + \lfloor p\rceil - 69)/12}$ (nearest semitone, then transposed) |
| Robot | $f_s / 440\cdot 2^{-(72 + \lfloor p\rceil - 69)/12}$ (one fixed note, C5 at $p = 0$) |

**Normalisation.** The output sample is divided by the sum of the windows that overlap it (floored at 0.5, so the gap
between sparse grains is not blown up): grains neither boost nor dip the level when their spacing changes.

**Latency.** The output runs a fixed $3 f_s/70 + 64$ samples behind the input (three periods of the lowest voice: a
grain reaches two periods ahead of its centre when the formant goes down an octave, one behind), reported to the
mixer for delay compensation. Grains are laid down as soon as the sample being read could reach them, so a grain is
always complete before it is read.

**Character and output.** Drive $\delta$, mix $\mu$ and output $O$ are smoothed with a 10 ms one-pole
$s \leftarrow s + (v - s)(1 - e^{-1/(0.01 f_s)})$. With $d = 1 + 6\delta$ the wet signal is shaped as
$\tanh(d\,x)/\sqrt d$ (unity slope at 0 %, a tube-like rounding with roughly constant loudness as it rises), then
$y = O\,\big(x_D + \mu(w - x_D)\big)$ with $x_D$ the dry signal delayed by the same latency.

<!-- knob: pitch -->
Pitch $p$ in semitones, ratio $\rho = 2^{p/12}$ (±12 st: ½ … 2). *Transpose*: the output period is $P/\rho$. *Quantize*:
rounded to whole semitones and added to the snapped note. *Robot*: chooses the one note the voice is held on, MIDI
$72 + \lfloor p\rceil$.

<!-- knob: formant -->
Formant shift $\varphi$ in semitones. Grains are read at
$\phi = \operatorname{clamp}\!\big(2^{(\varphi + [\text{Link}]\,p)/12},\ \tfrac12,\ 2\big)$ – $\phi > 1$ shortens each grain and raises
its resonances (a smaller vocal tract), $\phi < 1$ lowers them – while the grain spacing, and so the pitch, stays.

<!-- knob: mode -->
*Transpose*: follows the voice and shifts it by **Pitch**. *Quantize*: snaps the detected pitch to the nearest semitone
at once (no glide), then transposes – the hard-tuned effect. *Robot*: ignores the sung pitch and places every grain at
the period of one note (**Pitch** picks it), so the voice speaks on a monotone.

<!-- knob: link -->
On: the formant follows the pitch, $\phi = 2^{(\varphi + p)/12}$ – like a sample played faster or slower (the
"chipmunk" effect). **Formant** then adds to it. Off: formant and pitch are independent.

<!-- knob: drive -->
Drive $\delta = v/100$, $d = 1 + 6\delta$: the wet signal becomes $\tanh(dx)/\sqrt d$. Small signals gain $\sqrt d$
(up to +8.5 dB at 100 %), peaks are rounded off.

<!-- knob: mix -->
Wet share $\mu = v/100$, against the dry signal delayed by the same latency (so the blend does not comb).

<!-- knob: output -->
Output gain $O = 10^{o/20}$, smoothed over 10 ms.
