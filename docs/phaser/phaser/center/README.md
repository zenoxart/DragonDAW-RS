# Moonphase (phaser) › Phaser › Center

[Back to Phaser](../README.md) · [Moonphase (phaser)](../../README.md)

The frequency the effect works around. Drag it to move the effect through the spectrum.

Part of **Moonphase (phaser)** – 2–12 stage all-pass phaser with feedback.

**Range:** 100 Hz … 5.00 kHz (default: 900 Hz, logarithmic scale)

## Mathematics

$f_c$: the centre of the sweep.

**Knob position → value.** Logarithmic: $v = 100\cdot\left(\frac{5000}{100}\right)^{p}\ \text{Hz}$, $p = \frac{\ln(v/100)}{\ln(50)}$ — every factor of 1.479 takes a tenth of the travel; the centre ($p = 0.5$) is 707.1068 Hz.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
