# Harmonic geometry verification

## Single-array interface checkpoint

The current bundle has six signal nodes and six graphics nodes. HarmonicMorph,
RatioSequence and HarmonicVoices belong to Biotuner. GeometryUpload is removed.
GeometryRender, HarmonicInk, HarmonicRelief and HarmonicFlow read indexed geometry
arrays directly. GeometryView emits one image selected by its layout parameter.

Both installed Python wheels expose goofi.geometry. The geometry suite passed
all 46 methods and their dashboards, indexed connectivity through f16 transport,
part boundaries, field masks, malformed-data recovery, GPU geometry rendering,
harmonic shader state, peak alignment, phase wrapping, geometry blends, ratio
sequencing, continuous Chladni transitions and the Biotuner field reference.
These checks preceded the final upstream merge. The earlier combined archive
run timed out at recipe 07; recipe 07 passed in a separate process.

The ten archives now live in this bundle's examples directory. After relocation,
all ten rebuilt byte-for-byte, passed ZIP integrity checks, and retained valid
local documentation links. Example download integration is deferred.

The frontend checkpoint passed 712 unit tests, type checking with no errors or
warnings, and desktop disclosure/modulation plus phone touch-and-hold sessions.
The frontend production build completed with bundle-size and plugin-timing notices.
The cookbook is a local draft pending transfer to the website repository.

Final pre-push checks passed focused clippy with -D warnings, both Python
initialization/module-hygiene sessions, and codec imports in both installed
Python environments. The relocated Breathing lines archive passed its public
GPU/control/save-reload session (14.86 seconds). A combined run was stopped
after stalling before its first preview; a fresh process reported Windows
iceoryx2 cleanup errors but completed the focused session.

The final combined archive rerun rendered recipes 01 through 07, then failed
when iceoryx2 could not create another probe (NodeCreationFailure::InternalError).
Separate public sessions passed recipe 08 (native voice output and muting without
a device), recipe 09 (independent texture controls), and recipe 10 (live ratios).
The combined Windows suite is therefore not reported as passing. Full workspace
Rust tests and a new ten-recipe browser run were not repeated for this push.

The sections below record earlier checkpoints and their interfaces at that time.

Checked on Windows x86_64, 9 September 2026, with Rust 1.97.1, the repository's
two Python environments, and Chromium. Biotuner is pinned to the same revision
as the existing bundle: `f45570e674d8193c7780891b9053a39bf6168c1e`.

## Push checkpoint after upstream merge

### GPU geometry and bundle audit

`GeometryUpload` (signal) and `GeometryRender` (graphics) now belong to
harmonic-geometry. The renderer consumes numerical primitives, not a CPU image.
The public GPU session passed for 2D curves, 3D traces, graphs, point clouds and
triangle meshes, including a surface-to-wireframe change (30.65 seconds).
The updated Breathing lines archive passed live controls and save/reload
(31.41 seconds). GPU readback images were inspected. These are headless GPU
and public-session checks; no new browser session was run for this renderer.

Focused clippy passed with `-D warnings`. Its first run found an existing
codec-list initialization warning in `goofi-record`; using a vector initializer
removed it without changing codec order or selection.

The three shared nodes HarmonicMorph, RatioSequence and HarmonicVoices moved
to Biotuner. Recipe 07 now uses TuningReduction as a morph endpoint and shows
TuningMatrix output. Its dedicated session passed (45.16 seconds), including
the short-scale reduction fix. The complete 14-test audit was not green:
9 passed and 5 failed, with Windows iceoryx shared-memory errors among the
failures. Do not treat the focused passes as a full-suite pass.

The revised push includes these nodes, the ten patches, integration tests,
bundle ownership changes and required fixes. Unrelated working-tree edits
remain excluded. The cookbook website transfer follows the goofi push.

The merged branch passed the six-chord, 30-position Biotuner/GPU comparison
again (41.79 seconds). Its native build completed without compiler warnings.
The frontend passed Svelte typecheck with zero errors and warnings after
installing the merged lockfile and separating the Windows MIDI state import
from the similarly named component. The earlier broad node and browser results
below remain the coverage record; the full workspace suite was not repeated.

