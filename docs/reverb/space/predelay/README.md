# Dragon's Lair (reverb) › Space › Pre-delay

[Back to Space](../README.md) · [Dragon's Lair (reverb)](../../README.md)

The gap before the reverb starts, keeping the dry sound clear in front of it.

Part of **Dragon's Lair (reverb)** – Room/hall/plate reverb (8-line FDN) with pre-delay, damping and width; early reflections and late reverb each have their own level and EQ.

**Range:** 0.10 ms … 200 ms (default: 12.0 ms, logarithmic scale)

## Mathematics

Delay before the reverb (and early reflections): $t\,10^{-3} f_s$ samples.

**Knob position → value.** Logarithmic: $v = 0.1\cdot\left(\frac{200}{0.1}\right)^{p}\ \text{ms}$, $p = \frac{\ln(v/0.1)}{\ln(2000)}$ — every factor of 2.138 takes a tenth of the travel; the centre ($p = 0.5$) is 4.4721 ms.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
