# Jadefang (character EQ) › Controls › HMF freq

[Back to Controls](../README.md) · [Jadefang (character EQ)](../../README.md)

The frequency the effect works around. Drag it to move the effect through the spectrum.

Part of **Jadefang (character EQ)** – Six voicings of a four-band EQ with a driven input stage, a very broad Sub band and a boost-only Air band.

**Range:** 1.00 kHz … 8.00 kHz (default: 3.00 kHz, logarithmic scale)

## Mathematics

Bell centre frequency.

**Knob position → value.** Logarithmic: $v = 1000\cdot\left(\frac{8000}{1000}\right)^{p}\ \text{Hz}$, $p = \frac{\ln(v/1000)}{\ln(8)}$ — every factor of 1.231 takes a tenth of the travel; the centre ($p = 0.5$) is 2828.4272 Hz.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
