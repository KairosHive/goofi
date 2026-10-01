# Backend architecture: the decisions of the redesign

The redesign's steps have landed. This is the ledger of what binds the code that follows, and of
what was weighed and left out, so neither is proposed again.

## Decisions that bind

- The graph stays a `Mutex`. Each op runs on its own blocking task inside one `Txn`
  (`goofi-bridge/src/txn.rs`); the graph mutex orders the ops.
- The registry stays the explicit `TREE` in `goofi-bridge/src/ops/mod.rs`, with leaves built by
  the `const fn` constructors `read`, `write` and `effect`.
- Data services keep no history (`goofi-transport/src/services.rs`). A one-shot producer can
  lose its frame during a link; this is a documented property, not a defect.
- Audio loads `.rs` files and VST3 modules in-process and never unloads them
  (`audio-engine.md`).
- Error enums only where a caller branches or a boundary needs context. No caller branches on a
  transport, record, build or supervisor error today, so each stays a `String`. The first caller
  that branches (the Windows handle limit as a named fault, `windows-cleanup.md`) brings the
  enum with it.
- A wait on another thread or process ends when the other side answers or is gone, never on a
  clock: the recorder's drains, a node runtime's flush, the audio clock's boundary and a hosted
  child's tick are liveness waits. A deadline stays only where the other side is being ended
  (`STOP_TIMEOUT`, the scope's close) or a one-shot probe is bounded like a build.
- A node's runtime wears its halt (`Halt::wear`); a wait on a child process reads it, so a
  shutdown ends the wait without a deadline.
- Arming serials are minted at settle, one per slot however often the batch armed it; an echo
  of a node's runtime state (`Txn::echo`) is built after the settle.

## Not to be done

- A touched-path projection in place of the record-level diff (`goofi-bridge/src/doc.rs`
  `diff_ops`): a record's projection derives from the catalog and from stream resolution, not
  from patch writes alone, so a touched set would need runtime-side marks as well. The diff is
  linear in the patch.
- `library get --source` off the lock: one file read under a read transaction; the probes and
  builds a scan waits on already run off it in `prebuild`.
- An `AppState` split and an actor newtype: the typed ops left no reader that branches on
  either; a split would add a second owner of the same locks.
- Moving the `record start` plugin mutex into its op: it must also cover the plugin `pre_op`
  hook, which runs before the op.
- A per-socket op queue that holds an op's events behind its reply: a second scheduler beside
  the op path; a slow op parked everything behind it. The socket already forwards logs and
  events while an op runs.
- A rate limiter inside `useLiveValue`: the gesture design has to follow the op path; a
  preview's latest-wins slot is not a queue.
- A supervisor `Child` over the PTY child: portable-pty spawns and reaps it, the reaper holds
  its handle in `wait`, and a stop reaches it by pid, so the roster lease stays the instance's
  own.
- A `Scope` tree with one release pass per resource `Kind`: a `Scope` holds finishes, children
  and workers; ports, paths and devices are index leases that drop with their owners, and
  `AppState::shutdown` is the one release order.
- `Graph::touched` and `Runtime::changed` into `Txn`: both are the graph's own log of what a
  batch asked, consumed by `settle`, and the mutex the transaction holds scopes them to the
  batch; a worker that writes under the graph lock with no transaction would need one.

## Deferred to entries of their own

- The SPA npm build and the nested cargo builds of shipped nodes inside
  `goofi-bridge/build.rs` become an explicit step, separately.
- A reader for the npy + sidecar format is a feature and needs its own entry.
