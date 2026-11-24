# DSP foundations

The building blocks every stock effect and instrument uses, with the exact formulas of the implementation
(`crates/engine/src/dsp`). Throughout: $f_s$ is the sample rate, $n$ the sample index, $x[n]$ the input,
$y[n]$ the output; all processing is 32-bit float.

## Levels and decibels

$$g = 10^{L/20}, \qquad L = 20\log_{10} g \quad(g \le 10^{-9} \Rightarrow L = -180\ \text{dB})$$

Both run per sample in every detector, so they are computed with base-2 arithmetic instead of `log10`/`pow`:

- **`fast_log2`**: $x = 2^{e}\, m$ with the exponent $e$ read from the IEEE bits and the mantissa reduced to
  $m \in [\sqrt{1/2}, \sqrt 2)$; then $\log_2 m = \frac{2}{\ln 2}\operatorname{artanh} t$ with
  $t = \frac{m-1}{m+1}$, evaluated as the odd series $t + t^3/3 + t^5/5 + t^7/7 + t^9/9$
  (absolute error ≈ $2\cdot10^{-7}$).
- **`fast_exp2`**: $2^{y} = 2^{i}\cdot e^{f\ln 2}$ with $i = \operatorname{round}(y)$, $f = y - i \in[-\tfrac12,\tfrac12]$;
  $2^i$ is written straight into the exponent bits and $e^{z}$ is a degree-7 Taylor polynomial (relative error ≈ $2\cdot10^{-7}$).
- $L \to g$: $g = 2^{L\cdot\log_2 10 / 20}$; $g \to L$: $L = \log_2 g \cdot 20/\log_2 10$.

## Parameters: knob position and value

Every parameter has a range $[v_{min}, v_{max}]$ and a knob position $p \in [0, 1]$:

| Scale | value from position | position from value |
|---|---|---|
| linear | $v = v_{min} + p\,(v_{max}-v_{min})$ | $p = \frac{v - v_{min}}{v_{max}-v_{min}}$ |
| logarithmic ($v_{min}>0$) | $v = v_{min}\left(\frac{v_{max}}{v_{min}}\right)^{p}$ | $p = \frac{\ln(v/v_{min})}{\ln(v_{max}/v_{min})}$ |
| choice / switch | $v = \operatorname{round}(\ldots)$ | |

A logarithmic knob spends equal travel per ratio: every octave of frequency (or doubling of a time) takes the same angle.
Dragging moves $p$ by $\Delta y / P$ for a vertical mouse movement of $\Delta y$ pixels, where the full sweep $P$ is 180–320 px depending on the knob's size (×0.2 with Shift).

### Smoothing (zipper-free automation)

Parameter changes are not applied at once. Every 32 samples (a *slice*) the value moves toward its target with a
one-pole step

$$v_{k+1} = v_k + c\,(v_t - v_k), \qquad c = 1 - e^{-32/(0.025\,f_s)}$$

i.e. a time constant of 25 ms. Logarithmic parameters are smoothed in the log domain
($\ln v$ moves linearly), so a frequency sweep glides evenly per octave. Choices and switches change at once.
The value snaps to the target when it is within $10^{-4}$ of the range.

### The insert's Mix

Each insert slot blends dry and processed signal:

$$y[n] = (1 - m[n])\,d[n - D] + m[n]\,w[n]$$

where $D$ is the effect's reported latency (look-ahead, oversampling filters, FFT blocks): the dry path is
delayed by the same amount, so blending never comb-filters. $m$ ramps linearly across the block when it changes.
Inserts whose input has been silent (peak $< 10^{-6}$) for 4 blocks after their tail has died away are skipped.

## Smoothers and detectors

**One-pole low-pass** (also the basis of every "smooth" and envelope):

$$y[n] = (1 - a)\,x[n] + a\,y[n-1], \qquad a = e^{-2\pi f_c / f_s}$$

and its high-pass complement $x[n] - y[n]$.

**Time-constant form** used for attack/release in milliseconds $t$:

$$a(t) = e^{-1/(t\cdot 10^{-3} f_s)}, \qquad y[n] = x[n] + a\,(y[n-1] - x[n])$$

after $t$ ms a step has covered $1 - e^{-1} \approx 63\,\%$ of the way.

**Envelope follower**: the same recursion on $|x|$ with $a_{att}$ while $|x| > y$ and $a_{rel}$ otherwise.

**Peak hold**: a rise passes immediately; a fall is held for $h$ samples before the detector follows. With
$h$ ≈ 30 ms a 20 Hz wave reads as its amplitude through the whole cycle, so a compressor does not ride the
bass waveform (which would add odd harmonics).

## Filters

### Biquad (RBJ "Audio EQ Cookbook")

$$y[n] = b_0 x[n] + b_1 x[n-1] + b_2 x[n-2] - a_1 y[n-1] - a_2 y[n-2]$$

(transposed direct form II), all coefficients divided by $a_0$. With $f$ clamped to $[10, 0.49 f_s]$:

$$\omega_0 = 2\pi f / f_s,\quad \alpha = \frac{\sin\omega_0}{2Q},\quad A = 10^{G/40},\quad Q \ge 0.025$$

