# Harmonic geometry

Nine signal nodes and five shaders connect Biotuner's harmonic geometry to goofi.
The [cookbook](../../examples/harmonic-geometry/Cookbook.html) includes ten working
patches. The [survey](SURVEY.md) records the source review and implementation plan.

| Node | Role |
| --- | --- |
| `RatioSequence` | Timed ratio steps and pitch glides → current ratio, target, anchor chord, step, phase, and text readout |
| `HarmonicMorph` | Two ratio or peak rows → a shared harmonic frame; phase, stretch, extension, and component fades |
| `HarmonicGeometry` | 46 methods → curves, graphs, point clouds, meshes, or scalar fields |
| `HarmonicModes` | Harmonic frame(s) → bounded Chladni mode arrays with optional mode interpolation |
| `HarmonicTransport` | Scalar field → granular equilibrium, particles, tracer flow, or streaming |
| `GeometryBlend` | Matched fields, sampled curves, or matched meshes → a geometry transition |
| `GeometryMetrics` | Geometry → named measurements and a labeled vector |
| `GeometryView` | Geometry → transparent image, dashboard, and finite RGBA field upload |
| `HarmonicVoices` | Harmonic frame → fixed pitch/gain columns that retain component fades |
| `graphics:HarmonicLissajous` | Packed harmonics → a projected 3D light trace |
| `graphics:HarmonicChladni` | Packed modes/harmonics → signed plate/open waves or nodal density with D4 symmetry |
| `graphics:HarmonicInk` | Signed texture + optional BioColors palette → color, nodes, or contours |
| `graphics:HarmonicFlow` | A vector field → persistent seeded ink, with explicit freeze and reset |
| `graphics:HarmonicRelief` | Signed texture → twelve morphable organic and material textures, with parallax and directional light |

## Cable contracts

`harmonic` is a TABLE of aligned 1D float32 arrays: `ratios`, `amplitudes`,
`phases` (radians), and `damping`. Metadata holds `base_freq` (Hz), `equave`, and
`morph`. At most 32 components are supported. Silent slots stay in this frame;
ordinary `tuning`, `peaks`, `amps`, and `phases` outputs omit them. No `sfreq`
is inherited from an analyzed waveform. `packed` is `[4,32]`, with these four
rows and zero padding. `HarmonicModes.modes` is `[4,32]`, or `[4,64]` for fixed
field blends: m, n, amplitude, phase.

`geometry` is a TABLE with `type` (STRING), `coordinates` (ARRAY or numbered TABLE
of arrays for sets), and `info` (JSON STRING with parameters and metadata).
Optional `edges`, `faces`, and `weights` retain connectivity. `grid` is a numbered
TABLE of coordinate arrays. Optional `coverage` is a 0..1 array matching a field.
No Python objects, pickle, or null document leaves are used.

Curve coordinates are `[N,2]` or `[N,3]`; `trajectory` transposes them to `[2/3,N]`.
Scalar fields are `[H,W]`; vector fields are `[H,W,2]`. Mesh coordinates are `[N,3]`
and faces are `[T,3]` triangle indices. NaN outside a physical domain stays in
geometry data. `GeometryView.field` and transport `field`/`flow` encode finite
RGBA arrays: RGB repeats a scalar, or RG holds a vector; A holds coverage.
Upload these through `graphics:SignalIn`. The Flow shader accepts its ARRAY directly.

`HarmonicRelief` reads the same signed texture as Ink. It keeps the harmonic field
separate from the material. Depth changes the relief; seam changes its metal band;
camera and light change only the view. Select `texture_a` and `texture_b` from
jade, brushed metal, woven silk, porous stone, sand, dunes, lichen, coral, cells,
spores, pollen, and plankton. `texture_mix` blends their
height detail, color, roughness, and reflectance with a smooth endpoint curve.
`texture_scale` sets repeats; `texture_depth` sets the strength of surface detail.
`density` changes grain and colony coverage. Organic finishes have low gloss,
small relief, no metal seams, and field-dependent grain bands or warped cells.
They are procedural visual features, not tracked particles or biological models.
The renderer has no history or clock animation. Drive the upstream modes and
texture mix independently. Mirrored field extension fills any aspect ratio;
it is an artistic mapping, not a new physical boundary condition. Masked input
shows the opaque background. A render pass rebuilds the height map from the current
field each frame. It never reads an earlier height map. The float16 target uses
two channels to retain height precision. The default is 512 × 512, with at most
41 height samples, eight intersection refinements, four normal samples, and ten
shadow samples per pixel. These sample the prepared height instead of evaluating
the full relief rule at each ray step.

