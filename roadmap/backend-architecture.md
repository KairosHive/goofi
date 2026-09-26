# Backend architecture: the redesign a from-scratch build would make

`multi-engine-graph.md` keeps the seam decisions that bind every engine. Each step lands on
`main` behind the existing situations. A step that changes a wire format or the document shape
updates the frontend and the Python wheels in the same commit.

## Order

**Phase 0**, each alone, in any order: §3.1-3, §3.6; §4.A-E, §4.J; §5.1-9; §1.A1; §2.B1.

**Phase 1**: §5.14 and §5.10; then §3.4 and §3.5; then §4.F and §4.G1-G4; then §1.A2 and §1.A3.

**Phase 2**: §2.B2-B5, §3.7-12, §4.H-I, §5.11-13 and 15-16.

## 1. The patch model and the runtime are two types

`Graph { patch: PatchDoc, viewpoint, runtime: Runtime }`. `Runtime` holds the engines, waker,
instance, epoch, time, evaluator, the catalog (classes, unavailable, origins), the mint
(`next_uid`, generations, `arm_serial`), an `instances` table (engine, generation, health, typed
params, bindings, serials), `watched`, `view_wants` and `refreshed`. No index beside the patch;
settle builds its own short-lived maps. The patch holds param values only; typed params with
bounds derive from the catalog class. `next_uid` stays a process-lifetime counter because undo
restores deleted uids. `arrangement_warning` becomes a return value of load. `VariableStore`'s
parallel maps become one `IndexMap<String, Variable>`. `touched` and `open_batches` move into
§3's `Txn`.

- **A1** Binding state (`rewritten`, `terms`, `vars`, `bind_error`) is derived at settle,
  memoized by text; the evaluator id compiles only when the rewritten text changes. The rebind
  and invalidate functions and their call sites go.
- **A2** The `Runtime` struct with the `instances` table.
- **A3** Births at settle: commands record `Added`, `Removed` and `Restart` in the transaction's
  change log and never call the engine. Settle does removals, then births (mint a generation),
  then remove+insert for a restart or a changed class, then bindings, `build_view` and
  `engine.settle`. Serials are minted at settle; events that read runtime state are built after
  settle. The benches in `goofi-tests/examples/` settle after `add_node`. The editing situation
  proves an add plus a rollback performs zero inserts.

## 2. One typed document, and contracts generated from it

```rust
pub struct PatchDoc {                          // archive body, browser replica, paste fragment
    nodes: IndexMap<Uid, NodeRecord>,
    links: IndexMap<LinkKey, Link>,            // "oooo.slot>iiii.slot", derived from the link
    variables: IndexMap<String, Variable>,
    variable_groups: IndexMap<String, Group>,
    arrangement: Layout,                       // panel state is plain JSON
}
pub struct NodeRecord { type_id, name, pos: [f64; 2], scope: Option<Uid>,
    params: IndexMap<String, IndexMap<String, ParamEntry>>,
    viewers: Option<Value>, baseline: Option<Value>, record: Vec<Armed> }
pub struct ParamEntry { value: Option<Scalar>, mode: Option<Mode>,
    expression: Option<String>, reference: Option<String>, triggers: Option<bool> }
pub struct Archive { version: i64, goofi: String, patch: PatchDoc, viewpoint: Option<Value> }
```

Spellings: `expression`/`reference`, `pos` as `[x, y]`, links as a keyed map, sources inline,
variables as an ordered map. `viewpoint` and ephemeral variables stay out of the replica. A
node's class derives from the catalog, not pinned per instance. Values a type no longer declares
drop on load or paste. Rename `doc.rs`'s existing `Patch` type (Applied/Stale/Gap).

- **B1** One wire spelling in the projection and a `variable_groups` root; frontend, e2e and
  test readers follow.
- **B2** Typed archive and fragment through serde; one `admit(doc) -> (PatchDoc, warnings)` for
  paste and load. Unresolvable links are dropped with a warning in the reply. `MANIFEST_VERSION`
  becomes 2; the example patches are converted once.
