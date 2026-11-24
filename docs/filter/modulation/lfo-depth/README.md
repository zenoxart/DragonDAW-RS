# Alembic (filter) › Modulation › LFO depth

[Back to Modulation](../README.md) · [Alembic (filter)](../../README.md)

How strong the effect is overall.

Part of **Alembic (filter)** – Filter: two filters (serial or parallel), low / high / band pass, notch, peak and shelves, 12 / 24 / 48 dB slopes, styles (clean, smooth, ram, hack), drive, stereo spread, LFO and envelope follower.

**Range:** 0.00 oct … 4.00 oct (default: 0.00 oct)

## Mathematics

$\lambda$ (octaves): $f \times 2^{\lambda\sin}$.

**Knob position → value.** Linear: $v = 0 + p\,(4)\ \text{oct}$, so each tenth of the travel is 0.4 oct.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
