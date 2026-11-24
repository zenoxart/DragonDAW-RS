# Wyrmstack (amp & cabinet) › Amp › Gate

[Back to Amp](../README.md) · [Wyrmstack (amp & cabinet)](../../README.md)

Sets the gate.

Part of **Wyrmstack (amp & cabinet)** – Guitar and bass amplifier with speaker cabinet for direct input: five amps (clean, crunch, lead, bass, fuzz), gate, tone stack, presence, power-amp sag, four cabinets and three microphone positions.

**Range:** -90.0 dB … -20.0 dB (default: -90.0 dB)

## Mathematics

Threshold of the noise gate before the amp. Its envelope rises at once and falls over 80 ms; above the threshold
the gain opens in ~1 ms, below it closes over 80 ms. At −90 dB the gate is off.

**Knob position → value.** Linear: $v = -90 + p\,(70)\ \text{dB}$, so each tenth of the travel is 7 dB.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
