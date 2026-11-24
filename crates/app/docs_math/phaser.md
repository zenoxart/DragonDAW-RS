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

<!-- knob: rate -->
LFO frequency (triangle).

<!-- knob: depth -->
$\delta$: sweep range $\pm 2\delta$ octaves around the centre.

<!-- knob: stages -->
$S \in \{2, 4, 6, 8, 12\}$ all-pass sections: more stages, more notches.

<!-- knob: feedback -->
$g$: output fed back to the input of the cascade (sharper, more resonant notches).

<!-- knob: center -->
$f_c$: the centre of the sweep.

<!-- knob: mix -->
$\mu$: $y_{out} = x(1 - \mu/2) + \mu y/2$ — at $\mu = 1$ dry and wet are equal, giving the deepest notches.
