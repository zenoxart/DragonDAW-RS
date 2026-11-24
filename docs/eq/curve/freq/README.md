# Prism (parametric EQ) › Curve › Freq

[Back to Curve](../README.md) · [Prism (parametric EQ)](../../README.md)

The frequency of the selected band the effect works around. Drag it to move the effect through the spectrum.

Part of **Prism (parametric EQ)** – 7-band parametric EQ with shelves, cuts and notch per band.

**Range:** 20.0 Hz … 20.00 kHz (default: 30.0 Hz, logarithmic scale)

Every band has this control; pick the band with the band buttons.

## Mathematics

Centre / corner frequency $f$ (logarithmic knob: $f = 20\cdot 1000^{p}$ Hz). Clamped to $[10, 0.49 f_s]$.

**Knob position → value.** Logarithmic: $v = 20\cdot\left(\frac{20000}{20}\right)^{p}\ \text{Hz}$, $p = \frac{\ln(v/20)}{\ln(1000)}$ — every factor of 1.995 takes a tenth of the travel; the centre ($p = 0.5$) is 632.4555 Hz.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
