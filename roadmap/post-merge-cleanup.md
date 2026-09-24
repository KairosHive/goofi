# Post-merge cleanup: what the audit of main found after #12, #13 and #14

Decided 2026-09-24 from a full audit of main at 35462dac (backend, frontend, tests and CI).
Nothing here is a redesign; each item names one defect, where it is, how it fails, and the fix.
Items are ordered by severity. Remove each item when it lands, and this file when it is empty.

## 1. The page's paint cap beats against the manager's serve rate

`frontend/src/lib/api/frames.ts` (`requestFlush`) with `frontend/src/lib/api/paintCap.ts`. The
reducer already serves one slot at most every `VIEWER_INTERVAL` (33.3 ms, phase-locked from the
previous target in `backend/goofi-bridge/src/reducer.rs`). The page applies the same 33.3 ms gate
a second time, anchored on the ACTUAL start of the previous flush, then waits a `setTimeout` and
then a `requestAnimationFrame`. A flush that starts just after a vsync ends its cooldown just
after the second vsync, so the paint lands on the third: every interval rounds up to 50 ms on a
60 Hz display (41.7 ms on 120 Hz). A 30 Hz stream paints at about 20 Hz, one frame in three
overwrites `pending` and is charged to the stream's drop meter, and the HUD reads a lower number
than the arrival rate. This is the stutter seen on the EEG buffer. The client cap is a second
owner of one rate; delete `paintCap.ts`, the timer in `requestFlush`, `MAX_VIEWER_FPS` on the
page, and paint whatever is pending on the next animation frame. Keep the rate the manager owns,
at 30. The cadence design from the same session: each socket may declare its display rate on
subscribe and a slot's serve interval becomes the fastest rate among its connections, so a node
emitting slower than every display is served at its own rate and one emitting faster is coalesced
to the display's.

## 2. A snapshot answers a stale full-resolution frame

`backend/goofi-bridge/src/reducer.rs`: `SlotReducers::latest` returns `full` before `latest`.
`full` is set when a non-reduced frame decodes and cleared only on the loop's next wake, and only
when a snapshot was asked while the demand is narrow. Scenario: a viewer opens on a graphics node;
the first frames arrive full f32 before any demand and set `full`; the box lands and every later
frame is `|u1` ready, forwarded, `latest` cleared, `full` untouched. Minutes later `node snapshot`
returns that first frame. Fix: clear `full` the moment the loop first pushes a narrow demand, or
answer `None` with the existing "ask again" reason while the demand is narrow.

## 3. The drain worker pulses "settled" on every wake

`backend/goofi-bridge/src/lib.rs` (the drain thread): `drain_status()` settles, then
`state.settled_now()` runs unconditionally, whatever `applied` says. A node whose param follows a
moving expression reports `ParamValues` on every run in which a value changed, so a modulated
patch wakes the drain at the modulation rate. Each pulse rings every reducer (`poke_all`), each
reducer re-reads its address under the graph lock, and every `/data` socket re-takes the graph
lock for `stream_behind`. With R reducers and V viewers at 30 Hz that is about 30·(R+V) graph-lock
acquisitions a second for nothing that moved. Fix: pulse only when the settle applied something.

## 5. Range labels are hover-only on hybrid devices

`frontend/src/lib/viewers/ViewerFeed.svelte`: the three range labels rest at `opacity: 0`, show
on `:hover`, and rest visible only under `(hover: none) and (pointer: coarse)`. A touch laptop or a
pen tablet with a mouse matches `hover: hover`, so a finger never reveals them, and there is no
`:focus-within` rule, so a keyboard user never sees them. AGENTS.md requires touch and tablet
support. Fix: add `:focus-within`, and rest the labels visible in the docked viewer panel or above
a card-width threshold; extend `tests/e2e/tests/viewer.spec.ts` with a touch project check.

## 6. The stuck watchdog counts from session start

`backend/goofi-tests/src/lib.rs`: any owner alive longer than `STUCK` (600 s) aborts the whole
test binary, measured from `Goofi::new`, not from the last progress. `WAIT` is 180 s per `until`,
so a green situation with four slow waits on a loaded runner can pass 600 s and lose every result
in its file. Fix: count from the last `until` that made progress, or scale `STUCK` to N×`WAIT`.

## 7. Budgeted waits on loaded runners

Two shapes, both seen on 2026-09-23:

- Locally, `cargo test --workspace --no-fail-fast` at the machine's 8 threads times out the
  hosted and Python node tier (17 situations in `python.rs`, `nodes.rs`, `plugins.rs`,
  `signals.rs`), all green at `--test-threads=4`. The waits are budgets (`WAIT`), not races. Two
  assertions are load-sensitive ratios: `python.rs` "four Sleepers under twice one" and
  `running.rs` 200 Hz pace at 85 %. AGENTS.md documents the command that fails; either document
  the thread count or set it in `.cargo/config.toml` for the test profile.
