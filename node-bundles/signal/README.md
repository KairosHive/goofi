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

## Morph

One frame picked along the frames wired into `input`, which must share a shape: `position`
counts in frames and loops, so 1.5 is halfway from the second to the third. `linear` mixes the
two nearest; `spline` runs one closed Catmull-Rom curve through all of them, which passes through
every frame and stops at none. The node runs on its own clock, so an expression on `position`
glides between held frames — the token grids of `DINO` nodes, say, on their way to `DinoDecode` —
and it sends nothing while neither the frames nor the point move, the frames told apart by their
senders' stamps.

## RandomMap

Map any number of inputs to `map.outputs` values through a seeded random matrix.
Every input is flattened and joined; the matrix is scaled so its largest singular
value is 1, and rebuilt when `map.seed`, the output count or the input size changes.

## Splice

Two frames of one shape, `[rows, columns, ...]`, cut together: each cell of the first two axes
takes `a`, `b` or a mix, by a fractal map, or by a wired `mask` (`[height, width]` in 0..1, 1 for
`a`, read at each cell). The map is value noise over three octaves with blobs `scale` cells
across, thresholded so `a` keeps its `share` of the cells, and `edge` cells wide at each seam.
Between two `DINO` token grids and `DinoDecode`, `scale` 2 cuts a collage of small fragments and
`scale` 9 lets two textures meet along organic seams, each keeping its forms; the decoder draws
each seam as one surface. `drift` moves the map through a third axis of the noise, and the
weights follow it continuously: one unit grows a new map, and about 0.05 a second at 30 frames a
second makes the pieces glide. On pictures it is a pixel patchwork.

## ArrayPlayback, StringPlayback, TablePlayback

Play back a stream that goofi recorded: `playback.file` is the `.npy`, `.txt` or `.csv` file in a
recording folder. The `.jsonl` sidecar beside it gives each frame's instant, shape and meta, so
the frames leave as they were recorded, one frame per run in order. `common.max_frequency` must
stay above the recorded frame rate. `playback.loop` starts over at the end.

Without a sidecar, an array takes its frame shape and `sfreq` from the folder's `manifest.json`,
else one sample per `fallback.channels` at `fallback.sfreq`. A text plays a line per
`fallback.sfreq` second. A CSV row carries its own instant.
