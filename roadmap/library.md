# The node library

Find, inspect, filter and install node bundles in goofi. The library includes builtin nodes,
installed external nodes, saved local nodes and nodes available from registered Git sources.

Status: pending. This specification records the product decisions agreed through 2026-10-04.
No bundles have moved and no implementation builds have run. This session targets the local,
self-compiled version. Release tooling, website changes and new node content are deferred.

## Bundles and sources

- Every node belongs to a bundle. A bundle contains node sources and their required files,
  and can contain nodes for more than one engine. It does not require a plugin package.
- `signal`, `audio`, `graphics` and `eeg` stay builtin. Move `biotuner`, `complexity`,
  `computer-vision`, `harmonic-geometry`, `image`, `image-generation`, `inception`, `ml`
  and `simulation` to `../goofi-nodes` (`KairosHive/goofi-nodes`). Bundle-specific nodes
  stay with their bundle, regardless of engine.
- A source is a Git clone URL or a `<uname>/<repo>` GitHub shorthand, with an optional
  `@<ref>` for a branch or tag. Support GitHub, GitLab and self-hosted Git through the user's
  installed, authenticated Git, including private repos. No separate GitHub login is required.
- Register one source per repo. Registering it with another ref replaces the selected ref.
  Installed bundles in that repo share its checkout and revision.
- The repo identity and checkout path remain `<uname>/<repo>` and
  `~/.goofi/nodes/<uname>/<repo>/`. The host is not part of that storage identity; sources
  with the same owner/repo on different hosts cannot both be registered.
- `~/.goofi/nodes/_local/` is the user's local bundle. Save patch-authored nodes there.
  Replace `.goofi/custom/` directly. No repo can replace `_local`.
- Register no default source, including `KairosHive/goofi-nodes`. Name that source in setup
  documentation. A fresh home contacts no remote.

## Node entries and selection

Each node has a structured entry with its declaration name, engine, bundle, bundle source
and author. Sources describe `builtin`, `local`, `patch`, `plugin` or Git. A Git source
includes its repo URL and branch/tag when known; a plugin source identifies the plugin.
For Git, author means the repo owner or namespace, such as the GitHub org/uname. Unknown
metadata remains unknown.

For example, an external entry has these fields. The ref is illustrative:

```json
{
  "name": "Peaks",
  "engine": "signal",
  "bundle": "biotuner",
  "author": "KairosHive",
  "source": {
    "kind": "git",
    "url": "https://github.com/KairosHive/goofi-nodes",
    "ref": "main"
  }
}
```

- Select nodes by declaration name. Use optional `--bundle`, `--engine`, `--author`,
  `--source` and `--ref` fields to distinguish equal names. Plugin identity is available
  when needed. The source selector accepts a source kind or repo URL.
- All supplied fields constrain the match, in any order. Exactly one match succeeds.
  No match reports the selector; multiple matches report candidates and useful fields
  to distinguish them. There is no implicit source preference.
- Equal names can coexist in different engines, bundles or sources. Duplicate engine/name
  declarations within the same bundle/source are errors.
- CLI, frontend, MCP and scripts use the same selection rules and operations. The existing
  `node add --name` supplies an instance name independently of the declaration name.
- Keep resolved structured references in the graph, copy/paste, undo and saved patches.
  A later name collision does not change an existing reference. Repo updates do not create
  a different node identity; author, ref and observed commit describe the source.
- Show node and bundle names in the UI. Show engine, author and source where they help
  distinguish equal names. Keep source file, language, description, tags and observed
  revision available for inspection when known.

Examples of the planned selection behavior:

```sh
goofi node add Peaks
goofi node add Peaks --bundle biotuner
goofi node add Peaks --bundle biotuner --author KairosHive --engine signal
goofi node add Peaks --bundle biotuner --source https://github.com/KairosHive/goofi-nodes
```

## Library behavior

- Provide source registration/removal, browsing, inspection, search/filtering, whole-bundle
  installation/removal, repo update checks/pulls, refresh and preparation status.
- Registration lists available bundles and nodes without installing or loading them.
  Read declarations without executing node code. Fields that require execution remain
  unknown until preparation. Incomplete entries can still offer installation.
- Install only whole bundles. A node's install action installs its containing bundle.
  Obtain selected bundle files and required helpers without installing unrelated bundles.
  Avoid downloading unrelated bundle contents. Reuse one checkout when more bundles from
  the same repo are installed.
