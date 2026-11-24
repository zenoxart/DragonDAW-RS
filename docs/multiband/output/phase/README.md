# Quintessence (multiband dynamics) › Output › Processing

[Back to Output](../README.md) · [Quintessence (multiband dynamics)](../../README.md)

Sets the processing.

Part of **Quintessence (multiband dynamics)** – Multiband dynamics: up to four bands, each compressing or expanding (downward or upward) with threshold, range, ratio, knee, attack, release, gain, solo, bypass and per-band external sidechain.

**Range:** Options: Crossover, Dynamic phase (default: Crossover)

## Mathematics

Crossover (LR4 split, all-pass compensated) or Dynamic phase (shelf/bell filters on the whole signal, no
phase shift while idle). Solo always uses the split.

**Knob position → value.** A choice of 2: the value is the option's index $0 \dots 1$.

**Smoothing.** Takes effect at once (no glide).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
