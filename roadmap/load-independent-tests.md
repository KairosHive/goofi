# Load-independent tests: what the cleanup left

The rules are in AGENTS.md. The test side was cleaned on 2026-09-25; what remains is product
behaviour that fails on a busy machine, and gaps the cleanup could not close.

## Product deadlines that judge speed

A slow but correct answer must not fail. Each of these is a fixed wall-clock ceiling that turns a
starved thread into an error. Decide per site: a liveness check (the peer is gone) or no ceiling.

- Recording: `goofi-record` `SETTLE` (3 s, the stop's settle), `AudioCapture::flush` and
  `recording_boundary` (3 s, `goofi-bridge/src/record.rs`, `goofi-audio`). `record stop` fails
  if the drain is starved past them.
- Hosted and Python subprocess tiers: `TICK_TIMEOUT` (10 s) and `COLD_START_TIMEOUT` (30 s,
  60 s; `goofi-signal/src/hosted.rs:18-19`, `goofi-python/src/subproc.rs:18,22`) kill a child that
  is alive but slow. `ask` has no halt signal, so without a ceiling a hung child would outlive its
  node; `backend-architecture.md` §4.F's event wake is the place to add one.

## Gaps

- **The workspace build is a different test binary.** `cargo test --workspace` turns on
  `goofi-python/embed` through goofi-cli's default `python` feature, while goofi-tests' own
  `embed` stays off, so `io.rs` (`cfg(not(embed))`) runs beside the in-process tier only there.
  `-p goofi-tests` and CI never build that combination. Make the two builds agree.
- **An audio ring overflow cannot be driven.** The recorder drain does not empty the audio ring;
  the audio control half does, on its own tick, and no public lever freezes it. The recording
  situation overflows the record service instead, so block renumbering in the control half has
  no test.
- **One unexplained e2e stall.** Once in a loaded full run (2026-09-25), a newly added hosted
  Python node (`pads` in `midi-learn.spec.ts`) ran at 30 updates/s while no frame reached its
  viewers for 60 s. Not reproduced in 12 later runs; the fleet then still shared one home.
