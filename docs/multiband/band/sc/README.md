# Quintessence (multiband dynamics) › Band › External sidechain

[Back to Band](../README.md) · [Quintessence (multiband dynamics)](../../README.md)

Uses another mixer channel (chosen in the Sidechain input menu) to control the effect.

Part of **Quintessence (multiband dynamics)** – Multiband dynamics: up to four bands, each compressing or expanding (downward or upward) with threshold, range, ratio, knee, attack, release, gain, solo, bypass and per-band external sidechain.

**Range:** On / off (default: off)

Every band has this control; pick the band with the band buttons.

## Mathematics

The band's detector reads the same band of the external sidechain (split with its own crossovers) instead of
its own signal.

**Knob position → value.** A switch: 0 = off, 1 = on.

**Smoothing.** Takes effect at once (no glide).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
