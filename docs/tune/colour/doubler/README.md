# Aurum Tune (pitch correction) › Colour › Doubler

[Back to Colour](../README.md) · [Aurum Tune (pitch correction)](../../README.md)

Sets the doubler.

Part of **Aurum Tune (pitch correction)** – Real-time, zero-latency pitch correction for tracking: snaps a monophonic voice to a key (retune speed, humanize, flex-tune, natural vibrato). Put it on the track's insert and arm the track to sing through it.

**Range:** 0.00 … 1.00 (default: 0.00)

## Mathematics

$D$: level $0.7D$ of the two delayed, swaying copies.

**Knob position → value.** Linear: $v = 0 + p\,(1)$, so each tenth of the travel is 0.1.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
