# Post-merge cleanup: what the audit of main found after #12, #13 and #14

Decided 2026-09-24 from a full audit of main at 35462dac (backend, frontend, tests and CI).
Nothing here is a redesign; each item names one defect, where it is, how it fails, and the fix.
Items are ordered by severity. Remove each item when it lands, and this file when it is empty.

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
