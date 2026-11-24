# Familiar (auto-filter) › Lfo › Stereo phase

[Back to Lfo](../README.md) · [Familiar (auto-filter)](../../README.md)

Which part of the stereo signal the band works on: both, mid, side, left or right.

Part of **Familiar (auto-filter)** – Auto-filter: a resonant low-, band-, high-pass or notch whose cutoff an LFO (free or synced, six shapes, stereo phase) and an envelope follower (positive or negative, from the input or the sidechain) move.

**Range:** 0.00 ° … 180 ° (default: 0.00 °)

## Mathematics

Phase offset of the right side's LFO (0 … 180°): the filter moves in opposite directions at 180°.

**Knob position → value.** Linear: $v = 0 + p\,(180)\ \text{°}$, so each tenth of the travel is 18 °.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
