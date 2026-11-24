# Alembic (filter) › Filter A › Drive

[Back to Filter A](../README.md) · [Alembic (filter)](../../README.md)

How hard the signal is pushed into the saturation stage; more drive adds harmonics and density.

Part of **Alembic (filter)** – Filter: two filters (serial or parallel), low / high / band pass, notch, peak and shelves, 12 / 24 / 48 dB slopes, styles (clean, smooth, ram, hack), drive, stereo spread, LFO and envelope follower.

**Range:** 0.00 dB … 24.0 dB (default: 0.00 dB)

## Mathematics

$D$ (dB): $\tanh(dx)/\sqrt d$ before the filters.

**Knob position → value.** Linear: $v = 0 + p\,(24)\ \text{dB}$, so each tenth of the travel is 2.4 dB.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
