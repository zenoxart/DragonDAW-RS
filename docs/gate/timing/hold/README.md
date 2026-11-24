# Portcullis (gate) › Timing › Hold

[Back to Timing](../README.md) · [Portcullis (gate)](../../README.md)

How long the effect stays engaged after the signal drops before it starts to release.

Part of **Portcullis (gate)** – Gate / expander: threshold with hysteresis, range, ratio, knee, attack, hold, release, look-ahead, styles (Classic, Clean, Vocal, Guitar, Upward), sidechain filters, external sidechain, flip (ducking), mix.

**Range:** 1.00 ms … 500 ms (default: 20.0 ms, logarithmic scale)

## Mathematics

Samples the gate stays open after the level has fallen below $T - H$: $t_h\,10^{-3} f_s\,\sigma_h$.

**Knob position → value.** Logarithmic: $v = 1\cdot\left(\frac{500}{1}\right)^{p}\ \text{ms}$, $p = \frac{\ln(v/1)}{\ln(500)}$ — every factor of 1.862 takes a tenth of the travel; the centre ($p = 0.5$) is 22.3607 ms.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
