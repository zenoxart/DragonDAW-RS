# Playlist

The arrangement: audio clips and pattern clips on tracks.

- **Tools** (toolbar, with icons): draw, paint, select, slice, erase, mute, playback (scrub), zoom, slip edit.
- **Snap** to the grid, Alt while dragging = no snap.
- **Zoom:** Ctrl + wheel, Ctrl + right-drag a box, the zoom buttons; **Fit** shows the whole song.
- **Split** a pattern by channel: when the tracks below already hold clips, the split parts go to new tracks instead.
- **Clips:** drag to move, Shift to copy, the dB handle sets the gain, fades at the corners; double-click an audio clip for the clip window and [Audio repair](../audio-repair/README.md).
- **Mute / solo:** right-click a mute button to reset every mute in the playlist.
- **Loop and punch** regions snap to bars (Alt: beats); **markers** ask for a name.

## How it works

**Clip edges.** Every audio clip gets 3 ms raised-cosine fades at both edges (de-click), $w(t) = \tfrac12(1 - \cos\pi t/T)$,
so a clip that starts or ends mid-waveform does not click. User fades use the same shape over their length.

**Normalize** sets the clip gain to $g = 10^{T/20}/\max|x|$ so the clip's peak reaches the target $T$ dBFS.

**Stretch mode (time stretching)** uses WSOLA (waveform-similarity overlap-add): frames of 40 ms (at least 256
samples) with a $\sin^2$ window, synthesis hop $H_s = N/2$, analysis hop $H_a = H_s/\alpha$ for a stretch factor $\alpha$.
Each new frame is taken from within ±10 ms of its nominal input position, at the offset whose waveform correlates
best with the continuation of the previous frame, so the overlap adds in phase and the pitch stays. Pitch shifting
is a stretch by $2^{p/12}$ followed by resampling back to the original length.

**Tempo detection** (for audio dropped in): an onset-strength envelope (positive spectral flux) is autocorrelated over
the lags of 70 … 180 BPM, $R(\ell) = \sum_k e_k e_{k-\ell}$; lags whose double also correlates are rewarded (metrical
consistency), and the best lag gives $\text{bpm} = 60\,f_{env}/\ell$, rounded to 0.1.

[Manual](../README.md)
