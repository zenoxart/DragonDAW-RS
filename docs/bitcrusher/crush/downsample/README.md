# Corroder (bit crusher) › Crush › Downsample

[Back to Crush](../README.md) · [Corroder (bit crusher)](../../README.md)

Holds every n-th sample, folding high frequencies back down for a lo-fi sound.

Part of **Corroder (bit crusher)** – Bit depth and sample-rate reduction.

**Range:** 1.00 × … 64.0 × (default: 4.00 ×, logarithmic scale)

## Mathematics

$R$: hold factor; the effective sample rate is $f_s/R$ and frequencies above $f_s/2R$ alias.

**Knob position → value.** Logarithmic: $v = 1\cdot\left(\frac{64}{1}\right)^{p}\ \text{×}$, $p = \frac{\ln(v/1)}{\ln(64)}$ — every factor of 1.516 takes a tenth of the travel; the centre ($p = 0.5$) is 8.0000 ×.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
