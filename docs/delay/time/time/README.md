# Echo Cavern (delay) › Time › Time (free)

[Back to Time](../README.md) · [Echo Cavern (delay)](../../README.md)

Sets the time (free).

Part of **Echo Cavern (delay)** – Delay: tempo-synced or free, stereo / ping-pong / tape, cross feedback, resonant filters, drive and diffusion in the loop, pitch-shifted repeats, ducking, freeze, width, and a modulation matrix (two LFOs and an envelope follower routed through four slots to time, feedback, filters, drive, pitch, width or mix).

**Range:** 1.00 ms … 2000 ms (default: 375 ms, logarithmic scale)

## Mathematics

Free delay time in ms (used when Sync = Free).

**Knob position → value.** Logarithmic: $v = 1\cdot\left(\frac{2000}{1}\right)^{p}\ \text{ms}$, $p = \frac{\ln(v/1)}{\ln(2000)}$ — every factor of 2.138 takes a tenth of the travel; the centre ($p = 0.5$) is 44.7214 ms.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
