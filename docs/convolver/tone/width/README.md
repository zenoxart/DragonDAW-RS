# Reliquary (convolution reverb) › Tone › Width

[Back to Tone](../README.md) · [Reliquary (convolution reverb)](../../README.md)

The stereo width: 0 is mono, 100 % unchanged, above that wider.

Part of **Reliquary (convolution reverb)** – Convolution reverb. Built-in rooms, halls, a plate, a spring, speaker cabinets and a telephone line, or any impulse-response file; size stretches the response, damping darkens its tail.

**Range:** 0.00 % … 200 % (default: 100 %)

## Mathematics

Mid / side width $w$ of the wet signal: $m = \tfrac12(l + r)$, $s = \tfrac12(l - r)\,w$, $l' = m + s$, $r' = m - s$.

**Knob position → value.** Linear: $v = 0 + p\,(200)\ \text{\%}$, so each tenth of the travel is 20 %.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
