# Templates

File › New from template:

- **Mixing:** buses only (drums, bass, music, vocals, FX) with their effect chains, routed into the master.
- **Beats:** the beat buses.
- **Recording:** the mixing buses with every effect muted, main and backing vocal channels with the channel strip into the vocal bus, and a recording channel into the main vocal.

## How it works

A template is a project file with buses, routing and effect chains but no audio. Starting from one copies it into a
new project (new ids, so two projects from one template never share state) and keeps its mixer routing and
plugin-delay compensation exactly as in a hand-built project: every bus input is delayed to its slowest input (see
[Mixer](../mixer/README.md)).

[Recording](../recording/README.md) · [Manual](../README.md)
