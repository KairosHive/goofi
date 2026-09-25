# Backend architecture: the redesign a from-scratch build would make

Assessed 2026-09-23 from a full read of the backend (about 45k lines outside the tests, 26 crates).
Planned 2026-09-25: four read-only surveys checked every finding against `145b8f68` and turned
each change into types and a commit sequence. Citations below are from that read. This file owns
the structural work; `multi-engine-graph.md` keeps the seam decisions that bind every engine.
It absorbs `op-path.md` (§3).

## What stays

- One graph across engines, reached through one `Engine` trait (`goofi-node/src/seam.rs:206`).
- Every mutation is a command with an exact inverse, and undo is per actor
  (`goofi-graph/src/command.rs:53`, `:785`).
- A deferred settle delivers a batch once, from settled state (`goofi-graph/src/lib.rs:3070`).
- A session directory with one lock is the aliveness answer (`goofi-core/src/session.rs`).
- Situations in `goofi-tests` exercise real sessions through the op vocabulary.
- One binary that is server, CLI, client and child host.
- Engines stay in the manager's process; isolation is per node (`multi-engine-graph.md`).

## Order

The first draft ordered 1, 2, 3, then 4. The plan changes that: most of §3, §4 and §5 does not
depend on the patch model, and two of them make §1 smaller. §4's runtime on `Desired` turns a
birth into "a new handle and its first `Desired`"; §5's `boot(Config)` leaves §1 one boot path to
change instead of two; §3's `Txn` gives §1's change log its home.

**Phase 0: independent and cheap.** Each lands alone, in any order.

- §3.1 `node_touched_clear` stops resyncing itself; §3.2 dispatch off the tokio workers;
  §3.3 log and `/params` notifiers; §3.6 `execute(Ctx)`.
- §4.A codec errors; §4.B port bundle and service table; §4.C SDK hash in the version symbol;
  §4.D `doorbell_driven` goes; §4.E `Clock` and guarded `Timer`; §4.J manifest interning.
- §5.1-9: read-only session list, `sync::Mutex`, one tokenizer, child stderr level, boundary
  panics, timed child waits, recorder `Take`, the `goofi-supervisor` split, the `Session` value.
- §1.A1 bindings derived at settle; §2.B1 one wire spelling.

**Phase 1: foundations.**

1. §5.14 `boot(Config)` and §5.10 `Scope` and `Manager`.
2. §3.4 `Txn` and §3.5 typed events.
3. §4.F one child RPC (after the 8-thread starvation fix lands), §4.G1-G4 one node runtime.
4. §1.A2 `Runtime` struct, §1.A3 births at settle.

**Phase 2: on the foundations.** §2.B2-B5, §3.7-12, §4.H-I, §5.11-13 and 15-16.

Each step lands on `main` behind the existing situations. A step that changes a wire format or
the document shape updates the frontend and the Python wheels in the same commit.

## 1. The patch model and the runtime are two types

`Graph` (`goofi-graph/src/lib.rs:418-476`) has 23 fields and one impl of about 3200 lines
(`:523-3725`). It holds the document, the scan catalog (`unavailable`, `origins`) and the runtime
(`engines`, `waker`, `epoch`, `generations`, `arm_serial`, `watched`, `view_wants`, `refreshed`).

- **Births and removals are not settle decisions.** `insert_node_at` calls `engine.insert`
  synchronously (`lib.rs:1457-1460`), which spawns the node thread
  (`goofi-signal/src/engine.rs:393`); `remove_node` (`:2398-2402`), `restart_node`
  (`:2464-2471`) and `clear` (`:3195-3200`) do the same. A batch that adds a node and rolls back
  still starts and stops a thread.
- **Derived binding state is invalidated by hand.** `ParamSource` caches `rewritten`, `vars`,
  `terms`, the evaluator `id` and `bind_error` (`:348-363`). `rebind_naming`, `rebind_ports`,
  `invalidate_bindings_reading{,_param}` and `rebind_all` (`:720-812`) keep them in step from 19
  call sites; `load_doc` needs a full `rebind_all` (`:3710`).
