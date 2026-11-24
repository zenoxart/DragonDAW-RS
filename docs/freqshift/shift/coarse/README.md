# Mercury Wheel (frequency shifter) › Shift › Shift

[Back to Shift](../README.md) · [Mercury Wheel (frequency shifter)](../../README.md)

Sets the shift.

Part of **Mercury Wheel (frequency shifter)** – Frequency shifter (every partial moves by the same number of Hz, up, down or both ways) and ring modulator, with an LFO on the shift and a feedback delay for spiralling barber-pole sweeps.

**Range:** 0.00 Hz … 2.00 kHz (default: 100 Hz)

## Mathematics

The shift $\Delta_c$ in Hz (0 … 2000).

**Knob position → value.** Linear: $v = 0 + p\,(2000)\ \text{Hz}$, so each tenth of the travel is 200 Hz.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
