# Dragontamer (compressor) › Timing › Look-ahead

[Back to Timing](../README.md) · [Dragontamer (compressor)](../../README.md)

Lets the detector see the signal slightly in advance so the effect can react before a peak arrives (adds this much latency, compensated by the mixer).

Part of **Dragontamer (compressor)** – Compressor with eight styles (Clean, Classic, Opto, Vocal, Mastering, Bus, Punch, Pumping), look-ahead, hold, sidechain filters and external sidechain, auto makeup and parallel mix.

**Range:** 0.00 ms … 20.0 ms (default: 0.00 ms)

## Mathematics

The audio is delayed by $D = \lfloor t\,10^{-3} f_s\rfloor$ samples (max 20 ms) while the detector is not, so
the gain is already falling when a transient arrives. $D$ is reported as latency and compensated by the mixer.

**Knob position → value.** Linear: $v = 0 + p\,(20)\ \text{ms}$, so each tenth of the travel is 2 ms.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
