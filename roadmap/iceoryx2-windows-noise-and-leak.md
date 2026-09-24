# iceoryx2 on Windows: noise, and what a session owns

Two upstream defects in `iceoryx2-pal-posix`, both reproduced against 0.9.3 and `main`:

- `mman.rs` reads `FindNextFileA`'s `ERROR_NO_MORE_FILES` as a failure and logs it: ~80,000
  records a test run, pure noise. The same call in `dirent.rs` passes the ignore list.
- `.port_tag` and `.node_monitor*` files carry a PROTECTED DACL their owner cannot unlink through
  (#1869). iceoryx2's own dead-node reclaim therefore fails, and its liveness answer — a
  `LockFileEx` try whose every failure reads as "alive" — never even reaches it.

goofi no longer asks iceoryx2 who is alive and no longer sweeps iceoryx2's machine-global root; a
session (`goofi_core::session`) owns every ephemeral resource and its `alive.lock` is the one
aliveness answer. On Windows the boot sweep takes each file's DACL back by name first
(`take_path`), the only part of the old repair that remains.

## Remaining

- The noise is upstream's. A stderr filter deadlocks the Python subprocess tier, so it stays until
  the upstream fix lands.
- Unverified on Windows from this machine, and CI does not prove it: the Windows job runs the unit
  tests only, and the situations sit out until iceoryx2 can listen there.
- The shared-memory sweep by prefix lists a directory, which macOS's POSIX shm namespace does not
  offer. What a crash leaves there is reclaimed by iceoryx2's own path or a reboot.
