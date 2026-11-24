# Athanor (multiband saturator) › Band › Tone

[Back to Band](../README.md) · [Athanor (multiband saturator)](../../README.md)

The frequency of the selected band the effect works around. Drag it to move the effect through the spectrum.

Part of **Athanor (multiband saturator)** – Multiband saturation: up to four bands (Linkwitz–Riley crossovers), each with a style (clean / warm tube, tape, transformer, soft / hard clip, foldback, rectify), drive, dynamics, tone, level, mix and a resonant feedback loop.

**Range:** -12.0 dB … 12.0 dB (default: 0.00 dB)

Every band has this control; pick the band with the band buttons.

## Mathematics

High-shelf gain after the shaper at the band's centre frequency.

**Knob position → value.** Linear: $v = -12 + p\,(24)\ \text{dB}$, so each tenth of the travel is 2.4 dB.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
