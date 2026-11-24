Holds the spectrum of a moment and plays it on indefinitely.

**Analysis.** STFT with $N = 2048$, hop $N/4$, periodic Hann window; per bin the live magnitude is smoothed over
frames, $M_k \leftarrow 0.6M_k + 0.4|X_k|$ (≈ 50 ms, so the held moment is not one frame's accident). Switching
*Freeze* on copies $M$ into the held spectrum $\hat M$.

**Shaping.** Shift (bins resampled, $\hat M'_k = \hat M(k/2^{s/12})$ with linear interpolation), tilt
($\cdot 10^{t\log_2(f/1\text{kHz})/20}$) and blur (a one-pole across bins forwards and backwards, coefficient
$0.95\,b$).

**Resynthesis.** Each bin keeps its own phase and advances it by the expected amount per hop plus a random jitter,

$$\phi_k \leftarrow \phi_k + 2\pi k\frac{H}{N} + \pi\,d\,u,\qquad Y_k = \hat M'_k(1 + 0.3\,d\,v)\,e^{i\phi_k}$$

($u, v$ uniform in −1 … 1, $d$ the drift), the negative frequencies set to the conjugates, inverse FFT, synthesis
window and overlap-add normalised by $\sum w^2 = 1.5$. The drift keeps the drone alive instead of a static buzz.

**Fading.** The drone's level $a$ ramps towards 1 while frozen and back to 0 after, by one hop per
fade time; the output is $x_D(1 - a\,m) + m\,y$ with $x_D$ the input delayed by $N$ (the reported latency).

<!-- knob: freeze -->
Holds the current spectrum (on) or lets it go (off).

<!-- knob: fade -->
How long the drone takes to come in and to fade away.

<!-- knob: blur -->
Smears the held spectrum across neighbouring frequencies: from a sharp chord to a wash.

<!-- knob: shift -->
Transposes the held spectrum.

<!-- knob: tilt -->
Tilts it brighter or darker (dB per octave around 1 kHz).

<!-- knob: drift -->
Random motion of phases and levels (0 = static).

<!-- knob: mix -->
How much the drone replaces the input while frozen.

<!-- knob: output -->
Output gain.
