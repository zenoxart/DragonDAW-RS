# Quintessence (multiband dynamics) › Output › Crossover 1

[Back to Output](../README.md) · [Quintessence (multiband dynamics)](../../README.md)

Sets the crossover 1.

Part of **Quintessence (multiband dynamics)** – Multiband dynamics: up to four bands, each compressing or expanding (downward or upward) with threshold, range, ratio, knee, attack, release, gain, solo, bypass and per-band external sidechain.

**Range:** 30.0 Hz … 1.00 kHz (default: 200 Hz, logarithmic scale)

## Mathematics

First crossover $f_1$ (LR4, −6 dB point of both halves). Band 1 is everything below it.

**Knob position → value.** Logarithmic: $v = 30\cdot\left(\frac{1000}{30}\right)^{p}\ \text{Hz}$, $p = \frac{\ln(v/30)}{\ln(33.3333)}$ — every factor of 1.420 takes a tenth of the travel; the centre ($p = 0.5$) is 173.2051 Hz.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
