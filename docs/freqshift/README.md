# Mercury Wheel (frequency shifter) (freqshift)

*Modulation*

Frequency shifter (every partial moves by the same number of Hz, up, down or both ways) and ring modulator, with an LFO on the shift and a feedback delay for spiralling barber-pole sweeps.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Motion](motion/README.md) – LFO rate, LFO depth, Feedback, Delay
- [Output](output/README.md) – Mix, Output
- [Shift](shift/README.md) – Mode, Shift, Fine

## How it works

A frequency shifter moves every partial by the same number of hertz, $f \to f + \Delta$, unlike a pitch shifter
(which multiplies). Harmonic sounds become inharmonic: metallic, bell-like, or – for small $\Delta$ – a slow
phasing.

**Analytic signal.** Two chains of four second-order all-passes, $y[n] = a^2(x[n] + y[n-2]) - x[n-2]$, with
coefficients (O. Niemitalo)
$a = (0.6924, 0.9361, 0.9882, 0.9987)$ and $(0.4022, 0.8562, 0.9723, 0.9953)$, the first path delayed by one sample,
produce two outputs $I$ and $Q$ 90° apart from ~20 Hz to ~0.45 $f_s$: the analytic signal $I + iQ$ has only positive
frequencies.

**Shift.** Multiplying by $e^{i\omega t}$ and taking the real part:

$$y_\uparrow = I\cos\omega t + Q\sin\omega t,\qquad y_\downarrow = I\cos\omega t - Q\sin\omega t,\qquad \omega = 2\pi\Delta/f_s$$

– single sideband: only $f + \Delta$ (or $f - \Delta$) comes out. *Ring mod* multiplies the input by
$\sin\omega t$ instead, giving both $f \pm \Delta$.

**Motion.** $\Delta(t) = \Delta_c + \Delta_f + D\sin(2\pi f_{lfo} t)$. Feedback adds the delayed output back to the
input, $x' = x + g\,y[n - d]$: each round shifts again, the classic endless barber-pole sweep.

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
