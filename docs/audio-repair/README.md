# Audio repair

Double-click an audio clip › **Audio repair…**: a spectrogram with time and time × frequency selection.

- **Check the recording** lists hum, clicks, clipping and background noise, each with a one-click fix.
- **Tools** (icon tiles): ☁ De-noise (learned noise profile), ⚡ De-hum (50 / 60 Hz found automatically), ✨ De-click, 📈 De-clip, 🔥 Distortion repair, 🏠 De-reverb, 🎯 Spectral attenuate (Shift+drag a box around one sound), 🔊 Gain.
- Every tool has **Preview** and **Apply**; every repair is one undo step, and A / B compares with the original.

## How it works

All spectral tools use an STFT with $N = 2048$, hop 512 (75 % overlap), Hann analysis and synthesis windows and
normalised overlap-add, so an untouched spectrum reconstructs the input exactly.

**De-noise.** The noise profile is the mean magnitude $\bar m_k$ per bin of a noise-only stretch (or the quietest tenth of
the clip); its power is $\lambda_k = (\sigma\bar m_k)^2\cdot\frac4\pi$ (Rayleigh mean → power, $\sigma$ the sensitivity).
Per frame a decision-directed Wiener gain:

$$\gamma_k = \frac{|X_k|^2}{\lambda_k},\quad \xi_k = 0.97\,\frac{|\hat S_k|^2_{prev}}{\lambda_k} + 0.03\max(\gamma_k - 1, 0),\quad G_k = \max\!\left(\frac{\xi_k}{1 + \xi_k},\ 10^{-R/20}\right)$$

smoothed across neighbouring bins with $(1, 2, 1)/4$ and floored again at the reduction limit $R$. Leaning on the
previous frame's clean estimate keeps the gains steady (no "musical noise").

**De-hum.** The mains frequency (50 or 60 Hz, whichever with its harmonics is stronger) and its harmonics below
$0.45f_s$ get narrow RBJ notches, run forwards and then backwards over the clip (zero phase: nothing is shifted).

**De-click.** The second difference $|x_{i+1} - 2x_i + x_{i-1}|$ is compared to its local median over 20 ms blocks; a
sample above $k = 60/\text{sensitivity}$ times the median starts a click, which extends (up to 3 ms) while neighbours stay
above 0.3 of that. The gap is filled by linear prediction of order 32 (autocorrelation method, Levinson–Durbin, fitted on up to 1024
samples on each side; predicted forwards from before and backwards from after the gap and crossfaded), so tones carry on
through it; near the clip's edges, where there is too little context, by a cubic matching value and slope.

**De-clip.** Runs of samples stuck at the clip level are replaced by a cubic through the waveform's slopes on either
side; if the restored peaks would exceed full scale, the clip is turned down.

**De-reverb.** The late reverb in frame $t$ is estimated from the smoothed spectrum 50 ms earlier, decayed by the room's
reverb time: $\hat R_{t,k} = e^{-2\ln(1000)\,T_d/T_{60}}\,\bar P_{t-d,k}$. The gain is
$G = \sqrt{\max(1 - 2a\hat R/P,\ 0.15^2)}$, smoothed over frames ($G \leftarrow G + 0.4(G_{prev} - G)$).

**Attenuate selection.** A time × frequency rectangle is multiplied by $10^{g/20}$ with soft edges in both time and
frequency, so nothing rings.

[Pitch correction](../pitch-correction/README.md) · [Manual](../README.md)
