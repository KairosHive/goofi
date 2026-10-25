# Reactivity: one dependency ledger for the backend

## Why

The backend recomputes whole where a change is local, and re-derives on a clock what moves only
on an event. The op tail walks the whole graph on every write; the status drain re-derives every
expression twenty times a second; autosave walks the workspace and archives the patch every five
seconds on a quiet patch; every engine recomputes every node's `Desired` on every settle; a size
cell moving on one graphics node settles every engine. Each place keeps a flag, a mirror or a
poke of its own to limit the damage (`resettle`, `Engine::dirty`, `Shared::replan`, `last` maps,
`retap`, `poke_all`, `Stamp`), and none of them says what depends on what.

Measured 2026-10-24/25 (`op_cost`, `op_parts`, `idle_cost`, release, Linux):

| what | 50 nodes | 200 nodes | 500 nodes |
|---|---|---|---|
| param edit, p50 | 5.5 ms | 53 ms | 309 ms |
| node move, p50 | 2.5 ms | 39 ms | 269 ms |
| of which `settle` | 0.3 ms | 8 to 13 ms | 42 to 85 ms |
| of which `replica` | 0.7 ms | 15 to 77 ms | 67 to 190 ms |
| quiet patch, manager CPU, no expressions | | 18 ms/s | |
| quiet patch, manager CPU, one expression per node | | 46 ms/s | |
| of which the status drain | | 3 → 24 ms/s | |
| of which autosave | | 12 to 14 ms/s | |

The node move costs what the param edit costs although no engine reads a position. The status
drain's 21 ms/s is `derive_bindings` re-parsing 200 texts at 20 Hz on a patch nobody touches.
Autosave's 12 ms/s is a workspace walk, a whole archive and its YAML every five seconds.

## The model

The patch is a value. Each commit publishes a new snapshot that shares every untouched part with
the one before: the node map holds an `Arc` per node, and links, machines and the arrangement sit
behind an `Arc` of their own. A write does `Arc::make_mut` on the one node it touches, so a
snapshot is a clone of pointers and the change set after a batch is one pointer compare per
node. The design is the Zustand store rather than Elm or Redux: the ops stay the plain functions
they are and call the graph; there is no message enum and no reducer. Consumers subscribe with a
selector and run at commit, once, on the settled snapshot: each engine, the projection, the
followers, the recorder and autosave compare the part they read by pointer and recurse into what
differs. Readers on other threads hold the last snapshot and never take the writer's lock.

Dependencies are neither declared nor tracked. They fall out of the tree: a node's record changed
when its pointer did. The few inputs that cross nodes are spelled once, where the change set is
computed from the two snapshots: a changed edge reaches both its ends, a reborn node reaches its
neighbours, a changed name or variable reaches the bindings that spell it, and a library scan
reaches every node.

Undo is one history of snapshots. A transaction takes the snapshot before its first write and
pushes it with its label at commit; a preview publishes without pushing, and the commit that ends
a drag pushes the snapshot from before the drag's first step. Undo and redo swap a snapshot in and
let the settle's diff deliver the consequences: births, removals, re-derivations and the document
ops. There is one stack for every client, tab, shell and agent, so the command inverses, the
replay tolerance for stale targets, the forward planners for layout inverses and the per-actor
bookkeeping go. The variable store is shared with the data plane, so its values stay in place and
its configuration is snapshotted at history time through `config` and put back through `restore`.

What is not in the snapshot is runtime: instances and their health, generations, the compiled
bindings, the watched outputs and the viewers' wants. The derived bindings move to the runtime,
keyed by `(uid, key)`, and re-derive for changed nodes and for every node on the rare global moves.

### The graph's levels

