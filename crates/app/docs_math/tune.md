Real-time pitch correction: YIN pitch detection every 128 samples, a target note from key and scale, a
smoothed pitch ratio and a pitch-synchronous overlap-add (TD-PSOLA) shifter that keeps the formants.

**YIN detection** on the last 1024 samples of the mono sum, periods for 85 … 1100 Hz:

$$d(\tau) = \sum_{n}(x_n - x_{n+\tau})^2,\qquad d'(\tau) = \frac{d(\tau)}{\frac1\tau\sum_{k=1}^{\tau} d(k)}$$

the first $\tau$ with $d' < 0.15$, walked down to its local minimum (else the global minimum if $< 0.2$),
refined by parabolic interpolation $\tau^* = \tau + \frac{d'_{\tau-1} - d'_{\tau+1}}{2(d'_{\tau-1} + d'_{\tau+1} - 2d'_\tau)}$.
Frequency $f = f_s/\tau^*$, clarity $1 - d'(\tau)$; frames with clarity ≤ 0.8 are left uncorrected.
MIDI pitch $m = 69 + 12\log_2(f/440)$.

**Target.** A slow centre $c$ follows $m$ ($c \leftarrow c + 0.08(m - c)$, jumping when $|m - c| > 1$ st); the target
note $n$ is the nearest pitch class of the scale to $c$ (searched ±6 st). The stabilizer keeps the held note
while $100\,|c - n_{held}| < 50 + \text{stabilizer}$ cents. Correction in semitones:

$$\Delta = A\,\big(n - m + V(m - c)\big)\quad(\Delta = 0 \text{ if } 100|n - m| \le \text{flex})$$

($V$ = natural vibrato kept, $A$ = amount), ratio $\rho^* = 2^{\Delta/12}$.

**Retune speed.** $\rho \leftarrow \rho^* + (\rho - \rho^*)\,e^{-1/(t\,10^{-3} f_s\,h)}$ per sample (instant below 0.5 ms —
the hard-tune effect), where humanize lengthens it for held notes:
$h = 1 + 7H\cdot\text{clamp}\left(\frac{t_{held} - 0.12}{0.3}, 0, 1\right)$.

**PSOLA.** Grains two periods long, windowed and centred on pitch marks one period $T_0 = f_s/f$ apart
(aligned to waveform peaks), are laid down every $T_0/\rho$: the pitch becomes $\rho f$ while each grain's
shape — the formant envelope — stays. Delay ≈ one period + 1 ms (about 5 ms on a 220 Hz voice).

**Doubler.** Two copies of the tuned voice at $14 + 1.5\sin(2\pi\,0.37t)$ ms (left) and $23 - 1.5\sin(\dots)$ ms
(right), level $0.7\,D$ — a few cents of moving detune, like a second take.

$y = (1 - \mu)x + \mu\,w$.

<!-- knob: key -->
Root of the scale (C … B).

<!-- knob: scale -->
The allowed pitch classes relative to the key.

<!-- knob: speed -->
$t$ (ms): time constant of the ratio's glide to the target; < 0.5 ms = instant (hard tune).

<!-- knob: humanize -->
$H$: held notes (after 120 ms) are retuned up to 8× more slowly, keeping natural movement inside long notes.

<!-- knob: flex -->
Notes already within this many cents of the target are left alone.

<!-- knob: vibrato -->
$V$: share of the deviation from the slow centre (vibrato, scoops) that is kept.

<!-- knob: amount -->
$A$: fraction of the correction applied (0.5 = halfway to the note).

<!-- knob: mix -->
$\mu$: dry/corrected.

<!-- knob: stabilizer -->
Hysteresis (cents beyond the midpoint) before switching to a neighbouring note.

<!-- knob: doubler -->
$D$: level $0.7D$ of the two delayed, swaying copies.
