# iceoryx2 on Windows: noise, and what a session owns

## Remaining

- Verify the session sweep on Windows (`remove_tree`/`take_path` in
  `goofi-supervisor/src/session.rs`, run from `Session::release`). CI does not prove it: the
  Windows job runs no situation.

## Not to be done

- Filtering iceoryx2's `ERROR_NO_MORE_FILES` noise from stderr (`mman.rs`, upstream): a stderr
  filter deadlocks the Python subprocess tier. Wait for the upstream fix.
- A shared-memory sweep by prefix on macOS: its POSIX shm namespace cannot be listed. A crash's
  segments are reclaimed by iceoryx2's own path or a reboot.
