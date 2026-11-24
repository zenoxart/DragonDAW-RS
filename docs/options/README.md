# Options

Audio (driver, device, buffer), MIDI inputs, control surfaces and controller buttons, plugin folders, keyboard shortcuts (rebindable), autosave and undo steps, interface scale, splash and start page, compact tracks and mixer, reduced motion.

**Performance:** stock effects skip silent input after their tail has died away, the oversamplers run as polyphase filters, and the interface only redraws when something changes – so hundreds of stock effects fit in real time.

**Every knob and fader** (mixer, rack, instruments, effects): right-click › **Set value…** opens a box with the current value; type a new one in the control's units and press Enter. It is one step in the undo / redo history.

## How it works

**Buffer size** $B$ sets the output latency $B/f_s$ (e.g. 256 frames at 48 kHz = 5.3 ms) and the time the engine has to
compute one buffer. The CPU meter is the computation time as a share of that budget.

**Parallel mixing.** The mixer measures its own load, $\bar\ell \leftarrow 0.9\bar\ell + 0.1\ell$ with $\ell$ the
insert stage's time over the block's budget $n/f_s$. Above 30 % the chains that share nothing (no sidechain, no
modulation) of each mixer level run on worker threads – one per spare core, the audio thread working too – and their
results are merged in order, so the output is the same as serial processing; below 12 % it returns to one thread (the
gap is the hysteresis that stops it flapping).

**Denormals** are flushed to zero on every audio thread (decaying tails would otherwise cost 10–100× in the
floating-point unit).

**Undo limit** sets how many steps are kept in memory and in the saved history.

[Manual](../README.md)
