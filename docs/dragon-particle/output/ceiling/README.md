# Philosopher's Stone (one-knob mastering) › Output › Ceiling

[Back to Output](../README.md) · [Philosopher's Stone (one-knob mastering)](../../README.md)

The highest level the output may reach.

Part of **Philosopher's Stone (one-knob mastering)** – One-knob mastering: tone EQ with character, transient punch, glue compression, tube colour, widening, a soft clipper and a true-peak limiter, with auto gain for honest A/B.

**Range:** -3.00 dBTP … 0.00 dBTP (default: -0.30 dBTP)

## Mathematics

$C$ (dBTP): the limiter's ceiling and the clipper's reference.

**Knob position → value.** Linear: $v = -3 + p\,(3)\ \text{dBTP}$, so each tenth of the travel is 0.3 dBTP.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
