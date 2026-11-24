# Dragontamer (compressor) › Sidechain › Sidechain LPF

[Back to Sidechain](../README.md) · [Dragontamer (compressor)](../../README.md)

Sets the sidechain lpf.

Part of **Dragontamer (compressor)** – Compressor with eight styles (Clean, Classic, Opto, Vocal, Mastering, Bus, Punch, Pumping), look-ahead, hold, sidechain filters and external sidechain, auto makeup and parallel mix.

**Range:** 1.00 kHz … 20.00 kHz (default: 20.00 kHz, logarithmic scale)

## Mathematics

2nd-order RBJ low-pass on the detector path ($f \le 0.45 f_s$). Bypassed above 19.5 kHz.

**Knob position → value.** Logarithmic: $v = 1000\cdot\left(\frac{20000}{1000}\right)^{p}\ \text{Hz}$, $p = \frac{\ln(v/1000)}{\ln(20)}$ — every factor of 1.349 takes a tenth of the travel; the centre ($p = 0.5$) is 4472.1357 Hz.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
