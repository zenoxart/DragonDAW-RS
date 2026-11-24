# Shapeshifter (pitch & formant) › Character › Drive

[Back to Character](../README.md) · [Shapeshifter (pitch & formant)](../../README.md)

How hard the signal is pushed into the saturation stage; more drive adds harmonics and density.

Part of **Shapeshifter (pitch & formant)** – Real-time voice pitch and formant shifter: pitch ±12 semitones, formant, link, Transpose / Quantize / Robot modes, tube drive, mix and output.

**Range:** 0.00 % … 100 % (default: 0.00 %)

## Mathematics

Drive $\delta = v/100$, $d = 1 + 6\delta$: the wet signal becomes $\tanh(dx)/\sqrt d$. Small signals gain $\sqrt d$
(up to +8.5 dB at 100 %), peaks are rounded off.

**Knob position → value.** Linear: $v = 0 + p\,(100)\ \text{\%}$, so each tenth of the travel is 10 %.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
