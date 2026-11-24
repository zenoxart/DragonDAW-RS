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

<!-- knob: space -->
The built-in response: small and studio rooms ($T_{60}$ 0.45 / 0.8 s at 500 Hz), chamber (1.4 s), concert hall
(2.3 s), cathedral (4.5 s), plate (2 s, dense from the first sample, bright), spring, two speaker cabinets and a
telephone line. With an impulse-response file loaded, the file is used instead.

<!-- knob: size -->
Scale $s$ of the response in time. Built-in spaces: every reverb time and the early-reflection spacing are multiplied
by $s$. Files: the response is resampled, $h_s[n] = h(n/s)$ (linear interpolation), so a room grows or shrinks – and
its resonances move down or up with it, as a real room's would.

<!-- knob: damping -->
Extra loss of the highs over time. The response is split at 2.5 kHz (RBJ low-pass, $Q$ 0.707) and the upper band
multiplied by $e^{-6\,d\,t}$ ($d$ = damping / 100, $t$ in seconds): at full damping the highs lose an extra 52 dB per
second, so the tail darkens as it decays.

<!-- knob: reverse -->
Plays the response backwards, $h_r[n] = h[L - 1 - n]$, with a 30 ms fade-in at its start: the tail swells up to the
sound instead of following it.

<!-- knob: predelay -->
Delay of the input into the convolution (a fractional delay line, up to 250 ms): the gap between the dry sound and
the first reflection, which tells the ear how large the space is.

<!-- knob: low_cut -->
RBJ high-pass ($Q$ 0.707) on the wet signal. At 20 Hz it is out of the path.

<!-- knob: high_cut -->
RBJ low-pass ($Q$ 0.707) on the wet signal. At 20 kHz it is out of the path.

<!-- knob: width -->
Mid / side width $w$ of the wet signal: $m = \tfrac12(l + r)$, $s = \tfrac12(l - r)\,w$, $l' = m + s$, $r' = m - s$.

<!-- knob: mix -->
Wet share $m$: $y = (1 - m)\,x_D + m\,w$, the dry signal delayed by the block so the two stay aligned.

<!-- knob: output -->
Output gain $g = 10^{o/20}$.
