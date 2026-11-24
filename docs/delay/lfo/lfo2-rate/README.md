# Echo Cavern (delay) › Lfo › LFO 2 rate

[Back to Lfo](../README.md) · [Echo Cavern (delay)](../../README.md)

How fast the modulation moves (in Hz, or locked to the song when Sync is on).

Part of **Echo Cavern (delay)** – Delay: tempo-synced or free, stereo / ping-pong / tape, cross feedback, resonant filters, drive and diffusion in the loop, pitch-shifted repeats, ducking, freeze, width, and a modulation matrix (two LFOs and an envelope follower routed through four slots to time, feedback, filters, drive, pitch, width or mix).

**Range:** 0.02 Hz … 20.0 Hz (default: 2.00 Hz, logarithmic scale)

## Mathematics

As LFO 1.

**Knob position → value.** Logarithmic: $v = 0.02\cdot\left(\frac{20}{0.02}\right)^{p}\ \text{Hz}$, $p = \frac{\ln(v/0.02)}{\ln(1000)}$ — every factor of 1.995 takes a tenth of the travel; the centre ($p = 0.5$) is 0.6325 Hz.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
