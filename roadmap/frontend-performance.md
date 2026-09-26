# Frontend performance: what the audit left open

## Remaining

- Land the dev profile: `[profile.dev] opt-level = 1` and `[profile.dev.package."*"]
  opt-level = 2` in `Cargo.toml`. It was reverted because two engine situations failed under it,
  so they are speed-dependent races that most likely exist in `--release` today:
  `graphics::shaders_render_on_the_gpu` flakes (`State::ensure_out` remakes the state buffers on
  a size it should keep) and `audio::one_signal_speaks_through_another_band_by_band` read
  BandFilter's params before the edit landed (its waits changed in 78262aee; re-check). Fix the
  races, then land the profile with them.
- A pinch e2e in `touch.spec.ts`: a pinch that crosses the 0.3 zoom threshold
  (`ViewerFeed.svelte`), asserting `/data` goes quiet and resumes. Needs a CDP pinch helper;
  `window.goofi.query.arrivalRate` observes `/data`.
- The one measurement that settles the idle frame-rate drop, which did not reproduce headless:
  on the owner's display, Chrome's Performance panel over 5 s on `thought-sphere` fitted to the
  screen, once idle, once with the inspector open, once during a knob drag; read Commit /
  ProduceCanvasResource, FunctionCall, Layout and long tasks, note the DPR and whether a native
  graphics window was open. A GPU shared with the engine is the one cost the audit could not see.
- `FitToGraph.svelte`: a wholesale reset can arm a fit against nodes about to be replaced.
  Minor; it made two measurement runs incomparable.

## Not to be done

- Re-investigating "the reducer decodes every producer sample it drains": the subscriber is
  one-deep with safe overflow, so it drains at most one sample per wake.
- Re-investigating "per-frame `clientWidth` reads force layout after the same microtask's
  writes": Svelte's batch runs render effects before user effects, and the read sites are not
  reached by envelope or scalar frames.
