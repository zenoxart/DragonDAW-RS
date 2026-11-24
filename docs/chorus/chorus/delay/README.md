# Coven (chorus) › Chorus › Delay

[Back to Chorus](../README.md) · [Coven (chorus)](../../README.md)

Sets the delay.

Part of **Coven (chorus)** – Stereo chorus with quadrature LFOs.

**Range:** 5.00 ms … 30.0 ms (default: 12.0 ms, logarithmic scale)

## Mathematics

$D_0$ (ms): the shortest delay; the sweep runs from $D_0$ to $D_0 + 8\delta$ ms.

**Knob position → value.** Logarithmic: $v = 5\cdot\left(\frac{30}{5}\right)^{p}\ \text{ms}$, $p = \frac{\ln(v/5)}{\ln(6)}$ — every factor of 1.196 takes a tenth of the travel; the centre ($p = 0.5$) is 12.2474 ms.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
