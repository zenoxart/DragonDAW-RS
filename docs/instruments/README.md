# Instruments

Sampler, synth, the **Transmuter** (wavetable synth), piano, guitar and drum pads. Open one by clicking its channel in the rack. Instrument windows keep a sensible size range for their layout.

## Presets

Every category of the Transmuter, the piano and the guitar cycles through clearly different characters before it varies within one, so neighbouring presets sound different:

- **Piano** (acoustic categories): Bright & Close, Dark & Warm, Big Hall, Dry Studio, Staccato, Detuned, Shimmer, Una Corda, Old Radio, Swell, Pumping Pop, Ambient Wash – each also in a brighter, darker and wider shade. **Electric pianos:** Mellow, Barking, Stereo Tremolo, Phased Funk, Dreamy, Tape, Glassy, Dark Soul. **Toy & Celesta:** Music Box, Celesta Hall, Broken Toy, Felted Celesta, Glass Swell, 8-Bit Toy, Vibraphone, Glockenspiel.
- **Guitar** (acoustic, nylon, 12-string, banjo, mandolin, ukulele, world, harp): Big Body, Parlor, Piezo Stage, Old Strings, New Strings, Room Mic, Cassette, Ambient, Doubled, Intimate – combined with the play styles (fingerstyle, strum, palm mute …). **Electric:** Surf, Twang, Jazz Neck, 80s Chorus, Fuzz, High Gain, Blues Crunch, Tremolo, Phaser, Ambient Wash.
- **Transmuter bells:** Church Bell, Glockenspiel, Music Box, Crystal, Singing Bowl, Gamelan, Soft Celesta, Vibraphone, Wind Chimes, Dark Bell, 8-Bit Bell, Reverse Bell.

All presets are level-matched.

## How it works

**Envelopes** (every instrument): DAHDSR with bendable segments,
$b(x) = \frac{e^{kx} - 1}{e^{k} - 1}$, $k = 5\tau$, $\tau$ the segment's tension (see the [DSP notes](../dsp/README.md)).

**Synth.** Three oscillators; saw and square are band-limited with PolyBLEP – near each discontinuity the naive
waveform is corrected by the two-sample polynomial residual
$r(t) = \begin{cases} 2t - t^2 - 1 & 0 \le t < 1\\ t^2 + 2t + 1 & -1 < t < 0\end{cases}$ ($t$ in samples of the phase
increment from the edge), which removes most of the aliasing at a fraction of the cost of tables. Unison voices are
spread in detune and pan. The filter is a TPT state-variable filter ($g = \tan\pi f_c/f_s$).

**Transmuter (wavetable).** A table is a set of single-cycle frames; position selects and crossfades two neighbouring
frames. Each table is stored band-limited once per octave (partials above Nyquist removed), and a note reads the copy
for its octave, so high notes do not alias. Modulation (four envelopes, four LFOs, the matrix) is evaluated every 32
samples per voice; the values that move the sound glide between those points.

**Drakestring (plucked strings).** Each string is an extended Karplus–Strong loop of one period,
$N = f_s/f_0$ samples: a delay line read with 4-point Lagrange interpolation (fractional delay, so exactly in tune), a
one-pole loss filter $H(z) = \frac{1 - a}{1 - a z^{-1}}$ (brightness) and a loop gain from the ring time,
$g = 10^{-3 N / (T_{60} f_s)}/|H|$ per round trip (−60 dB after $T_{60}$, the loss filter's own damping divided out);
$T_{60}$ shortens with pitch, $\propto (82/f_0)^{0.55}$, as on a real string. A pluck fills the loop with noise low-passed by the
pick's hardness and comb-filtered by the picking position $\beta$ (notches at multiples of $f_0/\beta$). The strings
then sound through a resonant body (acoustic models) or a pickup and amp (electric).

**Emberkeys (piano).** Each key has one to three strings, each a waveguide: delay line, loss filter and a chain of
all-pass filters that make the upper partials sharp (stiffness inharmonicity, $f_n \approx n f_0\sqrt{1 + Bn^2}$). The
strings of a key are detuned by a hair (beating, two-stage decay), the scale is stretch-tuned (bass flat, treble
sharp). The hammer is a felt pulse, shorter and brighter the harder the key is struck, fed in at an eighth of the
string's length.

**Sampler.** Playback rate $r = 2^{(n - n_0)/12 + c/1200}$ for note $n$, root $n_0$ and fine tune $c$ cents; slide notes
move $r$ towards the new note's with a one-pole of the glide time, $r \leftarrow r + (r^* - r)(1 - e^{-1/(t_g f_s)})$.

[Channel rack](../channel-rack/README.md) · [Manual](../README.md)
