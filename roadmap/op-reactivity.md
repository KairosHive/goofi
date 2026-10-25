# Reactive settle: an op pays for what it changed

## Why

Every write op runs one tail (`Txn::commit` → `settle_and_project` → `reconcile_and_broadcast`)
that walks the whole graph whatever the write touched: every engine recomputes every node's
`Desired`, `derive_bindings` re-parses every expression, the followers walk every node, and
`replica` rebuilds and serializes the whole document for a diff. Measured 2026-10-24 (`op_cost`,
`op_parts`, release, the node threads running beside the op thread): a param edit p50 5.5 ms at
50 nodes, 53 ms at 200, 309 ms at 500; a node move, which no engine reads, 39 ms at 200 and
269 ms at 500. At 200 nodes the projection is 15 to 77 ms and the settle 8 to 13 ms; at 500 the
settle is 42 to 85 ms, superlinear, because `desired_of` scans every edge per slot. A drag step
pays the same tail, so the op thread serves one step per 53 ms at 200 nodes and every other op
waits behind it on the graph lock. Target: a flat sub-millisecond op at 500 nodes.

## The model

The backend holds a document tree, as a browser holds a DOM, and the settle is its flush. The
design follows a fine-grained reactive runtime (Svelte 5's signals): a **source** is a field a
command writes; a **derived** is a memo computed from sources and other deriveds, kept until an
input moves, and compared on recompute so an unchanged result stops the wave (equality cut-off);
an **effect** is a delivery to the world (a `Desired` sent to a node, a document op broadcast, a
follower told). A batch (one `Txn`) writes any number of sources; one flush marks what each write
reaches, recomputes the dirty deriveds in dependency order, then runs the effects whose inputs
moved. Nothing is recomputed that no write reached; nothing is delivered that did not change.

One decision departs from Svelte: dependencies are **declared, not tracked**. Svelte records what
a derived read while it ran; here the structure is known (a binding reads the names it spells, a
`Desired` reads its node and its wires), so each level declares its inputs and the flush is a
fixed topological order. Tracking through `&self` reads would need interior mutability on every
getter and buy nothing the structure does not already say.

### The levels, and what each depends on

```
Patch
├─ library (manifests)                → every node's typed params
├─ variables  name → {value, control, expression, lock}
│                                     → the bindings that spell `variables.<name>`, the feeds
│                                       that name it, the variable's own bind, the follower
├─ machines                           → the machines thread, the document
├─ arrangement, viewpoint             → the document
├─ links                              → edges (ports resolved) → both endpoints' Desired
└─ node (leaf, facade or port)
   ├─ identity  name, pos, scope, kind → the document record; the name: every binding that
   │                                     spells `nd('<name>')` and every multi-slot consumer
   │                                     downstream (its `source` label)
   ├─ literals  values               → typed (memo) → its own Desired consts, the document record
   ├─ sources   key → authored text  → the binding (memo per key) → the node's Desired subs,
   │                                     the referenced-groups set, the document record
   ├─ wiring    edges in, edges out  → its Desired (subs, targets) and its neighbours' Desired
   ├─ runtime   generation, health   → its Desired and its neighbours' (the service names), the
   │                                     armed set, the live pair
   ├─ arming    record               → its Desired, the armed set, the document record
   └─ watch     watched outputs      → its Desired
Engine
├─ plan (audio, graphics)             ← every node of the engine and the edges among them
└─ published facts (`system.*`)       → variables
Document (derived)                    ← every node record, links, variables, machines, arrangement
```

### The flush, in order

1. **Mark.** Each write set a bit in the ledger: per node `identity | literals | sources(key) |
   wiring | runtime | arming | watch`, per patch `library | variables(names) | machines |
   arrangement | links`. The mark pass spreads each bit along the arrows above, through two
   reverse indexes kept with the deriveds: `referrers` (a name or a variable → the `(uid, key)`
   bindings that spell it, unresolved names included, so a node born later reaches the binding
   that waited for it) and the **edge index** (uid → edges in, uid → edges out). A rename marks
   its referrers' sources and its downstream multi-slot consumers; a link marks both ends; a
   rebirth marks the node and its neighbours; a variable marks its referrers and feeds.
2. **Derive**, each level once, dirty entries only: typed (the memo exists); the bindings of the
   dirty keys, with `moved` deciding whether the node's Desired is marked; the edges, rebuilt
   only when `links` or a port moved; the engine plans of the engines with a dirty node.
3. **Deliver.** `Engine::settle` receives the dirty node set and computes `Desired` for those
   nodes alone, `wires_into` and `ringers` answered by the edge index in O(degree);
   `send_if_changed` is the cut-off. The followers run on their bits alone: the armed set on
   `arming | runtime`, the referenced groups on `sources | literals of a feed | variables`, the
   machines thread on `machines`, the live pair of a node on its own `runtime | sources`.
4. **Project.** The document is kept as state (the bridge's `GraphDoc` already is). For each
   dirty node its record is serialized once and diffed against the record the document holds,
   which yields that node's ops; a removed node is one `del`; the other roots are diffed only
   when their bit is set. `replica()` runs for the seed of a new socket and after a load, never
   in the op tail. The whole-document `diff_ops` stays as the parity check.

The status drain's settle is the same flush: a drain that staged nothing has an empty ledger and
costs nothing (today 2 to 25 µs on a graph with no expressions; a graph with expressions pays
`derive_bindings` whole on every wake, which this removes).

### What goes

`Graph::touched` (a `Vec<Touched>` scanned per key), `Runtime::changed`, `resettle` and each
engine's `dirty` flag fold into the one ledger. `derive_bindings`' whole walk, `uid_by_name`'s
scan per reference (the referrers index answers it), `resolved_edges` per settle, `build_view`'s
per-node allocations (a `NodeView` is built from the leaf's memos on demand), and `replica()` in
the op tail.

## Phases

Each phase lands with a probe number before and after, and a fixture that fails on the broken
variant and passes on the fix, five runs each way.

1. **The ledger and the engines.** The dirty bits replace `touched`; the edge index; the mark
   pass; `Engine::settle` over the dirty set. Fixtures: a link cut re-targets the producer; a
   rebirth re-subscribes every consumer; a rename reaches a multi-slot consumer's source label.
   Probe: `settle` flat across 50, 200 and 500 nodes.
2. **Projection by record.** The per-node record diff; `replica()` out of the tail. Fixture: after
   a sequence of every op kind, the replicated document equals a whole `replica()` (parity).
   Probe: the whole op sub-millisecond at 500 nodes.
3. **Bindings by reference.** The referrers index; dirty keys derived alone. Fixtures: `nd('x')`
   binds when `x` is born after it; a removed variable errors its readers; a rename follows.
   Probe: a graph of 500 expression sources, the idle settle and a param edit both flat.
4. **Followers on their bits.** Fixture: a recorder arming lands after an unrelated param edit
   (no regression), and an unrelated edit sends the recorder nothing.
