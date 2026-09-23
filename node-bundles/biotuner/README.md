# Biotuner bundle

This bundle owns harmonic analysis, tuning construction and reduction, color,
rhythm, and harmonic controls for sound. The harmonic-geometry bundle owns
geometric constructions and rendering.

Three shared harmonic nodes support both sound and geometry:

| Node | Role |
| --- | --- |
| HarmonicMorph | Morph two tuning or peak rows, including aligned weights and phases. Emit one labeled harmonic ARRAY for signal and graphics consumers. |
| RatioSequence | Emit one coherent endpoint transition ARRAY with mix and clock metadata. |
| HarmonicVoices | Convert a weighted harmonic ARRAY into fixed pitch/gain columns without dropping silent components. |

These are useful without a geometry node. HarmonicMorph uses Biotuner's shared
harmonic descriptor and transition functions. All bundle nodes use the same
pinned Biotuner revision as harmonic-geometry.

## Reuse the existing analysis nodes

- `Peaks.peaks` and `Peaks.amps` describe aligned frequencies and dB power.
  Use HarmonicMorph peaks mode with `amps_scale = db_power`.
- `HarmonicSpectrum.peaks` gives frequencies; `peakValues` gives harmonicity
  scores, not spectral amplitudes. It can weight a geometric structure in
  linear mode, but must not be described as measured acoustic power.
- `Tuning.tuning` gives pairwise, deduplicated interval ratios. Use ratios mode
  and uniform weights unless weights were computed for those exact degrees.
  Original peak amplitudes cannot be attached to these new degrees.
- `TuningReduction.reduced` can feed either HarmonicMorph endpoint. Its NaN
  padding is removed together with any aligned component data.
- `BioColors.rgb` feeds GeometryView or HarmonicInk palettes. Set source to
  tuning for ratio input, and set fund to the intended reference frequency.
- `TuningMatrix` and `Harmonicity` remain the musical-analysis nodes;
  GeometryMetrics is for geometry measurements.
- `TimbreControls` derives timbre measures and tilted partial gains from a
  tuning. HarmonicVoices instead preserves supplied component weights and
  silent slots. Both can control native oscillators, but their gain and voice
  identity rules differ. Use HarmonicVoices when a fade must match geometry.

Example 07 in `node-bundles/harmonic-geometry/examples` connects HarmonicSpectrum to both
the measured-peak endpoint and Tuning → TuningReduction for the other endpoint.
The resulting chord drives geometry, BioColors, and a TuningMatrix display.
Example 06 uses BioColors and EuclidRhythm. Example 08 uses HarmonicVoices for
the weighted sound route. These examples share capabilities rather than
reimplementing peak extraction, scale reduction, color, or rhythm analysis.

The new nodes do not emit alternate copies of their data. HarmonicMorph has
one [4,N] harmonic output; RatioSequence has one [2,N] transition output;
HarmonicVoices has pitch and gain outputs. Use Select for harmonic rows and
active columns. See the harmonic-geometry README for the shared array contract.

### PaletteView

Connect `BioColors.rgb` to `PaletteView.input` and show `image` in an image viewer.
PaletteView displays up to 16 normalized RGB colors as exact swatches with hex
labels and a blended ribbon. It also accepts palettes from other nodes. Set
`display.title` to label a mapping when comparing two palettes.
