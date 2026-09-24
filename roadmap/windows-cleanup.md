# Windows cleanup

Issues found in a Windows audit on 2026-09-24 (Windows 11, iceoryx2 0.9.3, Rust 1.97). Each entry
states the defect only; the fix for each is still to be decided. Related entries:
`iceoryx2-windows-noise-and-leak.md` and `windows-agent-quoting.md`.

## Stability

- **`goofi.exe` exits silently when it is not started through `cargo run`.** `goofi-cli`'s
  `build.rs` copies `python3*.dll` beside the executable, and from there the interpreter cannot
  find its home. Only `.cargo/config.toml` sets `PYTHONHOME`, and only for cargo commands. A
  direct start, or a second instance, stops with exit code 1 after "Preparing parameter
  expressions", with no message, because the embedded interpreter cannot initialize.
- **iceoryx2 has a limit of 1024 handles per process.** `iceoryx2-pal-posix` keeps all files,
  shared-memory segments, sockets and directory streams in one fixed table
  (`win32_handle_translator.rs`, `MAX_SUPPORTED_FD_HANDLES`). A node uses approximately 26 entries.
  At approximately 30 nodes the table is full, and new nodes fail with `ResourceCreationFailed`,
  `InternalFailure` or `HangsInCreation`. Reproduced: in one batch, 40 `signal:LFO` nodes were
  added and 16 of 41 nodes failed. When the table is full, `opendir` stops with a fatal panic in
  `Directory::new` (`directory.rs:253`) and ends the process. This panic makes the
  `goofi-tests --test all` binary crash. `goofi-transport/src/lib.rs` says that this limit is
  "far above what a patch opens", which is not true on Windows.
- **iceoryx2 leaks a file handle when the handle table is full.** `open_with_mode`
  (`fcntl.rs`) calls `CreateFileA` before it adds the handle to the table, and it does not close
  the handle when the add fails. The file stays on disk without its final permissions. Each
  subsequent open of that service waits for `creation_timeout`, then fails with
  `HangsInCreation` until the session ends.
- **Ctrl+C goes to all child processes.** Children share goofi's console, and they do not get
  their own process group (`goofi-core/src/child.rs`; unix uses `process_group(0)`). Before the
  orderly shutdown starts:
  - hosted node children stop, and the engine starts them again;
  - plugin services stop without `on_stop`;
  - Python nodes show `KeyboardInterrupt` errors.
- **`std::process::exit` does not release the session.** On Windows, `exit` calls
  `ExitProcess`, which does not run the `atexit` handler that `goofi-transport` registers. The
  session directory stays until the next boot removes it. This affects the second Ctrl+C
  (`exit(130)`), a panic in `serve` (`exit(101)`) and test binaries. The situation
  `transport::a_process_that_exits_without_releasing_leaves_no_record` fails because of this.
- **Closing the console window does not run the orderly shutdown.** The console handler returns
  immediately, and Windows then ends the process. `AppState::shutdown` does not run: recordings
  are not finalized, plugins do not get `on_stop`, and the session is not released.
- **A session can fail to start, or be removed by another process.**
  - `session.rs` takes the lock on `alive.lock` only after the directory is renamed into place.
    During that interval, a sweep in another goofi process can find the directory unlocked,
    decide that it is dead and remove it. `hold` then fails, and `expect("hold a session")`
    panics.
  - The rename fails if another process, for example antivirus or the indexer, has a file in
    the directory open.
- **The session base is a fixed path on drive C.** `C:\Temp\goofi-system` (and iceoryx2's
  `C:\Temp\iceoryx2`) cannot be created on a computer that has no drive C, or where users cannot
  write to the root of C. Boot then panics.
- **The socket path has almost no free space.** The worst-case listener socket path is 106 of
  the 108 bytes that iceoryx2 permits. A longer base, prefix or root breaks all nodes again.
- **A VST3 plugin that crashes during the scan can open a Windows error dialog.** No process sets
  `SetErrorMode`. The scan then waits for `SCAN_WAIT` (20 s). Also, `answered()` does not report
  the crash, because on Windows a crashed child has an NTSTATUS exit code, not a signal.
- **The audio device-list refresh opens all ASIO drivers.** `goofi-audio/src/host.rs` `named`
  enumerates every host each time the list is refreshed. For ASIO, this takes seconds and stops
  the node's control thread. ASIO enumeration while VST3 plugins are loaded has already caused
  `STATUS_HEAP_CORRUPTION`, which is why `warm()` exists.

