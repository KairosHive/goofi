# goofi

goofi is a real-time node-based platform for biosignals, audio, and graphics. Users build
patches in a browser. The backend is Rust; the frontend is SvelteKit.

Use ASD-STE100 Simplified Technical English. Read the code for implementation details.

## Guiding principles

1. **One capability interface.** UI, CLI, MCP, scripts, and tests use the same op vocabulary.
   Device and client differences belong in presentation, not separate behavior paths.
2. **One state owner.** Derive values from their source or project them one way. Avoid mirrored
   state, duplicate counters, and caches that must be kept in sync manually.
3. **Decide from settled state.** Apply a batch before scheduling work, broadcasting changes,
   or changing resource ownership. Intermediate mutations must not cause independent decisions.
4. **Keep the smallest coherent design.** Delete obsolete paths and share rules that must agree.
   Prefer clear code to explanation; comments explain exceptions, and docstrings state purpose.
5. **Fix the cause.** Trace callers and boundaries before changing a subsystem. Use types and
   shared schemas to prevent invalid states; report real boundary failures without panics.
   A structural refactor is appropriate when it removes a class of errors.
6. **Test observable behavior.** Exercise real sessions through the public interface. Extend
   the situation that would catch a regression, with a fixture that can reproduce the failure.
   Use browser tests for socket integration, layout integrity, and gestures.

## Pre-launch policy

goofi has not launched and has no production users or production data. Revisit this policy
before the first production deployment.

- Optimize for the smallest coherent design that represents the product today.
- Remove obsolete code, schemas, APIs, configuration, aliases, and transitional paths directly.
- Do not add backward-compatibility shims, legacy aliases, dual-read or dual-write paths, or
  data-preserving backfills unless the user explicitly asks for them.
- Internal interfaces are not public compatibility contracts. Update their callers and tests
  atomically when they change.
- Development and test data are disposable. Prefer recreating those databases over complicating
  the product to preserve local data.
- Treat migration history as a replaceable development baseline, but keep the checked-in
  migration chain and setup workflow coherent. Do not rewrite an already-applied migration
  without also resetting affected development and test databases.
- Preserve database invariants, transactional safety, migration idempotence, and deterministic
  setup. These are correctness properties, not backward-compatibility requirements.
- Consolidate the migration baseline only as an explicit, coordinated change rather than as
  incidental work in a feature branch.

## How we work

- Keep changes focused and preserve other work in the shared checkout. Match local style;
  do not reformat unrelated code.
- Run the checks relevant to the change. Builds and clippy must be warning-free; fix warnings
  rather than suppress them. Report failures and skipped checks explicitly.
- Verify audit findings against real callers and upstream guards. Review again after fixes;
  stop when no substantive findings remain.
- Commit at tested checkpoints with a `Co-Authored-By:` trailer naming the model used.

## Quick facts

- Work on `main`. Create branches or worktrees only when asked; never force-push.
- `roadmap/` is the committed backlog: one file per open feature, containing decisions and
  remaining work. Remove completed entries.
- Rust uses four spaces; frontend code uses tabs and single quotes. Do not run Prettier.
- The version is `[workspace.package].version`; the toolchain is in `rust-toolchain.toml`.
- One binary serves the app and acts as its CLI. It prints the URL without opening a browser;
  `--headless` serves only the API. `goofi help` lists commands.
- The manager owns the graph and document. Browser documents are read-only replicas, updated
  with versioned JSON merge patches. Document leaves cannot be null.
- Mutations are commands with inverses and session-specific undo. Fresh calls are strict;
  replay tolerates stale targets. Layout inverses use forward planners.
- Signal nodes schedule themselves; audio and graphics have their own clocks. Node processing
  does not run under the graph lock. Cross-engine transport is latest-wins shared memory.
- Rust nodes are `.rs` files built against an engine SDK; graphics nodes are `.wgsl` files.
  Python nodes use the shared marshalling interface in both automatic execution tiers.
- Each frame counts in full, including a Buffer window. Never infer sample overlap. Resample
  handles independent windows; Epoch captures supplied windows. Input clearing takes effect
  after a successful process call and before the next input drain.
- Params have one active source: constant, expression, or reference. Values use `/params/<node>`;
  errors use `/control`. Viewer demand must not change engine scheduling.
- A `.gfi` patch is an archive of its document and workspace. Recordings use native data files
  with metadata sidecars. Shared-memory and wire-format changes require matching consumers.
- The app targets a single user on localhost or a trusted LAN. WebSocket endpoints have no auth;
  retain Origin/Host checks. `/dev/*` requires debug mode.
- Support touch, tablet, and desktop in both orientations. Navigation must not dirty the patch.
  Preserve workspace layout and cable-drag behavior unless a redesign is requested.
- `panelty` owns panel mechanics; change that dependency upstream. Shared UI tokens live in
  `:root`; use container queries for panel sizing. UI primitives must not import stores.
- All Rust tests belong in `goofi-tests` and use public APIs. Prefer named sessions over isolated
  assertions. Check Svelte changes with typecheck and a relevant Playwright session.
- Tests must not open audio hardware or native windows. Use the test clocks and hosts.
- Rebuild both installed Python wheels after changing the Python API. Do not canonicalize venv
  interpreter paths; use the paths provided by setup.
- Reclaim shared memory through iceoryx2; never script deletion of `/dev/shm/iox2_*`. Declare an
  iceoryx2 node after its ports so the ports are dropped first.

## Run and test

With Rust, `uv`, and `npm` available, run from the repository root:

```sh
cargo run -p goofi-init
cargo run
```

Checks from the repository root:

```sh
cargo build --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --no-fail-fast
cargo test -p goofi-tests --features embed
npm --prefix frontend run check
npm --prefix frontend run test
```

For a focused Rust session, use `cargo test -p goofi-tests --test <situation>`.
For browser tests:

```sh
cd tests/e2e
npm install
npx playwright install chromium
npm run e2e
```
