# Reliquary (convolution reverb) › Output › Mix

[Back to Output](../README.md) · [Reliquary (convolution reverb)](../../README.md)

Blends the processed signal with the dry one: 0 leaves the input untouched, the top of the range is fully processed. The dry path is delay-compensated, so blending never smears or combs.

Part of **Reliquary (convolution reverb)** – Convolution reverb. Built-in rooms, halls, a plate, a spring, speaker cabinets and a telephone line, or any impulse-response file; size stretches the response, damping darkens its tail.

**Range:** 0.00 % … 100 % (default: 30.0 %)

## Mathematics

Wet share $m$: $y = (1 - m)\,x_D + m\,w$, the dry signal delayed by the block so the two stay aligned.

**Knob position → value.** Linear: $v = 0 + p\,(100)\ \text{\%}$, so each tenth of the travel is 10 %.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