All fourteen nodes are suitable for development use. The sequence, morph,
mode mapping, Chladni, ink, and relief path has the strongest behavioral
coverage. Other geometry methods, dashboards, metrics, transport, Lissajous,
field/curve blending, flow, and voices have integration coverage with the
limits below. Hardware audio, other GPU drivers, long runs, and all optional
parameter combinations are not certified by these checks.

## Notebook field mapping and density

Recipe 10 now uses the full anchor chord, Biotuner common-denominator mode
pairs, fixed endpoint field blends, and the notebook's nodal density with D4
maximum. Coordinates interpolation remains an option. The field's signed,
nodal, and antinodal output selector is separate from its density symmetry.
The material and plain ink consumers use explicit density input modes.

Checks for this revision:

- The public node session compares six source ratios at five transition
  positions each with `chladni_field_pairwise` and `chladni_nodal_density` from
  the installed Biotuner. Signed GPU error is below 0.003; D4 nodal density
  error is below 0.015, including float16 render storage. All six endpoint
  fields are distinct. Passed in 24.05 seconds.
- The existing coordinate-motion boundary and ping-pong session passed in
  9.30 seconds. The fixed-field comparison verifies no spatial scaling.
- Both shader archives loaded and rendered in 18.88 seconds. The recipe 10
  image capture now waits for pattern contrast instead of accepting the
  startup background; that strengthened session passed in 16.43 seconds.
- Focused clippy passed with `-D warnings`; CLI build passed without warnings.
- Browser test typecheck passed. Living-ratios controls, visible picture motion
  with a fixed texture, pause, full-chord report, and the desktop/phone cookbook
  passed in two Playwright sessions (18.6 seconds).

The numerical reference covers the default D4 maximum and explicit sigma 0.05;
it does not exhaust all output, symmetry, or pair-subset combinations. The full
workspace suite was not rerun for this focused revision. Windows iceoryx2 cleanup
messages remain in test logs; the sessions completed successfully.

## Earlier continuous Chladni state checkpoint

Recipe 10 previously selected open waves and modulated one voice of an anchor
chord. This did not demonstrate a Chladni state walk. It now starts at approach
0 with full-step glides and four seconds per state. Texture animation starts
off. A new nodalLines tab shows the same field without the surface texture.

RatioSequence sends both endpoint harmonic frames and the eased mix in one
transition TABLE. HarmonicModes maps the endpoints to integer modes, then
interpolates their coordinates. A single packet keeps the endpoint changes and
mix reset together at step boundaries. Separate harmonic input/target/mix ports
remain available; connecting both routes reports an error. Without source data,
the node waits, including while a loaded patch starts.

- The public GPU continuity session passed in 18.19 seconds. It verifies mode
  coordinates, substantial field movement within a step, continuity across a
  step boundary and a reversal, and identical fields on the return path.
- Both archive sessions passed in 23.23 seconds, including the existing
  separate-input route in recipe 09 and the new packet route in recipe 10.
- The app build passed in 19.85 seconds. Clippy passed with warnings denied,
  and the browser test TypeScript check passed.
- Two Chromium sessions passed in 17.9 seconds. The live test now requires
  visible image movement while texture animation is disabled, checks the
  Chladni and glide defaults, and opens the line view. The cookbook check covers
  desktop and phone. The corrected live demo was restored without node errors.

One full-size archive run reached the Windows paging limit with the live
renderer also open. It was stopped and rerun alone. A startup check then found
that the new optional input routes reported an error before their first frame;
the node now waits as the former required input did. The full workspace suite
was not repeated. Intermediate fractional modes remain visual transitions,
not physical closed-plate eigenmodes.

## Organic textures and successive ratios

This update supersedes the four-texture setup below. Recipes 09 and 10 open
on Play with visible controls; Canvas still fills the window. HarmonicRelief
has twelve finishes, including sand, dunes, lichen, coral, cells, spores,
pollen, and plankton. The organic finishes reduce broad relief and highlights.
Density, detail scale, detail height, and texture mix remain separate controls.

RatioSequence emits ordered fractions or decimals at a timed rate. Its public
session checks holds, log-frequency glides, looping, ping-pong, pause, reset,
clock changes, invalid-input recovery, and fixed anchor voices. The moving
tuning reaches HarmonicMorph and changes an actual GPU Chladni field. Recipe
10 adds a live ratio history and numeric readout to this route.

