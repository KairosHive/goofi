# Signal

## FmSignal

Generate a sinusoidal FM signal as sample blocks with sampling-rate metadata.
Connect `FmSignal.out` to `Buffer`, then `Psd` or `Peaks`.

- `fm.carrier`: carrier frequency in Hz.
- `fm.modulator`: spacing between spectral sidebands, in Hz.
- `fm.index`: modulation depth. Zero gives a sine wave.
- `output.sfreq`: sample rate in Hz. Increase it for wider spectra to avoid aliasing.

The source retains its oscillator phases between blocks. It emits the samples
for the elapsed time, independent of the node update rate. This is a signal
array; use an audio-engine bridge when an audio stream is needed.

## RandomMap

Map any number of inputs to `map.outputs` values through a seeded random matrix.
Every input is flattened and joined; the matrix is scaled so its largest singular
value is 1, and rebuilt when `map.seed`, the output count or the input size changes.

## ArrayPlayback, StringPlayback, TablePlayback

Play back a stream that goofi recorded: `playback.file` is the `.npy`, `.txt` or `.csv` file in a
recording folder. The `.jsonl` sidecar beside it gives each frame's instant, shape and meta, so
the frames leave as they were recorded, one frame per run in order. `common.max_frequency` must
stay above the recorded frame rate. `playback.loop` starts over at the end.

Without a sidecar, an array takes its frame shape and `sfreq` from the folder's `manifest.json`,
else one sample per `fallback.channels` at `fallback.sfreq`. A text plays a line per
`fallback.sfreq` second. A CSV row carries its own instant.
