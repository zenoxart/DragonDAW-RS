# Ouroboros (auto-panner) › Motion › Smooth

[Back to Motion](../README.md) · [Ouroboros (auto-panner)](../../README.md)

Sets the smooth.

Part of **Ouroboros (auto-panner)** – Auto-panner: moves the sound across the stereo field locked to the bars, at a free rate, or one step per hit (Trigger, with glide). Seven shapes incl. random, width / centre / phase, smoothing, constant-power or balance pan law, binaural mode for headphones.

**Range:** 0.00 % … 100 % (default: 10.0 %)

## Mathematics

$\sigma$: smoothing time $2\,\text{ms} + 0.25\sigma T_{cycle}$ — 0 % keeps square edges, 100 % rounds the
movement over a quarter of the cycle.

**Knob position → value.** Linear: $v = 0 + p\,(100)\ \text{\%}$, so each tenth of the travel is 10 %.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
