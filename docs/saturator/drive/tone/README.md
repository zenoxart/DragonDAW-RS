# Crucible (saturator) › Drive › Tone

[Back to Drive](../README.md) · [Crucible (saturator)](../../README.md)

The frequency the effect works around. Drag it to move the effect through the spectrum.

Part of **Crucible (saturator)** – Tube (12AX7, 12AT7, 12AU7, 6SN7, EF86, EL34, 6L6, KT88, 300B models with bias, blocking and sag), tape, soft/hard clip, foldback and asymmetric waveshaping with tone and parallel mix, plus three parallel tube ingredients to blend into the brew: a warm 6SN7, a hot 12AX7 and a crisp EF86 pentode.

**Range:** 1.00 kHz … 20.00 kHz (default: 12.00 kHz, logarithmic scale)

## Mathematics

One-pole low-pass after the shaper: $a = e^{-2\pi f/f_s}$ (6 dB/oct).

**Knob position → value.** Logarithmic: $v = 1000\cdot\left(\frac{20000}{1000}\right)^{p}\ \text{Hz}$, $p = \frac{\ln(v/1000)}{\ln(20)}$ — every factor of 1.349 takes a tenth of the travel; the centre ($p = 0.5$) is 4472.1357 Hz.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
