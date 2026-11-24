# Dragontamer (compressor) › Sidechain › Sidechain HPF

[Back to Sidechain](../README.md) · [Dragontamer (compressor)](../../README.md)

Sets the sidechain hpf.

Part of **Dragontamer (compressor)** – Compressor with eight styles (Clean, Classic, Opto, Vocal, Mastering, Bus, Punch, Pumping), look-ahead, hold, sidechain filters and external sidechain, auto makeup and parallel mix.

**Range:** 20.0 Hz … 500 Hz (default: 20.0 Hz, logarithmic scale)

## Mathematics

2nd-order RBJ high-pass ($Q = 0.707$, 12 dB/oct) on the detector path only. Bypassed at 20 Hz. Lowers the
detector's sensitivity to bass so kicks and bass notes do not pump the whole mix.

**Knob position → value.** Logarithmic: $v = 20\cdot\left(\frac{500}{20}\right)^{p}\ \text{Hz}$, $p = \frac{\ln(v/20)}{\ln(25)}$ — every factor of 1.380 takes a tenth of the travel; the centre ($p = 0.5$) is 100.0000 Hz.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
