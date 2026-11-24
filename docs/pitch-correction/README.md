# Pitch correction

- **Dragon Tune** (an insert effect) corrects live or recorded vocals in real time: key and scale, retune speed, humanize, flex-tune, vibrato, note stabilizer, doubler.
- **Pitch correction view** (double-click a clip › Audio repair › Pitch correction) shows the detected notes as blobs with their pitch curves.
  - **Tools:** ✋ Move (drag notes, snaps to semitones, Alt = free), 〰 Vibrato (drag up / down for more or less vibrato and drift), ✂ Split.
  - **One-shots:** a single hit, pluck, 808 or any sample too short for the note tracker becomes one note at its pitch, so you can retune it like any other note.
  - **Shape** (for the selected notes, or all): ⏳ **tail** – how long after the note the move continues, so a one-shot's ring-out or a held note's release follows the new pitch; 🔀 **transition** – how smoothly pitch and level glide in and out at the note's edges; 📶 **fade in** from silence at the note's start; 🔊 **volume** of the note in dB.
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

The real-time **Dragon Tune** effect is documented with its knobs ([Tune](../tune/README.md)).

[Audio repair](../audio-repair/README.md) · [Manual](../README.md)
