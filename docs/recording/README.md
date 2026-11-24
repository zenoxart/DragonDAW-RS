# Recording

- **Audio:** arm a track, press Record; latency is compensated, every pass becomes a take you can comp.
- **MIDI:** play your keyboard while recording – everything that is switched on keeps playing, so you hear the whole song while you record your part. The notes land in the current pattern (pattern mode) or a new pattern clip (song mode).
- **Count-in** and the metronome are on the transport bar.
- **Recording template** (File › New from template): the mixing buses with all effects muted, a main vocal and a backing vocal channel with the channel strip, both routed to the vocal bus, and a recording channel routed into the main vocal.

## How it works

**Latency compensation.** A recorded sample reaches the file one round trip after the sound it accompanies left the
speakers. The take is moved earlier by

$$L = L_{out} + L_{in} + \frac{F}{f_s} + L_{user}$$

where $L_{out}$ and $L_{in}$ are the latencies the driver reports (one buffer $B/f_s$ each when it reports none), $F$ the
frames waiting in the engine's input FIFO and $L_{user}$ the manual offset from Options (to correct a driver that
reports wrongly; measure it once with a loopback cable).

**Monitoring** passes the input through the track's inserts in the same block it arrives in, so it is heard one
output buffer later. A feedback guard mutes monitoring when a hot input (peak above 0.25) meets an output pinned at
full scale (above 0.97) for a moment – the signature of a microphone-to-speaker loop.

**Takes.** Every pass writes a new file; the previous main-lane clips move into a take lane, and comping copies a
range of a take back onto the main lane.

[Transport](../transport/README.md) · [Manual](../README.md)
