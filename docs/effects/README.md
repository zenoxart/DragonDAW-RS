# Effects

Every stock effect is a unit with its own faceplate:

- **Preset bar** on top: previous / next, the preset menu with 200 factory presets per effect in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets, and **Save**.
- **Knob menu** (right-click any control): **Set value…** (type an exact value, Enter sets it, undoable), reset, copy / paste values between knobs, MIDI learn, automation lane, and **Help**, which opens the knob's page in this manual.
- **A / B** compares two settings, the power button bypasses, the Mix knob blends dry and wet (delay-compensated).
- Windows fit their content and grow only as far as the panel can use.

## How it works

Every effect page has a **How it works** section with its signal flow and equations, and every knob page a
**Mathematics** section: what the control does in the algorithm, the exact mapping from the knob's position
$p \in [0,1]$ to its value, and how changes are smoothed. Shared building blocks (decibel conversions, envelope
followers, the biquad and state-variable filter equations, oversampling, pan laws) are derived once in the
[DSP notes](../dsp/README.md).

**Knob mapping.** Linear knobs: $v = v_{min} + p\,(v_{max} - v_{min})$. Logarithmic knobs (frequencies, times):
$v = v_{min}(v_{max}/v_{min})^{p}$, so every octave (or decade) takes the same travel. Dragging moves $p$ by the mouse
distance over 180 … 320 px (fine with Shift); **Set value…** inverts the mapping of the typed value.

**Insert mix.** An insert's Mix knob blends $y = (1 - m)\,x_D + m\,F(x)$ where $x_D$ is the dry signal delayed by the
effect's reported latency, so the blend never combs. A silent insert whose output has decayed to silence is skipped
until sound returns.

The pages of every effect, section and knob are listed in the [manual's index](../README.md#stock-effects).
