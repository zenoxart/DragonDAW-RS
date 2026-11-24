# Dragonforge (drum bus) › Harmonic Eq › Harmonic EQ 900 Hz

[Back to Harmonic Eq](../README.md) · [Dragonforge (drum bus)](../../README.md)

Sets the harmonic eq 900 hz.

Part of **Dragonforge (drum bus)** – Drum bus processor: input with target meter, drive (three colours, high-pass), harmonics (50 / 80 Hz focus), compression (glue / punch, mix), lift, five-band harmonic EQ in 1 dB steps, parallel Sub / Low / Loud / Grit, output.

**Range:** 0.00 dB … 10.0 dB (default: 0.00 dB)

## Mathematics

**Knob position → value.** Linear: $v = 0 + p\,(10)\ \text{dB}$, so each tenth of the travel is 1 dB.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
