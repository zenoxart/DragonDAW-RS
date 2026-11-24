# Purify (voice cleanup) › De Esser › Deesser range

[Back to De Esser](../README.md) · [Purify (voice cleanup)](../../README.md)

The most the effect may change the level.

Part of **Purify (voice cleanup)** – Post-production vocal chain in one plugin (after zenoxart/CleanVoice): valve vari-mu, leveller, vari-mu, de-esser, two 4-band multibands, parallel presence, oversampled saturation and a resonance suppressor.

**Range:** 0.00 dB … 24.0 dB (default: 6.00 dB)

## Mathematics

The deepest the de-esser may reduce, in dB: the gain computer's output is clamped to $\ge -\text{range}$. At 0 the
stage does nothing.

**Knob position → value.** Linear: $v = 0 + p\,(24)\ \text{dB}$, so each tenth of the travel is 2.4 dB.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
