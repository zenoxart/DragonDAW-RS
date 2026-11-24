# Piano roll

- **Toolbar:** tools (draw, paint, select, slice, erase, mute) and dropdown menus for **Key / scale**, **Stamp** (chords) and **Edit** (quantize, legato, strum, flam, humanize, transpose …); the instrument menu sits right next to **Snap**.
- **Notes:** draw, drag (the grabbed note snaps, Shift copies, Alt frees), arrow keys nudge, Ctrl+B duplicates the selection after itself.
- **Ctrl + click** with the draw or paint tool clears the selection instead of drawing a note.
- **Velocity and pan lanes:** hover the lane and drag across it to paint the values of every note you pass, as a line between the press and the mouse; the lane tabs switch between velocity, pan and the other properties. The lane's height is fixed, so fast clicks never resize it by accident.
- **Scales:** pick key and scale; rows in the scale are lit, roots marked.

## How it works

**Quantize** with grid $g$ ticks and strength $s \in [0,1]$ moves each start towards the nearest grid line:

$$t' = \operatorname{round}\big(t + s\,(\operatorname{round}(t/g)\,g - t)\big)$$

and, when ends are included, the end in the same way (the length never drops below one tick). At $s = 1$ notes land
exactly on the grid; at 0.5 they move halfway, keeping some feel.

**Humanize**: $t' = t + \operatorname{round}(u_1 T)$ and $v' = \operatorname{clamp}(v + u_2 V,\ 0.05,\ 1)$ with $u_1, u_2$
uniform in $[-1, 1]$ (a seeded generator, so the same edit gives the same result), $T$ the timing range in ticks and
$V$ the velocity range.

**Scales.** A scale is a set of intervals from the root; *snap to scale* moves a pitch to the nearest scale note
(ties go down). Chords stamped on a scale degree are the triads built from every other scale note.

**Strum** offsets the $j$-th note of a chord (from the bottom) by $j\,\tau$ ticks; **flam** adds a quieter grace note
$\tau$ ticks before every note; **arpeggiate** turns each chord into a run of steps of $\tau$ ticks with gate
$\gamma$ (each note lasts $\gamma\tau$).

**Legato** extends every note to the start of the next later note.

[Manual](../README.md)
