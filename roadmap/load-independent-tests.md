# Tests that load cannot flip

Decided 2026-09-24. Two rules for every test:

- **No test flakes under load.** An assertion states what was reached, never how fast. Forbidden:
  lower bounds on a count in a window, rate or pace thresholds, ratios of wall-clock durations,
  latency upper bounds, sleep-then-assert, and budgets tight enough for a busy runner to miss. An
  upper bound on a count, and a negative that load can only make more likely, are fine.
- **No test simulates load.** No filler producers, busy loops or stress threads to make a
  machine slow, and no "under load" scenario.

A poll-until against the harness `WAIT` (180 s, paid only by a failure) is the form. Where a
test needs a node to be "inside" something, the fixture holds a latch the test opens, not a sleep.

## Definite violations

1. `backend/goofi-tests/tests/all/running.rs:159` `a_producer_paces_itself_…`:
   `fast >= paceable * 85 / 100`, a rate floor. Keep the upper bounds; prove the new cap took
   effect by polling the producer's index stamp past a target, not by counting in a window.
2. `python.rs:696` `several_python_nodes_run_at_once_…`: `four < one * 2`, a duration ratio.
   Each Sleeper enters a shared barrier in `process`; four released together is concurrency,
   and a serial run deadlocks into the `WAIT` budget.
3. `python.rs:338-342` `a_nodes_own_python_thread_runs_while_the_child_is_idle`: more than 10
   ticks in a fixed 1.5 s. Poll until the counter passes `first + 10`.
4. `running.rs:480-551` `a_busy_node_never_holds_up_the_control_plane_…`: a 60 ms sleep as sync,
   five latency ceilings (100 ms to 5 s), and `stage == "creating"` resting on a 700 ms build
   outlasting the ops. Fixtures wait on latches the test releases; assert the ops return while
   the latches are shut, the stage while the build latch is shut, and that `shutdown` returns
   with the busy node's latch still closed.
5. `recording.rs:581-608` `arming_survives_a_rewire_…`: `took < 1_000_000` µs, and 5-8 s custom
   budgets. The finalize is already held by `release`: assert the op and the drain completed
   while it is held, with `WAIT` budgets.
6. `recording.rs:637-645, 919-927` (same test): five `_TestConst` at 100 kHz exist to load the
   drain so the audio ring overflows. Freeze the drain as the test does at 700-701, drive past
   the ring's second, release, and assert `dropped == driven - capacity`. Delete the fillers.
7. `recording.rs:657-664`: the graph lock held for a fixed 300 ms, expecting more than 64 frames.
   While holding it, poll the producer's index until it is 64 past the last drained one.
8. `recording.rs:700-721`: sleeps of 200, 80 and 50 ms as sync, and `landed >= frozen + 32`.
   Poll until the producer index is 32 past `frozen`; detect the parked disarm by a flag, not a
   sleep.
9. `backend/goofi-tests/tests/audio.rs:826-837` (Trap): a 2 ms busy loop per block and "not
   overran", which preemption flips; the `elapsed() > 100 ms` stall signal also passes on a slow
   machine alone. Give the audio watchdog a test clock, or let the Trap report a synthetic cost.
10. `tests/e2e/tests/socket.spec.ts:911-915`: a 1200 ms sleep, then `fps > 15`. Keep the upper
    bound; prove the stream live by polling for a new frame index.
11. `tests/e2e/tests/midi-learn.spec.ts:50, 81, 102, 119`: `waitForTimeout(500)` for the first
    frame. Poll `frameSummary` until it is there.
12. `tests/e2e/lib/app.ts:73-75, 94-99` `expectPristineWorkspace`, on every spec: `docSynced`
    within 2 s. Use the readiness budget `appReady` uses.
13. `tests/e2e/lib/invariants.ts:166-169` `settled()`: finite animations raced against 1 s, then
    swept. Poll the sweep until it is clean instead.

## Tight budgets and sleeps used as sync

- `src/probe.rs:14`: `WAIT = 5 s` for `expect_frame` (engines.rs, plugins.rs), polled under the
  graph lock. Use the harness `WAIT`.
- `transport.rs` (`MS200` in four situations): 200 ms for a ring or a status. Use `WAIT`; at 140
  gather wakes until both ids are seen.
- `running.rs:296-441` `many_viewers_…`: 5 s `holds_within` rounds and `sleep(250 ms)` for "the last
  emit lands". `WAIT` budgets; read `cached` once `reductions` stops moving.
- `running.rs:459-468` `a_viewer_that_stops_answering_…`: the slow peer's 300 ms ticker must keep
  answering inside a 3 s pong deadline under load. Answer pings from a task of its own, so only
  its reads are slow.
- `running.rs:136`, `python.rs:770`: more than 8 frames in 400 ms. Poll the index stamp instead.
- `graphics.rs:927, 956`: 10 s budgets on lavapipe. Use `WAIT`.
- `recording.rs:748-762` `settled`: "unchanged across 60 ms" is taken for a drained ring. Compare
  against the sample count `drive` knows.
- `recording.rs:980-1010` `an_armed_signal_slot_loses_no_tick_…`: 200 Hz into a 64-frame service
  drops a tick when the drain stalls 320 ms. Assert gaps equal the reported drop count, or
  record at a rate the service holds for longer than `WAIT` allows a stall.
- `recording.rs:895-899`: the engine began within 0.5 s of patch start. Compare against the
  engine's own start stamp.
- `recording.rs:256, 572, 585, 594, 963, 969`, `agent.rs:296, 332, 547`, `logging.rs:67`: 3-10 s
  custom budgets. Use `WAIT`.
- `children.rs:130, 153`: `started.elapsed() < 10 s`. The `!alive(pid)` check says it; drop these.
- `signals.rs:449-452`: two param messages to two nodes, then `stays`. Poll until `over` shows
  `dwell == 5` before moving `level`.
- `editing.rs:160`: `stays(value == 0.5)` while a producer still runs; a stale pick landing late
  fails it. Poll for a frame newer than the clear first.
- `goofi-bridge/src/lib.rs:2011`: an outer 2 s bound on a 50 ms send. Pause the tokio clock.
- `tests/e2e/tests/integrity.spec.ts:116-127, 157`: a 2 s blink watch, and a 1500 ms sleep
  before `expectIntact`. Poll for the painted spectrum.
- `tests/e2e/tests/touch.spec.ts:24, 67`, `lib/inspector.ts:223`: timed holds before a lift.
  Poll until the long-press UI shows, then lift.
- `tests/e2e/tests/socket.spec.ts:888`: a 400 ms sleep before a poll; delete it.
- `audio_priority.rs:48-51`: a 40 ms spin at RT priority. It checks that RLIMIT_RTTIME does not
  kill the process, not speed; keep it unless the rule is read to the letter.

## Seen on runners

- Locally, `cargo test --workspace --no-fail-fast` at 8 threads times out 17 hosted and Python
  situations (`python.rs`, `nodes.rs`, `plugins.rs`, `signals.rs`) that pass at 4. Under the
  rules this is a defect to find, not a thread count to set: trace what starves at 8.
- On CI, `--test audio` `a_patch_sounds_under_the_external_clock` missed its 180 s "reaches the
  octave" budget once (run 35939068722), 25 s alone.
