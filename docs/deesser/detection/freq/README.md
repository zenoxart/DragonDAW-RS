# Serpent's Hush (de-esser) › Detection › Detection high-pass

[Back to Detection](../README.md) · [Serpent's Hush (de-esser)](../../README.md)

The frequency the effect works around. Drag it to move the effect through the spectrum.

Part of **Serpent's Hush (de-esser)** – De-esser: single-vocal (relative) or allround detection, split- or wide-band processing, detection band, threshold, range, look-ahead, stereo link and listen.

**Range:** 2.00 kHz … 12.00 kHz (default: 5.50 kHz, logarithmic scale)

## Mathematics

$f_{hp}$: lower edge of the detection band (RBJ high-pass, $Q = 0.9$); the split shelf sits at $0.8 f_{hp}$.

**Knob position → value.** Logarithmic: $v = 2000\cdot\left(\frac{12000}{2000}\right)^{p}\ \text{Hz}$, $p = \frac{\ln(v/2000)}{\ln(6)}$ — every factor of 1.196 takes a tenth of the travel; the centre ($p = 0.5$) is 4898.9795 Hz.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
