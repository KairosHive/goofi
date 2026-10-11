# Windows cleanup

Defects found in a Windows audit on 2026-09-24, repaired on 2026-10-11 as far as goofi's own
code goes. What remains lives in iceoryx2, or needs a Windows CI runner to prove. Related:
`iceoryx2-windows-noise-and-leak.md`, `windows-agent-quoting.md`.

## In iceoryx2 (version 0.9.3), not fixable here without a fork

- `iceoryx2-pal-posix` permits 1024 handles per process (`MAX_SUPPORTED_FD_HANDLES` in
  `win32_handle_translator.rs`). A full table panics in `Directory::new` ("This should never
  happen"), and the panic ends the process as `STATUS_STACK_OVERFLOW`. Seen in the `tests/all`
  binary whenever its situations run in parallel on Windows (one process, many sessions); serial
  (`--test-threads=1`) passes. A headless goofi with nothing else open dies with
  `STATUS_STACK_OVERFLOW` while about the 50th `signal:Clock` is added (2026-10-11, twice, at 47
  and between 50 and 60); built against a copy of the crate with the table at 16384 it took 130
  (once). That exit code is what a user's goofi died with twice that day. A viewer attached to a patch of 15 hosted nodes
  logged `ServiceInCorruptedState` on two view doors, hundreds of times a minute; whether that
  is the same table is not proven.
- `open_with_mode` (`fcntl.rs`) leaks the `CreateFileA` handle when the table is full; every later
  open of that service fails with `HangsInCreation` until the session ends.
- The shared-memory and root directories are compile-time constants in
  `iceoryx2-pal-configuration` (`C:\Temp\iceoryx2\...`). goofi's session base sits beside them on
  purpose; a machine without a writable drive C cannot run iceoryx2 at all.
- `win32call!` prints each unignored Win32 error with its full 1024-byte buffer into stderr; each
  boot adds NUL-filled records to the process log (a stderr filter deadlocks the Python tier, see
  the noise file).
- Each iceoryx2 wait is a UDP socket whose option changes scan the handle table under one
  process-wide lock; the cost grows with nodes times rate.

Decision open: fork `iceoryx2-pal-posix` through `[patch.crates-io]` with the table raised and the
leak closed, or wait upstream.

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