- `RecordedOutput.serial` (`goofi-core/src/record.rs:72`) is a runtime value inside the record.
- Some events are built before settle (`node_restart` at `arms.rs:472`); with deferred births
  they would show the old instance's health.

**Design.** The in-memory patch IS the serde `PatchDoc` (§2), with no index beside it. Name
lookup and scope walks already scan (`:2894`, `:1536`); `scope_of` becomes `NodeRecord.scope`;
settle builds its own short-lived maps. The patch holds param values only; typed params with
bounds are derived from the catalog class on demand, and the runtime instance keeps a copy that
settle refreshes for touched keys.

```rust
pub struct Graph { patch: PatchDoc, viewpoint: Option<Value>, runtime: Runtime }
pub struct Runtime {
    engines, waker, instance: String, epoch, time, evaluator,
    catalog: Catalog { classes, unavailable, origins },
    mint: Mint { next_uid, generations: HashMap<Uid, u64>, arm_serial },
    instances: HashMap<Uid, Instance>, // engine, generation, health, typed params, bindings, serials
    watched, view_wants, refreshed,
}
```

Field fate: `nodes` → `NodeRecord` (manifest, isolation and engine derive from the class; health
→ `Instance`); `next_uid` stays a minted process-lifetime counter, because undo restores deleted
uids; `arrangement_warning` becomes a return value of load; `VariableStore`'s five parallel maps
become one `IndexMap<String, Variable>`; `touched` and `open_batches` go into §3's `Txn`.

Binding state (`rewritten`, `terms`, `vars`, `bind_error`) is derived at settle, memoized by
text; the evaluator `id` compiles only when the rewritten text changes.

**Births at settle.** Commands record `Added`, `Removed` and `Restart` in the transaction's change
log and never call the engine. At settle the runtime compares the patch with `instances` for each
logged uid: removals first (`engine.remove`, release binding ids, drop `watched`/`view_wants`),
then births (mint a generation, `engine.insert`), then remove+insert for a restart or a changed
class, then bindings, `build_view` and `engine.settle`. An add rolled back inside its batch
reaches settle as "absent and not running": nothing happens. The mechanism stays the one
`multi-engine-graph.md` fixed: explicit `insert`/`remove` from a log, no restart method, no
diff of whole snapshots. Events that read runtime state are built after settle.

Commits:

- **A1** bindings derived at settle; the rebind functions and their 19 call sites go
  (−260/+170).
- **A2** `Runtime` struct, a mechanical move with an `instances` table (net +60).
- **A3** births at settle, serials minted at settle, runtime events after settle; the benches in
  `goofi-tests/examples/` settle after `add_node`. The editing situation proves an add plus a
  rollback performs zero inserts (+180/−120).

## 2. One typed document, and contracts generated from it

- Two hand projections disagree: the browser doc (`goofi-bridge/src/projection.rs`) and the
  archive/paste `fragment()` (`goofi-graph/src/lib.rs:3250`, shared by `serialize` at `:3467`).
  Param source keys are `expr`/`ref` against `expression`/`reference`; `pos` is `{x, y}` against
  `[x, y]`; links are objects against four-element arrays (`:3313`); sources are inline against a
  side list; variables are a map against an array (`:3470-3492`). `capture_subtree_restore`
  (`command.rs:937`) is a third projection, into `AddNode`.
- Two hand parsers: `import_fragment` (`:3343`) and `load_doc` (`:3520-3724`), which bypasses the
  commands paste uses.
- `doc.rs:10` lists four roots; `variable_groups` is missing there and from the frontend
  `emptyDoc`.
- `viewers`, `baseline` (`projection.rs:54`, `:60`) and panel `state` (`layout.rs:138-160`) are
  stringified to keep null leaves out of the merge patch.
