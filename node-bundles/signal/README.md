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
