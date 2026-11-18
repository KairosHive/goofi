# Appendix A. The node inventory

Every node of the present program, from a survey of `node-bundles/` and the engines on
2026-11-18. The rewrite keeps every node. Counts: 215 node files in 15 bundles (signal 139,
graphics 56, audio 20; Rust 83, Python 80, WGSL 52), plus 4 nodes built into the audio engine and
the VST3 types of the machine. A coverage check compares this list with the files (part 12.5).

Marks: (R) Rust, (P) Python, (W) WGSL.

## A.1 Built into the audio engine

`AudioIn`, `AudioOut`, `AudioPlayback`, `MidiIn` (part 05.2.3).

## A.2 Bundle `signal` (signal engine; built in)

- (R) `ArrayPlayback`: replay a recorded array stream at its recorded instants.
- (R) `AudioIn`: the audio plane as frames: stream, fixed window or envelope.
- (R) `Autocorrelation`: autocorrelation per lane over lags.
- (R) `Buffer`: a rolling window on one axis; size in seconds, samples or updates.
- (R) `Clock`: a steady tick in bpm, Hz or seconds; pulse, phase, count.
- (R) `ColorLut`: a 3D colour-transfer table from a source and a reference, or from a file.
- (R) `Constant`: a number with a shape.
- (R) `Cycle`: the mean shape of one cycle of a periodic signal.
- (R) `Delay`: the stream delayed by samples, seconds or updates.
- (R) `Emd`: empirical mode decomposition.
- (R) `Fft`: forward and inverse FFT, polar or complex, with a window.
- (R) `Filter`: lowpass, highpass, bandpass, notch; zero-phase or causal.
- (R) `Format`: join strings by a separator or a template.
- (R) `FreqShift`: shift the spectrum by Hz.
- (R) `FromJson`, `ToJson`: JSON text to a table and back.
- (R) `Function`: one elementwise function (19 options).
- (R) `GraphicsIn`: a texture as an array: pixels, grey, rows or columns.
- (R) `Hilbert`: envelope, phase, frequency.
- (R) `Hold`: sample and hold on a pulse, with a ramp.
- (R) `Join`: concatenate or stack the inputs of a list slot.
- (R) `LFO`: sine, triangle, sawtooth, square; one value or a block.
- (R) `Math`: add, multiply, remap, and a bound (clamp, wrap, fold).
- (R) `Meta`: write the sampling frequency or channel labels onto a frame.
- (R) `Morph`: one frame along the wired frames by a position.
- (R) `Noise`: uniform, normal, pink; value, perlin, simplex, ridged.
- (R) `Normalize`: z-score, robust, min-max; from history, frame or buffer.
- (R) `Null`, `NullString`, `NullTable`: named pass-through anchors for expressions.
- (R) `Operation`: fold the inputs: arithmetic, min, max, mean, matrix product, correlations.
- (R) `Psd`: power spectral density (FFT or Welch) with band limits.
- (R) `Quantize`: pitches to the nearest note of a scale.
- (R) `Reduce`: collapse axes: mean, median, min, max, arg, std, sum, norm.
- (R) `Reshape`: permute axes, then reshape.
- (R) `Select`: keep or drop on one axis by position, slice, label or EEG group.
- (R) `Smoothing`: exponential, attack and decay, moving average, convolution.
- (R) `StringPlayback`, `TablePlayback`: replay recorded lines and rows.
- (R) `Switch`: route one of several inputs by an index.
- (R) `Table`, `TableSelect`, `TableNormalize`: build a table; take one field; normalize members.
- (R) `Text`: a string constant.
- (R) `Threshold`: above or below a level, with hysteresis, edge, dwell, hold.
- (R) `Variable`: a variable of the patch on a cable.
- (R) `Wavelet`: Morlet or Mexican-hat transform.
- (P) `Eigen`: eigenvalues and vectors of a matrix or its Laplacian.
- (P) `FmSignal`: an FM source in blocks.
- (P) `LslIn`, `LslOut`: Lab Streaming Layer in and out.
- (P) `MidiOut`: notes and one controller out.
- (P) `OscIn`, `OscOut`: OSC in (a table by address) and out.
- (P) `RandomMap`: a seeded random linear map.
- (P) `Resample`: anti-aliased resampling of a window.

## A.3 Bundle `audio` (audio engine; built in; all Rust)

`BandFilter`, `BandFollow` (the two halves of a vocoder), `Delay`, `Env` (ADSR per voice),
`Feedback`, `Filter` (state variable; cutoff in volts per octave), `FreqShift`, `Gain`,
`GraphicsIn` (texels as samples), `Limiter`, `Mixdown`, `Noise`, `Null`, `Osc`, `Quantize`,
`Reverb`, `SignalIn` (a signal frame as a looped waveform or an oscillator bank), `Slew`,
`Variable`.

## A.4 Bundle `graphics` (built in)

