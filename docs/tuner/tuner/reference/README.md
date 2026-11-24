# Siren's Fork (tuner) › Tuner › A4

[Back to Tuner](../README.md) · [Siren's Fork (tuner)](../../README.md)

Sets the a4.

Part of **Siren's Fork (tuner)** – Chromatic tuner: the played note, how many cents it is off, a needle or a strobe. A4 reference 430 – 450 Hz; mute silences the output while you tune.

**Range:** 430 Hz … 450 Hz (default: 440 Hz)

## Mathematics

The frequency of A4, $f_{ref}$ (430 … 450 Hz). Every note is measured against it.

**Knob position → value.** Linear: $v = 430 + p\,(20)\ \text{Hz}$, so each tenth of the travel is 2 Hz.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
