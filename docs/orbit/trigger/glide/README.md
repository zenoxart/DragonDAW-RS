# Ouroboros (auto-panner) › Trigger › Trigger glide

[Back to Trigger](../README.md) · [Ouroboros (auto-panner)](../../README.md)

Sets the trigger glide.

Part of **Ouroboros (auto-panner)** – Auto-panner: moves the sound across the stereo field locked to the bars, at a free rate, or one step per hit (Trigger, with glide). Seven shapes incl. random, width / centre / phase, smoothing, constant-power or balance pan law, binaural mode for headphones.

**Range:** 5.00 ms … 2000 ms (default: 120 ms, logarithmic scale)

## Mathematics

Trigger mode: time constant of the move to the next step.

**Knob position → value.** Logarithmic: $v = 5\cdot\left(\frac{2000}{5}\right)^{p}\ \text{ms}$, $p = \frac{\ln(v/5)}{\ln(400)}$ — every factor of 1.821 takes a tenth of the travel; the centre ($p = 0.5$) is 100.0000 ms.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
