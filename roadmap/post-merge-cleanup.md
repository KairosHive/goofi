# Post-merge cleanup: what the audit of main found after #12, #13 and #14

Decided 2026-09-24 from a full audit of main at 35462dac (backend, frontend, tests and CI).
Nothing here is a redesign; each item names one defect, where it is, how it fails, and the fix.
Items are ordered by severity. Remove each item when it lands, and this file when it is empty.

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
