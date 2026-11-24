A harmonic exciter: it does not boost the highs that are there (an EQ would), it *generates* new harmonics from
the band above a frequency and adds a little of them.

**Path.** $h = \mathrm{HP}^2_{f}(x)$ (two RBJ high-passes, $Q$ 0.707), driven $u = g\,h$ with $g = 1 + 15\,\text{drive}$, shaped

$$v = \tfrac{1 - c}{2}\tanh u + \tfrac{1 + c}{2}\cdot\tfrac12\tanh(u^2)$$

(the odd function gives odd harmonics, the even one even harmonics; $c$ = character). The new content is
$a = \mathrm{HP}^2_{f}(v/\sqrt g - h/2)$ – the fundamental share removed and the shaper's low products filtered out –
added as $y = x + 2A\,a$.

**Sub harmonics.** Below 90 Hz a flip-flop toggles on every upward zero crossing of the low band, giving a square at
half its frequency; scaled by the low band's level, rounded by a 70 Hz low-pass and added – an octave below the kick
or bass.

**Air**: a high shelf at 12 kHz on the result.

<!-- knob: freq -->
The split $f$: only the band above it is excited.

<!-- knob: drive -->
How hard the band is driven into the shaper (more and stronger harmonics).

<!-- knob: character -->
Blend $c$ from odd (−100 %, brighter, edgier) to even harmonics (+100 %, warmer).

<!-- knob: amount -->
How much of the new harmonics is added ($A$).

<!-- knob: air -->
Gain of the 12 kHz shelf.

<!-- knob: sub -->
Level of the generated sub octave.

<!-- knob: listen -->
Plays only what the exciter adds (the harmonics and the sub).

<!-- knob: output -->
Output gain.
