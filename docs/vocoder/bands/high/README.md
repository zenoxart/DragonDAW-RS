# Mandrake (vocoder) › Bands › High band

[Back to Bands](../README.md) · [Mandrake (vocoder)](../../README.md)

Sets the high band.

Part of **Mandrake (vocoder)** – Channel vocoder with 8 – 32 bands. The voice shapes a carrier: the input itself (with the voice on the sidechain) or a built-in saw, pulse, noise or chord that follows the voice's pitch.

**Range:** 2.00 kHz … 12.00 kHz (default: 7.00 kHz, logarithmic scale)

## Mathematics

Centre $f_{hi}$ of the highest band (and where the sibilance band starts).

**Knob position → value.** Logarithmic: $v = 2000\cdot\left(\frac{12000}{2000}\right)^{p}\ \text{Hz}$, $p = \frac{\ln(v/2000)}{\ln(6)}$ — every factor of 1.196 takes a tenth of the travel; the centre ($p = 0.5$) is 4898.9795 Hz.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
