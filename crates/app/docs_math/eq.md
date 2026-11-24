A 24-band parametric equaliser; each band is a cascade of up to 8 second-order sections, optionally
dynamic, placed on stereo, mid, side, left or right, in one of three processing modes.

**Band filter.** Bell, band pass and notch are one RBJ section (see the cookbook table in [DSP foundations]({DSP})). Pass filters
and shelves of slope $S$ dB/oct have order $N = S/6$, built from $\lfloor N/2\rfloor$ biquads plus one
first-order section when $N$ is odd:

- **Butterworth pass filters**: the section pairs take the pole angles
  $\theta_i = \frac{\pi(2i+1)}{2N}$ ($N$ even) or $\frac{\pi(i+1)}{N}$ ($N$ odd), each with
  $Q_i = \frac{1}{2\cos\theta_i}$; the band's $Q$ scales the sharpest section by $Q/\tfrac{1}{\sqrt2}$
  (resonance), and at 12 dB/oct the single section simply uses $Q$.
  The first-order section is the bilinear transform of $\frac{\omega}{s+\omega}$ / $\frac{s}{s+\omega}$:
  $k = \tan\frac{\pi f}{f_s}$, $b_0 = b_1 = \frac{k}{k+1}$ (LP) or $b_0 = -b_1 = \frac{1}{k+1}$ (HP), $a_1 = \frac{k-1}{k+1}$.
- **Steep shelves**: the gain is divided over the sections so the total stays $G$: each biquad shelf gets
  $2G/N$ dB, the first-order shelf $G/N$ dB with $H(s) = \frac{s + A\omega}{s+\omega}$ (low) or
  $\frac{As + \omega}{s+\omega}$ (high), its pole moved to $f/\sqrt A$ (low) or $f\sqrt A$ (high) so the
  half-gain point stays at $f$.

The response is $H(f) = \prod_{\text{sections}} H_k(f)$, in dB the sum of the sections; the display draws exactly
$\sum_b 20\log_{10}|H_b(e^{j2\pi f/f_s})|$ plus the output gain.

**Placement** (per band): Mid $m = \tfrac12(L+R)$ or Side $s = \tfrac12(L - R)$ is filtered and the channels
rebuilt as $L = m + s$, $R = m - s$; Left / Right filter one channel; Stereo filters both.

**Dynamic bands.** A band-pass detector at the band's frequency ($Q \ge 0.5$) on $\tfrac12(L+R)$, envelope
5 ms attack / 80 ms release; with $D$ = dynamic range and $T$ = threshold:

$$G_{dyn} = D\cdot\operatorname{clamp}\!\left(\frac{20\log_{10}e - T}{6}, 0, 1\right)\ \text{dB}$$

— the full range is reached 6 dB above the threshold. The band runs at $G + G_{dyn}$; the filters are retuned
on a 16-sample grid (minimum phase) or the kernels redesigned at most every 512 samples when $G_{dyn}$ moved by
0.25 dB (linear phase).

**Processing modes.**

- *Zero latency*: the sections run directly at $f_s$ (minimum phase, the response cramps towards Nyquist as
  every bilinear-transform filter does).
- *Natural phase*: the sections run at $2f_s$ behind 47-tap half-band filters, which moves the cramping an
  octave higher; latency = the half-band filters' delay.
- *Linear phase*: the summed magnitude response $|H(f)|$ (per placement) is sampled on an FFT grid
  ($N_{FFT} = 4096$), turned into a zero-phase impulse by an inverse FFT, centred, windowed with a Hann window
  of 2048 taps and transformed back; the audio is convolved by overlap-add in 2048-sample blocks. Placement is
  kept with a 2×2 kernel matrix: $L' = \frac{G_m + G_s}{2}K_L L + \frac{G_m - G_s}{2}K_R R$ (and symmetric for
  $R'$). Latency $= 2048 + 1024 = 3072$ samples.

**Auto gain.** $\Delta = -\frac{\sum_k w_k\,H_{dB}(f_k)}{\sum_k w_k}$ over 40 log-spaced frequencies
$f_k = 50\cdot 240^{k/39}$ Hz with the loudness-like weight $w = \frac{1}{1 + (\log_2(f/1500)/2.5)^2}$,
clamped to ±18 dB.

**EQ match** (from the analyser's average spectra): level-match $d_b = R_b - O_b - \overline{(R - O)}$,
clamp to ±limit, smooth over ±3 bands, then greedily place up to $n$ bells at the largest remaining deviation
$d$ with $Q = \text{clamp}(1/\text{octaves}, 0.3, 6)$ (width = how far the deviation keeps its sign and half its
size), subtracting each bell's response before the next.

<!-- knob: on -->
Includes the band's sections in the cascade (and in the drawn curve).

<!-- knob: type -->
RBJ bell, low/high shelf, low/high pass, band pass or notch; pass filters and shelves use the slope.

<!-- knob: freq -->
Centre / corner frequency $f$ (logarithmic knob: $f = 20\cdot 1000^{p}$ Hz). Clamped to $[10, 0.49 f_s]$.

<!-- knob: gain -->
$G$ (dB): peak or shelf gain, $A = 10^{G/40}$ in the RBJ formulas; for steep shelves split as $2G/N$ per section.

<!-- knob: q -->
$Q$: bandwidth of bells ($\alpha = \sin\omega_0/2Q$; the −3 dB bandwidth of a band pass is $f/Q$), shelf
steepness, and the resonance of pass filters (0.707 = Butterworth).

<!-- knob: stereo -->
Stereo, Mid $\tfrac12(L+R)$, Side $\tfrac12(L-R)$, Left or Right.

<!-- knob: dyn -->
$D$: dynamic gain change at full overshoot, $G_{dyn} = D\cdot\text{clamp}((X - T)/6, 0, 1)$; negative values
make the band cut when the band gets loud.

<!-- knob: thr -->
$T$: band-pass detector level (dBFS) where the dynamic gain starts.

<!-- knob: slope -->
$S \in \{6, 12, 18, 24, 30, 36, 48, 72, 96\}$ dB/oct: order $N = S/6$ (Butterworth pass filters, gain-split shelves).

<!-- knob: output -->
Final gain $10^{O/20}$ (times the auto-gain offset when on).

<!-- knob: auto_gain -->
Adds $\Delta = -\overline{H_{dB}}$ (loudness-weighted mean of the curve, see above), so boosting does not just
sound "better" by being louder.

<!-- knob: solo -->
Listens through an RBJ band pass at the soloed band's frequency ($Q \ge 0.3$).

<!-- knob: phase -->
Zero latency, linear phase (FFT convolution, 3072 samples latency) or natural phase (2× oversampled
minimum-phase sections).