`RatioSequence.ratio` and `target` are scalar arrays. `tuning` retains the fixed
anchor chord and replaces its selected voice with the current ratio. `step` is
one-based; `phase` runs from 0 to 1 inside a step. `label` is a STRING readout.
Written lists retain their order. The optional `clock` input takes one finite
time in seconds; otherwise the node uses monotonic elapsed time. Pause holds
position. Reset, list/direction edits, clock source changes, and backward clock
jumps restart at step one. Pitch glides join consecutive ratios in log frequency,
including the loop boundary. See **10 · Living ratios** for a complete example.

`RatioSequence.transition` is a TABLE with `input` and `target` harmonic TABLEs
and an eased scalar `mix`. `chord.state` selects single-ratio endpoints or full
anchor chords with the moving voice replaced at each endpoint. Connect it to
`HarmonicModes.transition`. The complete packet keeps a step's endpoint change and
mix reset together. Use either this packet or the separate input/target/mix
ports; connecting both routes reports an error. The existing `tuning` output
still supplies the moving anchor chord for open waves, sound, and other media.

`HarmonicModes` offers `per ratio` and `chord pairs` mappings. The latter calls
Biotuner's `chord_to_int_modes` and `chladni_field_pairwise`: `[1, 5/4, 3/2]`
becomes `[4, 5, 6]` and, with all pairs, `(4,5), (4,6), (5,6)`. Pair weights
are uniform and phases zero, as in Biotuner's chord-pair medium. Only active,
distinct ratios participate. Auto/all/root/adjacent pair subsets follow the
upstream builder. More than 32 pairs reports an error instead of dropping pairs.

`interpolation = fields` keeps two endpoint mode banks at fixed wavenumbers
and blends their weights. This produces `[4,64]`, accepted by HarmonicChladni,
and avoids spatial expansion from the grid origin. `coordinates` retains the
explicit fractional-wavenumber effect. The `mapping` STRING reports input
ratios, integer chord, actual pairs, mode-cap scaling, and blend amount. A cap
scales all chord wavenumbers by the same factor, following Biotuner; increase
max_mode or simplify the chord to retain integer endpoints.

Use `Tuning.tuning` in ratios mode, or `Peaks.peaks` / `HarmonicSpectrum.peaks`
in peaks mode. Select one leading row. Amplitudes and phases must belong to the
same components. `Peaks.amps` is in dB: select the matching amplitude scale.
`HarmonicSpectrum.peakValues` is a harmonicity score, usable as an explicit
structural weight. It is not spectral power. Do not pair peak amplitudes with
the pairwise degree list returned by `Tuning`.

## Transition limits

Fixed-duration traces and open interference fields are suited to continuous
ratio changes. Closed curves, integer modes, knots, graphs, and recursion rules
can change discretely. Discrete methods omit zero-weight components, so a newly
active component can change their structure. Render settled endpoints and blend
them when correspondence matters. Mesh blending requires identical faces and
can produce intersections. Fractional plate modes are visual intermediates,
not eigenmodes of the original plate. Component pairing uses sorted rank, not
peak identity tracking. There is no hidden temporal smoothing of peak detection.
Many upstream generators normalize component amplitudes. They show relative
harmonic structure, not calibrated signal power. Use a separate image opacity
or audio gain to fade the total level.

The pinned pairwise/triple plate wrappers use uniform pair weights. Sand is an
equilibrium result. Tracer and streaming outputs are velocity fields; only
HarmonicFlow owns an evolving ink state. The heavier upstream crystallization,
reaction-diffusion, plasma equilibrium, polygon eigenproblem, and volume-surface
extraction paths are surveyed but are not frame-rate nodes in this bundle.

HarmonicChladni implements the notebook density stage with `field/output = nodal`
and `field/density_symmetry = d4_max`. It takes the maximum over eight rotated
and reflected densities, after `exp(-w²/σ²)`. `d4_sum` averages that orbit;
non-square grids skip D4, as upstream does. The fixed sigma default is 0.05.
Signed output remains available. Match the consumer: HarmonicInk style density,
or HarmonicRelief input_kind density. The latter inverts its deposit Gaussian
so the supplied density controls organic deposits without a second Gaussian.
The field pass uses one previous-frame displacement texture, with no feedback
accumulation. It adds one graphics frame of latency.
