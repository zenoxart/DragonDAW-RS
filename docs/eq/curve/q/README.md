# Prism (parametric EQ) › Curve › Q

[Back to Curve](../README.md) · [Prism (parametric EQ)](../../README.md)

How narrow the affected range is of the selected band; high values focus on a small range and can ring.

Part of **Prism (parametric EQ)** – 7-band parametric EQ with shelves, cuts and notch per band.

**Range:** 0.10 … 18.0 (default: 0.90, logarithmic scale)

Every band has this control; pick the band with the band buttons.

## Mathematics

$Q$: bandwidth of bells ($\alpha = \sin\omega_0/2Q$; the −3 dB bandwidth of a band pass is $f/Q$), shelf
steepness, and the resonance of pass filters (0.707 = Butterworth).

**Knob position → value.** Logarithmic: $v = 0.1\cdot\left(\frac{18}{0.1}\right)^{p}$, $p = \frac{\ln(v/0.1)}{\ln(180)}$ — every factor of 1.681 takes a tenth of the travel; the centre ($p = 0.5$) is 1.3416.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
