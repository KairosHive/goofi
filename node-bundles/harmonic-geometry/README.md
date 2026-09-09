# Harmonic geometry

Eight signal nodes and five shaders connect Biotuner's harmonic geometry to goofi.
The [cookbook](../../examples/harmonic-geometry/Cookbook.html) includes nine working
patches. The [survey](SURVEY.md) records the source review and implementation plan.

| Node | Role |
| --- | --- |
| `HarmonicMorph` | Two ratio or peak rows → a shared harmonic frame; phase, stretch, extension, and component fades |
| `HarmonicGeometry` | 46 methods → curves, graphs, point clouds, meshes, or scalar fields |
| `HarmonicModes` | Harmonic frame(s) → bounded Chladni mode arrays with optional mode interpolation |
| `HarmonicTransport` | Scalar field → granular equilibrium, particles, tracer flow, or streaming |
| `GeometryBlend` | Matched fields, sampled curves, or matched meshes → a geometry transition |
| `GeometryMetrics` | Geometry → named measurements and a labeled vector |
| `GeometryView` | Geometry → transparent image, dashboard, and finite RGBA field upload |
| `HarmonicVoices` | Harmonic frame → fixed pitch/gain columns that retain component fades |
| `graphics:HarmonicLissajous` | Packed harmonics → a projected 3D light trace |
| `graphics:HarmonicChladni` | Packed modes/harmonics → a signed plate/open-wave field |
| `graphics:HarmonicInk` | Signed texture + optional BioColors palette → color, nodes, or contours |
| `graphics:HarmonicFlow` | A vector field → persistent seeded ink, with explicit freeze and reset |
| `graphics:HarmonicRelief` | Signed texture → sculpted jade and metal, with parallax, engraving, and directional light |

## Cable contracts

`harmonic` is a TABLE of aligned 1D float32 arrays: `ratios`, `amplitudes`,
`phases` (radians), and `damping`. Metadata holds `base_freq` (Hz), `equave`, and
`morph`. At most 32 components are supported. Silent slots stay in this frame;
ordinary `tuning`, `peaks`, `amps`, and `phases` outputs omit them. No `sfreq`
is inherited from an analyzed waveform. `packed` is `[4,32]`, with these four
rows and zero padding. `HarmonicModes.modes` is `[4,32]`: m, n, amplitude, phase.

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
camera and light change only the view. The renderer has no stored state or clock
animation. Drive the upstream modes to move the sculpture. Its rounded plate is
an artistic crop, not a new physical boundary condition. Masked input shows the
opaque studio background. A render pass rebuilds the height map from the current
field each frame. It never reads an earlier height map. The float16 target uses
two channels to retain height precision. The default is 512 × 512, with at most
41 height samples, eight intersection refinements, four normal samples, and ten
shadow samples per pixel. These sample the prepared height instead of evaluating
the full relief rule at each ray step.

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
