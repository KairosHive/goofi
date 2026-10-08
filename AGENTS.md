# goofi

goofi is a real-time node-based platform for biosignals, audio, and graphics. Users build
patches in a browser. The backend is Rust; the frontend is SvelteKit.

Use ASD-STE100 Simplified Technical English. Read the code for implementation details.

If a message gives you explicit directions, there are only three allowed actions:
1. Explain why the directions are incorrect or cannot be followed.
2. Request clarification if the directions are ambiguous.
3. Execute the directions.

Do not build a different design that you prefer. If you think the request is wrong, say so first.
Scope is exact: "push" is not "force-push", "remove these two" is two, and a fix for one symptom
does not authorize a change to a shared rule.

## Principles

1. **One capability interface.** UI, CLI, MCP, scripts, and tests use the same op vocabulary.
   Device and client differences belong in presentation, not separate behavior paths.
2. **One state owner.** Derive values from their source or project them one way. No mirrored
   state, duplicate counters, or caches that must be kept in sync manually.
3. **Decide from settled state.** Apply a batch before scheduling work, broadcasting changes,
   or changing resource ownership. Intermediate mutations must not cause independent decisions.
4. **Smallest coherent design.** Delete obsolete paths and share rules that must agree. Prefer
   clear code to explanation; comments explain exceptions, and docstrings state purpose.
5. **Fix the cause.** Trace callers and boundaries before changing a subsystem. Use types and
   shared schemas to prevent invalid states; report real boundary failures without panics.
   A structural refactor is appropriate when it removes a class of errors.

## Pre-launch policy

goofi has no production users or data. Revisit this policy before the first production release.

- Remove obsolete code, schemas, APIs, configuration, and aliases directly. Add no compatibility
  shims, dual-read or dual-write paths, or backfills unless the user asks for them.
- Internal interfaces are not contracts. A rename, schema field, wire format or shared rule
  changes in one commit with every reader: callers, tests, e2e helpers, generated frontend files,
  README, skills and roadmap. Search for the old form before you commit.
- Development and test data are disposable.

## How we work

- State a number or a result only from tool output you read in this session. A measurement that
  returns zero or nothing has failed. Label an estimate as an estimate. Say what you did not
  verify: a subagent report, a CI run you did not watch, a platform you could not run.
- A cause is a hypothesis until a fixture fails on the broken variant and passes on the fix.
  One probe run is not evidence; run five each way.
- A test failure is a defect. Do not call it a flake, load noise or pre-existing without that proof.
- When a test or guard fails, fix the code or the fixture. Never widen the assertion to pass.
- Run the relevant checks before you commit. Builds and clippy must be warning-free; fix warnings
  rather than suppress them. Report failures and skipped checks.
- Verify audit findings against real callers and upstream guards. Review again after fixes;
  stop when no substantive findings remain.
- Commit at tested checkpoints with a `Co-Authored-By:` trailer naming the model used. Match local
  style; do not reformat unrelated code. Rust uses four spaces; frontend code uses tabs and single
  quotes. Do not run Prettier.
- `roadmap/` holds one file per open feature: decisions and remaining work only. Remove shipped
  parts when they land.

## Shared checkout

Several sessions work in one checkout of `main`. A change you did not make is someone's work.

- Create branches or worktrees only when asked. Never force-push.
- Stage by path. Never use `commit -a`, `stash`, `checkout --`, `restore` or `reset` on tracked
  files. Read a diff before you discard it.
- Stop only processes you started, by pid. Never `pkill` by name or by `-f` pattern.
- When the harness or a reviewer denies a command, do not reach the same effect by another route.
  Report the denial and wait.

## Architecture

- One binary serves the app and acts as its CLI (`goofi help`). It prints the URL without opening
  a browser; `--headless` serves only the API; `--demo` withholds the host-touching ops. The
  Electron shell in `frontend/electron` runs it with `--shell`, holds its stdin, and installs the
  update that the `update start` op requests.
