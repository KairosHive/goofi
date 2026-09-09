# Timbre integration

The bundle has two routes to sound. TimbreControls exposes a selected spectrum
as native oscillator controls or VST notes. VitalPreset stores a spectrum in a
Vital oscillator preset. A bank of plugin notes and an oscillator's partials
are different sound constructions; the UI documentation must keep this clear.

## Current interface

- TimbreControls keeps all tuning rows on its analysis outputs. Its voice row
  selects one tuning, sorts and deduplicates positive ratios, and takes the
  lowest partials up to the configured count. The voice count is fixed at 1..8.
- pitch, gain, and gate are finite [voices, 1] columns. SignalIn in direct mode
  carries them to native audio parameter references. gain divides amplitudes
  by their sum before gating, so adding voices does not increase the peak bound.
- voices contains pitch columns followed by velocity columns, matching the
  existing VST voice input. Unused notes and notes outside MIDI range have zero
  velocity. Gate rises trigger notes; a held VST note does not follow changes
  to pitch or velocity in the current host.
- brightness, spread, and harmonicity are 0..1 controls per tuning row. Select
  one row before binding a plugin parameter. Native physical units need a range
  mapping with the existing Math node. Smooth or audio:Slew can smooth controls.
- VitalPreset accepts either a tuning or aligned partials and linear amplitudes.
  The latter route reads base_freq from the frequency frame, so exporting a
  TimbreControls spectrum does not require a second root-frequency setting.
- Exports happen on a write pulse and default to the patch workspace's presets
  directory. The files output lists generated artifacts. Load a .vital file in
  Vital's preset browser; it is not a VST3 component-state blob.
- The bell preset replaces the misleading inharmonic name. It changes the
  envelope and effects around a periodic wavetable. Exact fractional partial
  frequencies require additive synthesis or a rendered sample.

## Patch connections

For native additive synthesis, connect Tuning.tuning to TimbreControls.input.
Send pitch and gain through separate audio:SignalIn nodes in direct mode. Bind
their out ports to Osc's osc/pitch and Gain's gain/gain parameters. Connect
Osc.out to Gain.input, Gain.out to Mixdown.input, and Mixdown.out to AudioOut. An optional
gate input turns selected partials on and off. Slew can soften amplitude edges.

For a VST instrument, send TimbreControls.voices through one audio:SignalIn in
direct mode, then connect its out port to the VST voice input. This plays up to
eight plugin notes. Release the gates before changing a held note's pitch.

For a Vital oscillator timbre, connect TimbreControls.partials and amplitudes
to VitalPreset's matching inputs, select preset/row, and pulse preset/write.
Load the resulting .vital file in Vital. Use a separate note source to play it;
the preset already contains the partial spectrum.

## Proposed nodes, in priority order

### TimbreRender

Accept aligned partial frequencies and linear amplitudes, with optional phases
and decay times. Render on a pulse. Emit a waveform with sfreq metadata and an
optional WAV path. Use biotuner's existing synthesis functions, and connect the
result to SignalIn or AudioPlayback. This gives samplers and other VSTs access
to an exact inharmonic timbre without a Vital-specific preset format.

Remaining work: define duration and envelope controls, bound render size,
validate per-partial alignment, and test frequency, decay, and file playback
through the test audio host. Do not regenerate a sample on every analysis frame.

### TimbreMorph

Accept two spectra with partial frequencies and amplitudes, plus a mix control.
Emit one spectrum with stable partial identities. Interpolate positive
frequencies in log space and fade missing partials through zero amplitude.
The matching rule must be explicit; sorting each frame is not voice tracking.

Remaining work: choose and test the partial matching rule, retain identity
through changing counts, and verify that interpolation does not exchange voices
or introduce discontinuities. Use the same partial/amplitude interface as
VitalPreset and TimbreRender.

## Existing nodes and host changes

- Move RhythmPlayer timing to the audio clock. Accept per-pattern lengths from
  EuclidRhythm and emit every onset, including adjacent steps and narrow gates.
- Preserve source-degree identity in rhythm construction. Use that identity to
  assign each rhythm voice a pitch; never assume a rhythm row is a tuning index.
- Expose expensive timbre matching as a pulse-driven TimbreMatch node if users
  need those spectra in native synthesis. Move the matching controls out of
  VitalPreset at that point; keep the exporter focused on export.
- Settle VST bend-range and per-channel tuning behavior in
  vst3-per-note-tuning.md before claiming exact microtonal playback on arbitrary
  plugins. A generic normalized parameter mapper does not resolve this.

New nodes above are proposals, not installed capabilities.