The dropdown investigation found a shared binding defect: a bare string
global replaced the target parameter's option metadata. A graphics parameter
then received option index zero regardless of the selected text. The shared
mailbox now preserves target options and copies the source value, as string
references already do. Earlier browser checks verified selected values but
did not establish that each selected string reached the GPU as its index.

- The GPU material session passed in 16.50 seconds. It checks all twelve
  distinct finishes, exact mix endpoints, small continuous mix changes,
  density, held fields, retuning, relighting, finite values, and filled edges
  at 384 × 216 and 216 × 384.
- The final deterministic RatioSequence session passed in 13.00 seconds,
  including reset from a later step back to the first ratio.
- The new string-binding regression passed in 3.88 seconds. A global selects
  green, blue, and red; the session verifies the actual GPU pixels.
- Both embedded archive sessions passed in 17.68 seconds, including load,
  controls, output, and save/reload. A further recipe 09 capture check passed
  in 10.46 seconds after requiring valid mode data and a settled full image.
- Clippy for the embedded geometry test passed with warnings denied. The app
  build passed in 21.51 seconds. The E2E TypeScript check passed.
- Three focused Chromium sessions passed in 27.2 seconds. They check twelve
  dropdown choices, independent mixing, full-window landscape and portrait
  layouts, changing ratios, the visible history, pause, the numeric readout,
  and cookbook layout and filtering on desktop and phone.
- The live demo was restored on port 8650 with no node errors. GPU captures
  and browser views were inspected. The cookbook now includes ten recipes
  and a twelve-finish gallery.

Initial regression attempts exposed a missing test evaluator and invalid
global names; the final fixture uses the app's evaluator and grouped names.
One sequence run timed out during a transition. The final session waits for
pause and resume to reach the ratio node before advancing the separate clock.
One queued build hit a Windows executable lock; subsequent builds ran in
sequence. Large GPU readbacks reached the Windows paging limit, so the test
uses a 256-pixel source and 384-pixel output. Saved demos remain 512 pixels.
The passing sessions still print the existing iceoryx Windows cleanup messages.
No shared-memory files were removed by a script.

These textures are procedural field mappings. They do not track particles or
simulate biological growth. The renderer has no internal animation clock;
the ratio sequencer and texture LFO provide explicit motion. The full workspace
suite and recipes 01–08 were not repeated for this focused update. Their
previous results below remain historical checks.

## Full-window texture morph update

Recipe 09 now opens on Canvas, with a filled viewer and a mirrored field that
has no plate outline. Play retains the controls. Two texture selectors and an
independent LFO blend jade, brushed metal, woven silk, and porous stone. The
blend changes surface height, color, and reflection together.

- The expanded public GPU session passed in 14.40 seconds. It checks four
  distinct finishes, exact endpoints, a small continuous mix step, unchanged
  source data, stillness, retuning, control limits, and finite output. It also
  checks surface variation on every edge at 640 × 360 and 360 × 640.
- The focused embedded archive session passed in 10.93 seconds. It loads the
  seven-node recipe, renders its output, sets the mode and texture mixes
  independently, changes both texture selectors, and saves and reloads it.
- Clippy for the embedded geometry test passed with warnings denied. The app
  build and E2E TypeScript check passed without compiler warnings.
- Both focused Playwright sessions passed in 12.4 seconds: Canvas is selected
  on load, the picture fills landscape and portrait panels, texture selectors
  and the manual mix work, and the illustrated cookbook fits desktop and phone.
  No page errors or node errors remained. The GPU images and browser capture
  were inspected; the cookbook includes all four finishes on the same field.

One GPU run reached the Windows paging limit while the old preview process was
still open. The test passed after that task's saved preview was stopped. No
shared-memory files were removed by a script. Early shader checks caught and
resolved a header type name and a WGSL vector comparison error.

The full workspace suite and recipes 01–08 were not repeated for this update.
The previous results below remain historical checks. The live preview was
restarted with the updated shader and recipe after the focused tests.

## Initial Chladni material study

Recipe 09, **Jade resonance**, adds `HarmonicRelief` after the original bundle
checkpoint below. The renderer reads `HarmonicChladni.out` and keeps material,
camera, and light controls separate from the mode walk. The saved patch uses a
512 × 512 source field and output.