- Every write re-projects the whole graph and deep-diffs it (`goofi-bridge/src/lib.rs:1507-1518`,
  `doc.rs:97`); the follower does the same at `system.viewer_fps` (`:1479-1487`).
- A variable re-added at an index (undo of a delete) lands at the END of the replica's map,
  because a merge patch adds a new key and JS appends it.
- The frontend decoder is a hand port (`frontend/src/lib/codec/decode.ts:1-10`).

**Design.**

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

- Spellings: `expression`/`reference`, `pos` as `[x, y]`, links as a keyed map, sources inline,
  variables as an ordered map. `viewpoint` and ephemeral variables stay out of the replica.
- The replica delta becomes path ops, `doc_patch {from, v, ops: [put | del]}`, instead of a merge
  patch. Blobs travel as real JSON, `json_string` goes, and so does the rule that document leaves
  cannot be null. An order-changing edit re-sends its parent map, which fixes the variable order.
- The patch's fields are private; `PatchMut` is the only writer and records the paths it touched.
  At commit the paths are normalized and each becomes `put` or `del`. `GraphDoc` keeps only the
  version; `doc_state` is `to_value(&patch)`. The deep diff and the follower's re-projection go.
- Load is `admit(doc) → (PatchDoc, warnings)` plus the import paste uses. Unresolvable links are
  dropped with a warning in the reply. `MANIFEST_VERSION` becomes 2; the example patches are
  converted once.
- Rename `doc.rs`'s existing `Patch` type (Applied/Stale/Gap).

Commits:

- **B1** one wire spelling in the projection, `variable_groups` root; frontend, e2e and test
  readers follow (+90/−90).
- **B2** typed archive and fragment through serde; one `admit` for paste and load (−520/+320).
- **B3a-e** the graph stores the `PatchDoc`: scope, links, values-only params, one variable map,
  `projection.rs` deleted (about −575/+380).
- **B4** path-op replica with Rust and TypeScript appliers (+260/−240).
- **B5** generated TypeScript for the document, param descriptors and frame tags (+220/−160).

## 3. Typed ops with one transaction tail

- 73 ops: 21 read, 32 write, 20 effect. The args schema is a string DSL (`ops.rs:14-19`) re-split
  in `validate`, `phrase::parse_flags`, `usage` and `complete_args`. `/control` runs
  `Op::validate` only for plugin-hooked ops (`lib.rs:1392-1395`); the CLI and MCP never call it
  and check only as a side effect of parsing. `json` args and the steps of a `compound` are never
  checked. Handlers default unknown or mistyped keys silently.
- `resync_and_broadcast` is called by hand at ten sites. `node_touched_clear` resyncs itself
  (`arms.rs:590-593`) and so broadcasts an unsettled document inside a compound.
- A compound locks the graph per step (`arms.rs:114-184`); peers interleave and a rollback runs
  inverses under their changes (`command.rs:888-895`). Batching uses a bare settle counter
  (`lib.rs:3058-3066`) and a thread-local stamp (`command.rs:799-827`).
- 17 event names are pre-serialized strings, sent by the `events` vec and by direct sends (seven
  in `arms.rs`, five in the status drain, `reconcile_and_broadcast`, `term.rs:191`).
- `Command::Compound(vec![])` is the no-op inverse at 20 sites.
- The graph lock is held across filesystem and process work: `session save` (`arms.rs:1650-1663`),
  `download` (`patchfile.rs:35-39`), `load_patch` (rescan, `load_doc` thread spawns, fingerprint,
  autosave discard), `library refresh` and `library save` (rescans), `library get --source`.
- `/control` runs `call` inline on a tokio worker (`lib.rs:1155`), and so does `POST /patch.gfi`
  (`patchfile.rs:70`). `/control` polls the log every 50 ms per socket; `/params` locks the graph
  every 50 ms per connection (`:1745-1748`).
