# Shapeshifter (pitch & formant) › Voice › Pitch

[Back to Voice](../README.md) · [Shapeshifter (pitch & formant)](../../README.md)

Sets the pitch.

Part of **Shapeshifter (pitch & formant)** – Real-time voice pitch and formant shifter: pitch ±12 semitones, formant, link, Transpose / Quantize / Robot modes, tube drive, mix and output.

**Range:** -12.0 st … 12.0 st (default: 0.00 st)

## Mathematics

Pitch $p$ in semitones, ratio $\rho = 2^{p/12}$ (±12 st: ½ … 2). *Transpose*: the output period is $P/\rho$. *Quantize*:
rounded to whole semitones and added to the snapped note. *Robot*: chooses the one note the voice is held on, MIDI
$72 + \lfloor p\rceil$.

**Knob position → value.** Linear: $v = -12 + p\,(24)\ \text{st}$, so each tenth of the travel is 2.4 st.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