- **B3a-e** The graph stores the `PatchDoc`: scope, links, values-only params, one variable
  map; `projection.rs` deleted.
- **B4** The replica delta becomes path ops, `doc_patch {from, v, ops: [put | del]}`, with Rust
  and TypeScript appliers. Blobs travel as real JSON; `json_string` goes; the AGENTS.md rule
  that document leaves cannot be null goes. The patch's fields are private; `PatchMut` is the
  only writer and records the paths it touched; an order-changing edit re-sends its parent map.
  `GraphDoc` keeps only the version; the deep diff and the follower's re-projection go.
- **B5** Generated TypeScript (ts-rs) for the document, param descriptors and frame tags.

## 3. Typed ops with one transaction tail

```rust
pub enum Kind { Read, Write, Effect }
pub trait Op: 'static { const NAME: &str; const KIND: Kind; const DOC: &str;
    const POSITIONAL: &[&str] = &[];
    type Args: DeserializeOwned + JsonSchema;   // deny_unknown_fields
    type Out: Serialize; }
pub trait WriteOp: Op { fn run(tx: &mut Txn, a: Self::Args) -> Result<Self::Out, OpError>;
                        fn label(a: &Self::Args, o: &Self::Out) -> String; }
// ReadOp::run(&mut ReadCx, ..), EffectOp::run(&Cx, ..) alike.
```

The registry stays the explicit `TREE` in `ops.rs`, with `write::<NodeAdd>()` leaves built by
`const fn`. The graph stays a `Mutex`. Each op dispatches on its own blocking task and the graph
mutex orders them.

1. `node_touched_clear` stops resyncing itself.
2. Dispatch off the tokio workers through `spawn_blocking`, for `/control` and `POST /patch.gfi`.
3. Notifiers: logs push through a `log::set_listener` watch; `/params` reads a `LiveHub` of
   per-uid watches that the status drain fills at `LIVE_PERIOD` pacing and the transaction tail
   refreshes.
4. `Txn` holds the graph and history guards, an outbox of typed events, and `touched`/`edited`
   flags. The tail runs once when the transaction saw a change: fold the entries into one
   labelled entry, settle, render events from settled state, mark unsaved and bump `revision`,
   send `[doc_patch, outbox…]` under the doc guard. `Drop` without commit rolls back, also on
   unwind. A compound runs nested handlers on one `&mut Txn` and holds the graph for its whole
   run, reads included; the settle counter, `OPEN_BATCH`, `BatchScope` and `HistoryEntry.batch`
   go.
5. One `Event` enum and one `Broadcaster`; the events vec parameter goes.
6. `Command::execute(g, Ctx::Fresh | Ctx::Replay) -> Applied = Done(outcome, inverse) |
   Skipped(Gone | Stale)` replaces the empty-compound inverses.
7. **a-c** The op trait; `arms.rs` splits into `ops/<group>.rs`; `AppState` splits with it.
   Deserializing into `Args` is the validation on every path; `Op::validate` and the hook gate
   go; a plugin `pre_op` patches JSON before deserialization. The repeated pos parsing and
   `AddNode` capture go with typed args and `admit`. Actors become a newtype; the `record start`
   plugin mutex moves into its op.
8. Generated op types: arg declarations, flag parsing, completion, `op list` (structured args,
   no DSL string) and MCP JSON Schema (schemars on `Args` only) come from the types.
9. Server-owned undo: entries carry a label, an opaque navigation context and a merge token;
   write replies carry this actor's `{undo, redo}` labels; a stale entry is removed and
   reported; navigation context is restored after the flip. The browser marker stack and the
   `_recordGraphCmd` sites go.
10. Continuous motion: a gesture sends ordinary value ops with an envelope `merge` token; the
    commit folds same-token, same-`merge_key` entries into one undo entry. The client keeps one
    send in flight per control, latest value wins, and shows its local value until the reply's
    doc version has reached the replica. Scripts, tests and plugin editor-window knob bursts
    send the same token.
11. **a** `session save` persists, serializes and fingerprints under the guard, zips off it, and
    clears dirty only if `revision` did not move; the download zips off the lock. **b** Rescans
    (`load_patch`, `library refresh`, `library save`, `library get --source`) leave the lock once
    the runtime owns the catalog (§1).
