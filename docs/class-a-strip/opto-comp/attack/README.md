# Golem Strip (class-A channel strip) › Opto Comp › Comp attack

[Back to Opto Comp](../README.md) · [Golem Strip (class-A channel strip)](../../README.md)

How quickly the effect reacts when the signal rises. Short times catch transients, longer ones let them through.

Part of **Golem Strip (class-A channel strip)** – Recording channel: a one-fader noise suppressor, Class-A transformer mic preamp with stepped inductor-style EQ into a tube opto compressor (program-dependent opto attack, two-stage release with memory, fixed or manual timing). Zero latency – track through it.

**Range:** 0.50 ms … 300 ms (default: 10.0 ms, logarithmic scale)

## Mathematics

**Knob position → value.** Logarithmic: $v = 0.5\cdot\left(\frac{300}{0.5}\right)^{p}\ \text{ms}$, $p = \frac{\ln(v/0.5)}{\ln(600)}$ — every factor of 1.896 takes a tenth of the travel; the centre ($p = 0.5$) is 12.2474 ms.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
