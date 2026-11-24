# Tidecaller (volume shaper) › Sidechain Duck › Attack

[Back to Sidechain Duck](../README.md) · [Tidecaller (volume shaper)](../../README.md)

How quickly the effect reacts when the signal rises. Short times catch transients, longer ones let them through.

Part of **Tidecaller (volume shaper)** – Volume shaper and ducker. A tempo-synced gain curve (pump, duck, gate, saws, sine, swell) and / or ducking from a sidechain input.

**Range:** 0.10 ms … 50.0 ms (default: 2.00 ms, logarithmic scale)

## Mathematics

Time constant of the duck going down.

**Knob position → value.** Logarithmic: $v = 0.1\cdot\left(\frac{50}{0.1}\right)^{p}\ \text{ms}$, $p = \frac{\ln(v/0.1)}{\ln(500)}$ — every factor of 1.862 takes a tenth of the travel; the centre ($p = 0.5$) is 2.2361 ms.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
