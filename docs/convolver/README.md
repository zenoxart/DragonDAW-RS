# Reliquary (convolution reverb) (convolver)

*Space*

Convolution reverb. Built-in rooms, halls, a plate, a spring, speaker cabinets and a telephone line, or any impulse-response file; size stretches the response, damping darkens its tail.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Output](output/README.md) – Mix, Output
- [Space](space/README.md) – Space, Size, Damping, Reverse
- [Tone](tone/README.md) – Pre-delay, Low cut, High cut, Width

## How it works

Convolution reverb: the output is the input convolved with an impulse response $h$ – a recorded or synthesised
"fingerprint" of a space, $y[n] = \sum_k h[k]\,x[n-k]$.

**Partitioned convolution.** Direct convolution with a response of $L$ samples costs $L$ multiplies per sample
(several hundred thousand for a hall). Reliquary uses uniformly partitioned overlap-save: the response is cut into
$P = \lceil L/B \rceil$ partitions of $B = 256$ samples, each zero-padded to $N = 2B$ and transformed once,
$H_p = \mathrm{FFT}(h_p)$. Every $B$ input samples the last $N$ inputs are transformed, $X_t = \mathrm{FFT}(x_{t})$,
pushed into a frequency-domain delay line, and the output block is

$$Y_t = \sum_{p=0}^{P-1} X_{t-p}\,H_p,\qquad y_t = \text{last } B \text{ samples of } \mathrm{IFFT}(Y_t)/N$$

so the cost per sample is $O(P)$ complex multiplies on $N/2+1$ bins plus two FFTs per block, and the latency is
one block ($B$ samples, compensated by the mixer). Left and right share one complex FFT: $z = l + i\,r$, and since
both are real, $L_k = \tfrac12(Z_k + Z^*_{N-k})$, $R_k = \tfrac{1}{2i}(Z_k - Z^*_{N-k})$; the inverse transform of
$Y_l + i\,Y_r$ returns the left output in the real part and the right one in the imaginary part.

**Built-in spaces.** Rooms and halls are synthesised: decorrelated noise per side in six octave bands
(125 Hz … 4 kHz), each multiplied by its own decay $e^{-6.91\,t/T_{60}(f)}$ ($-60$ dB after $T_{60}$; highs shorter than
lows), a build-up $1 - e^{-t/\tau_b}$, and twelve early reflections spread over the first tens of milliseconds,
alternating sides. The spring is an impulse train at the spring's round trip (33 ms) through 24 first-order
all-passes (dispersion: the chirp), band-limited to 120 Hz … 4.5 kHz. Cabinets and the telephone are short filter
responses. Every response is normalised to equal energy, $h \leftarrow h\cdot\frac{1/\sqrt2}{\sqrt{\sum h^2}}$, so
switching spaces keeps the level.

**Changing the response** (space, size, damping, reverse, a new file) builds a new kernel on a worker thread; the
audio thread receives it through a bounded channel, copies the current input history into it and crossfades from
the old kernel to the new one over 20 ms. Nothing is allocated or freed on the audio thread.

**Output.** Wet $w$ through the tone filters and the width matrix, then
$y = \big((1 - m)\,x_D + m\,w\big)\cdot g$ with $x_D$ the input delayed by the block (aligned with the wet path).

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
