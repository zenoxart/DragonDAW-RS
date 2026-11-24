# Alembic (filter) › Filter A › Cutoff

[Back to Filter A](../README.md) · [Alembic (filter)](../../README.md)

The frequency the effect works around. Drag it to move the effect through the spectrum.

Part of **Alembic (filter)** – Filter: two filters (serial or parallel), low / high / band pass, notch, peak and shelves, 12 / 24 / 48 dB slopes, styles (clean, smooth, ram, hack), drive, stereo spread, LFO and envelope follower.

**Range:** 20.0 Hz … 20.00 kHz (default: 2.00 kHz, logarithmic scale)

## Mathematics

$f_0$: cutoff of A (before modulation and spread).

**Knob position → value.** Logarithmic: $v = 20\cdot\left(\frac{20000}{20}\right)^{p}\ \text{Hz}$, $p = \frac{\ln(v/20)}{\ln(1000)}$ — every factor of 1.995 takes a tenth of the travel; the centre ($p = 0.5$) is 632.4555 Hz.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
