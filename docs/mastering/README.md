# Mastering

The Mastering tab: the master chain as a signal flow with gain reduction, loudness (LUFS, true peak, LRA), the spectrum, a reference track comparison with loudness-matched A/B, and album projects with loudness matching.

## How it works

**Loudness (ITU-R BS.1770-4 / EBU R128).** Each channel is K-weighted (a +4 dB high shelf around 1.68 kHz and a 38 Hz
high-pass, coefficients derived for any sample rate) and squared; 100 ms blocks give energies $z_j$. Loudness of a
window is

$$L = -0.691 + 10\log_{10}\sum_c z_c\ \ \text{LUFS}$$

*Momentary* uses the last 400 ms (4 blocks), *short-term* the last 3 s (30 blocks). *Integrated*: 400 ms windows with
75 % overlap, an absolute gate at −70 LUFS, then a relative gate 10 LU below the loudness of what passed; the result is
the loudness of the windows that pass both. In real time the windows go into a 0.1 LU histogram (−70 … +10 LUFS, count
and energy per bin), so the gated value costs the same however long the song runs.

**True peak.** 4× oversampling with a windowed-sinc polyphase interpolator (12 taps per phase, raised-cosine window);
the largest $|$sample$|$ of the interpolated signal in dBTP. It catches peaks between samples that a converter will
produce.

**Reference comparison.** Loudness (integrated, short-term maximum, loudness range), RMS, sample and true peak, crest
factor and PLR (peak to loudness ratio), stereo width and correlation, and a third-octave tonal balance. The balance is
measured relative to each signal's own loudness, so it compares the *shape* of the spectra, not their level.

[Export](../export/README.md) · [Manual](../README.md)