```
Patch (a snapshot)
├─ nodes   uid → Arc<NodeEntry>      identity, literals, authored sources, viewers, arming
├─ links   Arc<IndexMap>             → edges (ports resolved) → both endpoints' Desired
├─ machines  Arc<IndexMap>           → the machines thread, the document
├─ arrangement  Arc<Layout>          → the document
└─ variables (shared store)          values on the data plane; configuration snapshotted
Runtime
├─ bindings  (uid, key) → derived    ← the node's sources, the names, the variables, the library
├─ instances, generations, health    ← the engines' reports
└─ watched, view wants               ← the reducers
Engine
├─ Desired per node (memo)           ← the node, its edges, its neighbours' identity and generation
└─ plan (audio, graphics)            ← the engine's nodes and the edges among them
Document                             ← per changed node, diffed against the record it holds
```

The settle: the changed set from the two snapshots, the bindings of changed nodes derived,
`Engine::settle(view, changed)` with `wires_into` and `ringers` answered by an edge index, the
followers run on the roots that moved, the document patched per changed node, and the snapshot
kept as the one the next settle diffs against.

## Where it applies

Gains are measured where a probe exists and marked **estimate** where they follow from the
cost shape and the trigger rate alone. N nodes, E edges, S sources, P params, B bindings, T
types, H heads.

### A. The op tail (every write, every drag step)

| where | recomputes | scoped to | gain |
|---|---|---|---|
| `Graph::replica` → `fragment(all_uids)` | every record, every link, serialized whole, each root cloned again | the dirty records, diffed in place | 15 to 77 ms → under 1 ms at 200 nodes (measured parts; target) |
| every `Engine::settle` | `desired_of` for every live node; `wires_into` and `ringers` scan every edge and every binding per slot: O(N·E) | the dirty nodes and their neighbours, O(degree) | 8 to 85 ms → under 1 ms (measured settle; estimate) |
| audio and graphics `plan::compile`, `sizes` twice, `follow_windows` | the whole plan per settle; graphics sends it with no comparison (`Plan` has no `PartialEq`) | a stage memo per node, the order a memo over the edges | part of the settle figure above |
| `derive_bindings` | every source re-parsed and re-resolved; `moved` scans `touched` per key; `uid_by_name` scans N per reference | the dirty keys and the referrers of what moved | 1 ms per settle at 200 sources (measured on the drain, below) |
| `update_param` → `Record::get_mut` | drops the node's whole typed memo; the rebuild resolves the type through `library_entries`, a Vec of every engine's every type | one `(group, name)`; a type index | O(T + P) per edit → O(1) (estimate) |
| `sync_followers` | the machines cloned and compared whole; every variable expression rebuilt and re-evaluated by the follower; `referenced_groups` walks every source and every feed; `armed` walks every uid; `set_taps` re-taps every reducer and pokes all | each on its bit: machines, variable binds, source refs and feeds, arming, taps that changed | O(N + S + machines) per op → O(changed) (estimate) |
| `settled_now` → `poke_all`, `settled` watch | every reducer and every `/data` socket takes the graph lock and re-derives its address | the `(uid, slot)` whose wiring or generation moved | O(reducers + sockets) lock takes per op → O(re-homed) (estimate) |
| plugin-editor edits | one whole tail per authored edit, up to 20 Hz | one batched transaction | N tails → 1 (estimate) |

### B. The manager's clocks (a quiet patch)

