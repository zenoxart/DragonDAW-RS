# Draconic Ghost (resonance suppressor) (ghost)

*Dynamics*

Dynamic resonance suppressor: finds the frequencies that stick out right now and pulls only those down, only while they're there. Zero-latency, low-latency and precise engines, focus EQ, soft / hard character, L/R or M/S with stereo link, sidechain, delta.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Controls](controls/README.md) – Depth, Sharpness, Selectivity, Attack, Release, Mix, Trim, Engine, Character, Resolution, Stereo, Stereo link, M/S balance, Delta, Bypass, Sidechain, Low cut, High cut, Solo, Focus 1 freq, Focus 1 gain, Focus 2 freq, Focus 2 gain, Focus 2 Q, Focus 3 freq, Focus 3 gain, Focus 3 Q, Focus 4 freq, Focus 4 gain, Focus 4 Q, Focus 5 freq, Focus 5 gain, Focus 5 Q, Focus 6 freq, Focus 6 gain

## How it works

A dynamic resonance suppressor: per analysis hop it finds the bands that stick out of their spectral
neighbourhood and turns exactly those down, with attack and release, then applies the cuts through a
filter bank (Zero / Low) or an STFT (Precise).

**Bands.** 60 bands at 1/6 octave: $f_b = 20\cdot 2^{b/6}$ Hz, $b = 0 \dots 59$ (those below $0.45 f_s$ are used).

**Analysis** (both channels in one complex FFT: $z = a + jb$, the channel spectra recovered by symmetry
$A_k = \tfrac12(Z_k + \overline{Z_{N-k}})$, $B_k = \tfrac{1}{2j}(Z_k - \overline{Z_{N-k}})$). Hann window of
$N = 1024 \cdot 2^{r}$ (resolution $r$), hop 128 samples (Zero / Low) or 256 (Precise); bin powers are summed
into the bands, $P_b$, and $L_b = 10\log_{10}P_b$.

**Focus EQ** (shapes the *sensitivity*, never the sound): $F(f) = \sum$ of a low shelf $\frac{g}{1 + e^{4x}}$,
four bells $g\,e^{-x^2/(2w^2)}$ with $w = 0.6/Q$ octaves, and a high shelf $\frac{g}{1 + e^{-4x}}$,
$x = \log_2(f/f_n)$. Weighted level $\ell_b = L_b + F(f_b)$, sensitivity $s_b = 2^{F(f_b)/6}$.

**Excess.** Reference = Gaussian average of the *neighbours* (the band itself left out):

$$\bar\ell_b = \frac{\sum_{j \ne b} \ell_j\,e^{-(j-b)^2/(2\sigma^2)}}{\sum_{j\ne b} e^{-(j-b)^2/(2\sigma^2)}},\qquad \sigma = \max\!\left(12\cdot 24^{-S_e/10},\ 0.75\right)\ \text{bands}$$

($S_e$ = selectivity 0…10: from 2 octaves wide down to 1/8 octave). Excess $x = \ell_b - \bar\ell_b$ with a
soft knee of width $2t$ ($t = 3$ dB Soft, 0.5 dB Hard): $e = 0$ ($x \le 0$), $\frac{x^2}{4t}$ ($x < 2t$), $x - t$.

**Reduction** (dB) per band:

$$R_b = \min\!\left(30,\ D^2\,\kappa_c\,s_b\,12\left(\frac{e}{12}\right)^{\gamma}\right)\,W_b,\qquad \gamma = 0.6 + 0.24\,S_h + 0.5[\text{Hard}]$$

with depth $D \in [0, 1]$ (the knob squared: fine control at small amounts), sharpness $S_h$ (higher $\gamma$ =
only the strongest peaks), M/S balance factor $\kappa_c$ and the low/high cut window $W_b$ (1 inside, a
raised-cosine fade over 1/3 octave outside: $\tfrac12 + \tfrac12\cos(3\pi o)$, $o$ = octaves beyond the cut).
The curve is then smoothed across bands with a Gaussian of width $\frac16\cdot16^{-S_h/10}\cdot 6 + 0.25[\text{Soft}]$.

**Ballistics** per hop $h$: $R \leftarrow R + (R^* - R)\,(1 - e^{-h/(t\,10^{-3}f_s)})$, attack $t_a$ while
rising, release $t_r$ (×1.3 in Soft) while falling. **Stereo link** $\lambda$:
$R_c \leftarrow (1-\lambda)R_c + \lambda\max(R_L, R_R)$.

**Zero / Low engine: parallel band-pass bank.**

$$y = x + \sum_i c_i\,\text{BP}_i(x),\qquad \text{BP}_i: \text{RBJ band pass at } f_i,\ Q = 8.65$$

The wanted linear gains $w_b = 10^{-R_b/20} - 1$ must hold *at the band centres*, where the neighbouring
band-passes overlap. With the interaction matrix $M_{ij} = \operatorname{Re}\,\text{BP}_j(e^{j2\pi f_i/f_s})$
(precomputed, inverted once by Gauss–Jordan), $c = M^{-1}w$ — evaluated only within ±8 bands of a cutting band,
clamped to $[-1, 0.25]$. The scalars $c_i$ are interpolated linearly across each hop (no zipper noise); bands
with $c = 0$ cost nothing. Low adds 64 samples (1.33 ms at 48 kHz) of look-ahead so a cut is in place when the
transient arrives. Delta is exactly $\sum c_i\,\text{BP}_i(x)$.

**Precise engine: STFT.** $\sqrt{\text{Hann}}$ analysis and synthesis windows, $N = 1024$, hop 256 (perfect
reconstruction), per-bin gain $10^{-R(\text{pos})/20}$ interpolated linearly between the bands on a log
frequency axis; latency 1024 samples.

**Output.** Mix $\mu$ with the latency-aligned dry, trim $10^{T/20}$, bypass crossfades over 10 ms; delta
outputs $x - y$; band solo plays one focus node's region through an RBJ band pass.

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
