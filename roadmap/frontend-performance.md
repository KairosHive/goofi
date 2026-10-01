# Frontend performance: what the audit left open

## Remaining

- The one measurement that settles the idle frame-rate drop, which did not reproduce headless:
  on the owner's display, Chrome's Performance panel over 5 s on `examples/harmonic-observatory.gfi` fitted to the
  screen, once idle, once with the inspector open, once during a knob drag; read Commit /
  ProduceCanvasResource, FunctionCall, Layout and long tasks, note the DPR and whether a native
  graphics window was open. A GPU shared with the engine is the one cost the audit could not see.

## Not to be done

- Re-investigating "the reducer decodes every producer sample it drains": the subscriber is
  one-deep with safe overflow, so it drains at most one sample per wake.
- Re-investigating "per-frame `clientWidth` reads force layout after the same microtask's
  writes": Svelte's batch runs render effects before user effects, and the read sites are not
  reached by reduced or scalar frames.
