# Backend architecture: what remains of the redesign

Each step lands on `main` behind the existing situations. A step that changes a wire format or
the document shape updates the frontend and the Python wheels in the same commit.

## Decisions that bind the remaining work

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
  that branches (the Windows handle limit as a named fault, §5) brings the enum with it.

## 1. The batch bookkeeping moves into the transaction

- `Graph::touched` (`goofi-graph/src/lib.rs`) is the last batch-local state outside `Txn`. Move
  it into the transaction.
- Arming serials are minted in `Graph::set_recorded`, at command time. Mint them at settle.
- Events that read graph or runtime state (`param_state_update` in `ops/node.rs`) are built
  before `Txn::commit` settles. Build them after settle.

## 2. One name for one thing

- Rename `goofi-bridge/src/doc.rs`'s `Patch` enum (`Applied`/`Stale`/`Gap`); the name collides
  with `PatchDoc` and the patch field of the graph.

## 3. Graphics recording goes through the executor

- `Graphics::set_recorder` (`graphics/goofi-graphics/src/lib.rs`) pushes a command to the
  render thread from the bridge. Fold it into `GraphicsHalf: Executor`.

## 4. Deadlines become liveness checks

Product deadlines judge speed, so a starved thread becomes an error. Make each a liveness
check that fails only when the other side is gone:

- The recorder's ceilings: `goofi-record` `SETTLE` (3 s, polled with a sleep), `Writer::flush`
  (5 s per lane), `AudioCapture::flush` and the audio `recording_boundary` closure (3 s each);
  each fails `record stop`.
- `TICK_TIMEOUT`/`COLD_START_TIMEOUT` in `goofi-runtime/src/hosted.rs` and
  `goofi-python/src/subproc.rs` drop a child that is slow but alive.
- `Exchange::ask` (`goofi-transport/src/exchange.rs`) parks on the listener and returns only
  on an answer, the child's exit or the deadline. Give it a halt signal so a shutdown can end a
  wait.

## 5. Windows process control

Needs a Windows host; the defects are listed in `windows-cleanup.md`:

- `CREATE_NEW_PROCESS_GROUP` so Ctrl+C stays with goofi.
- A Job object per child with `KILL_ON_JOB_CLOSE`, and `CTRL_BREAK` for a graceful stop in
  place of `taskkill` (`goofi-supervisor/src/child.rs`).
- A blocking console-close handler bounded by the scope deadlines.

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

## Not in this plan

- The SPA npm build and the nested cargo builds of shipped nodes inside
  `goofi-bridge/build.rs` become an explicit step, separately.
- A reader for the npy + sidecar format is a feature and needs its own entry.
