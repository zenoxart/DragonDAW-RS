# Wyrmstack (amp & cabinet) › Amp › Gain

[Back to Amp](../README.md) · [Wyrmstack (amp & cabinet)](../../README.md)

How hard the signal is pushed into the saturation stage; more drive adds harmonics and density.

Part of **Wyrmstack (amp & cabinet)** – Guitar and bass amplifier with speaker cabinet for direct input: five amps (clean, crunch, lead, bass, fuzz), gate, tone stack, presence, power-amp sag, four cabinets and three microphone positions.

**Range:** 0.00 % … 100 % (default: 50.0 %)

## Mathematics

Position $u$ in the amp's gain range: $G = 10^{(G_{lo} + u(G_{hi} - G_{lo}))/20}$ into the preamp.

**Knob position → value.** Linear: $v = 0 + p\,(100)\ \text{\%}$, so each tenth of the travel is 10 %.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
