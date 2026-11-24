# Quicksilver (flanger) › Flanger › Rate

[Back to Flanger](../README.md) · [Quicksilver (flanger)](../../README.md)

How fast the modulation moves (in Hz, or locked to the song when Sync is on).

Part of **Quicksilver (flanger)** – Through-zero style flanger with positive or negative feedback.

**Range:** 0.02 Hz … 5.00 Hz (default: 0.25 Hz, logarithmic scale)

## Mathematics

$f$ (Hz): sweep frequency.

**Knob position → value.** Logarithmic: $v = 0.02\cdot\left(\frac{5}{0.02}\right)^{p}\ \text{Hz}$, $p = \frac{\ln(v/0.02)}{\ln(250)}$ — every factor of 1.737 takes a tenth of the travel; the centre ($p = 0.5$) is 0.3162 Hz.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
