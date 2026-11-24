# Browser

- **Search:** typing lists every matching file name from all browser folders.
- **Keyboard:** arrow keys move through the list, Left / Right close and open folders, **Enter** adds the selected sound to the channel rack.
- **Preview:** clicking a sound plays it; **double-click** plays it twice and adds nothing.
- Drag a sound to the playlist, the rack or a channel.

## Current project › History

The undo history of the open project, oldest first: **0** is the start, the ▶ row is where the project is now, the rows after it (in italics) are steps you undid and can redo.

- **Double-click a step** to bring every parameter, clip, note and setting back to the state right after that step – backwards or forwards, any number of steps at once. Nothing is lost: the steps after it stay in the list.
- The history is **saved with the project** (in its `History` folder, up to the undo limit in Options) and with every autosave, so after reopening – also after a crash – you can still undo.
- If the project file was changed elsewhere, the saved history no longer fits and a new one starts.

## How it works

**Sample loading.** Files are decoded (WAV, FLAC, MP3, OGG Vorbis, AIFF, M4A/AAC) and resampled to the engine rate with a
windowed-sinc interpolator (128 taps, cutoff $0.95\cdot\frac{f_s}{2}$, 128× oversampled table with linear interpolation
between table entries, Blackman–Harris window). Decoded audio is cached and shared, so ten channels using one sample
hold it once.

**Key detection** (shown for samples and clips): a chromagram (energy folded onto the 12 pitch classes) is correlated
with the Krumhansl–Schmuckler major and minor profiles rotated to every root; the best Pearson correlation of the 24
gives the key and, clamped to $[0,1]$, the confidence.

**History storage.** Each undo step is stored as the difference between two JSON snapshots of the project: objects
store only changed fields (a removed field as *null*), arrays of equal length only changed items, anything else is
replaced whole. The saved list is tied to the project file by a hash of the saved project, so a file changed elsewhere
starts a fresh history instead of applying differences to the wrong state. Jumping to a step applies the differences
one after another, undoing or redoing until the position matches.

[Manual](../README.md)
