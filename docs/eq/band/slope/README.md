# Prism (parametric EQ) › Band › Slope

[Back to Band](../README.md) · [Prism (parametric EQ)](../../README.md)

How steeply the filter cuts beyond its frequency (dB per octave).

Part of **Prism (parametric EQ)** – 7-band parametric EQ with shelves, cuts and notch per band.

**Range:** Options: 6 dB/oct, 12 dB/oct, 18 dB/oct, 24 dB/oct, 30 dB/oct, 36 dB/oct, 48 dB/oct, 72 dB/oct, 96 dB/oct (default: 12 dB/oct)

Every band has this control; pick the band with the band buttons.

## Mathematics

$S \in \{6, 12, 18, 24, 30, 36, 48, 72, 96\}$ dB/oct: order $N = S/6$ (Butterworth pass filters, gain-split shelves).

**Knob position → value.** A choice of 9: the value is the option's index $0 \dots 8$.

**Smoothing.** Takes effect at once (no glide).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
