# Golem Strip (class-A channel strip) › Gate › Noise suppressor

[Back to Gate](../README.md) · [Golem Strip (class-A channel strip)](../../README.md)

Sets the noise suppressor.

Part of **Golem Strip (class-A channel strip)** – Recording channel: a one-fader noise suppressor, Class-A transformer mic preamp with stepped inductor-style EQ into a tube opto compressor (program-dependent opto attack, two-stage release with memory, fixed or manual timing). Zero latency – track through it.

**Range:** 0.00 % … 100 % (default: 0.00 %)

## Mathematics

One fader $a = v/100$ for a four-band downward expander. Bands: < 250 Hz, 250 Hz – 1.2 kHz, 1.2 – 5 kHz, > 5 kHz,
split complementarily (each band is the remainder of the one before minus a Butterworth low-pass), so they sum back
to the input exactly. Per band $k$, a power envelope gives $L_k = 10\log_{10}(\tfrac12 e_k)$ dB and a noise floor
$F_k$: the minimum of $L_k$ over a sliding 2 s window (8 slots of 250 ms), rising at most 30 dB/s and kept within
−96 … −40 dBFS. With

$$\text{margin} = 3 + 9a,\quad \text{ratio} = 1.5 + 2.5a,\quad \text{depth} = 30a$$

the attenuation is $A_k = \min\big((F_k + 4 + \text{margin} - L_k)(\text{ratio} - 1),\ \text{depth}\big)$ dB while the band
is within that distance of its floor, else 0; the gain opens fast and closes slowly. At 0 % the stage is
bit-transparent.

**Knob position → value.** Linear: $v = 0 + p\,(100)\ \text{\%}$, so each tenth of the travel is 10 %.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
