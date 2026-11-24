# Reliquary (convolution reverb) › Tone › Pre-delay

[Back to Tone](../README.md) · [Reliquary (convolution reverb)](../../README.md)

The gap before the reverb starts, keeping the dry sound clear in front of it.

Part of **Reliquary (convolution reverb)** – Convolution reverb. Built-in rooms, halls, a plate, a spring, speaker cabinets and a telephone line, or any impulse-response file; size stretches the response, damping darkens its tail.

**Range:** 0.00 ms … 250 ms (default: 10.0 ms)

## Mathematics

Delay of the input into the convolution (a fractional delay line, up to 250 ms): the gap between the dry sound and
the first reflection, which tells the ear how large the space is.

**Knob position → value.** Linear: $v = 0 + p\,(250)\ \text{ms}$, so each tenth of the travel is 25 ms.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
