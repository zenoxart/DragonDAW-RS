# Scrying Glass (analyzer) › View › Speed

[Back to View](../README.md) · [Scrying Glass (analyzer)](../../README.md)

How quickly the pitch is pulled to the target note.

Part of **Scrying Glass (analyzer)** – Analyzer insert: spectrum, peak and RMS levels, momentary and short-term loudness, true peak, stereo correlation and a vectorscope. The audio passes unchanged.

**Range:** Options: Fast, Medium, Slow (default: Medium)

## Mathematics

How fast falling bands follow: each block a band moves 50 % (fast), 25 % (medium) or 8 % (slow) of the way down to
its new reading; rising bands jump at once.

**Knob position → value.** A choice of 3: the value is the option's index $0 \dots 2$.

**Smoothing.** Takes effect at once (no glide).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
