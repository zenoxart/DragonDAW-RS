# Echo Cavern (delay) › Time › Sync

[Back to Time](../README.md) · [Echo Cavern (delay)](../../README.md)

Locks the timing to the song's tempo in note values instead of free milliseconds / Hz.

Part of **Echo Cavern (delay)** – Delay: tempo-synced or free, stereo / ping-pong / tape, cross feedback, resonant filters, drive and diffusion in the loop, pitch-shifted repeats, ducking, freeze, width, and a modulation matrix (two LFOs and an envelope follower routed through four slots to time, feedback, filters, drive, pitch, width or mix).

**Range:** Options: Free, 1/32, 1/16, 1/16 dotted, 1/8 triplet, 1/8, 1/8 dotted, 1/4 triplet, 1/4, 1/4 dotted, 1/2, 1 bar (default: 1/8 dotted)

## Mathematics

Note value $b$ (beats); the time follows tempo changes: $t = 60b/\text{BPM}$.

**Knob position → value.** A choice of 12: the value is the option's index $0 \dots 11$.

**Smoothing.** Takes effect at once (no glide).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