12. The tail projects touched paths only (after §2).

Not to be done: a per-socket op queue that holds an op's events behind its reply (a second
scheduler beside the op path; a slow op parked everything behind it), and a rate limiter inside
`useLiveValue` (the gesture design has to follow the op path).

## 4. One node runtime, one protocol per boundary

One per-node control runtime, `goofi-runtime`, merged from `goofi-control` and the host runtime;
three node models stay (block DSP, shader, host node). The shared `Core` holds the mailbox
handle, `Desired`, one `PortBundle`, `Bindings` (delivery, `evaluate_where(time.now())`, the
pulse edge), one `FaultState` and the stamps. An `Executor` trait (`arrive`, `params_changed`,
`pulse`, `refresh`, `next_wake`, `run`) has three implementations; graphics host producers use
the signal executor in the control thread, and `Local` goes. Data services keep no history; the
one-shot race stays a documented property.

- **A** `EncodeError` on size limits in the codec instead of `as` casts and `expect`s; an
  undecodable input frame is a process fault on its wire, not a dropped error.
- **B** `PortBundle<P>` drops its ports before its node; one `ServiceKind` table; the subprocess
  service declares `max_nodes`; swallowed status and control loans are reported; a failed open in
  `goofi-control` is retried, not deduplicated away; slots at index 64 and above get a doorbell.
  Wheels rebuilt.
- **C** The SDK hash rides the version symbol; `open` checks it, not the crate version.
- **D** `doorbell_driven` and `NodeView.rings` go.
- **E** One `Clock` enum; a `Timer` whose `catch_unwind` runs inside the held guard; the
  graphics tick gets a panic guard; cpal contention gets its own counter (not an xrun) and
  `render_into` a panic guard.
- **F** One child RPC: `trait Call` with in-process, hosted and Python implementations behind
  one `CodecNode`; the frame is `[entry][now][payload]` for all; Python gets an eager `Setup`, a
  `Stop` and `now` (no `on_param_changed`); both directions wake on iceoryx2 events after a
  readiness handshake instead of polling. Wheels rebuilt. The codec's subprocess protocol splits
  from the frame format here.
- **G1** Shared bindings and faults (one pulse gate, one fault mechanism, one `common` group;
  bindings evaluate with the current `now`).
- **G2** Signal on whole-state `Desired` with dedup; the ack planner is deleted, with an
  `Applied { version, refused }` report (a refusal is a process fault and the engine re-sends)
  and a ring once when a producer first applies a target. The `transport.rs` situation is
  rewritten.
- **G3** Graphics without `Local`.
- **G4** One `desired_of`/`rides_the_plan`.
- **H** Graphics host `.rs` files built after boot run hosted, as signal does; audio keeps
  loading in-process and never unloads (`audio-engine.md`).
- **I** Native ABI stays bytes; inputs cross as borrowed GOOF frames, params only as deltas,
  outputs are written into the loan (`frame-copies.md`).
- **J** Leaked manifests are interned by type and describe hash, not moved to `Arc`.

Fold graphics recording (`set_recorder` from the render thread) into the executor.

## 5. An ownership tree, one session type, one boot path

`goofi-supervisor` holds `child`, `worker`, `scope` (absorbing `registry`), `session` and `log`
with a progress sink; `goofi-core` keeps the vocabulary and its name; `startup` and indicatif
move to the CLI. `Scope` owns `Child`, `Worker`, `Port`, `Path` and `Device` handles plus
`Finish` steps (plugin `on_stop`, recorder finalize); `close()` stops the subtree, then makes
one pass per `Kind` over the whole tree: finish, children on one shared deadline, workers
joined, ports, paths, devices. `boot` returns a `Manager { state, scope }` that is not `Clone`;
dropping it is the shutdown.

