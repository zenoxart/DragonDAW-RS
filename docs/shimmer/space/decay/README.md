# Seraph (shimmer reverb) › Space › Decay

[Back to Space](../README.md) · [Seraph (shimmer reverb)](../../README.md)

How long the tail rings out.

Part of **Seraph (shimmer reverb)** – Shimmer reverb: a dense feedback delay network whose tail is pitch-shifted (an octave up by default) on every round, so it rises into a glowing choir-like halo.

**Range:** 0.50 s … 30.0 s (default: 6.00 s, logarithmic scale)

## Mathematics

$T_{60}$: time to fall by 60 dB.

**Knob position → value.** Logarithmic: $v = 0.5\cdot\left(\frac{30}{0.5}\right)^{p}\ \text{s}$, $p = \frac{\ln(v/0.5)}{\ln(60)}$ — every factor of 1.506 takes a tenth of the travel; the centre ($p = 0.5$) is 3.8730 s.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