- The browser keeps an undo marker stack that must stay 1:1 with the server's, and the server
  records fresh no-ops only to keep it so (`command.rs:839-840`). It is already broken: an inline
  viewer change (`SlotViewer.svelte:39-41`) records both a server `node edit` and a client
  `set_view` marker; undoing the marker sends a fresh edit that clears the server's redo run.

**Design.**

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

- The registry stays the explicit `TREE` in `ops.rs`, with `write::<NodeAdd>()` leaves built by
  `const fn`. Deserializing into `Args` is the validation, on every path; `Op::validate` and the
  hook gate go. Arg declarations, flag parsing, completion, `op list` (structured args) and MCP
  schemas come from the types. A plugin `pre_op` patches JSON before deserialization.
- `Txn` holds the graph and history guards, an outbox of typed events, and `touched`/`edited`
  flags. The tail runs because the transaction saw a change, once: fold the entries into one
  labelled entry, settle, render events from settled state, mark unsaved and bump `revision`,
  send `[doc_patch, outbox…]` under the doc guard. `Drop` without commit rolls back, also on
  unwind. A compound runs nested handlers on one `&mut Txn`, atomic against peers; the settle
  counter, `OPEN_BATCH`, `BatchScope` and `HistoryEntry.batch` go.
- One `Event` enum and one `Broadcaster`; the events vec parameter goes.
- `Command::execute(g, Ctx::Fresh | Ctx::Replay) -> Applied = Done(outcome, inverse) |
  Skipped(Gone | Stale)` replaces the 20 empty compounds.
- Off-lock work: `session save` persists, serializes and fingerprints under the guard, zips off
  it, and clears dirty only if `revision` did not move. The download zips off the lock. Rescans
  leave the lock once the runtime owns the catalog (§1).
- Logs push through a `log::set_listener` watch. `/params` reads a `LiveHub` of per-uid watches
  that the status drain fills at `LIVE_PERIOD` pacing and the transaction tail refreshes.
- Undo is server-owned: entries carry a label, an opaque navigation context and a merge token;
  write replies carry this actor's `{undo, redo}` labels; the browser stack and about 40
  `_recordGraphCmd` sites go, which fixes the viewer desync.
- **Continuous motion.** A gesture sends ordinary value ops with an envelope `merge` token; the
  commit folds same-token, same-`merge_key` entries into one undo entry. The client keeps one
  send in flight per control, latest value wins, and shows its local value until the reply's
  doc version has reached the replica. Scripts and tests send the same token.

**Taken out before, and why.** A per-socket op queue that held an op's events behind its reply
was built and removed: a queue beside the op path is a second scheduler, and a slow op parked
everything behind it. A rate limiter inside `useLiveValue` was built and removed because the
gesture design had to follow the op path. The gesture design above now follows it.

Commits: 1 `node_touched_clear` (−5, +25 test); 2 dispatch through `spawn_blocking` (+60/−10);
3 notifiers (+110/−40); 4 `Txn` (+280/−230); 5 typed events (+200/−150); 6 `execute(Ctx)`
(+70/−60); 7a-c the op trait and `arms.rs` split into `ops/<group>.rs` (+900/−1200); 8 generated
op types (+250/−150); 9 server-owned undo (+150/−350); 10 gestures (+120/−20); 11a save and
download off the lock (+60/−20); 11b rescans off the lock, after §1 (+150/−60); 12 the tail
projects touched paths only, after §2 (net −100).

## 4. One node runtime, one protocol per boundary

- Signal and graphics host nodes run `NodeRuntime` (`goofi-host/src/runtime/mod.rs`) on a
  sequenced, acked `InSlot`/`OutSlot`/`SetParam` protocol (`wire.rs:19-38`, acks `mod.rs:292-325`,
  planner `goofi-signal/src/runtime/plan.rs`). Audio and graphics control use `goofi-control`: a
  10 ms tick and a whole-state `Desired` with dedup (`goofi-control/src/lib.rs:28`, `:31-48`).
