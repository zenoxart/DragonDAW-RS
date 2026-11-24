# Portcullis (gate) › Threshold › Ratio

[Back to Threshold](../README.md) · [Portcullis (gate)](../../README.md)

How strongly the level is changed once it passes the threshold: 4:1 lets 1 dB through for every 4 dB above it.

Part of **Portcullis (gate)** – Gate / expander: threshold with hysteresis, range, ratio, knee, attack, hold, release, look-ahead, styles (Classic, Clean, Vocal, Guitar, Upward), sidechain filters, external sidechain, flip (ducking), mix.

**Range:** 1.00 :1 … 100 :1 (default: 100 :1, logarithmic scale)

## Mathematics

$R$ of the expansion below $T$: the output falls $R$ dB per dB of input below the threshold (100 = gate).

**Knob position → value.** Logarithmic: $v = 1\cdot\left(\frac{100}{1}\right)^{p}\ \text{:1}$, $p = \frac{\ln(v/1)}{\ln(100)}$ — every factor of 1.585 takes a tenth of the travel; the centre ($p = 0.5$) is 10.0000 :1.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
