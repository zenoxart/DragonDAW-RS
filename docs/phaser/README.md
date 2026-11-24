# Moonphase (phaser) (phaser)

*Modulation*

2–12 stage all-pass phaser with feedback.

The preset bar on top of the unit holds 200 factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.

## Sections

- [Mix](mix/README.md) – Mix
- [Phaser](phaser/README.md) – Rate, Depth, Stages, Feedback, Center

## How it works

A cascade of 2–12 second-order all-pass filters whose frequencies sweep, with feedback.

**All-pass stages.** RBJ all-pass, $Q = 0.6$, stage $s$ at $f_s' = f\,(1 + 0.15 s)$ (slightly spread
stages give irregular notch spacing), with the swept centre

$$f = f_c\cdot 2^{2\,\delta\,m[n]}$$

where $m$ is a triangle LFO (left and right 90° apart) — the sweep spans $\pm 2\delta$ octaves around
$f_c$. Coefficients are updated every 16 samples.

**Loop.** $u = x + g\,y_{prev}$, $y = A_S(\dots A_1(u))$, $\text{out} = x(1 - \tfrac\mu2) + \tfrac\mu2\,y$.
Each all-pass section adds up to 360° of phase; where the cascade's phase reaches $180° (2k+1)$ the dry and
wet cancel: $S$ stages ($S/2$ biquads… here each stage is a 2nd-order section) give notches whose number
grows with the stage count; feedback $g$ sharpens them into peaks between.

The formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).

[All effects](../effects/README.md) · [Manual](../README.md)