- A graphics host node runs BOTH: a `goofi-control` half and a `NodeRuntime` thread behind the
  `Local` mailbox (`producer.rs:118-176`, `goofi-host/src/local.rs`), which turns the whole-state
  diff back into `SetParam` (`local.rs:69-82`).
- Duplicates: `desired_of`/`rides_the_plan` (audio `lib.rs:555-614`, graphics `:274-320`); the
  pulse gate (control `:689-697`, host `mod.rs:531-540`); the `common` group
  (`goofi-host/src/lib.rs:139-143`, graphics `:147`, `producer.rs:61-81`); two fault mechanisms
  (`NodeRuntime::set_fault` `mod.rs:784`, `Faults::settle` control `:95`).
- `NodeRuntime` evaluates bindings with the previous run's `now` (`mod.rs:519`, set at `:641`).
- In-process built `.rs` nodes msgpack every param and input frame on every call
  (`goofi-host-sdk/src/host.rs:117-140`). `HostedNode` and `RemoteNode` are near copies with
  different frames (`goofi-signal/src/hosted.rs:22-28`), cold-start timeouts (30 s, 60 s) and
  hand-over. The Python child runs `setup()` lazily on the first process call, so a pulse or
  refresh can reach an instance whose setup never ran (`goofi-pymod/src/serve.rs:92-140`); it
  never runs `stop()`. Neither Python tier has `now`.
- Children poll: `Exchange::ask` re-publishes every 1 ms (`goofi-transport/src/lib.rs:495-518`),
  children poll every 500 µs (`hosted.rs:176`, `serve.rs:70`).
- Graphics and audio `.rs` files always load in-process; each rebuild after boot leaks a library
  (`producer.rs:28-41`, `goofi-audio/src/scan.rs:47-62`). `leak_manifest` leaks once per changed
  file (`goofi-node/src/describe.rs:197-268`). `open` checks the crate version, not the SDK hash
  (`goofi-build/src/lib.rs:294-310`).
- Port cleanup depends on struct field order at seven sites (`transport.rs:70-72` and others).
  The subprocess service omits `max_nodes` (`goofi-transport/src/lib.rs:445-453`). Status and
  control loans are swallowed (`transport.rs:336-341`, `:388-390`); failed opens in
  `goofi-control` are dropped and, because of dedup, never retried (`:434`, `:492`). Slots at
  index 64 and above get no doorbell (`seam.rs:353`).
- `doorbell_driven` is true in every real engine and feeds only `NodeView.rings`.
- The cpal callback `try_lock`s the runtime and renders silence on contention; that is by design
  for authoring events, but contention counts as an xrun and `render_into` has no panic guard.
  The graphics tick has no `catch_unwind`; a panic poisons the runtime mutex
  (`goofi-graphics/src/lib.rs:186`, `:247`, `:567`).
- The codec encoder casts lengths with `as` (`goofi-codec/src/lib.rs:31-176`, `:498-536`) and
  `expect`s at seven sites; decode errors are dropped in `goofi-control` (`:591-610`) and the
  transport (`:177`, `:236`, `:375`).

**Design.** One per-node control runtime, `goofi-runtime`, merged from `goofi-control` and the
host runtime; three node models stay (block DSP, shader, host node). The shared `Core` holds the
mailbox handle, `Desired`, one `PortBundle`, `Bindings` (delivery, `evaluate_where(time.now())`,
the pulse edge), one `FaultState` and the stamps. An `Executor` trait (`arrive`,
`params_changed`, `pulse`, `refresh`, `next_wake`, `run`) has three implementations; graphics
host producers use the signal executor in the control thread, and `Local` goes.

