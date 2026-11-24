# Portcullis (gate) › Sidechain › Sidechain low-pass

[Back to Sidechain](../README.md) · [Portcullis (gate)](../../README.md)

Sets the sidechain low-pass.

Part of **Portcullis (gate)** – Gate / expander: threshold with hysteresis, range, ratio, knee, attack, hold, release, look-ahead, styles (Classic, Clean, Vocal, Guitar, Upward), sidechain filters, external sidechain, flip (ducking), mix.

**Range:** 500 Hz … 20.00 kHz (default: 20.00 kHz, logarithmic scale)

## Mathematics

RBJ low-pass on the detector.

**Knob position → value.** Logarithmic: $v = 500\cdot\left(\frac{20000}{500}\right)^{p}\ \text{Hz}$, $p = \frac{\ln(v/500)}{\ln(40)}$ — every factor of 1.446 takes a tenth of the travel; the centre ($p = 0.5$) is 3162.2776 Hz.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
