# Sandglass (granular) › Pitch › Reverse

[Back to Pitch](../README.md) · [Sandglass (granular)](../../README.md)

Sets the reverse.

Part of **Sandglass (granular)** – Live granular processor: scatters short grains of the last seconds of the input – size, density, delay and spray, pitch and its randomness, reversed grains, stereo spread, feedback and freeze.

**Range:** 0.00 % … 100 % (default: 0.00 %)

## Mathematics

Probability that a grain plays backwards.

**Knob position → value.** Linear: $v = 0 + p\,(100)\ \text{\%}$, so each tenth of the travel is 10 %.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
