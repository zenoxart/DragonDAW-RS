# Grimoire (lo-fi) › Digital › Bits

[Back to Digital](../README.md) · [Grimoire (lo-fi)](../../README.md)

The bit depth the signal is reduced to; fewer bits sound grittier.

Part of **Grimoire (lo-fi)** – Lo-fi ageing: tape wow and flutter, a narrowing worn-out band, saturation, tape hiss, vinyl crackle, and bit-depth and sample-rate reduction.

**Range:** 4.00 bit … 24.0 bit (default: 24.0 bit)

## Mathematics

Bit depth $b$ (24 = off).

**Knob position → value.** Linear: $v = 4 + p\,(20)\ \text{bit}$, so each tenth of the travel is 2 bit.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