| where | recomputes | trigger | scoped to | gain |
|---|---|---|---|---|
| status drain → `drain_status` → `settle` → `derive_bindings` | every source, before the "nothing touched" return | every 50 ms and every report | an empty ledger costs nothing | 21 ms/s → 0 at 200 expressions (measured) |
| autosave `tick` | `fingerprint` walks the workspace; `archive` and `to_yaml` whole; `write_variables` re-reads every wide file before the `last` compare | every 5 s, and 500 ms after any settle pulse | a doc version bit, the watcher's paths, the variables whose value moved | 12 to 14 ms/s → 0 when quiet; 60 to 70 ms per tick off the graph lock (measured per second; per tick an estimate) |
| status beat | per uid: hex, ufreq, error, tier, stage; `leaves.contains` O(N) each; `last.retain` O(N²); `live_pairs` builds JSON per driven node | every 500 ms | the uids a report touched, births and removals | 3 ms/s at 200 nodes (measured), quadratic growth (estimate) |
| `LiveHub::fill` | every watched node's pair serialized and string-compared | every 50 ms and every op | nodes with a `ParamValues` or `BindingErrors` report | in the drain figure; the pair memo also feeds the beat |
| variables producer | `variable_ringers` walks every leaf, source and feed under the graph lock, then rings every consumer door and view door | every 500 ms on the timeout, and every op | ringers whose set changed; ring new subscribers only | 1 ms/s CPU (measured), plus a spurious wake of every bound node and reducer twice a second (unmeasured) |
| machines thread | `Msg::Configure` clones and deep-compares every machine; a card drag's `pos` is in the model, so each drag frame reconciles every head and re-evaluates `interval_of` per head | every op | the edited machine; `pos` out of what a run reads | 1 ms/s idle (measured); per drag frame O(H) Python evals → 0 (estimate) |
| follower periodic pass | every computed variable with no stream behind it re-evaluated, time-reading or not | every viewer interval | expressions that declare `t` | O(variables) evals per frame → O(time-dependent) (estimate) |
| MIDI poll `resync` | `referenced_groups` whole under the graph lock; `group_taken` walks the layout per port | every 2 s and every op | the memo above; the listed set | small CPU; one graph lock take per 2 s → 0 (estimate) |

### C. The engines' control halves

| where | recomputes | trigger | scoped to | gain |
|---|---|---|---|---|
| `Shared::replan` pokes | a graphics size or render source moving, a shader compile landing, an audio channel count or ring resize each settle every engine whole | per tick or per frame when a cell moved | the node's own size and its downstream sizes | one full settle (8 to 85 ms) per poke → local (estimate) |
| `schedule` | sorts the ready list on every pop; `order.contains` per node: O(N² log N) | per plan compile | a heap and a set; recompute when edges moved | part of the settle figure (estimate) |
| `Faults::settle` | the whole fault set restated and diffed; audio clones `disabled` and scans `devices()` twice | per settle | per-node fault memos | O(N) per settle → O(changed) (estimate) |
| audio `published()` → `status()` | a `NodeCost` per node and a host enumeration | per settle | the device and the rate changing | O(N) per settle → 0 (estimate) |
| `fit_rings`, `land_births`, `drain`'s `making` scan | every stage matched against every instance; every node visited for a ready | per settle, per drain wake | the instance whose ring moved; `on_ready` sets the bit | O(S·N) → O(changed) (estimate) |
| VST3 mirrors `seen`, `shown`, `known` | K params scanned per block on the audio thread and per settle | per block, per settle | dirty params | O(K) per block → O(changed) on the realtime thread (estimate) |
| audio plan apply | `stages.iter().find` per stage and `reads_inbox` per inbox on the callback thread: O(S²) | per plan swap | a stage index; a per-stage diff | realtime-thread risk, not CPU (estimate) |
| `set_view_demand` | one viewer's want rebuilds `resolved_edges` and re-offers every want to every engine; again on every settle | per demand change, per settle | the one key | O(W·E·engines) → O(engines) (estimate) |

### D. The node halves and the frame paths

