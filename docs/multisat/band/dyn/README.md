# Athanor (multiband saturator) › Band › Dynamics

[Back to Band](../README.md) · [Athanor (multiband saturator)](../../README.md)

Sets the band 1 dynamics of the selected band.

Part of **Athanor (multiband saturator)** – Multiband saturation: up to four bands (Linkwitz–Riley crossovers), each with a style (clean / warm tube, tape, transformer, soft / hard clip, foldback, rectify), drive, dynamics, tone, level, mix and a resonant feedback loop.

**Range:** -100 % … 100 % (default: 0.00 %)

Every band has this control; pick the band with the band buttons.

## Mathematics

$\delta$ (±100 %): adds up to $\pm 12$ dB of drive as the band level rises from −36 to 0 dBFS.

**Knob position → value.** Linear: $v = -100 + p\,(200)\ \text{\%}$, so each tenth of the travel is 20 %.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
