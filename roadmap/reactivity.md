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

The backend holds a document tree, as a browser holds a DOM, and a settle is its flush. The
design follows a fine-grained reactive runtime (Svelte 5's signals): a **source** is a field a
writer sets; a **derived** is a memo computed from sources and other deriveds, kept until an
input moves, and compared on recompute so an unchanged result stops the wave (equality cut-off);
an **effect** is a delivery to the world (a `Desired` sent to a node, a document op broadcast, a
follower told, a file written). A batch writes any number of sources; one flush marks what each
write reaches, recomputes the dirty deriveds in dependency order, then runs the effects whose
inputs moved. Nothing is recomputed that no write reached; nothing is delivered that did not
change; nothing runs on a clock that can run on an event.

One decision departs from Svelte: dependencies are **declared, not tracked**. Svelte records what
a derived read while it ran; here the structure is known (a binding reads the names it spells, a
`Desired` reads its node and its wires), so each level declares its inputs and the flush is a
fixed topological order. Tracking through `&self` reads would need interior mutability on every
getter and buy nothing the structure does not already say.

### The primitive, and who owns it

`goofi_supervisor::react`, beside `timing` and `ready`: `goofi-supervisor` is under every crate
that needs it (core, node, graph, runtime, the engines, the bridge, record) and holds no goofi
knowledge of its own. The ops use it; the graph uses it; neither owns it. Four pieces:

- `Version`: a counter a source bumps on write. Every source is a plain field plus a version;
  no wrapper type, so a `&mut` write stays a `&mut` write.
- `Memo<T>`: a derived value with the versions of its declared inputs it was computed from, a
  recompute closure, and `PartialEq` on `T` for the cut-off. `memo.read(inputs, compute)` answers
  the held value when every input version matches, else recomputes, compares, and bumps its own
  version only when the value moved.
- `Ledger<K, Bits>`: the dirty set of one owner, keyed by what the owner addresses (a uid, a
  `(uid, slot)`, a variable name) with a bitset per key naming which part moved, plus root bits
  for the owner's singletons. Writers `mark`; the flush `take`s. A mark pass spreads bits along
  declared arrows through reverse indexes the owner keeps (referrers, edges in and out).
- `Door`: the cross-thread edge, which is `timing::Event` as it stands. An owner on another
  thread is an effect: it is rung with the keys that moved, never told to re-read everything.

Each owner (the graph, an engine's control half, a node half, the bridge's workers, the
recorder) runs its own flush under its own lock; the primitive holds no lock and no thread.

### The graph's levels

```
Patch
├─ library (manifests)                → every node's typed params, a type → entry index
├─ variables  name → {value, control, expression, lock}
│                                     → the bindings that spell `variables.<name>`, the feeds
│                                       that name it, the variable's own bind, the follower,
│                                       the machines that read it
├─ machines                           → the machines thread (the edited machine), the document
├─ arrangement, viewpoint             → the document
├─ links                              → edges (ports resolved) → both endpoints' Desired
└─ node (leaf, facade or port)
   ├─ identity  name, pos, scope, kind → the document record; the name: every binding that
   │                                     spells `nd('<name>')` and every multi-slot consumer
   │                                     downstream (its `source` label)
   ├─ literals  values               → typed (memo, per key) → its Desired consts, the record
   ├─ sources   key → authored text  → the binding (memo per key) → the node's Desired subs,
   │                                     the referenced-groups set, the document record
   ├─ wiring    edges in, edges out  → its Desired (subs, targets) and its neighbours' Desired
   ├─ runtime   generation, health   → its Desired and its neighbours' (the service names), the
   │                                     armed set, the live pair, the status beat
   ├─ arming    record               → its Desired, the armed set, the document record
   └─ watch     watched outputs      → its Desired
Engine
├─ plan (audio, graphics)             ← the engine's nodes and the edges among them; a stage is
│                                       a memo over its node, the order a memo over the edges
└─ published facts (`system.*`)       ← the device and the rate, not the settle
Document (derived)                    ← every node record, links, variables, machines, arrangement
```

The flush: **mark** (writers set bits; the pass spreads them through the referrers and edge
indexes), **derive** (typed, the bindings of dirty keys, the edges when links moved, the plans
of engines with a dirty node), **deliver** (`Engine::settle` over the dirty set, `Desired` built
for those nodes with `wires_into` and `ringers` answered by the edge index; the followers on
their bits), **project** (each dirty node's record serialized once and diffed against the record
the document holds; `replica()` only for a socket seed and after a load).

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

Each step lands with a probe number before and after and a fixture that fails on the broken
variant and passes on the fix, five runs each way.

1. **The primitive** in `goofi_supervisor::react`, with its own tests of the semantics (a memo
   recomputes once per input move, an equal result does not bump, a ledger takes what was marked).
2. **The graph's ledger and the engines** (A: `settle`, the edge index, `Engine::settle` over
   the dirty set; C: the plans as memos, `replan` as a bit on the moved cell). Probe: `settle`
   flat across 50 to 500 nodes. Fixtures: a link cut re-targets the producer; a rebirth
   re-subscribes every consumer; a rename reaches a multi-slot consumer's label; a graphics size
   move settles no audio node.
3. **Projection by record** (A). Fixture: the replicated document equals a whole `replica()`
   after every op kind. Probe: the whole op under a millisecond at 500 nodes.
4. **Bindings and indexes** (A, E): the referrers index, dirty keys derived alone, the type and
   name indexes. Fixtures: `nd('x')` binds when `x` is born after it; a removed variable errors
   its readers. Probe: 500 expression sources, the idle settle and a param edit both flat.
5. **The clocks** (B): the drain's settle on an empty ledger, autosave on the doc version and
   the watcher's paths, the beat and the live pair from one per-node memo, the variables producer
   and the machines thread rung with keys. Probe: `idle_cost` at 200 nodes with expressions
   under 5 ms/s.
6. **The halves and the frame paths** (D): wake on demand, `Desired` applied by field,
   `ParamValues` as a delta, the render tick's groups cached. Probes: `hop_latency`,
   `graphics_lag`, and `idle_cost` with audio and graphics nodes.
