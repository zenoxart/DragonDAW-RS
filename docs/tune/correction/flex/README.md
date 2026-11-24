# Aurum Tune (pitch correction) › Correction › Flex-tune

[Back to Correction](../README.md) · [Aurum Tune (pitch correction)](../../README.md)

Sets the flex-tune.

Part of **Aurum Tune (pitch correction)** – Real-time, zero-latency pitch correction for tracking: snaps a monophonic voice to a key (retune speed, humanize, flex-tune, natural vibrato). Put it on the track's insert and arm the track to sing through it.

**Range:** 0.00 ct … 50.0 ct (default: 0.00 ct)

## Mathematics

Notes already within this many cents of the target are left alone.

**Knob position → value.** Linear: $v = 0 + p\,(50)\ \text{ct}$, so each tenth of the travel is 5 ct.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