- Installation prepares dependencies and nodes automatically. The user does not need
  separate clone, build or refresh commands. Show preparation progress and failures.
  An installed bundle with a failed node remains visible and can be retried.
- Current checkout files are the source of truth. Manual edits and updates take effect on
  refresh or app start. Unchanged, prepared installed nodes can load without remote access.
- Show upstream changes on explicit refresh or library panel entry. Pull only on user action,
  with fast-forward behavior. Preserve local edits; leave divergence/conflicts to manual Git.
- Uninstall removes the installed selection. Keep the shared checkout, downloaded Git objects
  and Python packages. Remove unneeded checkout files only when user work is preserved.
  Source unregistration does not uninstall its bundles.
- Library management works without an active goofi server. Saving a node from an open patch
  requires that patch's context. App and standalone operations produce the same results.
- Install, update and removal are allowed during active use. Restarting goofi is an accepted
  recovery when changed code or shared dependencies prevent continued use.
- Library navigation and management do not enter patch undo or dirty the patch.

## Patch loading

Patches carry structured node metadata, including bundle sources and observed revisions.
Use the current installed source for a node that is already available.

- Try to load declared nodes from their available sources, including builtin, patch, plugin
  and Git sources. Use source carried by the archive where available.
- When declared nodes are unavailable, show a dialog that lists them and their bundle/source.
  Offer **Install and load**, **Load anyway**, and **Cancel**.
- **Install and load** uses the saved source metadata to obtain the missing bundles and prepare
  their nodes through the normal library operations. The source does not have to be registered
  before opening the patch. Install a bundle once when several missing nodes require it.
- Missing `_local` nodes have no remote source to install from. Load them when their source is
  present locally or carried by the archive; otherwise report them as unavailable.
- Try to resolve and prepare other sources. If a source cannot be obtained or a node cannot be
  prepared, show the failure and keep the option to load anyway.
- **Load anyway** skips unavailable nodes and loads the remaining patch. Report skipped nodes
  and connections that cannot load. Keep the remaining nodes, parameters, links and layout.
- **Cancel** keeps the current patch open. Do not install or pull without the user's selected
  action. Do not silently substitute a same-named node from another source.
- CLI, MCP and scripts expose the same install, skip and cancel choices through operation
  arguments/results. The dialog is presentation of those choices.

## Bundle authoring and examples

- Bundles contain sources, helpers, assets and dependency declarations. Python and WGSL nodes
  keep their current bundle-level source convention. Repo-root node declarations are invalid;
  repo-root documentation, licences, Cargo files and shared helpers are allowed.
- Rust bundles use ordinary Cargo packages/workspaces, dependency declarations and lockfiles.
  goofi supplies the SDK binding for the running version. Authors need no absolute path to a
  goofi checkout. Bundles must work without unrelated bundles being checked out.
- Python requirements use the existing shared environments. Support `requirements.txt` and
  `requirements-gil.txt`; do not create bundle-specific environments. Report dependency
  conflicts and preparation failures.
- Helpers, assets and dependencies must work from their bundle context. Source and dependency
  changes must take effect after preparation. Keep generated artifacts in the runtime.
- A bundle can include `examples/*.gfi`. List examples with the bundle; the public demo offers
  examples from loaded bundles.
- Move the four EEG examples to `node-bundles/eeg/examples/`. Move `musical-features.gfi`
  with `biotuner`. Delete `harmonic-observatory.gfi` and the old repo-root examples route.
- The plugin interface remains in `sdk/README.md`.

## Scope and completion

Follow `AGENTS.md`. Choose the smallest implementation that meets this specification.
Implementation order, module boundaries, cache strategy and Git/build command sequences
are left to the build. This item adds no library-specific locking requirement.

Completion means:

- The four builtin bundles are the only shipped bundles. The nine external bundles, their
  required files and licences are in `../goofi-nodes` and can be installed through the library.
- A fresh goofi runs without external bundles or their packages. Setup, CI and smoke tests
  use the builtin set. Git is listed with the local development prerequisites.
- Structured entries and selectors serve all callers. Replace old node ID parsing, custom
  paths, last-root-wins selection and the unmanaged `--extra-nodes` route directly.
- The library panel supports the specified operations and metadata. The add menu offers
  available loaded nodes; uninstalled entries stay in the library. Touch, tablet and desktop
  layouts and cable dragging still work.
- Patch loading supports installation, explicit skipping and cancellation through the common
  interface. Saved local and patch-authored source remains portable in archives.
- Delete existing goofi test cases that depend on moved product nodes, including browser cases.
  Preserve unrelated cases in mixed files. Do not keep removed cases through external installs,
  copied product code or replacement fixtures.
