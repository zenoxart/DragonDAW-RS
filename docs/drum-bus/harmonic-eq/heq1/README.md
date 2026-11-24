# Dragonforge (drum bus) › Harmonic Eq › Harmonic EQ 60 Hz

[Back to Harmonic Eq](../README.md) · [Dragonforge (drum bus)](../../README.md)

Sets the harmonic eq 60 hz.

Part of **Dragonforge (drum bus)** – Drum bus processor: input with target meter, drive (three colours, high-pass), harmonics (50 / 80 Hz focus), compression (glue / punch, mix), lift, five-band harmonic EQ in 1 dB steps, parallel Sub / Low / Loud / Grit, output.

**Range:** 0.00 dB … 10.0 dB (default: 0.00 dB)

## Mathematics

Harmonic EQ band, in whole dB 0 … 10 (values are rounded to the step). Band $k$ adds
$a_k(0.5\tanh(2b_k) + 0.5b_k)$ with $a_k = 10^{G_k/20} - 1$, gliding over ≈ 5 ms; the five bands sit at 60, 180, 900 Hz,
3.5 and 10 kHz.

**Knob position → value.** Linear: $v = 0 + p\,(10)\ \text{dB}$, so each tenth of the travel is 1 dB.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
