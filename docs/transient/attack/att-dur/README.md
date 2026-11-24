# Talon (transient shaper) › Attack › Attack duration

[Back to Attack](../README.md) · [Talon (transient shaper)](../../README.md)

Sets the attack duration.

Part of **Talon (transient shaper)** – Transient designer: attack (Needle / Nail / Blunt) and sustain (Linear / Non-linear / Soft linear) sections with level, sensitivity, duration and shape, mix, output, and a guard (Off / Clip / Limit / Soft clip) after the output.

**Range:** 1.00 ms … 200 ms (default: 25.0 ms, logarithmic scale)

## Mathematics

$d_a$: length of the attack window; the sustain window starts at $0.6 d_a$.

**Knob position → value.** Logarithmic: $v = 1\cdot\left(\frac{200}{1}\right)^{p}\ \text{ms}$, $p = \frac{\ln(v/1)}{\ln(200)}$ — every factor of 1.699 takes a tenth of the travel; the centre ($p = 0.5$) is 14.1421 ms.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