- The manager owns the graph and document. Browser documents are read-only replicas, updated with
  versioned path operations (`put`, `del`). Mutations are commands with inverses and per-actor
  undo; fresh calls are strict, replay tolerates stale targets, and layout inverses use forward
  planners.
- Signal nodes schedule themselves; audio and graphics have their own clocks. Node processing
  does not run under the graph lock. Cross-engine transport is latest-wins shared memory.
- Params have one active source: constant or expression. Values use `/params/<node>`; errors use
  `/control`. Viewer demand must not change engine scheduling.
- Every frame carries engine stamps: the tick's `time`, the node's `index` and `ufreq`, its `emit`
  instant, and its `source` frames, cut where a chain meets a node's earlier run. Stamps are never
  inherited. Each frame counts in full, including a Buffer window; never infer sample overlap.
  Input clearing takes effect after a successful process call and before the next input drain.
- Pitch is Hz on the signal plane and volts per octave on the audio plane (0 V is C4). Scales
  come from `goofi_core::scale`.
- Rust nodes are `.rs` files built against an engine SDK. `goofi_build` opens every dynamic library
  and reads its entry symbols. A signal or graphics node built after the boot scan runs hosted in
  `goofi host`, over the exchange the Python subprocess tier uses; audio loads its library
  in-process. Graphics also takes `.wgsl` shaders and Python sources marked `# goofi: graphics`.
- A `.gfi` patch is an archive of its document and workspace. Recordings are native data files
  with metadata sidecars.
- `goofi_supervisor::layout` owns every path: the home (`~/.goofi`, what the user keeps), the
  runtime (tools for one version, filled by `goofi-provision`) and the session. Nothing else reads
  a path or tool from the environment. `goofi-init` provisions a development build and removes
  `target/` artifacts untouched for three days.
- A session (`goofi_supervisor::session`) owns every ephemeral resource; its lock is the one
  aliveness answer. Children, long-lived threads, iceoryx2 nodes, scratch paths and devices are
  minted through `goofi_supervisor::{child, worker, scope}` and `goofi_transport`, and listed by
  `session status`. Declare an iceoryx2 node after its ports. A child's output goes to the process
  log unless its owner wires a pipe or a file. `AppState::shutdown` is the one release order.
- `plugins/` ships plugins as source, loaded from `~/.goofi/plugins/<id>`; `sdk/README.md` is the
  plugin interface. `skills/` holds agent guides, embedded at build and seeded into a patch's
  workspace when it has none.
- The app targets one user on localhost or a trusted LAN. WebSockets have no auth; keep the
  `origin` guard (Sec-Fetch allowlist and Host check). `/dev/*` requires debug mode.
- Support touch, tablet and desktop in both orientations. Navigation must not dirty the patch.
  Keep workspace layout and cable-drag behavior unless a redesign is requested. `panelty` owns
  panel mechanics; change it upstream. UI tokens live in `:root`; panels size by container
  queries; UI primitives do not import stores.
- After a Python API change, rebuild both installed wheels. Do not canonicalize venv interpreter
  paths.

## Tests

- Test observable behavior through the public interface. All Rust tests live in `goofi-tests` as
  named situations; extend the situation that owns a surface, with a fixture that reproduces the
  failure. Use Playwright for socket integration, layout and gestures, and typecheck for Svelte.
- A test asserts the state reached, never how fast: no rate floors, latency ceilings or sleeps.
  Poll against the harness `WAIT`. No test simulates load, opens audio hardware or native windows.

```sh
cargo run -p goofi-init          # provision, then: cargo run
cargo build --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --no-fail-fast
npm --prefix frontend run check
npm --prefix frontend run test
cd tests/e2e && npm install && npx playwright install chromium && npm run e2e
```

Situations share the binary `tests/all` (`cargo test -p goofi-tests <situation>::`). Situations that
set process-wide environment have their own (`--test <situation>`).
