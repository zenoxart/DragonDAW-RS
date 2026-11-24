# Wisp (exciter) › Harmonics › Frequency

[Back to Harmonics](../README.md) · [Wisp (exciter)](../../README.md)

The frequency the effect works around. Drag it to move the effect through the spectrum.

Part of **Wisp (exciter)** – Harmonic exciter: generates new harmonics above a chosen frequency (odd or even), adds air on top and, if asked, sub harmonics below the low end. Listen plays only what it adds.

**Range:** 800 Hz … 12.00 kHz (default: 3.00 kHz, logarithmic scale)

## Mathematics

The split $f$: only the band above it is excited.

**Knob position → value.** Logarithmic: $v = 800\cdot\left(\frac{12000}{800}\right)^{p}\ \text{Hz}$, $p = \frac{\ln(v/800)}{\ln(15)}$ — every factor of 1.311 takes a tenth of the travel; the centre ($p = 0.5$) is 3098.3867 Hz.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
