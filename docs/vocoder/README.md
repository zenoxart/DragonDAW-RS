# Mandrake (vocoder) (vocoder)

*Vocal*

Channel vocoder with 8 – 32 bands. The voice shapes a carrier: the input itself (with the voice on the sidechain) or a built-in saw, pulse, noise or chord that follows the voice's pitch.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Bands](bands/README.md) – Low band, High band, Attack, Release, Sibilance, Emphasis
- [Carrier](carrier/README.md) – Carrier, Bands, Pitch, Formant
- [Output](output/README.md) – Mix, Output

## How it works

A channel vocoder: the spectral envelope of a voice (the *modulator*) is imposed on a *carrier*.

**Filter banks.** $N$ bands, log-spaced between the low and high band: $f_k = f_{lo}\,\rho^k$,
$\rho = (f_{hi}/f_{lo})^{1/(N-1)}$. Every band is two cascaded RBJ band-passes with $Q = \sqrt\rho/(\rho - 1)$ (neighbours
cross near −3 dB), once on the modulator and once per side on the carrier (the carrier's centres moved by the formant
shift, $f_k\,2^{\varphi/12}$).

**Envelopes.** Per band the rectified modulator band is followed with separate attack and release,
$e \leftarrow v + (e - v)\,c_{a|r}$, $c = e^{-1/(t\,10^{-3}f_s)}$. The output is

$$y = G\sum_{k=0}^{N-1} \mathrm{BP}'_k(\text{carrier})\cdot e_k + s\cdot\text{sib},\qquad G = 3.2\cdot 4\,\sqrt{Q/N}$$

(the gain compensates for the band count and width).

**Carriers.** *Input + sidechain voice*: the track's own audio is the carrier and the sidechain is the voice.
The built-in carriers follow the voice's pitch: YIN on the last 2048 samples every 512 (60 – 800 Hz, confidence
> 0.5) gives $f_0$, played at $f_0\,2^{p/12}$ – PolyBLEP saw, pulse (difference of two saws half a cycle apart),
noise, or a chord of root, fifth and octave.

**Sibilance.** Vocoded carriers have little energy above the top band, so "s" and "t" would vanish: the voice above
$f_{hi}$ (three high-passes) passes, plus high-passed noise following its level, scaled by the sibilance amount.

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
