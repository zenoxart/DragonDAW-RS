# Grimoire (lo-fi) › Digital › Sample rate

[Back to Digital](../README.md) · [Grimoire (lo-fi)](../../README.md)

How fast the modulation moves (in Hz, or locked to the song when Sync is on).

Part of **Grimoire (lo-fi)** – Lo-fi ageing: tape wow and flutter, a narrowing worn-out band, saturation, tape hiss, vinyl crackle, and bit-depth and sample-rate reduction.

**Range:** 2.00 kHz … 48.00 kHz (default: 48.00 kHz, logarithmic scale)

## Mathematics

The reduced sample rate $f_r$ (48 kHz = off).

**Knob position → value.** Logarithmic: $v = 2000\cdot\left(\frac{48000}{2000}\right)^{p}\ \text{Hz}$, $p = \frac{\ln(v/2000)}{\ln(24)}$ — every factor of 1.374 takes a tenth of the travel; the centre ($p = 0.5$) is 9797.9590 Hz.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
