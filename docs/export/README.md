# Export

File › Export renders the song or the loop as WAV or FLAC, the whole mix or stems per track or bus, with optional normalisation and a tail for reverbs and delays.

## How it works

**Rendering** runs the same engine offline (block by block, faster or slower than real time) for the range plus a
tail (2 s by default) and the master's latency, which is then cut from the start.

**Sample-rate conversion** uses the sinc resampler of the sample loader (128 taps, Blackman–Harris).

**Normalisation.** *Peak*: $g = 10^{T/20}/\max|x|$. *Loudness*: $g = 10^{(T - L_I)/20}$ from the integrated loudness $L_I$,
reduced if needed so the true peak stays at or below −1 dBTP: $g \leftarrow g\cdot 10^{(-1 - TP')/20}$ when
$TP' = TP + 20\log_{10}g > -1$.

**Integer formats** (16 / 24-bit WAV and FLAC) are quantised with TPDF dither,
$q = \operatorname{round}(x\,(2^{b-1} - 1) + u_1 - u_2)$ with $u_1, u_2$ uniform in $[0,1)$: a triangular ±1 LSB noise that
makes the quantisation error independent of the signal (no distortion on fades), at the cost of a noise floor at
about $-6.02b$ dBFS. 32-bit float is written without dither and never clips.

[Manual](../README.md)
