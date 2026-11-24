# Philosopher's Stone (one-knob mastering) › Tone › Low

[Back to Tone](../README.md) · [Philosopher's Stone (one-knob mastering)](../../README.md)

Sets the low.

Part of **Philosopher's Stone (one-knob mastering)** – One-knob mastering: tone EQ with character, transient punch, glue compression, tube colour, widening, a soft clipper and a true-peak limiter, with auto gain for honest A/B.

**Range:** -6.00 dB … 6.00 dB (default: 1.00 dB)

## Mathematics

$L$: user part of the 90 Hz low-shelf gain.

**Knob position → value.** Linear: $v = -6 + p\,(12)\ \text{dB}$, so each tenth of the travel is 1.2 dB.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
