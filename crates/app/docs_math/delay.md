A stereo feedback delay with filters, saturation, diffusion and a pitch shifter inside the loop, and a
four-slot modulation matrix.

**Delay time.** Free: $t$ ms; synced: $t = b\cdot\frac{60}{\text{BPM}}$ s with $b$ beats per note value
(1/32 = 0.125, 1/16 = 0.25, dotted ×1.5, triplet ×2/3, 1 bar = 4). In samples $D = t f_s$. The read position
glides to $D$ by $\hat D \leftarrow \hat D + \kappa(D - \hat D)$ per sample, $\kappa = 0.01$ (digital) or
$0.0002$ (Tape — a slow, pitch-bending glide like a tape machine). Left reads $\hat D\,\tau_m + m$, right
$\hat D(1 + o)\tau_m + m$ with the R offset $o$, modulation $m = 0.003 f_s M \sin(2\pi\cdot0.6\,t)$ and
matrix time factor $\tau_m = 1 + \text{clamp}(0.25\,\text{mod}_{time}, \pm0.9)$.

**Loop.** Per channel, the repeat $y$ (read from the line) passes:

1. pitch shifter (when $\ne 0$ st): two taps rotating through a 50 ms window $W$ with phase increment
   $\frac{1 - r}{W}$, $r = 2^{\text{st}/12}$, cross-faded with $\sin^2(\pi\phi)$ and $\sin^2(\pi(\phi + \tfrac12))$
   (they sum to 1);
2. one-pole high-pass (low cut) → one-pole low-pass (high cut) → RBJ bell at the high-cut frequency,
   $Q = 0.7 + 8\rho$, gain $12\rho$ dB (resonance $\rho$);
3. drive: $\tanh(g\,y)/\sqrt g$ with $g = 10^{d/20}$;
4. diffusion: two Schroeder all-passes per channel (7.3 / 11.1 ms left, 8.9 / 13.7 ms right),
   $w = y + g\,d$, $y' = d - g\,w$ with $g = 0.7\,\text{diffusion}$;
5. feedback and cross feedback: $(y_L', y_R') = (f\,y_L + c\,y_R,\ f\,y_R + c\,y_L)$, $f \le 0.98$;
   Tape adds $\tanh(1.2y)/1.2$; all modes end in a soft loop limiter
   $\ell(x) = x$ for $|x| \le 1$, else $\operatorname{sgn}x\,(1 + 3\tanh\frac{|x| - 1}{3})$, so even runaway
   feedback stays bounded.

Writing: Stereo $L \leftarrow x_L + y_L'$, $R \leftarrow x_R + y_R'$; Ping-pong $L \leftarrow \frac{x_L + x_R}{2} + y_R'$,
$R \leftarrow y_L'$ (the repeats alternate sides); Freeze writes the repeats back unchanged ($f = 1$, no input).

**Output.** Ducking with a 10 ms / 250 ms envelope $e$ of the input:
$g_{duck} = 10^{-\frac{K}{20}\,\text{clamp}\left(\frac{20\log_{10}e + 40}{40}, 0, 1\right)}$ — full ducking of $K$
dB at 0 dBFS input, none below −40 dBFS. Width on the wet signal ($m \pm W s$), then
$y = (1-\mu)x + \mu\,g_{duck}(m \pm Ws)$.

**Modulation matrix.** Sources: LFO 1/2 (sine, triangle, square, sample & hold; free Hz or synced
$\frac{\text{BPM}}{60\,b}$ Hz), envelope $\text{clamp}\left(\frac{20\log_{10}e + 48}{48}, 0, 1\right)$. Each slot adds
$\text{source}\times\text{amount}$ to its target; at 100 %: time ±25 %, feedback ±0.5, filters ±3 octaves
($f\cdot2^{3a}$), drive +24 dB, pitch ±12 st, width ±1, mix ±0.5. Filters and pitch are updated every 32 samples.

<!-- knob: sync -->
Note value $b$ (beats); the time follows tempo changes: $t = 60b/\text{BPM}$.

<!-- knob: time -->
Free delay time in ms (used when Sync = Free).

<!-- knob: feedback -->
$f$: the gain of each repeat (before filters and drive): after $n$ repeats the level is $\approx f^n$, e.g. 0.5
falls 6 dB per repeat.

<!-- knob: mode -->
Stereo (each side its own loop), Ping-pong (mono input, repeats alternate), Tape (slow time glide,
$\tanh(1.2y)/1.2$ in the loop).

<!-- knob: lowcut -->
One-pole high-pass in the loop: every repeat loses more lows ($n$ repeats ≈ $6n$ dB/oct slope).

<!-- knob: highcut -->
One-pole low-pass in the loop (repeats get darker), with the resonance bell at the same frequency.

<!-- knob: mod -->
$M$: 0.6 Hz sine on the read position, depth $0.003 f_s M$ samples (up to 3 ms) — chorus-like wobble.

<!-- knob: offset -->
$o$: the right channel's time factor $(1 + o)$, ±50 %.

<!-- knob: mix -->
$\mu$: dry/wet.

<!-- knob: cross -->
$c$: share of each side's repeat fed into the other side's loop.

<!-- knob: drive -->
$d$ (dB): $\tanh(g y)/\sqrt g$ in the loop, $g = 10^{d/20}$ — the repeats saturate more each time round.

<!-- knob: reso -->
$\rho$: bell at the high-cut frequency, $Q = 0.7 + 8\rho$, $+12\rho$ dB.

<!-- knob: diffusion -->
All-pass coefficient $g = 0.7\,\text{diffusion}$ of the two-stage diffuser: repeats smear into a reverb-like wash.

<!-- knob: duck -->
$K$ (dB): the wet signal is lowered by up to $K$ dB while the input plays (see formula).

<!-- knob: freeze -->
Infinite repeat: feedback 1, no new input, filters bypassed — the loop content holds.

<!-- knob: width -->
$W$: side gain of the wet signal.

<!-- knob: pitch -->
Pitch of each repeat in semitones, $r = 2^{\text{st}/12}$: repeats climb or fall cumulatively.

<!-- knob: lfo1_sync -->
Free (rate in Hz) or a note value: $f = \text{BPM}/(60b)$.

<!-- knob: lfo1_rate -->
Free LFO frequency in Hz.

<!-- knob: lfo1_shape -->
Sine, triangle, square or sample & hold (see [DSP foundations]({DSP})).

<!-- knob: lfo2_sync -->
As LFO 1.

<!-- knob: lfo2_rate -->
As LFO 1.

<!-- knob: lfo2_shape -->
As LFO 1.

<!-- knob: m1_src -->
Off, LFO 1, LFO 2 or the input envelope.

<!-- knob: m1_dst -->
Time, feedback, low cut, high cut, drive, pitch, width or mix (scales above).

<!-- knob: m1_amt -->
Amount $a$ (±100 %): target += source × $a$.
