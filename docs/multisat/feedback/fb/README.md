# Athanor (multiband saturator) › Feedback › Feedback

[Back to Feedback](../README.md) · [Athanor (multiband saturator)](../../README.md)

How much of the output is fed back into the effect; more feedback means more repeats or a stronger resonance.

Part of **Athanor (multiband saturator)** – Multiband saturation: up to four bands (Linkwitz–Riley crossovers), each with a style (clean / warm tube, tape, transformer, soft / hard clip, foldback, rectify), drive, dynamics, tone, level, mix and a resonant feedback loop.

**Range:** 0.00 % … 100 % (default: 0.00 %)

Every band has this control; pick the band with the band buttons.

## Mathematics

$\phi = 0.95\,\text{fb}$: output of the shaper fed back into its input through a delay of $f_s/f_{fb}$ samples.

**Knob position → value.** Linear: $v = 0 + p\,(100)\ \text{\%}$, so each tenth of the travel is 10 %.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