- The focused public GPU session passed in 18.73 seconds. It checks finite
  visible output, relighting with an unchanged source field, exact stillness
  when controls are held, a different picture after retuning, the control
  limits, and external controls that pass through zero.
- The final embedded archive session passed for all **nine** patches in 147.61 seconds,
  including live globals, save/reload, and the original audio example.
- Clippy for the embedded geometry test and its dependencies passed with
  warnings denied. The E2E TypeScript check also passed.
- The final app build passed without compiler warnings.
- Both final Playwright sessions passed in 1.4 minutes: all nine live patches,
  the relief and light slider endpoints, tablet layouts, and the cookbook on
  desktop and phone.
- The material was visually inspected after two tuning passes. It has no
  clock reads and no stored simulation state. Its motion is the upstream
  mode interpolation. This is a height-field rendering, not a physical solid
  vibration model; its fixed ray budget can lose very thin features at steep
  views or high spatial frequencies.

The first browser run found a generated light slider that stopped one step
before its endpoint: binary arithmetic had written `0.031400000000000004`
instead of `0.0314`. The archive builder now derives steps from the decimal
limits. Recipes 01, 02, and 05 also receive the corrected step values. An initial
height-pass version requested unsupported float16 packing; the final version
uses ordinary float32 arithmetic and a two-channel height encoding.
The relief preview also waits for lit metal, since its studio background has
enough spatial variation to pass a generic nonempty-image check on its own.
A capture rerun with the live preview also active timed out in recipe 01's
dashboard. The final run passed after the private preview was saved and paused;
the preview was then restored.

The full workspace suite and frontend unit suite were not repeated for this
shader-only addition. Their earlier results and machine failures remain below.

## What the sessions check

The public Rust sessions are in
[`harmonic_geometry.rs`](../../../backend/goofi-tests/tests/harmonic_geometry.rs).
They load real Python nodes and use goofi operations, output probes, the graphics
test clock, and the audio test clock.

- All 46 geometry methods produce data, a dashboard, and labeled metrics.
  The sweep also checks every exposed Faraday pattern, square symmetry,
  plate mode strategy, IFS contraction, and times-table mapping.
- All four transport methods run. Circular-domain masks survive CPU transport
  and conversion to finite RGBA arrays.
- Peak rows retain aligned amplitudes and phases when NaN padding is removed.
  The session changes counts, checks silence and invalid input, and then recovers.
- Chord phases cross the wrap by the shortest path. Harmonic and subharmonic
  extensions preserve zero-growth endpoints, consolidate repeated tones, and
  keep a nearby new tone silent until growth starts.
- Mode interpolation agrees with the two independently mapped endpoints.
- The WGSL cosine plate agrees with the Biotuner CPU field within `0.003`
  maximum absolute error. Lissajous and Ink render finite, nonempty frames.
  Flow evolves from its seed, then freezes exactly when its rate is zero.
- Field blends check endpoints, masks, and a domain mismatch. The same session
  switches to a 2D/3D curve blend and checks its common sample count.
- All eight `.gfi` archives load, render, respond to manual global controls,
  save, and reload. The sound example emits and mutes native audio on test clocks.
  Each archive has distinct node identities. Image captures wait for visible
  spatial variation after shader inputs arrive.

The Playwright session is in
[`harmonic-geometry.spec.ts`](../../../tests/e2e/tests/harmonic-geometry.spec.ts).
It opens all eight patches in the real app, switches every viewer tab, checks
frame delivery, changes auto and mix, checks node errors, and checks both tablet
orientations. A second session checks the cookbook on desktop and phone and
filters its 46-form atlas. Screenshots beside each recipe show the app layout.

## Check results

