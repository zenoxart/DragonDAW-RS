# Draconic Ghost (resonance suppressor) › Controls › Focus 2 Q

[Back to Controls](../README.md) · [Draconic Ghost (resonance suppressor)](../../README.md)

Sets the focus 2 q.

Part of **Draconic Ghost (resonance suppressor)** – Dynamic resonance suppressor: finds the frequencies that stick out right now and pulls only those down, only while they're there. Zero-latency, low-latency and precise engines, focus EQ, soft / hard character, L/R or M/S with stereo link, sidechain, delta.

**Range:** 0.30 … 8.00 (default: 1.00, logarithmic scale)

## Mathematics

Focus bell width $w = 0.6/Q$ octaves (standard deviation).

**Knob position → value.** Logarithmic: $v = 0.3\cdot\left(\frac{8}{0.3}\right)^{p}$, $p = \frac{\ln(v/0.3)}{\ln(26.6667)}$ — every factor of 1.389 takes a tenth of the travel; the centre ($p = 0.5$) is 1.5492.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