- Verify library and patch behavior through public sessions with small controlled repos.
  Follow the checks in `AGENTS.md`, including relevant browser sessions. Commit tested changes
  on `main` with the required model trailer and preserve other work in both checkouts.
- Update setup and SDK/library documentation. Website author instructions and generator changes
  belong to `../goofi-website` later; transfer its need to read `../goofi-nodes` to that backlog.
  Transfer external simulation/biotuner node-content work to `../goofi-nodes`. Keep graphics here.
- Publication/push is a separate action. Keep this entry while deferred library distribution
  or builtin node-content work remains.
- Favourites, hiding and a hosted library service are outside this feature's scope.

## Deferred library distribution

Status: pending. This work is required for release binaries, after the local-library feature.
`release-binaries.md` owns installer, provisioning and release delivery work; this section owns
the library's additional tool and authentication requirements.

- Bundle Git for Linux, Windows and macOS through the pinned tool manifest and `layout::Tool`.
  Make Git available to library operations and to uv/Cargo for Git dependencies. Standalone
  library CLI operations must provision required tools without starting a goofi server.
- Choose supported private-repo authentication paths: HTTPS credential helpers/browser login,
  SSH keys/agents, credential storage and the helper/SSH tools needed on each platform. Indexing,
  cloning and pulling must use the same authenticated access. The local version continues to
  use the user's installed, authenticated Git.
- Resolve dependency preparation on machines without host build tools. The earlier shipped
  biotuner Git requirement moves with its bundle; installation still needs Git. Source-only
  Python packages such as python-rtmidi 1.5.8 on 3.14t need a C++ compiler. Decide how those
  external requirements are supported and avoid repeated Git dependency downloads where possible.
  Builtin wheel availability remains part of release provisioning.
- Verify private source indexing, individual sparse bundle install, pull, Python preparation
  and external Cargo builds with host Git/tools unavailable. Keep SDK binding and permit
  dependencies outside the SDK vendor set, as specified in the bundle convention.

## Deferred node-content work

Status: pending. These proposals are separate from the local-library feature. Keep external
simulation and biotuner work with their bundles when they move to `../goofi-nodes`. Graphics and
its engine support remain in goofi. Implement shared engine features only for a concrete consumer.

### Shared graphics decisions and engine support

- Audit alpha conventions across generators, filters, viewers and recording. Shape scales RGB
  by coverage; Constant supplies independent RGB and alpha. Agree on one policy for new nodes
  and share it between Math and Composite.
- Decide texture color space, channel selection, border modes, units and invalid numeric results
  once. Preserve HDR values where the operation permits them.
- One graphics tick is one pass. Decide how to support bounded internal passes/sub-steps for a
  concrete consumer such as Bloom, wide Gaussian Blur, Optical Flow or Fluid. Fragment shaders
  cannot loop over a texture they write; do not hide unbounded per-pixel loops.
- The graphics tick has no per-stage budget or lower-rate stage execution. Measure the current
  behavior before adding scheduling controls; saturation applies to the tick.
- Decide between a shared point-splatting node and a compute stage before adding more particle
  models. `Swarm.wgsl` and `Physarum.wgsl` each walk an `[N, D]` position array in a fragment
  shader with a small cap; Physarum's overlay is capped at 512 agents.
- Image and Text use the shared [Rust/Python graphics producer](../sdk/graphics.md).
  Verify graphics changes in real GPU sessions with opaque/transparent images, unequal sizes,
  boundary values and mode changes. Measure sampling cost at useful frame sizes outside
  correctness tests; do not add timing thresholds to tests.

### Builtin graphics nodes

This build order follows TouchDesigner's [TOP catalog](https://docs.derivative.ca/TOP).
Names are proposals for capabilities the existing nodes do not provide. Graphics stays builtin.

First batch:

| Capability | Smallest useful scope | TOP reference |
| --- | --- | --- |
| Reorder | Build RGBA from input channels, luminance, zero or one; second input for alpha and channel packing. | Reorder |
| Color | Hue shift, saturation, value and monochrome in the agreed color space. Level keeps gain/offset/gamma/invert. | HSV Adjust, Monochrome |
| Fit / Crop | Contain, cover, stretch, crop and pad to the common output size; alignment and border color. Extend Transform with independent X/Y scale, pivot and flip. | Fit, Crop, Transform, Flip |
| Switch | Select one of two textures. Composite's `blend` provides a crossfade; many-input selection needs shared engine support. | Switch |
| Mask | Replace or multiply alpha from a selected channel of another image; invert and remap the mask; keep foreground RGB. | Matte |

