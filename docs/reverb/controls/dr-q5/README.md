# Dragon's Lair (reverb) › Controls › Decay 5 Q

[Back to Controls](../README.md) · [Dragon's Lair (reverb)](../../README.md)

Sets the decay 5 q.

Part of **Dragon's Lair (reverb)** – Room/hall/plate reverb (8-line FDN) with pre-delay, damping and width; early reflections and late reverb each have their own level and EQ.

**Range:** 0.30 … 6.00 (default: 1.00, logarithmic scale)

## Mathematics

$Q$: steepness / width of the decay-rate band's shape in octaves.

**Knob position → value.** Logarithmic: $v = 0.3\cdot\left(\frac{6}{0.3}\right)^{p}$, $p = \frac{\ln(v/0.3)}{\ln(20)}$ — every factor of 1.349 takes a tenth of the travel; the centre ($p = 0.5$) is 1.3416.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