- Whole-state `Desired` with dedup replaces the ack planner. The acks never closed the one-shot
  race: a frame published before the subscribe is lost either way. Two additions: an
  `Applied { version, refused }` report (a refusal is a process fault, and the engine re-sends),
  and a producer rings a target once when it first applies it.
- One child RPC: `trait Call` with in-process, hosted and Python implementations behind one
  `CodecNode`; the frame is `[entry][now][payload]` for all; Python gets an eager `Setup` and a
  `Stop`; both directions wake on iceoryx2 events after a readiness handshake.
- The native ABI stays bytes, but inputs cross as borrowed GOOF frames, params only as deltas,
  and outputs are written into the loan (`frame-copies.md`). Graphics host `.rs` files built
  after boot run hosted, as signal does; audio keeps loading in-process and never unloads
  (`audio-engine.md`). Leaked manifests are interned by type and describe hash.
- `PortBundle<P>` drops its ports before its node; one `ServiceKind` table; the SDK hash rides
  the version symbol; one `Clock` enum and a `Timer` whose `catch_unwind` runs inside the held
  guard; cpal contention gets its own counter and `render_into` a panic guard;
  `doorbell_driven` and `NodeView.rings` go.
- `EncodeError` on size limits; an undecodable input frame is a process fault on its wire.

Commits: A codec errors (+170/−70); B port bundle, service table, reported loan loss (+110/−70,
wheels); C SDK hash (+20/−6); D `doorbell_driven` (+5/−40); E `Clock`, `Timer`, panic isolation
(+90/−40); F `Call` and `CodecNode`, eager setup, event wake (+220/−280, wheels); G1 shared
bindings and faults (+160/−210); G2 signal on `Desired`, planner deleted, `transport.rs` situation
rewritten (+380/−820); G3 graphics without `Local` (+90/−220); G4 one `desired_of` (+70/−110);
H graphics hosted after boot (+35/−5); I native ABI (+220/−160); J manifest interning (+25/−5).
Net about −900.

## 5. An ownership tree, one session type, one boot path

- `Kind` claims to be the shutdown order (`goofi-core/src/registry.rs:9`) but nothing releases
  by kind. `Kind::Path` is never leased: `scratch()` (`goofi-transport/src/lib.rs:265`) and the
  mount (`goofi-bridge/src/lib.rs:364`).
- Most threads start detached and are never joined: `term.rs:156,181,239`,
  `plugins.rs:158,190,237,262`, `arms.rs:1714,2033`, `reducer.rs:367`, `record.rs:227`.
- Session state is three statics in two crates (`session.rs:64`, transport `:178`, `:206`) with
  an `atexit` hook (`:190-194`) beside `release_session()` (`goofi-cli/src/main.rs:212`).
  `goofi-client/src/lib.rs:19-23` is a third sweep, with a plain `remove_dir_all`.
- PTY harnesses have a second stop policy: a sleeping stop thread, a reap that polls every 25 ms
  (`term.rs:212-232`) and a hand-made lease. The PTY master is their liveness channel.
- `sessions()` deletes dead directories while listing them (`session.rs:210-220`).
- The harness rebuilds the boot by hand (`goofi-tests/src/lib.rs:188-231`) and skips plugins,
  `ensure_packages` and evaluator registration; tests lock `state.graph` at about 60 sites.
- Stopping polls: `Child::poll` (`child.rs:206-214`), `stop_recording` (`lib.rs:273`),
  `wait_released` (transport `:816`, called by three engines).
- `goofi-core` mixes the data vocabulary, the supervisor and the boot screen. The recorder's
  `Session` (`goofi-record/src/lib.rs:102`) is not the session.

**Design.**

- `goofi-supervisor` holds `child`, `worker`, `scope` (absorbing `registry`), `session` and `log`
  with a progress sink; `goofi-core` keeps the vocabulary; `startup` and indicatif move to the
  CLI.
