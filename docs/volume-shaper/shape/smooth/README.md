# Tidecaller (volume shaper) › Shape › Smooth

[Back to Shape](../README.md) · [Tidecaller (volume shaper)](../../README.md)

Sets the smooth.

Part of **Tidecaller (volume shaper)** – Volume shaper and ducker. A tempo-synced gain curve (pump, duck, gate, saws, sine, swell) and / or ducking from a sidechain input.

**Range:** 0.10 ms … 20.0 ms (default: 2.00 ms)

## Mathematics

Time constant of the final one-pole on the gain (0.1 … 20 ms): rounds the edges of the gate and the saws so they
do not click.

**Knob position → value.** Linear: $v = 0.1 + p\,(19.9)\ \text{ms}$, so each tenth of the travel is 1.99 ms.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
