# Reactivity: what each op and each clock still pays

The patch is a value and the settle a diff of two snapshots (`roadmap` decisions below hold; what
shipped is in AGENTS.md). What remains is where a settle, a clock or a half still recomputes whole.

Measured (release, Linux, the node threads running beside the op thread), before the restructure
and after it:

| what | before | after |
|---|---|---|
| param edit p50, 200 nodes | 53 ms | 3.3 to 4.2 ms |
| param edit p50, 500 nodes | 309 ms | 16 to 20 ms |
| node move p50, 500 nodes | 269 ms | 8 to 12 ms |
| `settle`, 500 nodes | 42 to 85 ms | 1.3 to 21 ms |
| `projection`, one record | (whole replica 67 to 190 ms) | 20 to 160 µs |
| quiet patch, manager CPU, 200 nodes | 18 ms/s | 5 ms/s |
| quiet patch, one expression per node | 46 ms/s | 9 ms/s |

The settle is the rest of the op, and it is not flat yet: it builds the view for every node,
collects every typed memo, resolves every edge, offers every viewer want against every edge and
compiles the audio and graphics plans whole, on every settle. The runs above spread with the load
of the node threads; the p50 columns are the reference.

## Decisions

The patch is a value whose snapshots share untouched parts, consumers compare pointers at commit,
and undo is one history of snapshots for every client — the Zustand store, not Elm or Redux, since
the ops are functions and stay so. Dependencies are neither declared nor tracked: they fall out of
the tree, and the few that cross nodes are spelled once where the change set is read off the two
snapshots (`Graph::diff`): a changed edge reaches both ends, a reborn node its neighbours, a name
or variable that moved the bindings that spell it, a library scan every node. The variable store
stays shared with the data plane: its values are frames, its configuration is versioned and
snapshotted. What is runtime stays out of the snapshot: instances, health, generations, the
compiled bindings, the watched outputs, the viewers' wants.

## Where it still applies

Gains are measured where a probe exists and marked **estimate** where they follow from the cost
shape and the trigger rate alone. N nodes, E edges, S sources, P params, B bindings, K plugin
params, H heads.

### A. The settle's remaining whole walks

| where | recomputes | scoped to | gain |
|---|---|---|---|
| `build_view` | a `NodeView` with its bindings vector for every leaf, per settle | the view built once per snapshot, nodes borrowed; or built for the changed set and the neighbours the engines ask for | O(N) allocations per settle → O(changed) (estimate) |
| `typed` map, `resolved_edges`, `offer_view_wants` | every leaf's memo collected, every link resolved, every want scanned against every edge, per settle | memos on the snapshot's pointers; the edge index kept across settles; wants offered per changed key | O(N + E + W·E) → O(changed) (estimate) |
| audio and graphics `plan::compile` | the whole plan, every settle, even one that changed a param alone; graphics sends it with no comparison | a stage memo per node, the order a memo over the edges, `Plan: PartialEq` | part of the settle figure (estimate) |
| `derive_variables` | every variable expression re-derived when any node changed | the referrers of what moved, as a param's binding is | O(variables) parses per settle → O(moved) (estimate) |

### B. The manager's clocks, remaining

| where | recomputes | trigger | scoped to | gain |
|---|---|---|---|---|
| status beat | per uid: hex, ufreq, error, tier, stage; `leaves.contains` O(N) each; `last.retain` O(N²); `live_pairs` builds JSON per driven node | every 500 ms | the uids a report touched, births and removals | 2 ms/s at 200 nodes (measured), quadratic growth (estimate) |
| `LiveHub::fill` | every watched node's pair serialized and string-compared | every 50 ms and every op | nodes with a `ParamValues` or `BindingErrors` report | in the drain figure; the pair memo also feeds the beat |
| follower periodic pass | every computed variable with no stream behind it re-evaluated, time-reading or not | every viewer interval | expressions that declare `t` | 2 to 3 ms/s idle (measured) → O(time-dependent) (estimate) |
| MIDI poll `resync` | `referenced_groups` whole under the graph lock; `group_taken` walks the layout per port | every 2 s | the listed set; the memo the settle's `groups` mark already gates | small CPU; one graph lock take per 2 s → 0 (estimate) |

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

### E. Indexes

`uid_by_name` and `name_taken` (per reference, per add), `library_entries` (per type lookup, per
typed rebuild, O(T²) per scan), the link scans in `stub_feed`, `port_inner` and `add_link`'s
`retain`, `ports_of` and `scope_members`, `rename_*`'s re-parse of every expression (the `refs`
already held answer it), `has_group` and `claim` O(V) per playhead, `feed_decls` filtering a
static manifest per call, `Hub::entry`'s `retain` sweep per miss. Each is O(N), O(E), O(T) or
O(V) per call where an index kept on the snapshot's pointers answers in O(1).

### F. Couplings that remain

- `Graph::epoch` is one counter for the whole graph: a knob drag reads as "the graph moved" to
  every reducer and `/data` socket, each of which takes the graph lock to re-derive its address.
  It stays as the lock-free version for readers outside the graph, and should stop being the gate:
  the settle knows which `(uid, slot)` re-homed.
- `Machines::take` answers true for `Configure`, so a model handed over unchanged still advances
  every head; the thread is now configured on the machines mark alone, so this costs one advance
  per machine edit.

## Order of work

Each step lands with a probe number before and after and a fixture that fails on the broken
variant and passes on the fix, five runs each way.

1. **The settle flat** (A): the view and the indexes kept across settles on the snapshot's
   pointers, the plans as memos, the variables derived by referrer. Probe: `settle` under a
   millisecond at 500 nodes.
2. **The engines' control halves** (C): `replan` as a bit on the moved cell, faults and status as
   per-node memos, the VST3 mirrors as dirty params, the audio plan applied by stage.
3. **The clocks, remaining** (B): the beat and the live pair from one per-node memo, the follower
   on declared time, the reducers re-homed by key instead of the epoch.
4. **The halves and the frame paths** (D): wake on demand, `Desired` applied by field,
   `ParamValues` as a delta, the render tick's groups cached, the lineage memoized. Probes:
   `hop_latency`, `graphics_lag`, and `idle_cost` with audio and graphics nodes.
5. **Indexes** (E), as each step reaches them.