- `Scope` owns `Child`, `Worker`, `Port`, `Path` and `Device` handles plus `Finish` steps
  (plugin `on_stop`, recorder finalize). `close()` stops the subtree, then makes one pass per
  `Kind` over the whole tree: finish, children on one shared deadline, workers joined, ports,
  paths, devices. `boot` returns a `Manager { state, scope }` that is not `Clone`; dropping it is
  the shutdown. Every detached spawn above gets an owner; the record beat goes; PTY children
  join through a `Stoppable` trait and reap on exit watches.
- One `Session` value: `hold()` locks a sibling `<id>.alive` file before the directory exists,
  so there is no rename window; `release()` is idempotent and runs on drop; `list()` is
  read-only and `sweep_dead()` has its own name. Transport takes an `Iox` handle built from the
  session; the statics and the `atexit` hook go. The binary releases explicitly on the
  serve-panic and second-Ctrl+C paths; a test binary that exits leaves its record for the next
  boot's sweep.
- `goofi_bridge::boot(Config) -> Manager` serves the binary and the harness. Plugins,
  requirements, the evaluator and fixture registration are `Config` fields. Situations move to
  ops and harness probes; `AppState.graph` becomes crate-private.
- Timed child waits (pidfd, `WaitForSingleObject`); recording stop and engine release join
  their workers.

Commits (★ = Phase 0): ★1 read-only `list()` and `sweep_dead` (+25/−30); ★2 `sync::Mutex` (§6);
★3 one tokenizer (§6); ★4 child stderr level (§6); ★5 boundary panics (§6); ★6 timed waits and
joins (+90/−45); ★7 recorder `Session` becomes `Take` (±30); ★8 the supervisor split
(+160/−110); ★9 the `Session` value (+220/−290); 10 `Scope` and `Manager` (+260/−170); 11 plugin
scopes (+60/−45); 12 PTY scopes (+80/−70); 13 Windows process control (see below); 14
`boot(Config)` (+230/−260); 15 situations off direct graph locks, and a harness gate on live
instances replaces `RUST_TEST_THREADS` (+110/−130); 16 error enums (§6).

`windows-cleanup.md`: only the `exit()`/atexit item falls out of §5. Commit 13 needs a Windows
host: `CREATE_NEW_PROCESS_GROUP` so Ctrl+C stays with goofi, a Job object per child with
`KILL_ON_JOB_CLOSE` and `CTRL_BREAK` for a graceful stop in place of `taskkill`, and a blocking
console-close handler bounded by the scope deadlines. The session race falls out of ★9.

## 6. Errors, locks, lexers

- `Result<_, String>` appears about 485 times (bridge 135, graph 74, record 47, core 44);
  `GoofiError` has one variant.
- 245 `lock().unwrap()` in production code (bridge 176) and 66 in tests, beside 53 poison-
  tolerant sites; one panic poisons a shared mutex.
- Boundary panics: `hold(...).expect` (transport `:187`), `iox_config`'s `expect`s on a long base
  path (`:321-322`), `new_mount` (`goofi-bridge/src/lib.rs:365`), `fresh_id` (`session.rs:60`),
  `nonce_hex`, `to_value(batch).unwrap()` (`lib.rs:1149`).
- Three lexers scan expressions and disagree on string literals; none handles `#` comments
  (`expr_rewrite.rs:169`, `goofi-node/src/lib.rs:240`, `:297`).
- Child stderr is always logged at `Level::Error` (`goofi-core/src/log.rs:139`).

**Design.** Error enums only where a caller branches or a boundary needs context: transport
(keeping the iceoryx2 kind, so the Windows handle limit is a named fault), record, build, and
`io::Error` in the supervisor; graph and bridge errors become §3's `OpError`.
`goofi_core::sync::Mutex` returns std's guard (so `Condvar` works) and logs a poisoned lock once
by name; it replaces every `lock().unwrap()`. The boundary panics return errors. One tokenizer,
`goofi_node::expr::tokens`, handles escapes, triple quotes, f-strings and comments for all three
scans. Child stderr defaults to warning; a traceback or an `ERROR:` line is an error.

