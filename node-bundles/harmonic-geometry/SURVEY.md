# Harmonic geometry: survey and implementation plan

Survey date: 9 September 2026. Source: the installed Biotuner revision
`f45570e674d8193c7780891b9053a39bf6168c1e`, also pinned by goofi's Biotuner bundle.
The installed source, not a version label on the documentation site, defines this plan.

## What the module contains

| Family | APIs surveyed | Use in goofi |
| --- | --- | --- |
| Harmonic descriptors | `HarmonicInput`, `HarmonicSequence`, scale provenance and alternate scales | One frame describes ratios, amplitudes, phases, damping, base frequency, and equave. Select a row before geometry; do not flatten channels into one chord. |
| Lissajous | 2D, 3D, compound, pairwise grid, phase drift, topology | Compound and fixed-duration traces support tuning motion. Closed curves use rational periods and need a separate geometry blend. |
| Harmonographs | lateral, rotary, 3D, peak adapter, linewidth damping | Damped traces with phase, duration, and rotation controls. Frequencies are normalized for drawing; audio keeps its own base frequency. |
| Circular geometry | star polygon, times table, chord times table, tuning circle, rose, epi/hypocycloid, interval-vector graph, chord graph, consonance polygon | Compare interval structure, modular multiplication, and rolling-circle curves. Preserve edges and weights. |
| Number and fractal geometry | Stern–Brocot, continued-fraction rectangles, Farey/Ford layouts, subharmonic tree, harmonic IFS | Bounded tree depths and seeded attractors. Integer choices are structural changes, not continuous physical changes. |
| Generative geometry | ratio L-system, recursive polygon, self-similar tuning, geometry sequence | Bounded recursive structures and repeatable snapshots. goofi already supplies the stream, so it does not need another sequence clock. |
| 3D geometry | Lissajous tube, harmonic knot, harmonic surface, 3D L-system, recursive polyhedron, harmonic point cloud | Preserve vertices and triangle indices. Project them for dashboards. Keep mesh blends strict about matching connectivity. |
| Rigid plates | rectangular, circular, polygon, box; per-ratio, pairwise symmetric/antisymmetric, triple antisymmetric; nodal density and contour/surface extraction | Expose analytical plate fields and pairwise cymatics. The polygon eigenproblem is a heavier snapshot operation. Marching cubes is not needed for the first bundle. |
| Other eigenmodes | `Elastic`, `ClosedSurface`, `PlasmaLattice` | Anisotropic plates and spherical modes have direct visual uses. Plasma lattice is an iterative equilibrium solve; keep it out of a frame-rate path. |
| Wave fields | harmonic, quasicrystal, standing lattice, Bessel/Laguerre vortex, multiple sources; `Acoustic` | Continuous open-domain interference, with signed fields retained before coloring. |
| Parametric response | `Faraday` | A separate capillary-wave model with pattern and dispersion controls. It is a modeled field, not a fluid simulation. |
| Transport | `Granular`, `Tracer`, `Streaming` | Convert a supplied field into equilibrium density or flow. Feed flow to a stateful GPU ink layer. The CPU transport outputs are not evolving particle simulations. |
| Morphogenesis | `Crystallization`, `ReactionDiffusion` | Each `respond` performs a fresh long simulation (defaults: 2,000/4,000 steps). Existing goofi reaction shaders already own evolving state. Do not restart these solvers on every tuning frame. |
| Coupling and structure | consonance, ratio complexity, spectral spread, amplitude entropy; prime limit, Tonnetz, pair distances, continued-fraction depths, symmetry, chord signature | Existing harmonic metrics remain in the Biotuner bundle. Geometry metrics measure the result; they do not replace harmonicity. |
| Extensions | harmonics, subharmonics, common harmonic fit, harmonic-series tuning | Fade in an extension independently of a chord morph. Consolidate duplicate frequencies before fading; the pinned extension helpers return duplicates. |
| Transitions | `interpolate_chords`, `fade_in_components`, `blend_fields` | Preserve their distinct meanings: sorted component pairing, component growth, and scalar-field crossfade. Add pitch-log interpolation and shortest-arc phase interpolation. |
| Metrics and plotting | `geometry_metrics`, comparison/normalization, sequence metrics, metrics log, plotting | Use native measurements and a reusable dashboard renderer. Do not create Matplotlib figures on the real-time path. |

## Constraints found in the source

