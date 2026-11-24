# Talon (transient shaper) › Sustain › Sustain amount

[Back to Sustain](../README.md) · [Talon (transient shaper)](../../README.md)

Sets the sustain amount.

Part of **Talon (transient shaper)** – Transient designer: attack (Needle / Nail / Blunt) and sustain (Linear / Non-linear / Soft linear) sections with level, sensitivity, duration and shape, mix, output, and a guard (Off / Clip / Limit / Soft clip) after the output.

**Range:** -100 % … 100 % (default: 0.00 %)

## Mathematics

$A_s$: sustain gain at the window peak $= 18 A_s$ dB.

**Knob position → value.** Linear: $v = -100 + p\,(200)\ \text{\%}$, so each tenth of the travel is 20 %.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
