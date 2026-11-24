# Seraph (shimmer reverb) › Space › Size

[Back to Space](../README.md) · [Seraph (shimmer reverb)](../../README.md)

The size of the simulated space.

Part of **Seraph (shimmer reverb)** – Shimmer reverb: a dense feedback delay network whose tail is pitch-shifted (an octave up by default) on every round, so it rises into a glowing choir-like halo.

**Range:** 20.0 % … 100 % (default: 70.0 %)

## Mathematics

Scales the four line lengths.

**Knob position → value.** Linear: $v = 20 + p\,(80)\ \text{\%}$, so each tenth of the travel is 8 %.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
