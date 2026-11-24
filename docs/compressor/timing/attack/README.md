# Dragontamer (compressor) › Timing › Attack

[Back to Timing](../README.md) · [Dragontamer (compressor)](../../README.md)

How quickly the effect reacts when the signal rises. Short times catch transients, longer ones let them through.

Part of **Dragontamer (compressor)** – Compressor with eight styles (Clean, Classic, Opto, Vocal, Mastering, Bus, Punch, Pumping), look-ahead, hold, sidechain filters and external sidechain, auto makeup and parallel mix.

**Range:** 0.05 ms … 200 ms (default: 10.0 ms, logarithmic scale)

## Mathematics

$t_a$ (ms): time constant while the reduction grows, $c_a = e^{-1/(t_a 10^{-3} f_s)}$; after $t_a$ the
reduction has reached 63 % of its target. Scaled by the style ($\sigma_a$).

**Knob position → value.** Logarithmic: $v = 0.05\cdot\left(\frac{200}{0.05}\right)^{p}\ \text{ms}$, $p = \frac{\ln(v/0.05)}{\ln(4000)}$ — every factor of 2.292 takes a tenth of the travel; the centre ($p = 0.5$) is 3.1623 ms.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
