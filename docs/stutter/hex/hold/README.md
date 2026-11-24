# Hex (stutter / glitch) › Hex › Repeats

[Back to Hex](../README.md) · [Hex (stutter / glitch)](../../README.md)

How long the effect stays engaged after the signal drops before it starts to release.

Part of **Hex (stutter / glitch)** – Tempo-synced stutter: catches a slice of the beat and repeats it, always or by chance, with gating, a pitch step per repeat and reversed slices.

**Range:** Options: 2, 4, 8, 16 (default: 4)

## Mathematics

Slices per group $H$ (2, 4, 8, 16): the captured slice plays $H - 1$ more times.

**Knob position → value.** A choice of 4: the value is the option's index $0 \dots 3$.

**Smoothing.** Takes effect at once (no glide).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
