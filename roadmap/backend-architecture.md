# Backend architecture: the redesign a from-scratch build would make

Each step lands on
`main` behind the existing situations. A step that changes a wire format or the document shape
updates the frontend and the Python wheels in the same commit.

## Order

§5.10.

## 1. The patch model and the runtime are two types

Remaining in the split: `touched` and `open_batches` move into §3's `Txn`; arming serials are
minted at settle, and events that read runtime state are built after settle.

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

Not to be done: a touched-path projection in place of the record-level diff (a record's projection
derives from the catalog and from stream resolution, not from patch writes alone, so a writer's
touched set would need runtime-side marks as well — a second bookkeeping the diff makes unnecessary;
the diff is linear in the patch); `library get --source` off the lock (one file read under a read
transaction; the probes and builds a scan waits on already run off it in `prebuild`); the `AppState`
split and an actor newtype (the typed ops left no reader that branches on either; a split would add
a second owner of the same locks), moving the `record start` plugin mutex into its op (it must also
cover the plugin `pre_op` hook, which runs before the op); a per-socket op queue that holds an op's
events behind its reply (a second scheduler beside the op path; a slow op parked everything behind
it), and a rate limiter inside `useLiveValue` (the gesture design has to follow the op path; a
preview's latest-wins slot is not a queue).

## 4. One node runtime, one protocol per boundary

`goofi-runtime` holds every node's thread, desired state, bindings, ports and faults behind one
`Executor` trait; data services keep no history, and the one-shot race stays a documented property.

Audio keeps loading `.rs` files in-process and never unloads them (`audio-engine.md`). Fold
graphics recording (`set_recorder` from the render thread) into the executor.

## 5. An ownership tree, one session type, one boot path

`goofi-supervisor` holds `child`, `worker`, `scope`, `session`, `log` and the progress sink;
`goofi-core` keeps the vocabulary. `Scope` owns `Child`, `Worker`, `Port`, `Path` and `Device` handles plus
`Finish` steps (plugin `on_stop`, recorder finalize); `close()` stops the subtree, then makes
one pass per `Kind` over the whole tree: finish, children on one shared deadline, workers
joined, ports, paths, devices. `boot` returns a `Manager { state, scope }` that is not `Clone`;
dropping it is the shutdown.

- Windows process control (`windows-cleanup.md`); needs a Windows host:
  `CREATE_NEW_PROCESS_GROUP` so Ctrl+C stays with goofi, a Job object per child with
  `KILL_ON_JOB_CLOSE` and `CTRL_BREAK` for a graceful stop in place of `taskkill`, and a
  blocking console-close handler bounded by the scope deadlines.

Not to be done: a supervisor `Child` over the PTY child (portable-pty spawns and reaps it, and
the reaper holds its handle in `wait`; a stop reaches it by pid, so the roster lease stays the
instance's own).

## 6. Errors

Error enums only where a caller branches or a boundary needs context. No caller branches on a
transport, record, build or supervisor error today, so each stays a `String`; the first caller
that branches — the Windows handle limit as a named fault is §5.10's — brings the enum with it.

## Smaller items

- Product deadlines judge speed, so a starved thread becomes an error: the recorder's 3 s
  ceilings (`goofi-record` `SETTLE`, `AudioCapture::flush`, `recording_boundary`) fail
  `record stop`, and the hosted and Python `TICK_TIMEOUT`/`COLD_START_TIMEOUT` kill a child that
  is slow but alive. Make each a liveness check; §4.F's event wake gives `ask` the halt signal it
  lacks.

Not in this plan: the SPA npm build and the nested cargo builds of shipped nodes inside
`goofi-bridge/build.rs` become an explicit step, separately; a reader for the npy + sidecar
format is a feature and needs its own entry.
