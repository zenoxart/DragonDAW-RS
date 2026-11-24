# Channel rack

- One row per channel (sampler, synth, drum pads, plugins): mute LED, pan, volume, mixer target and the step buttons.
- **Steps:** click to set, right-click to clear; the header's **+16 / −16** buttons add or remove a bar of 16 steps, the swing slider shuffles the off-beats.
- **Patterns:** the Pattern menu clones, makes unique, splits by channel and renames patterns.
- **Drum pads:** a pad channel's menu has **Split pads into channels**, which makes one channel per programmed sound with its notes.
- Right-click a mute LED to switch every channel of the rack back on.
- Drop a sample from the browser on a channel to replace its sound; Enter in the browser adds the selected sound as a new channel.

## How it works

**Steps.** A step is a sixteenth note, $\Delta = \tfrac14$ beat (24 ticks). Step $i$ of a pattern starting at beat $b_0$
plays at

$$b_i = b_0 + i\Delta + \sigma_i + \delta_i\Delta,\qquad \sigma_i = \begin{cases} \tfrac12 s\,\Delta & i\ \text{odd}\\ 0 & i\ \text{even}\end{cases}$$

with swing $s \in [0, 1]$ and the step's own shift $\delta_i \in [-\tfrac12, \tfrac12]$. At full swing the off-beat
sixteenths move half a step late (a 3:1 shuffle). Notes in the piano roll that sit exactly on an odd sixteenth get the
same swing.

**Velocity and pan** per step are linear: velocity $v \in [0,1]$ scales the instrument's level (and its brightness
where the instrument models it), pan uses the equal-power law of the mixer.

**Drum pads** pick the velocity layer whose range $[v_{lo}, v_{hi}]$ contains the hit; a choke group stops every other
pad of the group when a pad is triggered.

[Manual](../README.md)
