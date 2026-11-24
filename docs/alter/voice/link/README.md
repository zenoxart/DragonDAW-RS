# Shapeshifter (pitch & formant) › Voice › Link

[Back to Voice](../README.md) · [Shapeshifter (pitch & formant)](../../README.md)

How much the left and right (or mid and side) detectors move together.

Part of **Shapeshifter (pitch & formant)** – Real-time voice pitch and formant shifter: pitch ±12 semitones, formant, link, Transpose / Quantize / Robot modes, tube drive, mix and output.

**Range:** On / off (default: off)

## Mathematics

On: the formant follows the pitch, $\phi = 2^{(\varphi + p)/12}$ – like a sample played faster or slower (the
"chipmunk" effect). **Formant** then adds to it. Off: formant and pitch are independent.

**Knob position → value.** A switch: 0 = off, 1 = on.

**Smoothing.** Takes effect at once (no glide).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
