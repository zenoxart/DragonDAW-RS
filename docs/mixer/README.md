# Mixer

- **Strips:** the dB scale on the left, then the meter, then the fader – all three on the same scale, so a fader at −6 dB lines up with −6 on the meter.
- **Inserts:** up to 10 effects per channel, bypass LED, mix knob, drag to reorder; the selected channel's slots are on the right.
- **Routing:** route arrows send a channel to another; right-click routes to that channel only.
- **Right-click a strip:** reset fader, make the channel sticky (stays visible), dock it, **clone it with its effects**.
- Right-click an insert's enable LED to switch every channel or effect in the mixer back on.
- **Tools › Auto-balance** sets the faders by the roles of the channels.

## How it works

**Fader and meter scale.** Fader, meter and dB scale share one piecewise-linear map from level to height
$h \in [0, 1]$, through the points

| dB | +6.02 | 0 | −5 | −10 | −20 | −30 | −40 | −60 |
|---|---|---|---|---|---|---|---|---|
| $h$ | 1 | 0.80 | 0.69 | 0.58 | 0.40 | 0.27 | 0.16 | 0.04 |

linear in dB between neighbouring points and linear in *gain* below −60 dB ($h = 0.04\cdot g/10^{-3}$), so silence
sits at the bottom. The fader's maximum is +6.02 dB (gain 2). The detailed top of the scale gets most of the travel.

**Channel processing order:** inserts → swap / polarity → width → balance and fader → meters → post-fader sends and
output. Pre-fader sends tap after the inserts.

**Pan** on a stereo strip is a balance control (no loss at the centre):
$(g_L, g_R) = (1,\ 1 + p)$ for $p \le 0$, $(1 - p,\ 1)$ for $p > 0$. Mono sources (instruments, one-channel clips) use the
constant-power law $g_L = \sqrt2\cos\theta$, $g_R = \sqrt2\sin\theta$, $\theta = (p + 1)\frac\pi4$ – 0 dB on both sides at
the centre, +3 dB in the side the source is panned to.

**Width** $w \in [0, 2]$ scales the side signal: $m = \tfrac12(L + R)$, $s = \tfrac12(L - R)\,w$, $L' = m + s$, $R' = m - s$
(0 = mono, 1 = unchanged, 2 = twice as wide).

**Smoothing.** Fader, pan, width, send levels and insert mixes are ramped linearly across each engine block
($v_k = v_0 + (v_1 - v_0)\frac{k+1}{n}$, $n \le 128$), so moving them never clicks.

**Meters.** Each block reports the peak of each side; the display keeps the maximum since the last frame and decays it
by a factor 0.86 per drawn frame (≈ −1.3 dB per frame, ≈ −80 dB/s at 60 fps). Insert meters average the power per
block, $\bar p \leftarrow \bar p\,k + p\,(1 - k)$ with $k = e^{-n/(0.3 f_s)}$ (≈ 300 ms), and hold peaks with a
$e^{-n/(1.5 f_s)}$ fall per block.

**Delay compensation.** In processing order, every strip's inputs are aligned to the slowest one: an input from strip
$s$ is delayed by $d = \max_{s'} \ell_{s'} - \ell_s$ (the strip's accumulated latency), and the strip's own latency is
$\ell_i = \max_{s'} \ell_{s'} + \lambda_i$ with $\lambda_i$ the sum of its inserts' latencies. Parallel paths therefore
recombine sample-aligned.

**Auto-balance.** Each source is measured before its fader while it plays: K-weighted loudness over 400 ms windows,
silence, tails and bleed left out, and the loud half of what remains taken as its level. Its role comes from names
(channel, tracks, clips, instrument, samples) or, without a hint, from its sound (low-end and top energy, transience,
how much of the song it plays in). The lead vocal (or the kick) is the reference; every role is set a fixed number
of dB below it, and the sum is trimmed to leave −6 dB of headroom into the master.

[Effects](../effects/README.md) · [Manual](../README.md)