## Process control

- **A graceful stop never succeeds.** `request_stop` runs `taskkill /T` without `/F`, which does
  not stop a console process. Each stop waits for the full grace period: 5 s for an agent
  harness during shutdown, and 2 s for a plugin that does not reply.
- **Each child stop starts `taskkill.exe` twice.** `stop`, `wait_within` and `Drop` start one
  process for the graceful attempt and one for `/F /T`. Each start takes approximately
  50–150 ms, one after the other, on the engine thread or the shutdown thread. Shutdown and node
  removal become slower with each subprocess node.
- **Grandchildren continue to run after goofi is killed.** The liveness pipe stops only the
  direct children. Processes that those children start (Python `multiprocessing` workers, VST
  helpers, build tools) continue to run.

## Text encoding

- **Python pipes use cp1252, not UTF-8.** Python on Windows reads and writes pipes in the ANSI
  code page, but goofi sends and expects UTF-8.
  - A plugin receives non-ASCII request text as incorrect characters. Some characters (for
    example `ő`, `Ł`) cause `UnicodeDecodeError`, which stops the plugin service.
  - Non-ASCII text from Python nodes, from the subprocess tier and from the discover probe
    shows as U+FFFD in the log and in the "unavailable" reason.
  - `goofi-init` reads `sys.base_prefix` through such a pipe. A user name with non-ASCII
    characters gives an incorrect `PYTHONHOME` in `.cargo/config.toml`, and the embedded
    interpreter cannot start.
- **Console text shows incorrect characters.** `terminal_line` writes UTF-8 bytes to the saved
  console handle with `WriteFile`, and no code sets the console to UTF-8. `·`, `→`, `✓` and `…`
  in the startup banner show as `Â·` or similar.
- **Progress bars never show.** After `capture_stdio`, `stdout().is_terminal()` is always
  false, so the startup progress lines use a hidden draw target.
- **iceoryx2 writes each Win32 error to stderr, with a 1 KB block of NUL characters.** The
  `win32call!` macro prints each failure that it does not ignore, including expected
  "path not found" results at boot. It prints the full 1024-byte message buffer. Each boot
  therefore adds error records full of NUL characters to the process log.

## Performance

- **The hosted and Python subprocess tiers poll.** The child checks for requests with
  `sleep(500 µs)` (`hosted.rs`, `goofi-pymod/src/serve.rs`), and the parent checks for the reply
  with `sleep(1 ms)` (`goofi-transport/src/lib.rs`). On Windows, an idle child wakes up to 2000
  times each second, and each call gets approximately 1–2.5 ms of added latency.
- **Timed waits are rounded up to the 15.6 ms timer tick.** Condvar, channel and socket waits
  with a timeout use the default Windows timer resolution. The control tick (`TICK` = 10 ms) runs
  at approximately 64 Hz, not 100 Hz. Viewer pacing (`reducer.rs`, `lib.rs` follower) and
  recording waits get up to 16 ms of jitter.
- **Each iceoryx2 wait and notification is expensive.** iceoryx2 emulates each event with a
  local UDP socket. Each wait changes socket options several times, and each change scans the
  1024-entry handle table under one lock for the whole process. Each `stat` scans a 65536-entry
  port table. The cost increases with the number of nodes multiplied by their rate.
- **Signal nodes do not reach high target rates.** Measured on a debug build: a `signal:Clock`
  with `max_frequency` 250 emits at approximately 214 Hz, and at 1000 it emits at approximately
  626 Hz and uses 8% of one core.

## Python

- **The embedded interpreter does not process `.pth` files.** Its site-packages is given through
  `PYTHONPATH`, and Python does not read `.pth` files from `PYTHONPATH`. Packages that need a
  `.pth` file, for example `pywin32` and editable installs, do not import in the embedded
  interpreter.

## Tests

- **The e2e agent specs need a POSIX shell.** `tests/e2e/globalSetup.ts` configures the `_sh`
  agent as `sh` with `SHELL=/bin/sh`, and the specs send POSIX commands. Without Git's
  `usr\bin` on PATH, `agent.spec.ts` fails on Windows.
- **The Windows orphan test does not run.** The test that a child stops when goofi dies is
  unix-only (`goofi-tests` `children.rs`), so the Windows liveness pipe has no test.
