# Siren's Fork (tuner) › Tuner › Display

[Back to Tuner](../README.md) · [Siren's Fork (tuner)](../../README.md)

Sets the display.

Part of **Siren's Fork (tuner)** – Chromatic tuner: the played note, how many cents it is off, a needle or a strobe. A4 reference 430 – 450 Hz; mute silences the output while you tune.

**Range:** Options: Needle, Strobe (default: Needle)

## Mathematics

*Needle*: a needle over ±50 cents. *Strobe*: bands that drift at a speed proportional to the cents off and stand
still in tune – finer than a needle.

**Knob position → value.** A choice of 2: the value is the option's index $0 \dots 1$.

**Smoothing.** Takes effect at once (no glide).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
