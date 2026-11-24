# Lodestone (limiter) › Limit › Release

[Back to Limit](../README.md) · [Lodestone (limiter)](../../README.md)

How quickly the effect lets go after the signal falls. Short times pump, long ones sound smoother.

Part of **Lodestone (limiter)** – Look-ahead brickwall limiter with true-peak detection, eight styles (Allround, Transparent, Punchy, Dynamic, Aggressive, Modern, Bus, Safe) and 2× / 4× oversampling for mastering.

**Range:** 1.00 ms … 1000 ms (default: 80.0 ms, logarithmic scale)

## Mathematics

$t_r$ (ms), scaled by the style's $k_r$: $r = e^{-1/(t_r k_r 10^{-3} f_s')}$ for the fast path, 6× longer for
the slow path.

**Knob position → value.** Logarithmic: $v = 1\cdot\left(\frac{1000}{1}\right)^{p}\ \text{ms}$, $p = \frac{\ln(v/1)}{\ln(1000)}$ — every factor of 1.995 takes a tenth of the travel; the centre ($p = 0.5$) is 31.6228 ms.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant, in the log domain (even per ratio). See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
