# Serpent's Hush (de-esser) › Reduction › Look-ahead

[Back to Reduction](../README.md) · [Serpent's Hush (de-esser)](../../README.md)

Lets the detector see the signal slightly in advance so the effect can react before a peak arrives (adds this much latency, compensated by the mixer).

Part of **Serpent's Hush (de-esser)** – De-esser: single-vocal (relative) or allround detection, split- or wide-band processing, detection band, threshold, range, look-ahead, stereo link and listen.

**Range:** 0.00 ms … 15.0 ms (default: 0.00 ms)

## Mathematics

Delay of the audio path (max 15 ms, reported as latency) so the reduction is in place before the "s" begins.

**Knob position → value.** Linear: $v = 0 + p\,(15)\ \text{ms}$, so each tenth of the travel is 1.5 ms.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
