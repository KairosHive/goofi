# Windows cleanup

Defects found in a Windows audit on 2026-09-24. The fix for each is still to be decided. Related:
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
- Ctrl+C reaches every child: `goofi-core/src/child.rs` gives Windows children no process group.
  Hosted nodes restart, plugin services stop without `on_stop`, Python nodes raise
  `KeyboardInterrupt`.
- `std::process::exit` (`exit(130)`, `exit(101)`, test binaries) does not run the `atexit`
  release `goofi-transport` registers; the session directory stays until the next boot.
  `transport::a_process_that_exits_without_releasing_leaves_no_record` fails.
- Closing the console window does not run `AppState::shutdown`: the `ctrl_close` handler returns
  at once and Windows ends the process.
- `session::hold` takes `alive.lock` only after the rename (Windows refuses to move a folder
  with an open file); a sweep in that window removes the directory and `hold` panics. The
  rename also fails while another process holds a file in the directory open.
- The session base `C:\Temp\goofi-system` and iceoryx2's `C:\Temp\iceoryx2` are fixed paths;
  boot panics without a writable drive C.
- The listener socket path uses 106 of iceoryx2's 108 bytes; a longer base, prefix or root
  breaks every node.
- No process sets `SetErrorMode`, so a VST3 plugin that crashes in the scan opens an error dialog
  and the scan waits `SCAN_WAIT`; `answered()` does not report the crash, because the exit is an
  NTSTATUS code, not a signal.
- The audio device-list refresh (`goofi-audio/src/host.rs`) enumerates every host, so each
  refresh opens all ASIO drivers on the node's control thread.

## Process control

- `request_stop` runs `taskkill /T` without `/F`, which never stops a console process; each
  stop waits the full grace.
- Each child stop starts `taskkill.exe` twice, serially, on the engine or shutdown thread.
- The liveness pipe stops direct children only; their own children keep running after goofi
  is killed.

## Text encoding

- Python pipes use the ANSI code page, not UTF-8: non-ASCII request text breaks plugin
  services, non-ASCII node text shows as U+FFFD, and `goofi-init` writes a wrong `PYTHONHOME`
  for a user name with non-ASCII characters.
- `terminal_line` writes UTF-8 to a console that is not set to UTF-8; the banner's `·`, `→`,
  `✓`, `…` show as `Â·` or similar.
- After `capture_stdio`, `stdout().is_terminal()` is false, so the startup progress bars never
  show.
- iceoryx2's `win32call!` prints each unignored Win32 error with its full 1024-byte buffer;
  each boot adds NUL-filled error records to the process log.

## Performance

- The hosted and Python subprocess tiers poll (`sleep(500 µs)` in `hosted.rs` and
  `goofi-pymod/src/serve.rs`; `sleep(1 ms)` in `goofi-transport/src/lib.rs`).
- Timed waits round up to the 15.6 ms timer tick: the control `TICK` runs at about 64 Hz, and
  viewer pacing and recording waits get up to 16 ms of jitter.
- Each iceoryx2 wait is a UDP socket whose option changes scan the handle table under one
  process-wide lock; the cost grows with nodes times rate, and a `signal:Clock` misses its
  target rate above about 200 Hz.

## Python

- The embedded interpreter gets its site-packages through `PYTHONPATH`, so `.pth` files
  (`pywin32`, editable installs) are not processed.

## Tests

- The e2e agent specs need a POSIX shell (`tests/e2e/globalSetup.ts` pins `sh`).
- The child-stops-when-goofi-dies test (`goofi-tests` `children.rs`) is unix-only; the Windows
  liveness pipe has no test.
- Windows CI only boots `--headless --list-nodes`; it runs no situation.
