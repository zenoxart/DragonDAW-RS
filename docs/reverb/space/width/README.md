# Dragon's Lair (reverb) › Space › Width

[Back to Space](../README.md) · [Dragon's Lair (reverb)](../../README.md)

The stereo width: 0 is mono, 100 % unchanged, above that wider.

Part of **Dragon's Lair (reverb)** – Room/hall/plate reverb (8-line FDN) with pre-delay, damping and width; early reflections and late reverb each have their own level and EQ.

**Range:** 0.00 … 1.50 (default: 1.00)

## Mathematics

$W$: side gain of the wet signal (0 = mono reverb, 1 = as generated, 1.5 = wider).

**Knob position → value.** Linear: $v = 0 + p\,(1.5)$, so each tenth of the travel is 0.15.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
