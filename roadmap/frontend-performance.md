# Frontend performance: what the audit left open

Audit of 2026-09-16, run against the owner's report that the browser's frame rate falls as a
patch grows. Six static reviews and two live measurements produced 44 verified findings; the
fixes that fit the current design landed on 2026-09-18 and were reworked on 2026-09-22 to the
shape they should have had (the document replica as reactive state with one live view per node,
so a patch wakes the readers of the leaves it names and nothing is diffed; the write path off the
graph lock; the follower coalesced and an expression's compiled handle kept across rebinds; the
reducer woken by the producer's doorbell and never clocked; the inspector's blur and per-frame
layout removed; the viewer chrome cuts; zoom-aware viewer demand; one control message per period;
table reduction and the undeclared element budget). The two structural answers have entries of
their own: `viewer-render-surface.md` and `data-plane-bandwidth.md`. This file is what remains,
and what must not be looked at again.

## What was measured, for the record

Headless Chromium with software GL, no hardware, a DEBUG backend; trust the scaling laws, not
the absolute frame rates.

- Idle frame rate did not fall with node count at DPR 1. Main-thread busy share grows with
  VISIBLE viewers: ~0.35–0.4 % per uPlot viewer at DPR 1, ~0.8 ms per redraw at DPR 2, which puts
  saturation near 20–25 visible charts on a HiDPI display. Owned by the render surface.
- Every document patch cost ~0.2–0.25 ms per node before the patch-scoped sync; a knob emitted a
  patch per pointer event. The backend held the graph lock 9–65 ms per op in the debug build and
  re-seeded every tab when it fell behind.
- The inspector cost 3–15 ms per frame from `backdrop-filter` and a per-frame layout.
- `/data` dominates whenever anything is visible; `doc_patch` is 0/s idle; layout and style are
  not a cost.

## Remaining

- **Ops that do not block** — every op is the standard interaction, so only a write should hold
  the graph, every op must stay atomic, and a slow op must not park a socket's event drain. The
  first attempt (a per-socket queue with the op's events held behind its reply) was taken out:
  a queue beside the op path is not the design. To be designed whole, with the op table.
- **Continuous-motion edits** — a knob, a slider or a number field under a pointer emits an op
  per event today. A rate limiter in the control was built and taken out: the edit path for a
  gesture (what is sent, when, and what the control shows meanwhile) is one design, to be made
  after the op path above.
- **The dev profile** — `Cargo.toml`: `[profile.dev] opt-level = 1` and
  `[profile.dev.package."*"] opt-level = 2` give 6–9× on the serve path and drop the status
  drain from 15–20 % of a core to ~2 %. It was built and REVERTED: with either half alone, two
  engine situations fail on a clean tree, so they are speed-dependent races, not a miscompile,
  and they most likely exist in `--release` today. `tests/audio.rs
  one_signal_speaks_through_another_band_by_band` fails deterministically (the harmonic layout
  does not attenuate: the BandFilter reads its params before the edit that set them is applied
  to the running node), and `graphics::shaders_render_on_the_gpu` is flaky (state buffers are
  remade under the Count node: `State::ensure_out` remakes on a size it should keep; the shipped
  loop hid it by reading a stale frame). Fix both races, then land the profile with them.
- **A pinch e2e** — `touch.spec.ts`: a pinch that crosses the 0.3 zoom threshold, asserting
  `/data` goes quiet and resumes. Needs a CDP pinch helper and a way to observe `/data` from the
  test (`window.goofi.query.arrivalRate` is the existing hook).
- **The one measurement that settles the idle drop** — it did not reproduce headless. On the
  owner's display: Chrome's Performance panel over 5 s on `thought-sphere` fitted to the screen,
  once idle, once with the inspector open, once during a knob drag; read Commit /
  ProduceCanvasResource, FunctionCall, Layout and long tasks, note the DPR and whether a native
  graphics window was open. A GPU shared with the engine is the one cost the audit could not see.
- **Data worker on arrival** and the `/data` levers: `data-plane-bandwidth.md`.
- `FitToGraph.svelte` fits the camera to whatever graph the page loads with; a wholesale reset
  can arm a fit against nodes about to be replaced. Minor; it made two measurement runs
  incomparable.

## Refuted — do not re-investigate

- "The reducer decodes every producer sample it drains": the subscriber is one-deep with safe
  overflow, so it drains at most one sample per wake.
- "Per-frame `clientWidth` reads force layout after the same microtask's writes": Svelte's batch
  runs render effects before user effects, and the read sites are not reached by envelope or
  scalar frames.
- "uPlot's cursor-point divs are compositor layers per series": they carry `display: none` until
  hovered.
