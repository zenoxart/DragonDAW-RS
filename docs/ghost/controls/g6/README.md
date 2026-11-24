# Draconic Ghost (resonance suppressor) › Controls › Focus 6 gain

[Back to Controls](../README.md) · [Draconic Ghost (resonance suppressor)](../../README.md)

Sets the focus 6 gain.

Part of **Draconic Ghost (resonance suppressor)** – Dynamic resonance suppressor: finds the frequencies that stick out right now and pulls only those down, only while they're there. Zero-latency, low-latency and precise engines, focus EQ, soft / hard character, L/R or M/S with stereo link, sidechain, delta.

**Range:** -12.0 dB … 12.0 dB (default: 0.00 dB)

## Mathematics

Focus gain $g$ (dB): raises ($+$) or lowers the sensitivity there; $s_b = 2^{F/6}$, so +6 dB doubles it.

**Knob position → value.** Linear: $v = -12 + p\,(24)\ \text{dB}$, so each tenth of the travel is 2.4 dB.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
