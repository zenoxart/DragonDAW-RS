# Wisp (exciter) › Extremes › Air

[Back to Extremes](../README.md) · [Wisp (exciter)](../../README.md)

Sets the air.

Part of **Wisp (exciter)** – Harmonic exciter: generates new harmonics above a chosen frequency (odd or even), adds air on top and, if asked, sub harmonics below the low end. Listen plays only what it adds.

**Range:** 0.00 dB … 12.0 dB (default: 0.00 dB)

## Mathematics

Gain of the 12 kHz shelf.

**Knob position → value.** Linear: $v = 0 + p\,(12)\ \text{dB}$, so each tenth of the travel is 1.2 dB.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
