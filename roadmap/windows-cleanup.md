# Windows cleanup

Defects found in a Windows audit on 2026-09-24 and checked against the tree again on 2026-10-01.
The fix for each is still to be decided. Related:
`iceoryx2-windows-noise-and-leak.md`, `windows-agent-quoting.md`.

## Stability

- `goofi.exe` started outside `cargo run` exits with code 1 and no message: the embedded
  interpreter finds no `PYTHONHOME` (only `.cargo/config.toml` sets it).
- iceoryx2 permits 1024 handles per process (`win32_handle_translator.rs`); at about 30 nodes new
  nodes fail, and a full table panics in `Directory::new`, which crashes the `tests/all` binary.
  The `goofi-transport/src/lib.rs` comment that the limit is "far above what a patch opens" is
  wrong on Windows.
- iceoryx2 `open_with_mode` (`fcntl.rs`) leaks the `CreateFileA` handle when the table is full;
  every later open of that service fails with `HangsInCreation` until the session ends.
- Ctrl+C reaches every child: `goofi-supervisor/src/child.rs` sets a process group on unix only;
  Windows children get no group and no job object.
  Hosted nodes restart, plugin services stop without `on_stop`, Python nodes raise
  `KeyboardInterrupt`.
- Closing the console window races the shutdown: `managed_stop` (`goofi-cli/src/main.rs`) selects
  on `ctrl_close`/`ctrl_shutdown`, but tokio's handler returns at once, so the `Manager` drop that
  is the shutdown runs only within the console's own grace.
- The session base `C:\Temp\goofi-system` and iceoryx2's `C:\Temp\iceoryx2\shm` are fixed paths
  (`goofi-supervisor/src/session.rs` `system_base`, `shm_dir`);
  boot panics without a writable drive C.
- The listener socket path (`iox_root` + `shm_prefix` + service name) uses about 106 of iceoryx2's
  108 bytes and nothing checks the length; a longer base, prefix or root breaks every node.
- No process sets `SetErrorMode`, so a VST3 plugin that crashes in the scan opens an error dialog
  and the scan waits `SCAN_WAIT`; `answered()` (`goofi-audio/src/vst3/mod.rs`) reports an NTSTATUS
  exit as "exited with {code}", never as a crash.
- The audio device-list refresh (`named` in `goofi-audio/src/host.rs`) walks `available_hosts()`;
  a refresh that does not narrow with `only` loads every ASIO driver on the node's control thread.

## Process control

- `request_stop` (`goofi-supervisor/src/child.rs`) runs `taskkill /T` without `/F`, which a console
  process refuses, so a Windows child never gets a graceful ask and each stop waits the full grace
  before `force_kill`.
- `Child::stop` starts `taskkill.exe` twice, serially and synchronously, on the engine or shutdown
  thread.
- The liveness pipe is armed per `child::spawn`; a process a child starts by other means (a Python
  node's subprocess, an agent's shell) keeps running after goofi is killed, and `taskkill /T` is
  the only tree stop.

## Text encoding

- Python pipes use the ANSI code page, not UTF-8: non-ASCII request text breaks plugin
  services, non-ASCII node text shows as U+FFFD, and `goofi-init` writes a wrong `PYTHONHOME`
  for a user name with non-ASCII characters.
- `terminal_line` writes UTF-8 to a console that is not set to UTF-8; the banner's `·`, `→`,
  `✓`, `…` show as `Â·` or similar.
- After `capture_stdio`, `terminal_width` (`goofi-supervisor/src/log.rs`) asks
  `stdout().is_terminal()` on Windows where unix asks the saved terminal fd, so the startup
  progress bars never show.
- iceoryx2's `win32call!` prints each unignored Win32 error with its full 1024-byte buffer;
  each boot adds NUL-filled error records to the process log.

## Performance

- Timed waits round up to the 15.6 ms timer tick: the executor's `goofi_runtime::TICK` (10 ms)
  runs at about 64 Hz, and viewer pacing and recording waits get up to 16 ms of jitter.
- Each iceoryx2 wait is a UDP socket whose option changes scan the handle table under one
  process-wide lock; the cost grows with nodes times rate, and a `Clock` misses its target
  rate above about 200 Hz.

## Python

- The embedded interpreter gets its site-packages through `PYTHONPATH`
  (`point_embedded_python_at_its_venv` in `goofi-cli/src/main.rs`), so `.pth` files
  (`pywin32`, editable installs) are not processed.

## Tests

- The e2e agent specs need a POSIX shell (`tests/e2e/globalSetup.ts` pins `sh`).
- `children::a_hard_killed_parent_still_stops_its_child` (`goofi-tests/tests/all/children.rs`) is
  unix-only; the Windows liveness pipe has no test.
- Windows CI only boots (the setup action's `GOOFI_BOOT_ONLY=1` run); the `platform` job's
  situations are `if: runner.os != 'Windows'` because iceoryx2 cannot create a listener under the
  session's root there.
