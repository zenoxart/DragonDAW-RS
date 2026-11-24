# Dragontamer (compressor) › Compression › Threshold

[Back to Compression](../README.md) · [Dragontamer (compressor)](../../README.md)

The level above which the effect starts working. Lower it to act on more of the signal.

Part of **Dragontamer (compressor)** – Compressor with eight styles (Clean, Classic, Opto, Vocal, Mastering, Bus, Punch, Pumping), look-ahead, hold, sidechain filters and external sidechain, auto makeup and parallel mix.

**Range:** -60.0 dB … 0.00 dB (default: -18.0 dB)

## Mathematics

$T$ in the static curve: the level (dBFS of the detector) where compression begins. Every dB the detector
reads above $T$ (beyond the knee) comes out as $1/R$ dB.

**Knob position → value.** Linear: $v = -60 + p\,(60)\ \text{dB}$, so each tenth of the travel is 6 dB.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
