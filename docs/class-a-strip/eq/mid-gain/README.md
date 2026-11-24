# Golem Strip (class-A channel strip) › Eq › EQ mid gain

[Back to Eq](../README.md) · [Golem Strip (class-A channel strip)](../../README.md)

The output level after the effect, to match the loudness before and after (compare with the power button).

Part of **Golem Strip (class-A channel strip)** – Recording channel: a one-fader noise suppressor, Class-A transformer mic preamp with stepped inductor-style EQ into a tube opto compressor (program-dependent opto attack, two-stage release with memory, fixed or manual timing). Zero latency – track through it.

**Range:** -18.0 dB … 18.0 dB (default: 0.00 dB)

## Mathematics

Gain $g$ of the mid band, −18 … +18 dB. Its $Q$ tightens with the gain as inductor bands do:
$Q = 1 + 0.6\,|g|/18$ (1 at 0 dB, 1.6 at ±18 dB).

**Knob position → value.** Linear: $v = -18 + p\,(36)\ \text{dB}$, so each tenth of the travel is 3.6 dB.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
