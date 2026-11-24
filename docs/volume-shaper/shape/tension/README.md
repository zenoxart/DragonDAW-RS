# Tidecaller (volume shaper) › Shape › Tension

[Back to Shape](../README.md) · [Tidecaller (volume shaper)](../../README.md)

Sets the tension.

Part of **Tidecaller (volume shaper)** – Volume shaper and ducker. A tempo-synced gain curve (pump, duck, gate, saws, sine, swell) and / or ducking from a sidechain input.

**Range:** -100 % … 100 % (default: 0.00 %)

## Mathematics

Bends the curve: $g \leftarrow g^{k}$, $k = 2^{2\tau}$ ($\tau$ = tension / 100). Positive values keep the gain low for
longer (a deeper pump), negative ones recover sooner.

**Knob position → value.** Linear: $v = -100 + p\,(200)\ \text{\%}$, so each tenth of the travel is 20 %.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
