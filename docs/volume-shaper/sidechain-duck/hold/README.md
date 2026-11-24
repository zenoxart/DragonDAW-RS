# Tidecaller (volume shaper) › Sidechain Duck › Hold

[Back to Sidechain Duck](../README.md) · [Tidecaller (volume shaper)](../../README.md)

How long the effect stays engaged after the signal drops before it starts to release.

Part of **Tidecaller (volume shaper)** – Volume shaper and ducker. A tempo-synced gain curve (pump, duck, gate, saws, sine, swell) and / or ducking from a sidechain input.

**Range:** 1.00 ms … 500 ms (default: 30.0 ms, logarithmic scale)

## Mathematics

How long the duck stays down after the detector falls below the threshold.

**Knob position → value.** Logarithmic: $v = 1\cdot\left(\frac{500}{1}\right)^{p}\ \text{ms}$, $p = \frac{\ln(v/1)}{\ln(500)}$ — every factor of 1.862 takes a tenth of the travel; the centre ($p = 0.5$) is 22.3607 ms.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
