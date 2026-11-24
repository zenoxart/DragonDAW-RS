# Plugins

VST3 and CLAP plugins: Options › Plugins sets the folders, scanning runs in a separate process so a broken plugin cannot take the app down. Plugin presets (.vstpreset and Dragon presets) are in the preset row of the plugin window; delay compensation is automatic.

## How it works

**Scanning** runs every plugin file in a child process first, so a plugin that crashes while loading is marked bad
instead of taking the DAW down.

**Parameters** of hosted plugins are normalised to $[0,1]$ as the formats define; changes are sent as sample-accurate
parameter events inside the block, and automation is ramped like the stock effects'.

**Latency** a plugin reports joins the plugin delay compensation: every input of a mixer strip is delayed to its
slowest input, $d_{s\to i} = \max_{s'} \ell_{s'} - \ell_s$, where $\ell_s$ is the accumulated latency of strip $s$'s
output; the master's total latency is removed from exports.

[Manual](../README.md)
