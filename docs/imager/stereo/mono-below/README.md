# Wingspan (stereo imager) › Stereo › Mono below

[Back to Stereo](../README.md) · [Wingspan (stereo imager)](../../README.md)

Sets the mono below.

Part of **Wingspan (stereo imager)** – Widens the stereo image and keeps the bass mono; a goniometer and correlation meter show the phase relation of the result.

**Range:** 20.0 Hz … 500 Hz (default: 120 Hz, logarithmic scale)

## Mathematics

$f_m$: side content below it is removed (RBJ low-pass subtracted from the side).

**Knob position → value.** Logarithmic: $v = 20\cdot\left(\frac{500}{20}\right)^{p}\ \text{Hz}$, $p = \frac{\ln(v/20)}{\ln(25)}$ — every factor of 1.380 takes a tenth of the travel; the centre ($p = 0.5$) is 100.0000 Hz.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