- (W) `AudioIn` (audio as a running picture), `Blur`, `ColorMap`, `Composite` (24 blend modes),
  `Constant`, `Displace`, `Edge`, `Elementary`, `Feedback`, `Gradient`, `Ising`, `Lenia`,
  `Level`, `Life`, `Lookup`, `Lut3D`, `Math`, `NeuralCA`, `Noise`, `Reaction`, `Sandpile`,
  `Shade`, `Shape`, `SignalIn` (a signal frame as texture, line plot, trajectory or topomap),
  `Tessellate`, `Threshold`, `Transform`, `Upscale`, `Wave`, `Window`.
- (R) `Variable`: an image variable as a texture.
- (P, graphics) `Camera`, `NdiIn`, `VideoFile`.
- (P, signal) `NdiOut`.

## A.5 Bundle `eeg` (signal; built in)

(R) `Specparam`. (P) `EEGReference`, `Connectivity`, `EegPlayback`, `EegPowerBands`,
`EegPreprocess`, `GraphMetrics`.

## A.6 Bundle `simulation`

- (R, signal) `Attractor`, `Boolean`, `Branching`, `Hopfield`, `Kuramoto`, `NeuralMass`,
  `Oscillator`, `Physarum`, `Population`, `Quantum`, `Readout`, `Reservoir`, `Spiking`, `Swarm`.
- (W) renderers of the same name: `Attractor`, `Boolean`, `Branching`, `Hopfield`, `Kuramoto`,
  `Physarum`, `Quantum`, `Reservoir`, `Spiking`, `Swarm`.

## A.7 Bundle `biotuner`

(R, audio) `BioFilter`. (P) `Peaks`, `Tuning`, `TuningMatrix`, `TuningReduction`, `Harmonicity`,
`HarmonicSpectrum`, `HarmonicObservatory`, `HarmonicMorph`, `HarmonicVoices`, `RatioSequence`,
`BioColors`, `PaletteView`, `BioElements`, `EuclidRhythm`, `Polyrhythm`, `RhythmPlayer`,
`ScaleForaging`, `MicrotonalKeys`, `TimbreControls`, `VitalPreset`.

## A.8 Bundle `complexity` (all Python)

`DetrendedFluctuation`, `FractalDimension`, `Hjorth`, `LempelZiv`, `PermutationEntropy`,
`SampleEntropy`, `SpectralEntropy`, `SvdEntropy`, `WaveletLeaders`, `ZeroCrossings`.

## A.9 Bundle `harmonic-geometry`

(P) `HarmonicGeometry`, `HarmonicModes`, `HarmonicTransport`, `GeometryBlend`,
`GeometryMetrics`, `GeometryView`. (W) `GeometryRender`, `HarmonicChladni`, `HarmonicFlow`,
`HarmonicInk`, `HarmonicLissajous`, `HarmonicRelief`.

## A.10 Bundle `computer-vision`

(P) `Flow`, `Objects`, `PoseEstimation`, `Segment`. (W) `Skeleton`.

## A.11 Bundle `ai` (all Python)

`LLM`, `Decision`, `Agent`, `VoiceAgent`, `SpeechToText`, `TextToSpeech`, `DINO`, `DinoDecode`.

## A.12 Bundles `image`, `image-generation`, `ml`, `texture`, `inception`

- `image` (P): `ImageFile`, `RealESRGAN`.
- `image-generation` (P): `FluxRT`.
- `ml` (P): `Decoder`, `Onnx`, `ReinforcementLearning`, `Weights`.
- `texture` (P): `AdaptiveMask`, `PromptMask`, `TextureTransfer`, `MaterialSpace`,
  `MaterialEncoder`, `SpaceDraw`.
- `inception` (W): `FluidPigments`, `FractalJewelry`, `RibbonSpace`, `ThoughtField`,
  `TimeWarpFbm`.

## A.13 Dependencies of the bundles

| Bundle | Python packages and models |
| --- | --- |
| signal | mido, pylsl, python-osc, python-rtmidi, scipy |
| graphics | opencv, an NDI binding |
| eeg | mne, scipy, networkx; sample recordings on first use |
| biotuner, harmonic-geometry | a pinned biotuner, pillow |
| complexity | antropy, PyWavelets |
| computer-vision | mediapipe, opencv |
| ai | pydantic-ai with providers, elevenlabs, torch, transformers; provider keys; model downloads |
| image, texture | pillow, scipy, torch, transformers, diffusers; model downloads |
| image-generation | an NVIDIA GPU and a prepared model checkout |
| ml | onnxruntime |
| audio, simulation, inception | none |

## A.14 Nodes the rewrite adds

- Loader and writer nodes for datasets (part 04.9).
- A `Feedback` node on the signal plane.
- Event sources: a threshold edge, a MIDI note (part 03.6.4).

## A.15 Candidates named in the present roadmap, outside the rewrite

- Graphics: channel reorder, HSV colour, fit and crop, switch, mask, function, operation,
  convolve, image, text, remap, pattern; later key, morphology, bloom, lens, histogram, cache.
- Audio: `Clock`, `Seq`, `HzToOct`, `MidiCC`, `Sampler`.
- Simulation: a grid for `Swarm`, delays for `NeuralMass`, Flow-Lenia, Swift-Hohenberg.
- Biotuner: `TimbreRender`, `TimbreMorph`, `TimbreMatch`.
