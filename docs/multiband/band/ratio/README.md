# Quintessence (multiband dynamics) › Band › Ratio

[Back to Band](../README.md) · [Quintessence (multiband dynamics)](../../README.md)

How strongly the level is changed once it passes the threshold of the selected band: 4:1 lets 1 dB through for every 4 dB above it.

Part of **Quintessence (multiband dynamics)** – Multiband dynamics: up to four bands, each compressing or expanding (downward or upward) with threshold, range, ratio, knee, attack, release, gain, solo, bypass and per-band external sidechain.

**Range:** 1.00 :1 … 20.0 :1 (default: 3.00 :1, logarithmic scale)

Every band has this control; pick the band with the band buttons.

## Mathematics

$R$: slope factor of the gain law; for compression the output slope above $T$ is $1/R$, for expansion the
input below $T$ falls $R$ dB per dB.

**Knob position → value.** Logarithmic: $v = 1\cdot\left(\frac{20}{1}\right)^{p}\ \text{:1}$, $p = \frac{\ln(v/1)}{\ln(20)}$ — every factor of 1.349 takes a tenth of the travel; the centre ($p = 0.5$) is 4.4721 :1.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
