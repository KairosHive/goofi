# Frontend performance: what the audit left open

Audit of 2026-09-16, run against the owner's report that the browser's frame rate falls as a
patch grows. The two structural answers have entries of their own: `viewer-render-surface.md` and
`data-plane-bandwidth.md`; the op path is `backend-architecture.md` §3. Headless Chromium with software GL and a
debug backend measured scaling laws, not absolute frame rates. This file is what remains, and what
must not be looked at again.

## Remaining

- **The dev profile** — `Cargo.toml`: `[profile.dev] opt-level = 1` and
  `[profile.dev.package."*"] opt-level = 2` give 6–9× on the serve path and drop the status
  drain from 15–20 % of a core to ~2 %. It was built and REVERTED: with either half alone, two
  engine situations fail on a clean tree, so they are speed-dependent races, not a miscompile,
  and they most likely exist in `--release` today.
  `audio.rs one_signal_speaks_through_another_band_by_band` fails deterministically (the
  BandFilter reads its params before the edit that set them is applied to the running node), and
  `graphics::shaders_render_on_the_gpu` is flaky (state buffers are remade under the Count node:
  `State::ensure_out` remakes on a size it should keep). Fix both races, then land the profile with
  them.
- **A pinch e2e** — `touch.spec.ts`: a pinch that crosses the 0.3 zoom threshold
  (`ViewerFeed.svelte`), asserting `/data` goes quiet and resumes. Needs a CDP pinch helper;
  `window.goofi.query.arrivalRate` is the hook that observes `/data`.
- **The one measurement that settles the idle drop** — it did not reproduce headless. On the
  owner's display: Chrome's Performance panel over 5 s on `thought-sphere` fitted to the screen,
  once idle, once with the inspector open, once during a knob drag; read Commit /
  ProduceCanvasResource, FunctionCall, Layout and long tasks, note the DPR and whether a native
  graphics window was open. A GPU shared with the engine is the one cost the audit could not see.
- `FitToGraph.svelte` fits the camera to whatever graph the page loads with; a wholesale reset
  can arm a fit against nodes about to be replaced. Minor; it made two measurement runs
  incomparable.

## Refuted — do not re-investigate

- "The reducer decodes every producer sample it drains": the subscriber is one-deep with safe
  overflow, so it drains at most one sample per wake.
- "Per-frame `clientWidth` reads force layout after the same microtask's writes": Svelte's batch
  runs render effects before user effects, and the read sites are not reached by envelope or
  scalar frames.
