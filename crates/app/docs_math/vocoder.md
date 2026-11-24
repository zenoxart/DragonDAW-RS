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

<!-- knob: carrier -->
What gets shaped: the input (with the voice on the sidechain), or a built-in saw, pulse, noise or saw chord that
follows the voice's pitch.

<!-- knob: bands -->
Number of bands $N$ (8 … 32). More bands, more intelligible; fewer, more robotic.

<!-- knob: pitch -->
Transposition $p$ of the built-in carriers in semitones: $f = f_0\,2^{p/12}$.

<!-- knob: formant -->
Moves the carrier bands against the voice bands by $2^{\varphi/12}$: positive values give a smaller, brighter
"mouth", negative a larger, darker one.

<!-- knob: low -->
Centre $f_{lo}$ of the lowest band.

<!-- knob: high -->
Centre $f_{hi}$ of the highest band (and where the sibilance band starts).

<!-- knob: attack -->
Time constant of a band envelope rising.

<!-- knob: release -->
Time constant of a band envelope falling (also the sibilance envelope's).

<!-- knob: sibilance -->
Share $s$ of the voice's top and its noise added back.

<!-- knob: emphasis -->
A high shelf at 2 kHz on the voice before the analysis: more weight for the consonants and upper formants.

<!-- knob: mix -->
Wet share: $y = x + m\,(w - x)$.

<!-- knob: output -->
Output gain $10^{o/20}$.
