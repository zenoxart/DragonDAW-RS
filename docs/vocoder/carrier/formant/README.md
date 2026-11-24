# Mandrake (vocoder) › Carrier › Formant

[Back to Carrier](../README.md) · [Mandrake (vocoder)](../../README.md)

Sets the formant.

Part of **Mandrake (vocoder)** – Channel vocoder with 8 – 32 bands. The voice shapes a carrier: the input itself (with the voice on the sidechain) or a built-in saw, pulse, noise or chord that follows the voice's pitch.

**Range:** -12.0 st … 12.0 st (default: 0.00 st)

## Mathematics

Moves the carrier bands against the voice bands by $2^{\varphi/12}$: positive values give a smaller, brighter
"mouth", negative a larger, darker one.

**Knob position → value.** Linear: $v = -12 + p\,(24)\ \text{st}$, so each tenth of the travel is 2.4 st.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
