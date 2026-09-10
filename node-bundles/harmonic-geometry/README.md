# Harmonic geometry

Six signal nodes and six shaders connect Biotuner's harmonic geometry to goofi.
They use three shared harmonic nodes from the [Biotuner bundle](../biotuner/README.md).
The [cookbook](examples/Cookbook.html) includes ten working
patches. The [survey](SURVEY.md) records the source review and implementation plan.

| Node | Role |
| --- | --- |
| `HarmonicGeometry` | 46 methods → curves, graphs, point clouds, meshes, or scalar fields |
| `HarmonicModes` | Harmonic frame(s) → bounded Chladni mode arrays with optional mode interpolation |
| `HarmonicTransport` | Scalar field → granular equilibrium, particles, tracer flow, or streaming |
| `GeometryBlend` | Matched fields, sampled curves, or matched meshes → a geometry transition |
| `GeometryMetrics` | Geometry → one labeled measurement array |
| `GeometryView` | Geometry → one image; choose transparent or dashboard layout |
| `graphics:GeometryRender` | Indexed geometry → antialiased curves, points, graphs and depth-tested matte meshes |
| `graphics:HarmonicLissajous` | Packed harmonics → a projected 3D light trace |
| `graphics:HarmonicChladni` | Packed modes/harmonics → signed plate/open waves or nodal density with D4 symmetry |
| `graphics:HarmonicInk` | Signed texture + optional BioColors palette → color, nodes, or contours |
| `graphics:HarmonicFlow` | A vector field → persistent seeded ink, with explicit freeze and reset |
| `graphics:HarmonicRelief` | Signed texture → twelve morphable organic and material textures, with parallax and directional light |

## Cable contracts

### Geometry

`HarmonicGeometry`, `GeometryBlend` and `HarmonicTransport` each emit only
`geometry`: one indexed ARRAY. Coordinates, part boundaries, connectivity,
weights, field grids and coverage are stored once. Descriptive parameters and
method names travel in metadata. There are no TABLE, trajectory or field copies.

Wire geometry directly to `GeometryRender.geometry` for curves, points, graphs
and triangle meshes. The renderer has a fixed oblique projection and fixed
`view/radius`; no automatic fit or camera motion. It provides antialiased lines,
transparent output and matte or wireframe meshes. It reads vertex indices, not
expanded triangle copies. The Breathing lines patch includes a geometryRender tab.

Rendering cost grows with pixels times primitives. Start at 256 square and
reduce sampling for dense meshes. This is a fragment renderer, not a native
vertex-buffer pipeline. It does not interpolate between geometry packets;
new shapes arrive at the signal source's update rate.

For scalar fields, wire geometry to `HarmonicInk.geometry` or
`HarmonicRelief.geometry`. For vector flow, wire it to `HarmonicFlow.flow`.
These shaders read the same array directly. No CPU image or SignalIn adapter
is needed. When an Ink/Relief geometry input is present, it takes precedence
over their texture input. GeometryView emits one image: display/layout selects
dashboard (RGB) or transparent image (RGBA). GeometryMetrics emits only values,
with names on its row axis.

The shared `goofi.geometry.encode/decode` functions and the graphics prelude
define the format. It is tiled as `[pages,256,4]`. Eight header texels precede
vertices, part lengths, edges, faces, weights and grids. Integer counts and
indices use base-1024 digit pairs, preserving exact topology through f16 uploads.
Coordinates appear once; field coverage occupies the fourth component. Domain
NaNs remain in CPU data, while coverage preserves their mask on the GPU.

### Harmonics

Biotuner's `HarmonicMorph.harmonic` is one `[4,N]` ARRAY, with rows labeled
ratio, amplitude, phase and damping. Metadata holds base_freq, equave and morph.
Up to 32 components are supported. Silent slots retain their frequency; their
column labels start with silent. Active columns start with active. There are no
duplicate tuning, peaks, amplitude, phase or packed outputs.

Use the existing Select node on axis 1 with `include=active*`, then another
Select on axis 0 with `include=ratio` and `squeeze=true`, for an active tuning.
Examples 06, 07 and 10 use this route. Existing Biotuner nodes still provide
peak analysis, tuning reduction, colors, interval matrices and rhythms.

HarmonicVoices consumes the same harmonic array and emits pitch and gain only.
GeometryBlend blends formed geometry; HarmonicMorph changes harmonic inputs.
GeometryMetrics measures shape; Harmonicity and TuningMatrix measure intervals.
HarmonicModes.modes remains `[4,32]`, or `[4,64]` for fixed-field blends:
m, n, amplitude and phase. Its mapping string is a human-readable report.

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

`RatioSequence.transition` is its only output: a `[2,N]` ARRAY with input and
target endpoint ratios. Eased mix, one-based step, phase and selected voice are
metadata. Feed this same packet to HarmonicMorph.transition and
HarmonicModes.transition. This keeps endpoint changes and the mix reset together.
Consumers derive their current state; the sequence emits no ratio/tuning copies.
Select single-ratio or anchor-chord endpoints with chord/state.

Written lists retain order. An optional clock supplies elapsed seconds; otherwise
the node uses monotonic time. Pause holds position. Reset, list/direction edits,
clock-source changes and backward jumps restart at step one. Log-frequency glides
join consecutive ratios, including the loop boundary. See **10 · Living ratios**.
Use either the transition input or the separate endpoint/mix inputs on a consumer.

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
