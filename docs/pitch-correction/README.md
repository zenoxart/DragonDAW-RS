# Pitch correction

- **Dragon Tune** (an insert effect) corrects live or recorded vocals in real time: key and scale, retune speed, humanize, flex-tune, vibrato, note stabilizer, doubler.
- **Pitch correction view** (double-click a clip › Audio repair › Pitch correction) shows the detected notes as blobs with their pitch curves.
  - **Tools:** ✋ Move (drag notes, snaps to semitones, Alt = free), 〰 Vibrato (drag up / down for more or less vibrato and drift), ⏱ Timing (drag a note left / right to move it in time, drag its right edge to make it longer or shorter), ✂ Split.
  - **One-shots:** a single hit, pluck, 808 or any sample too short for the note tracker becomes one note at its pitch, so you can retune it like any other note.
  - **Shape** (for the selected notes, or all): ⏳ **tail** – how long after the note the move continues, so a one-shot's ring-out or a held note's release follows the new pitch; 🔀 **transition** – how smoothly pitch and level glide in and out at the note's edges; 📶 **fade in** from silence at the note's start; 🔊 **volume** of the note in dB; ⏱ **timing** (ms earlier / later) and ↔ **length** (%) – the gaps to the neighbouring notes stretch to make room, the pitch stays; 🎭 **formant** (semitones) – moves the voice's resonances, its timbre, without changing the pitch: up sounds smaller and brighter, down bigger and darker.
  - **Correct pitch** pulls the notes onto their semitones and calms vibrato and drift; **Preview** and **Apply** render the result (one undo step).

## How it works

**Pitch tracking (YIN).** For each frame the difference function $d(\tau) = \sum_j (x_j - x_{j+\tau})^2$ is normalised by
its running mean, $d'(\tau) = d(\tau)\,\tau/\sum_{u=1}^{\tau} d(u)$ (with $d'(0) = 1$). The period is the first $\tau$
where $d'$ dips below 0.15, followed down to its local minimum; without such a dip the global minimum is used when it is
below 0.2, otherwise the frame is unvoiced. $f_0 = f_s/\tau$, confidence $1 - d'(\tau)$.

**Notes.** Consecutive voiced frames form a note while the pitch stays within ~0.8 semitone of the note so far; a
note lasts at least 80 ms. Each note keeps its pitch curve (seconds, MIDI), its centre pitch and level. A one-shot that
the tracker cannot follow is taken as a single note at the pitch of its steady frames, or its strongest spectral peak.

**Correction.** An edited note is re-pitched by pitch-synchronous granular shifting (two-period Hann grains placed at
the new period), with 15 ms moves in and out so the edges do not click. *Flatten drift / vibrato* scales the curve's
deviation from its centre, $m'(t) = \bar m + (1 - \kappa)(m(t) - \bar m)$. Everything outside edited notes is left
sample-identical.

**Timing, length and formants.** A note with a timing, length or formant edit is resynthesised offline by
pitch-synchronous overlap-add. Analysis marks are one period $P(t)$ apart (from the note's pitch curve, snapped to the
waveform's positive peak; 10 ms apart outside the note), each the centre of a Hann grain two periods long. A piecewise
linear time map takes output time to input time through the knots
$(t_a, t_a), (s', s), (s' + \lambda L, e), (t_b, t_b)$ – the region $[t_a, t_b]$ reaches at most 0.25 s past the note and
never into a neighbouring note, $s'$ is the new start, $\lambda$ the length factor. Synthesis marks follow at the new
period $P/r$ (pitch ratio $r$); each takes the grain whose analysis mark lies nearest the mapped time, so periods are
repeated or skipped to stretch or move without changing the pitch. A formant shift of $\phi = 2^{F/12}$ reads each
grain at speed $\phi$ over half-length $P/\phi$: the waveform of one period is compressed, its spectral envelope
moves up by $\phi$, while the grain spacing – the pitch – is unchanged. The sum is normalised by the overlapping windows
and crossfaded into the original over 5 ms at the region's edges, where the time map is the identity.

The real-time **Dragon Tune** effect is documented with its knobs ([Tune](../tune/README.md)).

[Audio repair](../audio-repair/README.md) · [Manual](../README.md)
