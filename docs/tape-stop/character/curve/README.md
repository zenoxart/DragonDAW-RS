# Petrify (tape stop) › Character › Curve

[Back to Character](../README.md) · [Petrify (tape stop)](../../README.md)

Sets the curve.

Part of **Petrify (tape stop)** – Tape stop and spin-up: the sound slows to a halt (pitch falling with the speed) and starts again. Triggered by its switch (automate it) or at the end of every bar, 2 or 4 bars.

**Range:** -100 % … 100 % (default: 30.0 %)

## Mathematics

Bend $c$ of both ramps (see above).

**Knob position → value.** Linear: $v = -100 + p\,(200)\ \text{\%}$, so each tenth of the travel is 20 %.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