| Check | Result |
| --- | --- |
| Workspace build, all targets, two build jobs | Passed; no compiler warnings |
| Workspace Clippy, all targets, warnings denied | Passed |
| Embedded geometry test Clippy, warnings denied | Passed |
| Frontend build | Passed |
| Svelte typecheck | Passed; zero errors and warnings |
| Frontend unit tests | Passed; 96 files, 725 tests |
| E2E TypeScript check | Passed |
| Geometry sessions in the workspace run | Passed; all five sessions |
| Geometry sessions with subprocess Python | Passed; all five sessions, 187.98 seconds |
| Embedded archive/control/audio session | Passed; all eight patches, 45.80 seconds |
| Embedded phase/extension session in a separate process | Passed; 13.29 seconds |
| Eight-patch browser session on the final app build | Passed |
| Cookbook desktop, phone, atlas filter | Passed; both Playwright sessions completed in 1.3 minutes |
| Archive integrity | All eight archives passed |

The full `cargo test --workspace --no-fail-fast -j 2` run completed with **19
failed targets**. This is not an all-green workspace result. Failures included
Windows access violations and heap corruption in existing test binaries,
iceoryx2 node/service creation failures, a filter timeout, the missing
`computer-vision/requirements.txt`, and missing `onnxruntime` for Decoder.
The existing Biotuner sessions passed within the bundle tests. These failures
were not repaired as part of the harmonic geometry bundle.

The failed targets were the CLI unit-test binary, and the `agent`, `audio`,
`browser`, `bundles`, `contracts`, `editing`, `filter_golden`, `graphics`,
`inspect`, `nodes`, `recording`, `running`, `session`, `signals`, `simulation`,
`smoke`, `subpatches`, and `transport` situations. The local run log is
`target/harmonic-geometry/workspace-tests.log`.

## Machine limits found during verification

An unrestricted parallel build exceeded this machine's Windows paging limit.
The complete build passed with `-j 2`. Early combined Rust runs also hit an
iceoryx2 `NodeCreationFailure::InternalError` while creating a test probe;
the extension session passed when run on its own. The final six-test embedded
run passed five sessions, then failed while iceoryx2 created the mode output
service in the last session. This combined run remains a recorded failure;
the same phase/extension session passed in the workspace and subprocess runs.
No shared-memory files were deleted by a script. Resource reclamation stayed
with iceoryx2.

The ordinary browser fleet has a 60-second startup limit. This machine's first
VST catalog scan exceeded that limit. Browser checks used one separately started
debug server with a private `GOOFI_HOME`, the repository build cache, and the
Python environment from `.cargo/config.toml`. The actual patch and browser tests
were unchanged. The temporary launcher configuration is not part of the bundle.

The examples open no audio hardware or native windows. Listening on an actual
audio device, VST pitch behavior, other GPU drivers, and long performance sessions
were not tested. The supplied signal is synthetic data, not a physiological model.

## Rebuild and repeat

From the repository root:

```sh
cargo run -p goofi-init
cargo build --workspace --all-targets -j 2
cargo clippy --workspace --all-targets -j 2 -- -D warnings
cargo test -p goofi-tests --test harmonic_geometry -j 2 -- --test-threads=1
cargo test -p goofi-tests --features embed --test harmonic_geometry -j 2 -- --test-threads=1
npm --prefix frontend run check
npm --prefix frontend run test
```

The embedded feature supplies the same Python expression evaluator as the app.
The archive-control session needs that feature. To run the browser sessions,
build the frontend and CLI, then run from `tests/e2e`:

```sh
npm install
npx playwright install chromium
npx playwright test harmonic-geometry --project desktop --workers 1
```

For a direct Windows binary launch, use the `PYTHONHOME` and `PYTHONPATH` that
setup writes to `.cargo/config.toml`; `cargo run` already applies them.

To repeat the embedded phase/extension session separately after a Windows
shared-memory failure:

```sh
cargo test -p goofi-tests --features embed --test harmonic_geometry phase_wrap_extensions_and_mode_walks_keep_their_endpoint_weights -j 2 -- --exact
```

To rebuild the delivered files on Windows:

```sh
.gfivenv/Scripts/python.exe node-bundles/harmonic-geometry/examples/build_patches.py
.gfivenv/Scripts/python.exe node-bundles/harmonic-geometry/examples/build_images.py
.gfivenv/Scripts/python.exe node-bundles/harmonic-geometry/examples/build_cookbook.py
```

On Linux/macOS use `.gfivenv/bin/python`. Run the Rust sessions first to produce
the raw frames consumed by `build_images.py`. The archives use fixed ZIP timestamps
and derive the goofi version from the workspace manifest.
