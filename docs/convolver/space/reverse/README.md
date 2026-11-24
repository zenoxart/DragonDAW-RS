# Reliquary (convolution reverb) › Space › Reverse

[Back to Space](../README.md) · [Reliquary (convolution reverb)](../../README.md)

Sets the reverse.

Part of **Reliquary (convolution reverb)** – Convolution reverb. Built-in rooms, halls, a plate, a spring, speaker cabinets and a telephone line, or any impulse-response file; size stretches the response, damping darkens its tail.

**Range:** On / off (default: off)

## Mathematics

Plays the response backwards, $h_r[n] = h[L - 1 - n]$, with a 30 ms fade-in at its start: the tail swells up to the
sound instead of following it.

**Knob position → value.** A switch: 0 = off, 1 = on.

**Smoothing.** Takes effect at once (no glide).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
