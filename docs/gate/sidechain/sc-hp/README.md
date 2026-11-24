# Portcullis (gate) › Sidechain › Sidechain high-pass

[Back to Sidechain](../README.md) · [Portcullis (gate)](../../README.md)

Sets the sidechain high-pass.

Part of **Portcullis (gate)** – Gate / expander: threshold with hysteresis, range, ratio, knee, attack, hold, release, look-ahead, styles (Classic, Clean, Vocal, Guitar, Upward), sidechain filters, external sidechain, flip (ducking), mix.

**Range:** 20.0 Hz … 2.00 kHz (default: 20.0 Hz, logarithmic scale)

## Mathematics

RBJ high-pass on the detector (12 dB/oct, $Q = 0.707$).

**Knob position → value.** Logarithmic: $v = 20\cdot\left(\frac{2000}{20}\right)^{p}\ \text{Hz}$, $p = \frac{\ln(v/20)}{\ln(100)}$ — every factor of 1.585 takes a tenth of the travel; the centre ($p = 0.5$) is 200.0000 Hz.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