Second batch:

| Capability | Smallest useful scope | TOP reference |
| --- | --- | --- |
| Function | Per-channel abs, sign, power, root, log, exp, sin/cos, floor, ceil, round and fract; define invalid-domain results. Math keeps scale/range behavior. | Function |
| Operation | Scalar operands use Math; texture operands use Composite's add/subtract/multiply/divide/minimum/maximum, with the shared alpha policy above. | Math, Function |
| Edge / Convolve | Shared neighborhood sampling; Sobel magnitude/direction, Laplacian, sharpen, emboss and a small custom kernel. Presets share one node. | Edge, Convolve, Emboss |
| Image | Load a still image from the patch workspace onto the graphics plane, preserve alpha and report decode failures. ImageFile in the external image bundle currently loads onto the signal plane. | Movie File In |
| Text | Render a string with font, size, alignment, wrapping, foreground and background. Needs a font/raster upload path. | Text |
| Remap | Sample an image at absolute UV coordinates from another texture with explicit outside-frame behavior. Could be a Displace mode. | Remap |
| Pattern | Checker, grid, stripes and radial/angular coordinates. Extend Gradient/Shape where appropriate; Wave belongs to simulation. | Gradient, Circle, Rectangle |

After the foundations:

| Capability | Proposed scope |
| --- | --- |
| Key | Luma and chroma keys, soft selection and spill suppression. |
| Morphology | Dilate and erode masks; opening/closing as compositions. |
| Channel Mix | A channel matrix only after Reorder proves insufficient. |
| Color space / Tone map | RGB↔HSV, linear↔sRGB and HDR-to-display after the color contract is agreed. |
| Bloom | Bright-region extraction and multi-scale blur/add; use a patch first. A node needs internal passes. |
| Normal / Slope | Height-to-gradient and height-to-normal; share derivative kernels with Edge. |
| Corner Pin | Four-corner projective warp. |
| Lens / Polar | Lens distortion and cartesian↔polar; prefer Remap presets where sufficient. |
| Layout | Arrange images in a row, column or grid inside a texture. |
| Resample | Nearest, linear and a proper downsample filter with explicit border modes. Upscale provides enlargement with linear, FSR1 and NIS; add a node only for a distinct resize stage. |
| Blur extensions | Bilateral blur and mask-driven radius inside Blur. |
| Composite extensions | Hue, saturation, color and luminosity blend modes inside Composite. |

Additional engine support:

| Capability | Required support |
| --- | --- |
| Cache / Delay / Hold | Bounded GPU frame history with capture, freeze, reset and indexed delay; one allocation/advancement owner; no viewer-driven clock. |
| Analyze / Histogram | GPU reduction for min/max/mean and distributions, with a defined result format. |
| Sample / Texture-to-signal | Read pixels, rows or regions through transport; specify readback rate and cost; never viewer snapshots. |
| Media playback | Camera plays a video file. Add seek, pause, speed, timestamps and image sequences through the existing source owner. |
| Optical Flow | Motion vectors from successive frames need history and a pyramid or internal passes; after Cache. |
| External texture I/O | Screen capture and Spout/Syphon/NDI; optional for the basic bundle. |

Do not add separate Add, Multiply, Over, Under, Invert, Limit, Circle, Rectangle, Movie File Out
or texture In/Out nodes merely to match TOP names; existing nodes cover those roles. General 3D
rendering (cameras, lights, materials) and vendor camera integrations are outside the core 2D work.

### Fractal textures

Port selected capabilities from `AntoineBellemare/fractal_visuals`, branch `mfractal-toolbox`.

Remaining steps:

1. Add a `curl` output mode to graphics Noise: a divergence-free RGB flow field for Displace and
   Feedback. Compose `curl_weave`, `rheoscopic` and `dye_diffusion` from those nodes.
2. Add `Fluid.wgsl` with state buffers and pressure projection after the shared sub-step decision.
   Resolve how projection passes advance across ticks or within them. Fluid stays with graphics.

Open:

- Re-measure c2 after a cascade change by running WaveletLeaders on `node snapshot --raw` frames;
  the existing `textures.rs` test checks kurtosis only.
- `amount` controls cascade sigma, warp distance and local-dimension spread; `fbm`, `ridged` and
  `billow` ignore it. Separate these meanings when changing controls.
