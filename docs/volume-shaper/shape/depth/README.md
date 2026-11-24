# Tidecaller (volume shaper) › Shape › Depth

[Back to Shape](../README.md) · [Tidecaller (volume shaper)](../../README.md)

How strong the effect is overall.

Part of **Tidecaller (volume shaper)** – Volume shaper and ducker. A tempo-synced gain curve (pump, duck, gate, saws, sine, swell) and / or ducking from a sidechain input.

**Range:** 0.00 % … 100 % (default: 80.0 %)

## Mathematics

How far the gain goes down, $d$ = depth / 100: the curve's 0 becomes $1 - d$ (−∞ dB at 100 %, −6 dB at 50 %), and
the ducker dips to $1 - d$.

**Knob position → value.** Linear: $v = 0 + p\,(100)\ \text{\%}$, so each tenth of the travel is 10 %.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
