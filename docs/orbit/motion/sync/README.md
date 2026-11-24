# Ouroboros (auto-panner) › Motion › Sync rate

[Back to Motion](../README.md) · [Ouroboros (auto-panner)](../../README.md)

Locks the timing to the song's tempo in note values instead of free milliseconds / Hz.

Part of **Ouroboros (auto-panner)** – Auto-panner: moves the sound across the stereo field locked to the bars, at a free rate, or one step per hit (Trigger, with glide). Seven shapes incl. random, width / centre / phase, smoothing, constant-power or balance pan law, binaural mode for headphones.

**Range:** Options: 1/16, 1/8 triplet, 1/8, 1/8 dotted, 1/4 triplet, 1/4, 1/4 dotted, 1/2, 1 bar, 2 bars, 4 bars (default: 1/2)

## Mathematics

$b$: beats per cycle, 1/16 … 4 bars.

**Knob position → value.** A choice of 11: the value is the option's index $0 \dots 10$.

**Smoothing.** Takes effect at once (no glide).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
