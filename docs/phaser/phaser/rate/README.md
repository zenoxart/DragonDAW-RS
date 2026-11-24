# Moonphase (phaser) › Phaser › Rate

[Back to Phaser](../README.md) · [Moonphase (phaser)](../../README.md)

How fast the modulation moves (in Hz, or locked to the song when Sync is on).

Part of **Moonphase (phaser)** – 2–12 stage all-pass phaser with feedback.

**Range:** 0.02 Hz … 8.00 Hz (default: 0.40 Hz, logarithmic scale)

## Mathematics

LFO frequency (triangle).

**Knob position → value.** Logarithmic: $v = 0.02\cdot\left(\frac{8}{0.02}\right)^{p}\ \text{Hz}$, $p = \frac{\ln(v/0.02)}{\ln(400)}$ — every factor of 1.821 takes a tenth of the travel; the centre ($p = 0.5$) is 0.4000 Hz.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
