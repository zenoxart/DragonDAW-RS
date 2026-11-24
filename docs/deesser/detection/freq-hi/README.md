# Serpent's Hush (de-esser) › Detection › Detection low-pass

[Back to Detection](../README.md) · [Serpent's Hush (de-esser)](../../README.md)

Sets the detection low-pass.

Part of **Serpent's Hush (de-esser)** – De-esser: single-vocal (relative) or allround detection, split- or wide-band processing, detection band, threshold, range, look-ahead, stereo link and listen.

**Range:** 4.00 kHz … 20.00 kHz (default: 16.00 kHz, logarithmic scale)

## Mathematics

$f_{lp}$: upper edge of the detection band, at least $1.2 f_{hp}$.

**Knob position → value.** Logarithmic: $v = 4000\cdot\left(\frac{20000}{4000}\right)^{p}\ \text{Hz}$, $p = \frac{\ln(v/4000)}{\ln(5)}$ — every factor of 1.175 takes a tenth of the travel; the centre ($p = 0.5$) is 8944.2715 Hz.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