1. `session::list()` is read-only; `sweep_dead()` is the only deleter (`goofi-core`'s
   `sessions(remove)` and `goofi-client`'s `list()` still delete while listing).
2. `goofi_core::sync::Mutex` (§6).
3. One tokenizer (§6).
4. Child stderr level (§6).
5. Boundary panics (§6).
6. Timed child waits (pidfd, `WaitForSingleObject`) replace polling in `Child::poll`,
   `stop_recording` and `wait_released`; recording stop and engine release join their workers.
7. The recorder's `Session` becomes `Take`.
8. The supervisor split.
9. One `Session` value: `hold()` locks a sibling `<id>.alive` file before the directory exists;
   `release()` is idempotent and runs on drop. Transport takes an explicit `Iox` handle built
   from the session; the statics and the `atexit` hook go; one read-only process
   `session::id()` stays for part-file tags. The binary releases explicitly on the serve-panic
   and second-Ctrl+C paths; a test binary that exits leaves its record for the next boot's
   sweep, and the situation `a_process_that_exits_without_releasing_leaves_no_record` goes.
   `goofi-client`'s own sweep goes.
10. `Scope` and `Manager`; every detached thread gets an owner (`term.rs`, `plugins.rs`,
    `arms.rs`, `reducer.rs`, `record.rs`); the record beat goes; `scratch()` and the mount are
    leased as `Kind::Path`. The rest of the transport file splits into names, services and
    exchange.
11. Plugin scopes.
12. PTY scopes: children join through a `Stoppable` trait and reap on exit watches; the
    sleeping stop thread, the 25 ms reap poll and the hand-made lease go.
13. Windows process control (`windows-cleanup.md`); needs a Windows host:
    `CREATE_NEW_PROCESS_GROUP` so Ctrl+C stays with goofi, a Job object per child with
    `KILL_ON_JOB_CLOSE` and `CTRL_BREAK` for a graceful stop in place of `taskkill`, and a
    blocking console-close handler bounded by the scope deadlines.
14. `goofi_bridge::boot(Config) -> Manager` serves the binary and the harness. Plugins,
    requirements, the evaluator and fixture registration are `Config` fields; tests boot with no
    evaluator unless the `Config` asks for one.
15. Situations move off direct `state.graph` locks to ops and harness probes; `AppState.graph`
    becomes crate-private; a harness gate on live instances replaces `RUST_TEST_THREADS`.
16. Error enums (§6).

## 6. Errors, locks, lexers

- Error enums only where a caller branches or a boundary needs context: transport (keeping the
  iceoryx2 kind, so the Windows handle limit is a named fault), record, build, and `io::Error`
  in the supervisor; graph and bridge `Result<_, String>` become §3's `OpError`.
- `goofi_core::sync::Mutex` (own, not parking_lot) returns std's guard so `Condvar` works and
  logs a poisoned lock once by name; it replaces every `lock().unwrap()`.
- The boundary panics return errors: `hold(...).expect`, `iox_config`'s `expect`s on a long
  base path, `new_mount`, `fresh_id`, `nonce_hex`, `to_value(batch).unwrap()`.
- One tokenizer, `goofi_node::expr::tokens`, handles escapes, triple quotes, f-strings and `#`
  comments for the three expression scans (`expr_rewrite.rs`, `goofi-node/src/lib.rs`).
- Child stderr defaults to warning; a traceback or an `ERROR:` line is an error.

## Smaller items

- Product deadlines judge speed, so a starved thread becomes an error: the recorder's 3 s
  ceilings (`goofi-record` `SETTLE`, `AudioCapture::flush`, `recording_boundary`) fail
  `record stop`, and the hosted and Python `TICK_TIMEOUT`/`COLD_START_TIMEOUT` kill a child that
  is slow but alive. Make each a liveness check; §4.F's event wake gives `ask` the halt signal it
  lacks.
- `cargo test --workspace` builds a goofi-tests binary nothing else builds: goofi-cli's default
  `python` feature turns on `goofi-python/embed` while goofi-tests' `embed` stays off, so `io.rs`
  runs beside the in-process tier only there. Make the builds agree.

Not in this plan: the SPA npm build and the nested cargo builds of shipped nodes inside
`goofi-bridge/build.rs` become an explicit step, separately; a reader for the npy + sidecar
format is a feature and needs its own entry.
