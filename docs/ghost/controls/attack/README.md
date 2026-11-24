# Draconic Ghost (resonance suppressor) › Controls › Attack

[Back to Controls](../README.md) · [Draconic Ghost (resonance suppressor)](../../README.md)

How quickly the effect reacts when the signal rises. Short times catch transients, longer ones let them through.

Part of **Draconic Ghost (resonance suppressor)** – Dynamic resonance suppressor: finds the frequencies that stick out right now and pulls only those down, only while they're there. Zero-latency, low-latency and precise engines, focus EQ, soft / hard character, L/R or M/S with stereo link, sidechain, delta.

**Range:** 0.10 ms … 50.0 ms (default: 3.00 ms, logarithmic scale)

## Mathematics

Time constant while a cut deepens, per hop: $1 - e^{-h/(t_a 10^{-3} f_s)}$.

**Knob position → value.** Logarithmic: $v = 0.1\cdot\left(\frac{50}{0.1}\right)^{p}\ \text{ms}$, $p = \frac{\ln(v/0.1)}{\ln(500)}$ — every factor of 1.862 takes a tenth of the travel; the centre ($p = 0.5$) is 2.2361 ms.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
