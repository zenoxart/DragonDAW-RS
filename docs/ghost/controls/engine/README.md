# Draconic Ghost (resonance suppressor) › Controls › Engine

[Back to Controls](../README.md) · [Draconic Ghost (resonance suppressor)](../../README.md)

Chooses the variant of the processing; each option changes the character of the effect.

Part of **Draconic Ghost (resonance suppressor)** – Dynamic resonance suppressor: finds the frequencies that stick out right now and pulls only those down, only while they're there. Zero-latency, low-latency and precise engines, focus EQ, soft / hard character, L/R or M/S with stereo link, sidechain, delta.

**Range:** Options: Zero, Low, Precise (default: Low)

## Mathematics

Zero (band-pass bank, no latency), Low (bank + 64 samples look-ahead), Precise (STFT, 1024 samples).

**Knob position → value.** A choice of 3: the value is the option's index $0 \dots 2$.

**Smoothing.** Takes effect at once (no glide).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
