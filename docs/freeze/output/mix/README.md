# Rime (spectral freeze) › Output › Mix

[Back to Output](../README.md) · [Rime (spectral freeze)](../../README.md)

Blends the processed signal with the dry one: 0 leaves the input untouched, the top of the range is fully processed. The dry path is delay-compensated, so blending never smears or combs.

Part of **Rime (spectral freeze)** – Spectral freeze: Freeze holds the spectrum of the moment and plays it on as an endless drone – blurred, shifted, tilted and drifting – fading in and out over the fade time.

**Range:** 0.00 % … 100 % (default: 100 %)

## Mathematics

How much the drone replaces the input while frozen.

**Knob position → value.** Linear: $v = 0 + p\,(100)\ \text{\%}$, so each tenth of the travel is 10 %.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
