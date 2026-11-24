# Dragon's Lair (reverb) › Space › Decay

[Back to Space](../README.md) · [Dragon's Lair (reverb)](../../README.md)

How long the tail rings out.

Part of **Dragon's Lair (reverb)** – Room/hall/plate reverb (8-line FDN) with pre-delay, damping and width; early reflections and late reverb each have their own level and EQ.

**Range:** 0.10 s … 20.0 s (default: 2.20 s, logarithmic scale)

## Mathematics

$T_{60}$ (s): loop gain $g_i = 10^{-3N_i/(T_{60} f_s)}$, the time for the tail to fall by 60 dB.

**Knob position → value.** Logarithmic: $v = 0.1\cdot\left(\frac{20}{0.1}\right)^{p}\ \text{s}$, $p = \frac{\ln(v/0.1)}{\ln(200)}$ — every factor of 1.699 takes a tenth of the travel; the centre ($p = 0.5$) is 1.4142 s.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
