# Echo Cavern (delay) › Feedback › Feedback

[Back to Feedback](../README.md) · [Echo Cavern (delay)](../../README.md)

How much of the output is fed back into the effect; more feedback means more repeats or a stronger resonance.

Part of **Echo Cavern (delay)** – Delay: tempo-synced or free, stereo / ping-pong / tape, cross feedback, resonant filters, drive and diffusion in the loop, pitch-shifted repeats, ducking, freeze, width, and a modulation matrix (two LFOs and an envelope follower routed through four slots to time, feedback, filters, drive, pitch, width or mix).

**Range:** 0.00 … 0.95 (default: 0.40)

## Mathematics

$f$: the gain of each repeat (before filters and drive): after $n$ repeats the level is $\approx f^n$, e.g. 0.5
falls 6 dB per repeat.

**Knob position → value.** Linear: $v = 0 + p\,(0.95)$, so each tenth of the travel is 0.095.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
