# Familiar (auto-filter) › Envelope › Env attack

[Back to Envelope](../README.md) · [Familiar (auto-filter)](../../README.md)

How quickly the effect reacts when the signal rises. Short times catch transients, longer ones let them through.

Part of **Familiar (auto-filter)** – Auto-filter: a resonant low-, band-, high-pass or notch whose cutoff an LFO (free or synced, six shapes, stereo phase) and an envelope follower (positive or negative, from the input or the sidechain) move.

**Range:** 0.50 ms … 200 ms (default: 5.00 ms, logarithmic scale)

## Mathematics

Attack of the envelope follower.

**Knob position → value.** Logarithmic: $v = 0.5\cdot\left(\frac{200}{0.5}\right)^{p}\ \text{ms}$, $p = \frac{\ln(v/0.5)}{\ln(400)}$ — every factor of 1.821 takes a tenth of the travel; the centre ($p = 0.5$) is 10.0000 ms.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
