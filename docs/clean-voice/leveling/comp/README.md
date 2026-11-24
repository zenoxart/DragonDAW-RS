# Purify (voice cleanup) › Leveling › Comp

[Back to Leveling](../README.md) · [Purify (voice cleanup)](../../README.md)

Sets the comp.

Part of **Purify (voice cleanup)** – Post-production vocal chain in one plugin (after zenoxart/CleanVoice): valve vari-mu, leveller, vari-mu, de-esser, two 4-band multibands, parallel presence, oversampled saturation and a resonance suppressor.

**Range:** 0.00 % … 100 % (default: 35.0 %)

## Mathematics

Amount $a = v/100$ of the leveller, a peak compressor with ratio 6 and a 6 dB knee:

$$T = -24a\ \text{dB},\qquad M = 20a\ \text{dB}$$

The detector follows 1 ms attack / 130 ms release. After the gain and make-up a brick-wall stage keeps peaks under
$-0.3$ dBFS (its gain follows a 0.05 ms / 40 ms detector, smoothed by $g \leftarrow g^* + \tfrac12(g - g^*)$). At 0 the
threshold sits at 0 dBFS and the make-up is 0 dB: transparent except for peaks above the ceiling.

**Knob position → value.** Linear: $v = 0 + p\,(100)\ \text{\%}$, so each tenth of the travel is 10 %.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
