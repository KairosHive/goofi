# One index of every resource goofi opens

Audit of 2026-09-14, after the session change (`3a22af97`). The goal: every file, process,
thread, socket, iceoryx2 port and device goofi opens is minted through one place under the
manager's supervision, indexed by the session, and released in one known order.

## What owns what today

The manager (`AppState`) reaches these directly: the session (`goofi_transport::session`,
parked in a static), the mount, the harnesses, the plugin services, the recorder, the slot
reducers and their one iceoryx2 node, the record drain. Everything else it reaches only through
`Graph.engines`, which `Drop for Graph` tears down.

The manager does NOT reach these at all:

| Resource | Where | Owner | Stopped by |
|---|---|---|---|
| Window loop, X11 connection, wake pipe | `goofi-window` | main thread `Loop` | process exit; nothing calls `Ui::stop` |
| cpal device cache (incl. ASIO warm-up) | `goofi-audio/src/host.rs` `SEEN` | process static | never |
| ffmpeg encoders and preset trials | `goofi-record/src/video.rs` `CHILDREN` | process static | `kill_encoders` on a SECOND Ctrl-C only |
| VST3 scanner child (own exe re-entered) | `goofi-audio/src/vst3/mod.rs` | local | poll to `SCAN_WAIT`, then kill |
| Python introspection probes, `cargo build`, `npm`, `uv` | `goofi-python/discover.rs`, `goofi-build`, `goofi-bridge/plugins.rs` | local `.output()` | reaped inline, no timeout, no kill path |
| Graphics compile thread | `goofi-graphics/src/scan.rs` | static sender | never |
| Status-drain worker, tap follower | `goofi-bridge/src/lib.rs` | detached | never / sender drop |
| stdio capture drains | `goofi-core/src/log.rs` | static | never |
| Signal node threads, control threads | `goofi-host`, `goofi-control` | `JoinHandle` dropped | `Halt`, never joined |

Five ways to run a child process coexist: harness (SIGTERM to the group, grace, kill), plugin
service (kill on timeout, kill on drop), Python node (liveness pipe, kill, wait), ffmpeg (global
registry, wait to a deadline), VST3 scan (poll, kill). Only the Python node joins the session.

Files nothing removes: `system/shipped/<version>/<key>/` and `system/build/{sdk,crates,target,
out}/` grow with every version and key; probe memos, VST3 verdicts and plugin frontend builds
grow with every source; `<temp>/goofi-export-*.gfi` and `goofi-import-*.gfi` outlive a crash.
Node state blobs live under `<mount>/.goofi/state/` and go with the mount. Recordings, the custom
library, plugins and plugin data are the user's and stay.

## The design

One registry per process, rooted in the session, in `goofi_core`:

- Every resource is minted through a typed constructor that returns a lease. A lease
  unregisters on drop, so the registry is always the truth and never a mirror. The registry
  answers `session status` with the full inventory, and shutdown walks it in one fixed order:
  children, then workers, then ports, then the mount.
- One child-process type replaces the five: spawn with `GOOFI_SESSION` set, a liveness pipe,
  a stop policy (signal the group, grace, kill), and a deadline for one-shot tools. Harnesses,
  plugin services, Python nodes, ffmpeg, the VST3 scanner and the build tools all use it.
- One worker type replaces the detached threads: a `Halt`, a name, and a join on shutdown. The
  graphics compile thread, the status drain, the tap follower and the log drains become owned.
- Files: ephemeral files go under `system_dir(id)` and are swept with it, the `.gfi` export and
  import temps included. Caches under `.goofi/system` get one boot pass that removes every
  version tree that is not the running version.
- Ports: the reducers' node and the record drain's node come from the same constructor the
  engines use, so the index lists them.

When engines become processes, each engine process runs its own registry under the same
session id, and the manager's registry lists the engine children. The SDK child-process tier
is one more child kind under the same type.

## Remaining work

1. `goofi_core::registry` with leases; `session status` lists it.
2. The one child type; move the five spawn paths onto it. Delete `CHILDREN` and `kill_encoders`.
3. The one worker type; own the four detached threads; join on shutdown.
4. Export and import temps under the session; a boot pass over `.goofi/system` caches.
5. `Ui::stop` on shutdown; the cpal device cache becomes engine-owned.
