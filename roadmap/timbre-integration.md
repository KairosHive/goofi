# Timbre integration

The biotuner bundle leaves this repo (`library.md`); these proposals go with it. A bank of plugin
notes and an oscillator's partials are different sound constructions; the UI documentation must
keep this clear.

## Proposed nodes, in priority order

### TimbreRender

Accept aligned partial frequencies and linear amplitudes, with optional phases and decay times.
Render on a pulse. Emit a waveform with sfreq metadata and an optional WAV path. Use biotuner's
existing synthesis functions, and connect the result to SignalIn or AudioPlayback. This gives
samplers and other VSTs an exact inharmonic timbre without a Vital-specific preset format.

Remaining work: define duration and envelope controls, bound render size, validate per-partial
alignment, and test frequency, decay, and file playback through the test audio host. Do not
regenerate a sample on every analysis frame.

### TimbreMorph

Accept two spectra with partial frequencies and amplitudes, plus a mix control. Emit one spectrum
with stable partial identities. Interpolate positive frequencies in log space and fade missing
partials through zero amplitude. The matching rule must be explicit; sorting each frame is not
voice tracking.

Remaining work: choose and test the partial matching rule, retain identity through changing
counts, and verify that interpolation does not exchange voices or introduce discontinuities. Use
the same partial/amplitude interface as VitalPreset and TimbreRender.

## Existing nodes and host changes

- Move RhythmPlayer timing from `time.monotonic` to the audio clock. Accept the per-row `steps`
  output of EuclidRhythm and emit every onset, including adjacent steps and narrow gates.
- Preserve source-degree identity in rhythm construction. Use that identity to assign each rhythm
  voice a pitch; never assume a rhythm row is a tuning index.
- Expose expensive timbre matching as a pulse-driven TimbreMatch node if users need those spectra
  in native synthesis. Move the matching controls out of VitalPreset at that point; keep the
  exporter focused on export.
- Settle VST bend-range and per-channel tuning behavior in `vst3-per-note-tuning.md` before
  claiming exact microtonal playback on arbitrary plugins. A generic normalized parameter mapper
  does not resolve this.
- A held VST note does not follow changes to pitch or velocity (`vst3/node.rs` acts only on gate
  edges); release the gates before changing a held note's pitch, or make the host retune it.