| where | recomputes | trigger | scoped to | gain |
|---|---|---|---|---|
| `next_wake` default | every audio and graphics half wakes; `sync()` rebuilds every `Param` with a scan per binding when the node holds any binding; `functional()` re-derived per binding; `open_io` re-reads the device strings | every 10 ms per node | a tap or ring with readers, an arrival, a swap, a const change | N wakes × O(P·B) per 10 ms → 0 when idle (estimate; the signal halves measured 1 to 2 ms/s for 201 nodes, they park) |
| `apply` / `apply_binds` | every const stored, every binding rebuilt (`Expression::new`) and re-evaluated, `take_where` O(B²) | every `Desired` that differs, so every param edit | the field that moved | O(P + B·eval) → O(1 + bindings on it) (estimate) |
| `Status::ParamValues` | the whole evaluated map cloned and the graph's copy replaced; `entry_error` re-derived on every report of any kind | per pass with a moved value; `Ufreq` at 4 Hz per node | a delta per key; the error on `Fault` and `BindingErrors` alone | O(B) per pass → O(changed) (estimate) |
| graphics render tick | three bind groups made per stage, the resolution written, `plan.demanded()` walked | per frame, up to 240/s | plan or size changes; demand when a reader cell moved | O(S) GPU objects per frame → 0 when unchanged (estimate) |
| `HostExecutor::source` | the whole upstream lineage copied and remapped per emission | per emission | a memo of the topology skeleton; the numbers patched | O(lineage) per emission → O(1) (estimate; `hop_latency` to confirm) |
| reducer loop | `asked_box` over every spec per frame, `union_specs` and `plan` per served frame, two store-lock takes for the cap, the taps cloned | per frame per slot | memos on (specs version, frame shape, cap) | O(specs) and lock contention per frame → O(1) (estimate) |
| recorder worker | `armed.lock().clone()` of two Strings per frame | per recorded frame | an `Arc<Armed>` swapped on arm | small (estimate) |

### E. Indexes the ledger needs anyway

`uid_by_name` and `name_taken` (per reference, per add), `library_entries` (per type lookup, per
typed rebuild, O(T²) per scan), the link scans in `stub_feed`, `port_inner`, `sinks`,
`single_input_source` and `add_link`'s `retain`, `ports_of` and `scope_members`, `rename_*`'s
re-parse of every expression (the `refs` already held answer it), `net_instances`'s O(U²)
netting on a load, `has_group` and `claim` O(V) per playhead, `feed_decls` filtering a static
manifest per call, `Hub::entry`'s `retain` sweep per miss. Each is O(N), O(E), O(T) or O(V) per
call where an index kept by the ledger's own marks answers in O(1).

### F. Couplings the ledger makes explicit

- `VariableStore::set` emits but never calls the watch; only `move_value` does. A user's write
  to a variable a transition reads wakes the machines thread through the op tail's
  `Msg::Configure`, not through the write. Declared, the variable's readers are its subscribers.
- `Graph::epoch` is one counter for the whole graph: a knob drag reads as "the graph moved" to
  every reducer and socket. It stays as the lock-free version for readers outside the graph, and
  stops being the gate.
- `Machines::take` answers true for `Configure`, so an unchanged model still advances every head.

## Order of work

One commit per step, the situations and the browser suite green at each, a probe number before
and after where one exists.

1. The roadmap, to this model.
2. The patch as a snapshot in `goofi-graph`: `Arc` per node and per root, authored sources apart
   from derived bindings, the settled snapshot, the changed set by pointer compare,
   `Engine::settle` over the changed set with an edge index; `touched` and `changed` go. Probe:
   `settle` flat across 50 to 500 nodes.
3. Projection by diff in the bridge: ops per changed node from the two snapshots; `replica()`
   for a seed and a load alone. Fixture: the replicated document equals a whole `replica()`
   after every op kind. Probe: the whole op under a millisecond at 500 nodes.
4. One history of snapshots: the `Command` enum, inverses, replay and the per-actor stacks go;
   ops call the graph; previews keep the pre-drag snapshot; the variable configuration rides the
   entry; the labels reach every client. Frontend, CLI docs, skills and tests follow.
5. The clocks: autosave, the variables producer, the machines thread, the taps and the recorder
   follow their root alone; the producer's 500 ms timeout goes. Probe: `idle_cost` at 200 nodes
   with expressions under 5 ms/s.
6. AGENTS.md and the roadmap, with what shipped removed. The halves and the frame paths (D)
   stay as the remaining work.
