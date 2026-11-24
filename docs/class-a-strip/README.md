# Golem Strip (class-A channel strip) (class_a_strip)

*Channel strip*

Recording channel: a one-fader noise suppressor, Class-A transformer mic preamp with stepped inductor-style EQ into a tube opto compressor (program-dependent opto attack, two-stage release with memory, fixed or manual timing). Zero latency – track through it.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Eq](eq/README.md) – EQ in, EQ high-pass, EQ low freq, EQ low gain, EQ mid freq, EQ mid gain, EQ high gain (12 kHz)
- [Gate](gate/README.md) – Noise suppressor
- [Opto Comp](opto-comp/README.md) – Comp in, Comp threshold, Comp ratio, Comp timing, Comp attack, Comp release, Comp gain
- [Output](output/README.md) – Order, Output
- [Preamp](preamp/README.md) – Preamp drive

## How it works

A zero-latency recording channel: noise suppressor → transformer preamp → EQ ⇄ opto compressor (order switchable)
→ line output. Everything runs per sample at the session rate.

**Preamp** (per channel, input $x$ multiplied by the drive gain $D = 10^{d/20}$):

1. *Input transformer.* A one-pole low-pass at 150 Hz, $\ell[n] = x + a(\ell[n-1] - x)$, $a = e^{-2\pi\,150/f_s}$, splits
   the low band; only it saturates (core flux): $\ell' = \tanh(1.6\,\ell)/1.6$, then $x' = \ell' + (x - \ell)$. Bass
   gains third harmonics first, the highs stay clean.
2. *Class-A stage* (single-ended, asymmetric): $y = x' + 0.08x'^2 - 0.01x'^3$, then $y \leftarrow y/(1 + 0.15|y|)$. The
   square term produces the second harmonic, growing smoothly with level.
3. *Coupling capacitor* (8 Hz DC blocker): $z[n] = y[n] - y[n-1] + r\,z[n-1]$, $r = 1 - 2\pi\,8/f_s$.
4. *Output transformer*: a one-pole low-pass at 28 kHz (capped at $0.45f_s$) and a 40 Hz peak, +0.6 dB, $Q$ 0.8 (the
   core's low-frequency resonance).

**Opto compressor.** Detector $X = 20\log_{10}\operatorname{hold}(\max(|L|, |R|))$ (a peak hold steadies low
frequencies), over-threshold $o = X - T$, soft knee $K = 8$ dB:

$$o' = \begin{cases} 0 & o < -K/2\\ \frac{(o + K/2)^2}{2K} & |o| < K/2\\ o & \text{else}\end{cases},\qquad G^* = o'\,(1 - 1/R)\ \text{dB}$$

The opto cell turns $G^*$ into the applied reduction:

* *light* $\lambda$ rises towards $G^*$ with the attack, $\lambda \leftarrow G^* + (\lambda - G^*)e^{-\Delta t/t_a}$, and
  falls to it at once (the cell's light follows the signal; its resistance recovers slowly);
* two release stages chase $\lambda$: $F$ with $t_{r,f}$ and $S$ with $t_{r,s}$ (each jumps up instantly and decays
  exponentially);
* *memory* $\mu$ moves towards $\min(G^*/12, 1)$ with a 2 s time constant while compressing and 6 s while
  recovering;
* applied reduction $G = 0.6F + 0.4S$, gain $10^{-G/20}$ times the make-up.

| timing | $t_a$ | $t_{r,f}$ | $t_{r,s}$ |
|---|---|---|---|
| Fixed | 1 ms | 50 ms | $0.5 + 2.5\mu$ s |
| Fix / Man | 1 ms | 50 ms | release knob |
| Manual | attack knob | release knob | 4 × release knob |

After the gain, a tube make-up stage processes $\tfrac12 x$ and is scaled by 2 (so its curve sits around normal
operating level).

**Output.** A line amplifier with rails near +24 dBFS: $y = 16\tanh\!\big(x\,O/16\big)$, $O = 10^{o/20}$. Below about
0 dBFS this is linear to within 0.1 dB; it only bounds extreme settings.

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
