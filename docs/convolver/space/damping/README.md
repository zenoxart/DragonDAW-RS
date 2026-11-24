# Reliquary (convolution reverb) › Space › Damping

[Back to Space](../README.md) · [Reliquary (convolution reverb)](../../README.md)

Rolls off the high frequencies of the tail as it decays, like soft surfaces in a room.

Part of **Reliquary (convolution reverb)** – Convolution reverb. Built-in rooms, halls, a plate, a spring, speaker cabinets and a telephone line, or any impulse-response file; size stretches the response, damping darkens its tail.

**Range:** 0.00 % … 100 % (default: 35.0 %)

## Mathematics

Extra loss of the highs over time. The response is split at 2.5 kHz (RBJ low-pass, $Q$ 0.707) and the upper band
multiplied by $e^{-6\,d\,t}$ ($d$ = damping / 100, $t$ in seconds): at full damping the highs lose an extra 52 dB per
second, so the tail darkens as it decays.

**Knob position → value.** Linear: $v = 0 + p\,(100)\ \text{\%}$, so each tenth of the travel is 10 %.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
