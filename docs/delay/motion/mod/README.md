# Echo Cavern (delay) › Motion › Modulation

[Back to Motion](../README.md) · [Echo Cavern (delay)](../../README.md)

Sets the modulation.

Part of **Echo Cavern (delay)** – Delay: tempo-synced or free, stereo / ping-pong / tape, cross feedback, resonant filters, drive and diffusion in the loop, pitch-shifted repeats, ducking, freeze, width, and a modulation matrix (two LFOs and an envelope follower routed through four slots to time, feedback, filters, drive, pitch, width or mix).

**Range:** 0.00 … 1.00 (default: 0.10)

## Mathematics

$M$: 0.6 Hz sine on the read position, depth $0.003 f_s M$ samples (up to 3 ms) — chorus-like wobble.

**Knob position → value.** Linear: $v = 0 + p\,(1)$, so each tenth of the travel is 0.1.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