| type | $b_0$ | $b_1$ | $b_2$ | $a_0$ | $a_1$ | $a_2$ |
|---|---|---|---|---|---|---|
| low-pass | $\frac{1-\cos\omega_0}{2}$ | $1-\cos\omega_0$ | $\frac{1-\cos\omega_0}{2}$ | $1+\alpha$ | $-2\cos\omega_0$ | $1-\alpha$ |
| high-pass | $\frac{1+\cos\omega_0}{2}$ | $-(1+\cos\omega_0)$ | $\frac{1+\cos\omega_0}{2}$ | $1+\alpha$ | $-2\cos\omega_0$ | $1-\alpha$ |
| band-pass | $\alpha$ | $0$ | $-\alpha$ | $1+\alpha$ | $-2\cos\omega_0$ | $1-\alpha$ |
| notch | $1$ | $-2\cos\omega_0$ | $1$ | $1+\alpha$ | $-2\cos\omega_0$ | $1-\alpha$ |
| all-pass | $1-\alpha$ | $-2\cos\omega_0$ | $1+\alpha$ | $1+\alpha$ | $-2\cos\omega_0$ | $1-\alpha$ |
| peak (bell) | $1+\alpha A$ | $-2\cos\omega_0$ | $1-\alpha A$ | $1+\alpha/A$ | $-2\cos\omega_0$ | $1-\alpha/A$ |

Shelves use $s = 2\sqrt A\,\alpha$:

- low shelf: $b_0 = A[(A+1)-(A-1)\cos\omega_0+s]$, $b_1 = 2A[(A-1)-(A+1)\cos\omega_0]$, $b_2 = A[(A+1)-(A-1)\cos\omega_0-s]$,
  $a_0 = (A+1)+(A-1)\cos\omega_0+s$, $a_1 = -2[(A-1)+(A+1)\cos\omega_0]$, $a_2 = (A+1)+(A-1)\cos\omega_0-s$
- high shelf: the same with the signs of the $(A-1)\cos\omega_0$ terms (and of $b_1$, $a_1$) flipped.

The magnitude response shown in the EQ displays is evaluated exactly:
$|H(e^{j\omega})| = \left|\frac{b_0 + b_1 e^{-j\omega} + b_2 e^{-2j\omega}}{1 + a_1 e^{-j\omega} + a_2 e^{-2j\omega}}\right|$.

Steeper slopes cascade $N$ identical 2nd-order sections: $N$ sections give $12N$ dB/octave.

### State-variable filter (topology-preserving transform, Zavalishin)

$$g = \tan\frac{\pi f_c}{f_s},\quad k = 2 - 1.95\,r\ (r\in[0,1]),\quad a_1 = \frac{1}{1 + g(g+k)},\ a_2 = g a_1,\ a_3 = g a_2$$

$$v_3 = x - s_2,\quad v_1 = a_1 s_1 + a_2 v_3,\quad v_2 = s_2 + a_2 s_1 + a_3 v_3,\quad s_1 \leftarrow 2v_1 - s_1,\ s_2 \leftarrow 2v_2 - s_2$$

Outputs: low $=v_2$, band $=v_1$, high $= x - k v_1 - v_2$. The trapezoidal integrators keep the cutoff exact
up to Nyquist and stay stable under fast modulation; $k \to 0.05$ approaches self-oscillation.

## Time

**Fractional delay line** (linear interpolation): for a delay $d = i + \phi$,
$y = x[n-i] + \phi\,(x[n-i-1] - x[n-i])$.

**LFO**: phase $\varphi \leftarrow \varphi + f/f_s \pmod 1$;
sine $\sin 2\pi\varphi$, triangle $4|\varphi - \tfrac12| - 1$, square $\operatorname{sgn}(\tfrac12-\varphi)$,
sample & hold a new uniform value in $[-1,1]$ at every wrap (xorshift generator).
Tempo-synced rates use $f = \frac{\text{BPM}}{60}\cdot\frac{1}{\text{beats per cycle}}$.

**DAHDSR envelope** with curve tension $\tau$: each segment maps its linear progress $x\in[0,1]$ through

$$b(x) = \frac{e^{kx} - 1}{e^{k} - 1},\quad k = 5\tau \qquad (b(x)=x \text{ for } \tau = 0)$$

attack $b(x)$ with $-\tau_a$, decay $1 + (S-1)\,b(x)$, release $L_r\,(1 - b(x))$ from the level at release.

## Oversampling

2× stages with a linear-phase half-band FIR of $2M+1 = 47$ taps:

$$h[k] = \tfrac12\operatorname{sinc}\!\left(\tfrac{k-M}{2}\right) w_B[k],\quad w_B[k] = 0.42 - 0.5\cos\frac{2\pi k}{2M} + 0.08\cos\frac{4\pi k}{2M}$$

normalised to unit DC gain. A half-band kernel is zero at every even offset from its centre, so the stage runs
polyphase: of the 47 taps each output needs only the $M+1 = 24$ odd-offset taps (or just the centre tap),
a quarter of the direct work. 4× is two stages. Latency is reported to the mixer and compensated.

## Pan laws

- **Mono sources** (constant power): $\theta = (p+1)\frac{\pi}{4}$, $g_L = \sqrt2\cos\theta$, $g_R = \sqrt2\sin\theta$ (0 dB at the centre, $-\infty$ / +3 dB at the extremes).
- **Stereo balance**: $p \le 0$: $(g_L, g_R) = (1, 1+p)$; $p > 0$: $(1-p, 1)$.

[Manual](../README.md)
