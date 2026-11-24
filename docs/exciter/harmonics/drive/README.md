# Wisp (exciter) › Harmonics › Drive

[Back to Harmonics](../README.md) · [Wisp (exciter)](../../README.md)

How hard the signal is pushed into the saturation stage; more drive adds harmonics and density.

Part of **Wisp (exciter)** – Harmonic exciter: generates new harmonics above a chosen frequency (odd or even), adds air on top and, if asked, sub harmonics below the low end. Listen plays only what it adds.

**Range:** 0.00 % … 100 % (default: 40.0 %)

## Mathematics

How hard the band is driven into the shaper (more and stronger harmonics).

**Knob position → value.** Linear: $v = 0 + p\,(100)\ \text{\%}$, so each tenth of the travel is 10 %.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
