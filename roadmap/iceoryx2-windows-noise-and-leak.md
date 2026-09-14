# iceoryx2 on Windows: noise, and what a session owns

Two upstream defects in `iceoryx2-pal-posix`, both reproduced against 0.9.3 and `main`:

- `mman.rs` reads `FindNextFileA`'s `ERROR_NO_MORE_FILES` as a failure and logs it: ~80,000
  records a test run, pure noise. The same call in `dirent.rs` passes the ignore list.
- `.port_tag` and `.node_monitor*` files carry a PROTECTED DACL their owner cannot unlink through
  (#1869). iceoryx2's own dead-node reclaim therefore fails, and its liveness answer — a
  `LockFileEx` try whose every failure reads as "alive" — never even reaches it.

## What goofi does about it (2026-09-14)

goofi no longer asks iceoryx2 who is alive, and no longer sweeps iceoryx2's machine-global root.
A session — `goofi_core::session` — is the one owner of every ephemeral resource, and its
`alive.lock` is the one aliveness answer, released by the OS on any exit:

- `.goofi/system/sessions/<id>/` holds the record (`session.json`, `alive.lock`).
- `<system>/goofi-system/<id>/iox/` is the iceoryx2 ROOT for that session, and every segment
  carries the prefix `g<id>_`. `/tmp/goofi-system` on unix (a unix socket path is capped at 108
  bytes, which a macOS `$TMPDIR` alone half spends), `%TEMP%\goofi-system` on Windows.
- `<temp>/goofi-workspaces/<id>/` is the patch workspace: removed on a clean shutdown only.

The boot pass (`goofi_transport::session`, run by the manager before any engine exists) removes
every record whose lock is free, every system directory whose record is dead or gone, and the
segments their prefixes name. Nothing a living session owns is ever enumerated. On Windows the
removal takes each file's DACL back by name first (`take_path`), which is the only part of the
old repair that remains. A spawned child joins its parent's session through `GOOFI_SESSION`.

Remaining: the noise is upstream's, and a stderr filter deadlocks the Python subprocess tier, so
it stays until the upstream fix lands. Unverified on Windows from this machine; the CI job is the
proof. The shared-memory sweep by prefix lists a directory, which macOS's POSIX shm namespace does
not offer — what a crash leaves there is reclaimed by iceoryx2's own path or a reboot.
