# Harmonic geometry verification

Checked on Windows x86_64, 9 September 2026, with Rust 1.97.1, the repository's
two Python environments, and Chromium. Biotuner is pinned to the same revision
as the existing bundle: `f45570e674d8193c7780891b9053a39bf6168c1e`.

## What the sessions check

The public Rust sessions are in
[`harmonic_geometry.rs`](../../backend/goofi-tests/tests/harmonic_geometry.rs).
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
[`harmonic-geometry.spec.ts`](../../tests/e2e/tests/harmonic-geometry.spec.ts).
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
.gfivenv/Scripts/python.exe examples/harmonic-geometry/build_patches.py
.gfivenv/Scripts/python.exe examples/harmonic-geometry/build_images.py
.gfivenv/Scripts/python.exe examples/harmonic-geometry/build_cookbook.py
```

On Linux/macOS use `.gfivenv/bin/python`. Run the Rust sessions first to produce
the raw frames consumed by `build_images.py`. The archives use fixed ZIP timestamps
and derive the goofi version from the workspace manifest.
