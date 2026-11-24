# Sandglass (granular) › Output › Output

[Back to Output](../README.md) · [Sandglass (granular)](../../README.md)

The output level after the effect, to match the loudness before and after (compare with the power button).

Part of **Sandglass (granular)** – Live granular processor: scatters short grains of the last seconds of the input – size, density, delay and spray, pitch and its randomness, reversed grains, stereo spread, feedback and freeze.

**Range:** -24.0 dB … 12.0 dB (default: 0.00 dB)

## Mathematics

Output gain.

**Knob position → value.** Linear: $v = -24 + p\,(36)\ \text{dB}$, so each tenth of the travel is 3.6 dB.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
