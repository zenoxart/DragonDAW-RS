# Tidecaller (volume shaper) › Shape › Rate

[Back to Shape](../README.md) · [Tidecaller (volume shaper)](../../README.md)

How fast the modulation moves (in Hz, or locked to the song when Sync is on).

Part of **Tidecaller (volume shaper)** – Volume shaper and ducker. A tempo-synced gain curve (pump, duck, gate, saws, sine, swell) and / or ducking from a sidechain input.

**Range:** Options: 1/32, 1/16, 1/8, 1/4, 1/2, 1 bar, 2 bars, 4 bars (default: 1/4)

## Mathematics

Length $R$ of one cycle: 1/32 note (0.125 beat) … 4 bars (16 beats).

**Knob position → value.** A choice of 8: the value is the option's index $0 \dots 7$.

**Smoothing.** Takes effect at once (no glide).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