## Smaller items

- Graphics records through `set_recorder` from its render thread (`goofi-bridge/src/lib.rs:207`,
  `goofi-graphics/src/lib.rs:227`); fold into §4's executor.
- The SPA npm build and the nested cargo builds of shipped nodes run inside
  `goofi-bridge/build.rs`; make them an explicit step, separately from this plan.
- Split the codec's subprocess protocol (`codec/lib.rs:497-636`) from the frame format with §4.F,
  and the rest of the transport file into names, services and exchange after ★9.
- `AppState` (about 25 fields) splits with §3.7's per-group modules. The repeated pos parsing
  (`arms.rs:348,392,766,1513`) and `AddNode` capture (`command.rs:704,965`, `lib.rs:3405`) go
  with typed args and `admit`. Actors become a newtype; the `record start` plugin mutex
  (`lib.rs:1391`) moves into its op.
- Not in this plan: a reader for the npy + sidecar format. It is a feature and needs its own
  entry.

## Open decisions

Recommendations first; each needs the user's word before its commit.

**Cross-cutting**

1. Phase order above (§3, §4 and §5 foundations before §1). *Yes.*
2. TypeScript generation. The §1/§2 plan wants ts-rs (it emits declarations into the existing
   `contracts.rs` regeneration check with no npm step); the §3 plan wants schemars plus an
   in-repo emitter (ops need JSON Schema for MCP anyway). *ts-rs for every TypeScript type;
   schemars only on op `Args` for MCP and argument declarations.*
3. Per-socket dispatch. The §3 plan proposed an ordered lane per socket; that is close to the
   queue taken out before. *Dispatch each op on its own blocking task and let the graph mutex
   order them; the client's one send in flight per control keeps a gesture in order.*

**§1/§2**

4. Replace merge patches with path ops and drop the rule that document leaves cannot be null.
   *Yes (changes an AGENTS.md fact).*
5. The patch stores param values only; values a type no longer declares drop on load or paste.
   *Yes.*
6. A node's class derives from the catalog, not pinned per instance. *Derive.*
7. A load with bad links drops them and warns. *Yes.*
8. `LinkKey` is `"{out}.{slot}>{in}.{slot}"`, derived. *Yes.*

**§3**

9. A compound holds the graph for its whole run, reads included. *Yes.*
10. `op list` returns structured args, no DSL string. *Yes.*
11. A stale undo entry is removed and reported. *Yes.* Navigation context is restored after the
    flip. *Yes.*
12. The graph stays a `Mutex`, not an `RwLock`. *Yes.*
13. Plugin editor-window knob bursts share a merge token per node and param. *Yes.*

**§4**

14. Drop the ack planner for `Desired` + `Applied` + ring-on-new-target; an identical variables
    value re-sent no longer counts as an arrival. *Yes.*
15. Python: eager setup and `stop()` now; add `now` to the API; no `on_param_changed`, because a
    Python node reads `self.params` on every run. *Yes.*
16. Native ABI stays bytes with borrowed input frames and param deltas. *Yes.*
17. Manifests are interned, not moved to `Arc`. *Yes.*
18. Data services keep no history; the one-shot race stays a documented property. *Yes.*

**§5/§6**

19. Remove `atexit`; a test binary's record waits for the next boot's sweep, and the situation
    `a_process_that_exits_without_releasing_leaves_no_record` goes. *Yes.*
20. Keep one read-only process `session::id()` for part-file tags. *Yes.*
21. Engines take an explicit `Iox` handle. *Yes.*
22. An own `sync::Mutex`, not parking_lot. *Yes.*
23. `Kind::Finish`, so shutdown is a drop. *Yes.*
24. Keep the name `goofi-core` for the vocabulary. *Yes.*
25. Tests boot with no evaluator unless the `Config` asks for one. *Yes.*
