# Windows cleanup

Defects found in a Windows audit on 2026-09-24, repaired on 2026-10-11 as far as goofi's own
code goes. What was iceoryx2's (the handle table, the `C:\Temp` root, the DACL on its files, the
socket per wait) went with it (`data-layer.md`); what remains needs a Windows CI runner to prove.
Related: `windows-agent-quoting.md`.

## Open defects in goofi

- `session::a_patch_is_built_saved_and_opened_somewhere_else_unchanged` fails on Windows: after
  `session load`, the LFO reborn at its saved uid never reports a stage (`creating` until the
  harness gives up), with nothing in the process log. The committed tree before the 2026-10-11
  repairs fails the same way, so the cause is older. Passes on Linux CI.

## Tests and CI

- The `platform` job still runs no situation on Windows (`if: runner.os != 'Windows'` in
  `ci.yml`). On a Windows 11 machine (2026-10-11) with `--test-threads=1`, children, engines,
  headless, inspect, io, logging, plugins, python and transport passed and the run died at the
  session situation above; in parallel the situations hit the handle table. Enable them serially
  once that situation passes and a runner run is watched.
- The e2e agent specs need a POSIX shell (`tests/e2e/globalSetup.ts` pins `sh`).
