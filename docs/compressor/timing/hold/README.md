# Dragontamer (compressor) › Timing › Hold

[Back to Timing](../README.md) · [Dragontamer (compressor)](../../README.md)

How long the effect stays engaged after the signal drops before it starts to release.

Part of **Dragontamer (compressor)** – Compressor with eight styles (Clean, Classic, Opto, Vocal, Mastering, Bus, Punch, Pumping), look-ahead, hold, sidechain filters and external sidechain, auto makeup and parallel mix.

**Range:** 0.00 ms … 500 ms (default: 0.00 ms)

## Mathematics

After the reduction stops rising it is held for $h = t\,10^{-3} f_s$ samples before the release starts,
which avoids release ripple between closely spaced hits.

**Knob position → value.** Linear: $v = 0 + p\,(500)\ \text{ms}$, so each tenth of the travel is 50 ms.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
