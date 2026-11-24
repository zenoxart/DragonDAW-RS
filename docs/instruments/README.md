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

## Hearthfire, Basilica, Wyrmbass, Chorale, Sandstorm

Five more instruments, under **Add › Instrument** (or the command palette, *Add FM synth* … *Add granular instrument*).
Each has the themed faceplate of the stock units, a live display and a factory preset menu.

| Instrument | What it is |
|---|---|
| Hearthfire (FM synth) | Four operators, eight algorithms, feedback on operator 4, an ADSR per operator; velocity brightens. Electric pianos, bells, basses, brass. |
| Basilica (tonewheel organ) | Nine drawbars, percussion (2nd / 3rd, soft / fast), scanner vibrato and chorus V1–C3, key click, drive and a rotary speaker (off / slow / fast). |
| Wyrmbass (808 bass) | A sine with a pitch drop, long decay or hold, glide between overlapping notes, drive, tone, click and a sub octave. One note at a time. |
| Chorale (strings, choir, pads) | Strings, two choirs (aah / ooh), warm and glass pads and brass from up to seven detuned unison voices with formant filters, vibrato and breath or bow noise. |
| Sandstorm (granular) | Clouds of grains from a sample (**Load sample…**) or its built-in texture, played across the keyboard: position, scan, spray, grain size, density, pitch randomness, reverse, stereo spread. |

### How they work

**Hearthfire.** Each operator $k$ is a sine oscillator at $f_k = f_0 r_k 2^{d_k/1200}$ (ratio $r_k$, detune $d_k$ cents)
whose phase is offset by the summed outputs of its modulators: $y_k = L_k e_k \sin(2\pi\phi_k + 2\pi\sum_{m} y_m)$.
Operator 4 also feeds back the average of its last two outputs scaled by *Feedback*. The algorithm decides which
operators modulate which and which are carriers (heard); the carriers' sum is scaled by $0.5/\sqrt{N_c}$. Carriers
follow $\sqrt{v}$ of the velocity, modulators $1 - s + s\,v$ for velocity sensitivity $s$ – a harder note is brighter.

**Basilica.** The drawbars mix sines at $0.5, 1.5, 1, 2, 3, 4, 5, 6, 8 \times f_0$ (16′ … 1′), normalised by the sum of
the drawbar settings. Keys open and close with 5 ms ramps; percussion adds a decaying sine at the 2nd or 3rd harmonic,
and the key click a short burst of noise. The scanner vibrato is a delay swept at 6.9 Hz (up to 1.2 ms deep); the
chorus settings mix it with the dry signal. Drive is $\tanh(kx)/\sqrt{k}$ with $k = 1 + 4D$. The rotary speaker
splits at 800 Hz: horn and drum spin towards 0.8 / 0.7 Hz (slow) or 6.7 / 5.8 Hz (fast) with inertias of 0.6 s and
2.5 s, and their rotation drives amplitude and Doppler delay in each ear.

**Wyrmbass.** Frequency $f = 440\cdot 2^{(n + t + P e_p - 69)/12}$ with the pitch envelope $e_p$ falling from 1 with the
*Pitch time*; a new note while one is held glides there with a one-pole of the glide time,
$n \leftarrow n + (n^* - n)(1 - e^{-1/(t_g f_s)})$, without restarting. The click is a short burst at six times the
fundamental, the sub octave a sine at $f/2$; drive is $\tanh(kx)/k^{0.35}$ with $k = 1 + 9D$, then the tone low-pass.
The level decays by 60 dB over *Decay*, or stays while the key is held with *Hold* on.

**Chorale.** Each note is $U$ unison sawtooth-rich voices spread evenly over ±*Detune* cents and across the stereo
width, with a vibrato LFO. A low-pass tracks the key ($f_c \propto (f/261.6)^{0.3}$, opened by *Brightness*); the choir
modes pass the voices through three band-pass formants of the vowel *a* (730, 1090, 2440 Hz) or *u* (300, 870,
2240 Hz). *Breath / bow noise* adds filtered noise that follows the envelope.

**Sandstorm.** Each held note starts grains at the *Density* rate (±25 % jitter). A grain reads the sample at
position $p + \text{spray}\cdot u$ (random $u$) at speed $2^{(n-60)/12 + \rho u'/12}$ (*Pitch random* $\rho$), backwards
with the *Reverse* probability, under a Hann window of the grain size, panned at random within the *Spread*. The
read position advances at *Scan* % of the sample per second. The sum is scaled by $1/\sqrt{\text{density}\cdot\text{size}}$
so overlapping grains keep a steady level.

## MIDI effects

Every channel has a chain of MIDI effects in its instrument window (**MIDI effects › Add**): notes pass through
them, top to bottom, before the instrument. Each can be bypassed, moved or removed, and is saved with the channel.

| Effect | What it does |
|---|---|
| Arpeggiator | Plays the held notes one after another in time with the song: up, down, up-down, down-up, as played, random, or as repeated chords; rate 1/4 … 1/64 (with triplets), 1 – 4 octaves, gate, swing, fixed or played velocity, latch. |
| Chord | Turns every note into a chord (major, minor, 7, maj7, min7, sus2, sus4, power, octave, add9, dim, aug), with inversions, open voicing, strum and a softer velocity for the upper notes. |
| Note repeat | Retriggers held notes at a note value, each repeat quieter (velocity decay) and optionally higher or lower (pitch step), up to a number of repeats. |
| Scale | Transposes, then moves every note onto a scale (11 scales, any root): the nearest scale note at or below. |

### How it works

The chain sits in front of the instrument and is itself an instrument to the engine. A played note goes through
the effects at once; notes the effects make themselves (arpeggio steps, strummed chord notes, repeats) are
scheduled at exact sample times, and the instrument is rendered in pieces split at those times – so an arpeggio is
sample-accurate whatever the buffer size.

**Time.** While the song plays, steps lock to the song's grid: the first step falls on the first grid line at or
after the current position, $t = t_0 + (\lceil b/\lambda\rceil\lambda - b)\cdot\frac{60 f_s}{\text{bpm}}$ for position $b$ and
step $\lambda$ beats; stopped, they start at once and run at the song's tempo.

**Arpeggiator.** The held notes (in pitch order, or in the order played) are repeated over the octaves,
$n_{k,o} = n_k + 12o$; *up-down* walks the sequence with period $2N - 2$ (the ends not repeated). Each step plays for
$\gamma\lambda$ (gate $\gamma$). Swing delays every second step by $s\lambda/2$ and brings the next one forward by the same,
so pairs keep their total length.

**Chord.** Intervals from the root; inversion $i$ moves the lowest note up an octave $i$ times; open voicing drops
the second note from the bottom by an octave; strum starts note $k$ after $k\cdot\tau$ ms.

**Note repeat.** Repeat $k$ of a held note plays at $n + k\,\Delta$ semitones with velocity $v(1 - d)^k$, every
$\lambda$ beats, each for $\gamma\lambda$.

**Scale.** $n' = \max\{m \le n + t : (m - r) \bmod 12 \in S\}$ for transpose $t$, root $r$ and scale steps $S$; each
note's mapping is remembered so its note-off matches.

All buffers are fixed in size and the chain is configured off the audio thread, so nothing is allocated while
playing (checked by the real-time test with an arpeggiator, a chord, a scale and a note repeat in it).

[Channel rack](../channel-rack/README.md) · [Manual](../README.md)
