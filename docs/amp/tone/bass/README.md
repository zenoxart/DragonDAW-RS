# Wyrmstack (amp & cabinet) › Tone › Bass

[Back to Tone](../README.md) · [Wyrmstack (amp & cabinet)](../../README.md)

Sets the bass.

Part of **Wyrmstack (amp & cabinet)** – Guitar and bass amplifier with speaker cabinet for direct input: five amps (clean, crunch, lead, bass, fuzz), gate, tone stack, presence, power-amp sag, four cabinets and three microphone positions.

**Range:** 0.00 … 10.0 (default: 5.00)

## Mathematics

Low shelf at 110 Hz (80 Hz on the bass amp), $Q$ 0.6: $(v - 5)\cdot 2.4$ dB (±12 dB, flat at 5).

**Knob position → value.** Linear: $v = 0 + p\,(10)$, so each tenth of the travel is 1.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
