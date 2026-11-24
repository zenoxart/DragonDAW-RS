# Golem Strip (class-A channel strip) › Opto Comp › Comp gain

[Back to Opto Comp](../README.md) · [Golem Strip (class-A channel strip)](../../README.md)

The output level after the effect, to match the loudness before and after (compare with the power button).

Part of **Golem Strip (class-A channel strip)** – Recording channel: a one-fader noise suppressor, Class-A transformer mic preamp with stepped inductor-style EQ into a tube opto compressor (program-dependent opto attack, two-stage release with memory, fixed or manual timing). Zero latency – track through it.

**Range:** 0.00 dB … 30.0 dB (default: 4.00 dB)

## Mathematics

Make-up gain after the compressor, 0 … 30 dB, multiplied with the reduction gain before the tube stage.

**Knob position → value.** Linear: $v = 0 + p\,(30)\ \text{dB}$, so each tenth of the travel is 3 dB.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
