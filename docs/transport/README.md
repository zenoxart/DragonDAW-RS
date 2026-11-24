# Transport

| Control | What it does |
|---|---|
| Play (Space) | Plays the song or the pattern (the mode switch next to it). |
| Stop | Stops at the current position; **double-click** returns to the start. |
| Record | Records armed tracks (audio) or the played MIDI into the current pattern. |
| Count-in | Bars counted before recording starts. |
| Metronome | The click, also during recording. |
| Loop / Punch | Loop and punch regions on the ruler; dragging them always snaps to whole bars (Alt: to beats). |
| Tempo, signature | Drag or type; the playlist and the effects follow. |
| Arrow keys | With nothing selected, move the playhead; held, it moves faster and faster. |
| Markers | **Add › Marker** asks for a name and snaps to the bar (or beat). |
| Tab | Opens a small window with one big button for the Tab action, so it is easy to hit while playing. |

Keyboard controllers: see [Controllers](../controllers/README.md).

## How it works

**Time.** Positions are stored in beats; notes in ticks of $1/96$ beat (96 PPQ). The tempo map turns beats into time
piecewise: with tempo points $(b_i, \text{bpm}_i)$ and the seconds $s_i$ at each,

$$s(b) = s_i + (b - b_i)\frac{60}{\text{bpm}_i},\qquad b(s) = b_i + (s - s_i)\frac{\text{bpm}_i}{60},\qquad \text{frame} = s\,f_s$$

where $i$ is the last point at or before the position (tempo steps at each point; $s_{i+1}$ is accumulated from the
previous segment).

**Metronome.** On every beat a 30 ms sine burst: 1500 Hz on the first beat of a bar, 1000 Hz otherwise, with the
envelope $e(k) = \left(1 - k/(0.03 f_s)\right)^2$ (a squared linear decay: a soft, click-free tail) and level 0.9 / 0.6
times the metronome volume.

**Count-in.** Recording starts $\text{bars}\times\text{beats per bar}$ beats before the record position; the metronome
counts them in, nothing is written until the record position is reached.

**Loop and launch quantize.** A queued pattern or loop jump waits for the next grid line,
$b' = \lceil b/q - 10^{-9}\rceil q$ (the tiny epsilon keeps a launch exactly on a line from waiting a whole step).

[Manual](../README.md)
