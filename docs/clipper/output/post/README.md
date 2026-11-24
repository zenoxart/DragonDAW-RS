# Obsidian (soft clipper) › Output › Post gain

[Back to Output](../README.md) · [Obsidian (soft clipper)](../../README.md)

The output level after the effect, to match the loudness before and after (compare with the power button).

Part of **Obsidian (soft clipper)** – Soft clipper: clean below the threshold, a smooth shoulder up to the ceiling, post gain; plus input drive, soft / hybrid / hard shapes, up to 4× oversampling and a clip-only listen.

**Range:** -24.0 dB … 12.0 dB (default: 0.00 dB)

## Mathematics

$10^{P/20}$ after the curve.

**Knob position → value.** Linear: $v = -24 + p\,(36)\ \text{dB}$, so each tenth of the travel is 3.6 dB.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
