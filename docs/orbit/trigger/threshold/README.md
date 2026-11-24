# Ouroboros (auto-panner) › Trigger › Trigger threshold

[Back to Trigger](../README.md) · [Ouroboros (auto-panner)](../../README.md)

The level above which the effect starts working. Lower it to act on more of the signal.

Part of **Ouroboros (auto-panner)** – Auto-panner: moves the sound across the stereo field locked to the bars, at a free rate, or one step per hit (Trigger, with glide). Seven shapes incl. random, width / centre / phase, smoothing, constant-power or balance pan law, binaural mode for headphones.

**Range:** -60.0 dB … 0.00 dB (default: -24.0 dB)

## Mathematics

Trigger level (dB) of the 0.5/40 ms envelope; re-arms 3 dB below.

**Knob position → value.** Linear: $v = -60 + p\,(60)\ \text{dB}$, so each tenth of the travel is 6 dB.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
