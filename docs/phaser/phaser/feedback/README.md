# Moonphase (phaser) › Phaser › Feedback

[Back to Phaser](../README.md) · [Moonphase (phaser)](../../README.md)

How much of the output is fed back into the effect; more feedback means more repeats or a stronger resonance.

Part of **Moonphase (phaser)** – 2–12 stage all-pass phaser with feedback.

**Range:** 0.00 … 0.90 (default: 0.50)

## Mathematics

$g$: output fed back to the input of the cascade (sharper, more resonant notches).

**Knob position → value.** Linear: $v = 0 + p\,(0.9)$, so each tenth of the travel is 0.09.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