- `ridged` and `billow` cannot combine with `multifractal` because they share `fractal`. Splitting
  them needs a third knob; `kind` remains the basis and `fractal` the combination rule.
- Add the exact NumPy generators as a Python node emitting `[H, W]` to graphics SignalIn.
  Keep shader approximations for motion and the exact signal implementation for stimuli.
- Determine whether `universal_multifractal`'s Levy-stable cascade has a shader form; its
  variates are costly to obtain from a hash.

Do not add one node per rock/cloud texture: compose `marble`, `agate`, `veined` and related
textures from Noise, Displace, Lookup and Threshold. Do not port `eddies`, `vorticity` and
`plume` as written; their iterative projection depends on the shared graphics sub-step decision.

### External simulation bundle

Open node work, after the shared graphics decisions above:

- `Swarm` is O(N squared) and capped at 2000 particles. A uniform grid could lift the cap by an
  order of magnitude, but no current use requires it.
- Add long-range delays to `NeuralMass` using the ring-buffer design in `Spiking`.
- Flow-Lenia's mass-conserving transport fits a gather shader; measure total mass across ticks
  before it ships.
- A Swift-Hohenberg mode in `Reaction` needs a biharmonic and a stable, sufficiently small step.
- A Manna mode in `Sandpile` can use gather only if both sides choose the same random neighbors.
- Decide whether this bundle should contain audio-engine nodes.

Do not add:

- A simulation Fluid node: Feedback with Displace supplies advection and Feedback with Blur
  supplies diffusion. Pressure projection belongs to graphics Fluid above.
- A second noise node: add Ornstein-Uhlenbeck, fractional Brownian motion and 1/f^beta as signal
  Noise modes; it currently has `uniform`, `normal` and `pink`.
- A generic ODE node: param-source Python plus Function and state already provides it.
- An active-inference agent while its input/output contract remains a research question.
- A self-organizing map: it belongs with analysis nodes.
- A digital waveguide: physical audio modelling is a separate audio-engine question.
- Diffusion-limited aggregation or L-systems before goofi can draw their geometry.

### External biotuner timbre integration

A bank of plugin notes and an oscillator's partials are different sound constructions. Keep this
clear in later UI documentation. Proposed nodes, in priority order:

1. **TimbreRender.** Accept aligned partial frequencies and linear amplitudes, with optional
   phases and decay times. Render on a pulse using biotuner's synthesis functions. Emit a
   waveform with sfreq metadata and an optional WAV path for SignalIn or AudioPlayback, so
   samplers and other VSTs can play an exact inharmonic timbre without a Vital-specific preset.
   Define duration/envelope controls, bound render size, validate alignment and test frequency,
   decay and file playback through a test audio host in the owning repo. Do not render on every
   analysis frame.
2. **TimbreMorph.** Accept two frequency/amplitude spectra and a mix control. Emit one spectrum
   with stable partial identities. Interpolate positive frequencies in log space and fade missing
   partials through zero amplitude. Define/test matching and retain identity through changing
   counts; sorting each frame is not voice tracking. Verify that interpolation does not exchange
   voices or introduce discontinuities. Share the partial/amplitude interface with VitalPreset
   and TimbreRender.

Existing nodes and host dependencies:

- Move RhythmPlayer timing from `time.monotonic` to the audio clock. Accept EuclidRhythm's
  per-row `steps` and emit every onset, including adjacent steps and narrow gates.
- Preserve source-degree identity in rhythm construction to assign each rhythm voice a pitch;
  a rhythm row is not necessarily a tuning index.
- Add pulse-driven TimbreMatch for expensive matching if native synthesis needs those spectra.
  At that point move matching controls out of VitalPreset so it remains an exporter.
- Settle VST bend range and per-channel tuning in `vst3-per-note-tuning.md` before claiming
  exact microtonal playback on arbitrary plugins. A generic normalized parameter mapper is
  insufficient.
- A held VST note currently ignores pitch/velocity changes: `vst3/node.rs` acts only on gate
  edges. Release gates before changing a held pitch, or add host retuning.

## Continuation after context compaction

- Read this specification and `AGENTS.md`; inspect diffs in goofi and `../goofi-nodes`.
- Implementation is pending. This specification replaces the detailed stage plan and earlier
  string node ID grammar. The missing-node dialog supersedes the earlier report-only load rule.
- Roadmap maintenance does not start implementation. Wait for the user's build instruction.
- Resolve implementation choices from the specification and code. Follow `AGENTS.md` for
  checks and tested commits; keep remaining work current here.