- On CI, `--test audio` `a_patch_sounds_under_the_external_clock` took 192 s and its "reaches the
  octave" step missed the 180 s budget once (run 35939068722); it passes in 25 s alone. The
  unit-test step runs the situations with a binary of their own at the default thread count.

## 8. Fixed waits and a bundle dependency in the e2e suite

`tests/e2e/tests/viewer.spec.ts` has two `waitForTimeout(300)` before pixel assertions (after a
collapse and after a pan); convert both to `expect.poll` like the sibling steps.
`tests/e2e/tests/integrity.spec.ts` adds `graphics:TimeWarpFbm`, which lives only in
`node-bundles/inception`; the gallery step breaks when that bundle leaves the repo. Use a shipped
graphics node or a test-authored shader. `tests/zz_pt.spec.ts` is a tracked debug spec outside the
test dir whose import does not resolve; delete it.

## 9. Small backend defects

- `backend/goofi-graph/src/lib.rs` `set_view_watch`: inserts into `watched` before the leaf
  lookup and reports a change even when no leaf exists. The reducer checks `manifest(uid)` under
  one lock and calls `set_view_watch` under a later one; a node removed between the two leaves a
  `(uid, slot)` in `watched` that `remove_node` already ran past. Return `false` without a leaf.
- `backend/goofi-bridge/src/reducer.rs` `spawn_reducer`: `#[allow(clippy::too_many_arguments)]`
  was added instead of passing the `SlotReducer` handles as one value. AGENTS.md: fix, not
  suppress.
- `backend/goofi-codec/src/lib.rs` `content_hash`: hashes `Meta` in insertion order, so two
  frames that say the same thing with keys set in another order re-send in full instead of as
  stamps. Sort the carried keys before hashing.
- `backend/goofi-bridge/src/lib.rs` `send`: a `Dropped` send cancelled during flush stays
  buffered in the sink; the next write flushes it and the `reoffer` sends the frame again, one
  doubled frame per stall. Pre-existing; fix when the send path is next touched.

## 10. Coverage gaps the three PRs left

- The freeze below zoom 0.3 / 0.34 in `ViewerFeed.svelte` has no test; `viewer.spec.ts` zooms in
  only. Add a wheel-out past 0.3 asserting the worker unsubscribes while the pixels stay, then a
  zoom back above 0.34 that resubscribes.
- The worker's post-on-arrival dispatch (`dataWorker.ts`: decode, stamps posted as `{stamps}`,
  the corrupt-frame `catch`) has no test since `demandTicker.test.ts` went; `frames.test.ts` cites
  a `data.test.ts` idiom that does not exist.
- `frames.ts` `setStampsSink`: the `pending` branch (a stamps message landing between a frame's
  arrival and its flush) is untested; only restamping `current` is.
- Reducer: a spec change or a leaver forcing a full resend of an unchanged frame, and the
  `SendOutcome::Dropped → reoffer` path, have no situation. Scenario: a spec change on a held
  Constant returns stamps only and the viewer keeps the old reduction.

## 11. Housekeeping

- `frontend/src/lib/viewers/capacity.ts`: two consecutive doc comments on `MAX_ROWS`; the first
  is the old one.
- `frontend/src/lib/viewers/TrajectoryViewer.svelte`: a comment explains "not uPlot".
- `frontend/src/lib/viewers/decimate.test.ts` mentions a hit test that no longer exists; `frontend/src/lib/api/frames.test.ts` cites `viewer-fps-cap.spec.ts`, which
  does not exist; `frames.ts` says a delivery's draws run inside the budget, which holds for
  component viewers only (a glance push draws in the surface's own animation frame).
- `ViewerFeed.svelte` holds `labels` and `labelKey` for one fact, and writes `frame` on every
  delivery for line and image kinds only so `ViewerSurface` can show its fallback text while the
  plot already holds the CPU copy; a `renderable` flag would do and would spare three deriveds
  per frame under `flushSync`.
- `frontend/src/lib/viewers/capacity.test.ts` pins the literal spec objects it transcribes from
  `capacity.ts`. Keep one.
- Comment runs over two lines in the new code: `frames.ts` (stamps fold) and eleven in
  `reducer.rs`.
- `.github`: the `goofi-build` cache is saved under a key that `actions/cache/save` never
  overwrites, so it refreshes only when `Cargo.lock` changes; the Playwright cache key uses
  `package.json`, not the lockfile.
