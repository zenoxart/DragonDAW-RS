# Echo Cavern (delay) (delay)

*Delay*

Delay: tempo-synced or free, stereo / ping-pong / tape, cross feedback, resonant filters, drive and diffusion in the loop, pitch-shifted repeats, ducking, freeze, width, and a modulation matrix (two LFOs and an envelope follower routed through four slots to time, feedback, filters, drive, pitch, width or mix).

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Colour](colour/README.md) – Low cut, High cut, Drive (in the loop), Filter resonance, Pitch of the repeats
- [Controls](controls/README.md) – Slot 2 source, Slot 2 target, Slot 2 amount, Slot 3 source, Slot 3 target, Slot 3 amount, Slot 4 source, Slot 4 target, Slot 4 amount
- [Feedback](feedback/README.md) – Feedback, Cross feedback, Diffusion, Freeze
- [Lfo](lfo/README.md) – LFO 1 sync, LFO 1 rate, LFO 1 shape, LFO 2 sync, LFO 2 rate, LFO 2 shape
- [Mix](mix/README.md) – Mix
- [Modulation Slot](modulation-slot/README.md) – Slot 1 source, Slot 1 target, Slot 1 amount
- [Motion](motion/README.md) – Modulation, Ducking, Width
- [Time](time/README.md) – Sync, Time (free), Mode, R offset

## How it works

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

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
