# Dragon's Lair (reverb) › Controls › Decay 5 freq

[Back to Controls](../README.md) · [Dragon's Lair (reverb)](../../README.md)

Sets the decay 5 freq.

Part of **Dragon's Lair (reverb)** – Room/hall/plate reverb (8-line FDN) with pre-delay, damping and width; early reflections and late reverb each have their own level and EQ.

**Range:** 20.0 Hz … 20.00 kHz (default: 12.00 kHz, logarithmic scale)

## Mathematics

Centre or corner frequency $f_b$ of the decay-rate band.

**Knob position → value.** Logarithmic: $v = 20\cdot\left(\frac{20000}{20}\right)^{p}\ \text{Hz}$, $p = \frac{\ln(v/20)}{\ln(1000)}$ — every factor of 1.995 takes a tenth of the travel; the centre ($p = 0.5$) is 632.4555 Hz.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
