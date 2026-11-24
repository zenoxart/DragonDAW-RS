# Prism (parametric EQ) › Dynamic › Dynamic range

[Back to Dynamic](../README.md) · [Prism (parametric EQ)](../../README.md)

Sets the band 1 dynamic range of the selected band.

Part of **Prism (parametric EQ)** – 7-band parametric EQ with shelves, cuts and notch per band.

**Range:** -18.0 dB … 18.0 dB (default: 0.00 dB)

Every band has this control; pick the band with the band buttons.

## Mathematics

$D$: dynamic gain change at full overshoot, $G_{dyn} = D\cdot\text{clamp}((X - T)/6, 0, 1)$; negative values
make the band cut when the band gets loud.

**Knob position → value.** Linear: $v = -18 + p\,(36)\ \text{dB}$, so each tenth of the travel is 3.6 dB.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
