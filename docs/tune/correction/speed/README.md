# Aurum Tune (pitch correction) › Correction › Retune speed

[Back to Correction](../README.md) · [Aurum Tune (pitch correction)](../../README.md)

How quickly the pitch is pulled to the target note.

Part of **Aurum Tune (pitch correction)** – Real-time, zero-latency pitch correction for tracking: snaps a monophonic voice to a key (retune speed, humanize, flex-tune, natural vibrato). Put it on the track's insert and arm the track to sing through it.

**Range:** 0.10 ms … 400 ms (default: 20.0 ms, logarithmic scale)

## Mathematics

$t$ (ms): time constant of the ratio's glide to the target; < 0.5 ms = instant (hard tune).

**Knob position → value.** Logarithmic: $v = 0.1\cdot\left(\frac{400}{0.1}\right)^{p}\ \text{ms}$, $p = \frac{\ln(v/0.1)}{\ln(4000)}$ — every factor of 2.292 takes a tenth of the travel; the centre ($p = 0.5$) is 6.3246 ms.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