- Float ratios are approximated with denominator 10,000. Closed Lissajous periods
  can change greatly for a tiny ratio change. Torus knot windings and Chladni
  integer modes are also discrete. Limit rational complexity for closed shapes.
- `normalized_amplitudes()` replaces an all-zero amplitude vector with a uniform
  vector. Explicit silence must be handled before calling a generator.
  Many generators use normalized weights; their size is not an absolute power
  measurement. Overall image opacity and audio gain remain separate controls.
- `interpolate_chords` pairs sorted components by rank. It does not track the
  identity of moving peaks. Unequal counts use amplitude fades. Its default
  phase interpolation takes the long route across the phase wrap.
- Harmonic extensions can contain the same frequency more than once.
  `fade_in_components` matches each occurrence to a base frequency; duplicate
  entries must be consolidated at float32 wire precision to preserve the starting
  chord. Use exact matching after consolidation so a nearby new tone stays silent
  at zero growth. Shared extension phases also need shortest-arc interpolation.
- Circle/polygon fields use NaN outside their domain. Preserve this mask in data;
  convert it to transparent pixels before a shader texture upload.
- Pairwise/triple rigid-plate wrappers choose uniform pair weights. Peak
  amplitudes are not automatically pair amplitudes. State that limit clearly.
- `Granular` gives a Boltzmann equilibrium. `Tracer` and `Streaming` give flow
  fields or steady density. None is a persistent particle integrator.
- Geometry outputs differ: curves, sets, graphs, meshes, scalar fields, and
  vector fields. Keep connectivity and grids with each frame in a TABLE.
- goofi uploads arrays as float32. Bound indices and sample counts so topology
  indices remain exact; sanitize only at the image boundary, not in analysis.

## Bundle plan

1. **HarmonicMorph:** select two tuning or peak rows, or enter ratio presets;
   interpolate in ratio or pitch space, wrap phases, fade extra components, and
   grow harmonic/subharmonic extensions. Output one harmonic TABLE plus ordinary
   tuning, peak, amplitude, phase, and packed shader arrays.
2. **HarmonicGeometry:** a common generator interface across curves, circular
   graphs, fractals, meshes, plates, and wave fields. Instances select a method;
   all return the same geometry TABLE. Bound sampling and recursion costs.
3. **HarmonicTransport:** apply granular density or tracer/streaming flow to a
   supplied scalar field. Retain the field grid and mask.
4. **GeometryBlend:** blend common-grid fields, resampled open curves, or meshes
   with identical connectivity. Reject invalid pairings with a useful message.
5. **GeometryView:** render geometry into a labeled dashboard and a separate
   transparent image. Support 3D camera controls, weighted graph edges, fields,
   palette inputs, and harmonic context.
6. **GeometryMetrics:** expose Biotuner's geometry measurements as a labeled array
   and table for viewers, references, and recording.
7. **HarmonicModes:** expose Biotuner's bounded mode mappings as shader arrays.
   Permit interpolation between two mode sets without claiming that fractional
   modes are eigenmodes of a closed plate.
   **HarmonicVoices** also reads the shared frame and keeps its amplitude fades
   in fixed native pitch/gain columns, beside the existing TimbreControls node.
8. **Shaders:** a 3D Lissajous renderer, a Chladni/interference field renderer,
   a field color/contour renderer, and a flow-driven ink layer. Keep appearance
   controls separate from the field and its state.
9. **Examples:** ready-to-load patches for continuous curves, plate-to-wave
   transitions, flow and sand, knots and surfaces, interval/fractal geometry,
   and live peaks connected to the existing Biotuner bundle. Include controls,
   readable dashboard layouts, and no audio hardware or native windows.
10. **Verification and cookbook:** run the real nodes through public sessions;
    check every exposed method, empty/malformed data, changing counts, morph
    endpoints, shader rendering, and patch reloads. Inspect browser dashboards.
    Deliver an illustrated cookbook with recipes, source links, and limits.

## Source links

- [Pinned source tree](https://github.com/dav0dea/biotuner/tree/f45570e674d8193c7780891b9053a39bf6168c1e/biotuner/harmonic_geometry)
- [Harmonic geometry API](https://antoinebellemare.github.io/biotuner/api/harmonic_geometry.html)
- [Geometry schema source](https://antoinebellemare.github.io/biotuner/_modules/biotuner/harmonic_geometry/geometry_data.html)
- [Metrics and transition examples](https://antoinebellemare.github.io/biotuner/examples/harmonic_geometry/06_metrics_and_transitions.html)

The documentation is useful context but may describe a different revision.
