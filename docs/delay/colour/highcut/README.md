# Echo Cavern (delay) › Colour › High cut

[Back to Colour](../README.md) · [Echo Cavern (delay)](../../README.md)

Removes high frequencies above this point from the processed signal.

Part of **Echo Cavern (delay)** – Delay: tempo-synced or free, stereo / ping-pong / tape, cross feedback, resonant filters, drive and diffusion in the loop, pitch-shifted repeats, ducking, freeze, width, and a modulation matrix (two LFOs and an envelope follower routed through four slots to time, feedback, filters, drive, pitch, width or mix).

**Range:** 1.00 kHz … 20.00 kHz (default: 8.00 kHz, logarithmic scale)

## Mathematics

One-pole low-pass in the loop (repeats get darker), with the resonance bell at the same frequency.

**Knob position → value.** Logarithmic: $v = 1000\cdot\left(\frac{20000}{1000}\right)^{p}\ \text{Hz}$, $p = \frac{\ln(v/1000)}{\ln(20)}$ — every factor of 1.349 takes a tenth of the travel; the centre ($p = 0.5$) is 4472.1357 Hz.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
