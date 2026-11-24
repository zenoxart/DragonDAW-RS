# Athanor (multiband saturator) › Crossovers › Crossover 1

[Back to Crossovers](../README.md) · [Athanor (multiband saturator)](../../README.md)

Sets the crossover 1.

Part of **Athanor (multiband saturator)** – Multiband saturation: up to four bands (Linkwitz–Riley crossovers), each with a style (clean / warm tube, tape, transformer, soft / hard clip, foldback, rectify), drive, dynamics, tone, level, mix and a resonant feedback loop.

**Range:** 30.0 Hz … 1.00 kHz (default: 150 Hz, logarithmic scale)

## Mathematics

First LR4 crossover.

**Knob position → value.** Logarithmic: $v = 30\cdot\left(\frac{1000}{30}\right)^{p}\ \text{Hz}$, $p = \frac{\ln(v/30)}{\ln(33.3333)}$ — every factor of 1.420 takes a tenth of the travel; the centre ($p = 0.5$) is 173.2051 Hz.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
