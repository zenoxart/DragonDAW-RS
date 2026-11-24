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

<!-- knob: ns -->
One fader $a = v/100$ for a four-band downward expander. Bands: < 250 Hz, 250 Hz – 1.2 kHz, 1.2 – 5 kHz, > 5 kHz,
split complementarily (each band is the remainder of the one before minus a Butterworth low-pass), so they sum back
to the input exactly. Per band $k$, a power envelope gives $L_k = 10\log_{10}(\tfrac12 e_k)$ dB and a noise floor
$F_k$: the minimum of $L_k$ over a sliding 2 s window (8 slots of 250 ms), rising at most 30 dB/s and kept within
−96 … −40 dBFS. With

$$\text{margin} = 3 + 9a,\quad \text{ratio} = 1.5 + 2.5a,\quad \text{depth} = 30a$$

the attenuation is $A_k = \min\big((F_k + 4 + \text{margin} - L_k)(\text{ratio} - 1),\ \text{depth}\big)$ dB while the band
is within that distance of its floor, else 0; the gain opens fast and closes slowly. At 0 % the stage is
bit-transparent.

<!-- knob: drive -->
Preamp gain $d$ in dB, $D = 10^{d/20}$, applied before the transformer and class-A curves. More drive pushes the signal
further into the curves: the second harmonic grows roughly with level ($0.08x^2$), the low band's third with the
input transformer's $\tanh$. Bring the level back with **Output**.

<!-- knob: eq_on -->
Bypasses the whole EQ section (high-pass and the three bands) when off.

<!-- knob: hpf -->
High-pass at 50, 80, 160 or 300 Hz, 18 dB/octave: a third-order Butterworth made of an RBJ 2nd-order high-pass with
$Q = 1$ and a first-order high-pass at the same frequency (together the Butterworth poles of order 3).

<!-- knob: low_freq -->
Corner of the low shelf (35, 60, 110 or 220 Hz). The shelf is an RBJ low shelf with $Q = 0.55$: the low $Q$ gives the
broad bump before the shelf that inductor EQs have.

<!-- knob: low_gain -->
Gain of the low shelf, −16 … +16 dB (RBJ low shelf, $A = 10^{g/40}$, $Q$ 0.55).

<!-- knob: mid_freq -->
Centre of the mid band (360 Hz, 700 Hz, 1.6, 3.2, 4.8 or 7.2 kHz), an RBJ peaking filter.

<!-- knob: mid_gain -->
Gain $g$ of the mid band, −18 … +18 dB. Its $Q$ tightens with the gain as inductor bands do:
$Q = 1 + 0.6\,|g|/18$ (1 at 0 dB, 1.6 at ±18 dB).

<!-- knob: high_gain -->
Gain of the high shelf at 12 kHz (capped at $0.45f_s$), −16 … +16 dB, RBJ high shelf with $Q$ 0.6.

<!-- knob: comp_on -->
Bypasses the opto compressor and its tube make-up stage when off.

<!-- knob: threshold -->
Threshold $T$ in dB of the soft-knee curve: the over-threshold is $o = X - T$, with the 8 dB knee centred on it.

<!-- knob: ratio -->
Ratio $R$: above the knee every dB over the threshold is reduced by $1 - 1/R$ dB (2:1 → 0.5 dB, 10:1 → 0.9 dB).

<!-- knob: comp_mode -->
The opto cell's timing (see the table above). *Fixed* uses the cell's own program-dependent behaviour: the slow
release lengthens from 0.5 s up to 3 s with the memory $\mu$, which builds while it compresses hard. *Fix / Man* keeps
the fixed attack and fast release, and takes the slow release from the **Release** knob. *Manual* takes both from the
knobs.

<!-- knob: attack -->
Attack $t_a$ of the opto light (Manual timing only): $\lambda \leftarrow G^* + (\lambda - G^*)e^{-\Delta t/t_a}$ while the
reduction rises.

<!-- knob: release -->
Release in ms. *Fix / Man*: the slow release stage $t_{r,s}$. *Manual*: the fast stage $t_{r,f}$, with the slow one at
4×. The applied reduction is $0.6F + 0.4S$, so the first 60 % of a recovery follows the fast stage.

<!-- knob: comp_gain -->
Make-up gain after the compressor, 0 … 30 dB, multiplied with the reduction gain before the tube stage.

<!-- knob: order -->
*Pre > EQ > Comp*: the compressor reacts to the equalised signal (an EQ boost is compressed). *Pre > Comp > EQ*: it
reacts to the raw preamp output and the EQ shapes the compressed sound.

<!-- knob: output -->
Output level $o$ in dB into the line stage $y = 16\tanh(x\,10^{o/20}/16)$.
