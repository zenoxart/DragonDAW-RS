# Quicksilver (flanger) › Flanger › Feedback

[Back to Flanger](../README.md) · [Quicksilver (flanger)](../../README.md)

How much of the output is fed back into the effect; more feedback means more repeats or a stronger resonance.

Part of **Quicksilver (flanger)** – Through-zero style flanger with positive or negative feedback.

**Range:** -0.90 … 0.90 (default: 0.60)

## Mathematics

$g$ (−0.9 … 0.9): comb resonance; the sign shifts the comb by half a tooth (hollow vs. metallic).

**Knob position → value.** Linear: $v = -0.9 + p\,(1.8)$, so each tenth of the travel is 0.18.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
