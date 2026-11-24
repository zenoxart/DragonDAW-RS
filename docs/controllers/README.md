# Controllers

- **MIDI keyboards:** Options › MIDI picks the inputs; notes play the selected channel, sustain / sostenuto / soft pedals go to it.
- **Controller buttons:** Options › Control surface › Controller buttons – click **Learn** next to Record, Play / pause, Stop, Undo, Redo, Metronome or Quantize and press the button on your keyboard (transport and function buttons; any note or CC works). A learned button runs its command and never plays a note. MIDI Start / Stop (sent by many keyboards' Play and Stop buttons) works without learning.
- **Control surfaces:** pick the MCU input (and output for motor faders and LEDs) and the protocol: **MCU** (faders, pan, mute / solo / arm / select, transport, loop, click, undo, banks of 8) or **HUI** (keyboards in their "other DAW" mode: play, stop, record, rewind, forward, loop, return to zero).
- **MIDI learn:** right-click any knob › MIDI learn, then move a controller knob.

## How it works

**Control surfaces (MCU protocol).** Faders arrive as 14-bit pitch bend per strip, $u = (v + 8192)/16383 \in [0,1]$, and
map to gain with unity at 75 % of the travel:

$$G_{dB}(u) = \begin{cases} -60 + 80u & u < 0.75\\ 24\,(u - 0.75) & u \ge 0.75\end{cases}\quad(-60 … +6\ \text{dB},\ u = 0 \to \text{silence})$$

and the inverse drives motor faders ($v = \operatorname{round}(16383u)$, sent as LSB, MSB). V-pots send relative
values: bits 0–5 are the magnitude, bit 6 the sign (set = counter-clockwise). Buttons are notes: 0x00–0x07 arm,
0x08–0x0F solo, 0x10–0x17 mute, 0x18–0x1F select, 0x5B–0x5F rewind / forward / stop / play / record.

**HUI.** A button is two controller messages on channel 1: CC 0x0F selects a *zone*, CC 0x2F carries the *port*
in its low four bits with bit 6 set on press. Zone 0x0E ports 1–5 are rewind, forward, stop, play, record; zone 0x0F
port 0 return-to-zero, port 3 loop.

**MIDI learn** maps a controller linearly, $v = v_{min} + \frac{c}{127}(v_{max} - v_{min})$ (see
[Automation](../automation/README.md)).

[Manual](../README.md)
