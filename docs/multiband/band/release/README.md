# Quintessence (multiband dynamics) › Band › Release

[Back to Band](../README.md) · [Quintessence (multiband dynamics)](../../README.md)

How quickly the effect lets go after the signal falls of the selected band. Short times pump, long ones sound smoother.

Part of **Quintessence (multiband dynamics)** – Multiband dynamics: up to four bands, each compressing or expanding (downward or upward) with threshold, range, ratio, knee, attack, release, gain, solo, bypass and per-band external sidechain.

**Range:** 10.0 ms … 2000 ms (default: 150 ms, logarithmic scale)

Every band has this control; pick the band with the band buttons.

## Mathematics

Time constant (ms) while $|\Delta|$ shrinks.

**Knob position → value.** Logarithmic: $v = 10\cdot\left(\frac{2000}{10}\right)^{p}\ \text{ms}$, $p = \frac{\ln(v/10)}{\ln(200)}$ — every factor of 1.699 takes a tenth of the travel; the centre ($p = 0.5$) is 141.4214 ms.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
