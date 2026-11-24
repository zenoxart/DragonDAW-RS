# Golem Strip (class-A channel strip) › Opto Comp › Comp release

[Back to Opto Comp](../README.md) · [Golem Strip (class-A channel strip)](../../README.md)

How quickly the effect lets go after the signal falls. Short times pump, long ones sound smoother.

Part of **Golem Strip (class-A channel strip)** – Recording channel: a one-fader noise suppressor, Class-A transformer mic preamp with stepped inductor-style EQ into a tube opto compressor (program-dependent opto attack, two-stage release with memory, fixed or manual timing). Zero latency – track through it.

**Range:** 50.0 ms … 10000 ms (default: 400 ms, logarithmic scale)

## Mathematics

Release in ms. *Fix / Man*: the slow release stage $t_{r,s}$. *Manual*: the fast stage $t_{r,f}$, with the slow one at
4×. The applied reduction is $0.6F + 0.4S$, so the first 60 % of a recovery follows the fast stage.

**Knob position → value.** Logarithmic: $v = 50\cdot\left(\frac{10000}{50}\right)^{p}\ \text{ms}$, $p = \frac{\ln(v/50)}{\ln(200)}$ — every factor of 1.699 takes a tenth of the travel; the centre ($p = 0.5$) is 707.1068 ms.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
