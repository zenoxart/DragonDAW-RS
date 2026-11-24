# Jadefang (character EQ) › Controls › Sub

[Back to Controls](../README.md) · [Jadefang (character EQ)](../../README.md)

Sets the sub.

Part of **Jadefang (character EQ)** – Six voicings of a four-band EQ with a driven input stage, a very broad Sub band and a boost-only Air band.

**Range:** -10.0 dB … 10.0 dB (default: 0.00 dB)

## Mathematics

Low shelf at 18 Hz, $Q = 0.5$ — a very broad, gentle low-end tilt.

**Knob position → value.** Linear: $v = -10 + p\,(20)\ \text{dB}$, so each tenth of the travel is 2 dB.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
