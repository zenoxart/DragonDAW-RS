# Echo Cavern (delay) › Colour › Pitch of the repeats

[Back to Colour](../README.md) · [Echo Cavern (delay)](../../README.md)

Sets the pitch of the repeats.

Part of **Echo Cavern (delay)** – Delay: tempo-synced or free, stereo / ping-pong / tape, cross feedback, resonant filters, drive and diffusion in the loop, pitch-shifted repeats, ducking, freeze, width, and a modulation matrix (two LFOs and an envelope follower routed through four slots to time, feedback, filters, drive, pitch, width or mix).

**Range:** -12.0 st … 12.0 st (default: 0.00 st)

## Mathematics

Pitch of each repeat in semitones, $r = 2^{\text{st}/12}$: repeats climb or fall cumulatively.

**Knob position → value.** Linear: $v = -12 + p\,(24)\ \text{st}$, so each tenth of the travel is 2.4 st.

**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \leftarrow v + c\,(v_t - v)$, $c = 1 - e^{-32/(0.025 f_s)}$ — a 25 ms time constant. See [DSP foundations](../../../dsp/README.md).

Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.

Drag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.
