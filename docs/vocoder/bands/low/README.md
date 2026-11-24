# Mandrake (vocoder) › Bands › Low band

[Back to Bands](../README.md) · [Mandrake (vocoder)](../../README.md)

Sets the low band.

Part of **Mandrake (vocoder)** – Channel vocoder with 8 – 32 bands. The voice shapes a carrier: the input itself (with the voice on the sidechain) or a built-in saw, pulse, noise or chord that follows the voice's pitch.

**Range:** 60.0 Hz … 500 Hz (default: 100 Hz, logarithmic scale)

## Mathematics

Centre $f_{lo}$ of the lowest band.

**Knob position → value.** Logarithmic: $v = 60\cdot\left(\frac{500}{60}\right)^{p}\ \text{Hz}$, $p = \frac{\ln(v/60)}{\ln(8.3333)}$ — every factor of 1.236 takes a tenth of the travel; the centre ($p = 0.5$) is 173.2051 Hz.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
