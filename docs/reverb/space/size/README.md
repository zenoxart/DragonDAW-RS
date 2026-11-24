# Dragon's Lair (reverb) › Space › Size

[Back to Space](../README.md) · [Dragon's Lair (reverb)](../../README.md)

The size of the simulated space.

Part of **Dragon's Lair (reverb)** – Room/hall/plate reverb (8-line FDN) with pre-delay, damping and width; early reflections and late reverb each have their own level and EQ.

**Range:** 0.10 … 1.00 (default: 0.60)

## Mathematics

$S$: line lengths $\propto (0.3 + 1.7S)$ and early taps $\propto (0.5 + S)$. Larger sizes lower the modal
density per second (more distinct echoes) and need a longer $T_{60}$ to sound equally long.

**Knob position → value.** Linear: $v = 0.1 + p\,(0.9)$, so each tenth of the travel is 0.09.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
