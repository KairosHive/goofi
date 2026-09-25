# The graph, with more than one engine

Designed with the user 2026-08-29 and hardened by an adversarial audit the same day. The `Engine`
trait, the settle point and the crate carve are built; `backend-architecture.md` §1 owns the
Patch/Runtime split that follows. This file keeps the seam decisions that bind every engine, and
what remains at the seam.

## The three parts

**The graph** (`goofi-graph`) is the op authority for the MODEL: every op is a command with an
exact inverse, serialized under one lock, producing one delta and one undo history. It routes each
op's propagation to the engine that owns the node, through one trait, and never computes a service
name or touches an endpoint.

**An engine** is the authority for RUNTIME state: the node instances, their health, their
within-engine transport, and its own node library. The graph never sees within-engine transport.

**The transport** (`goofi-transport`) is cross-engine communication, one shared mechanism for every
engine: iceoryx2 names and rendezvous, and a resolver that is a PURE function from
`(instance, uid, slot, generation)` to a service name and its config — a phone book, not a
switchboard. Engines and the bridge depend on it; the graph never does.

## Decisions that bind

**The seam is a small trait, not twelve notifications** — twelve notifications is
decisions-from-unsettled-state. `insert` and `remove` stay explicit, because a birth mints a
generation and is not derivable from settled state; there is NO restart method — a restart is
graph-side record work plus remove+insert with a fresh generation, and remove purges the engine's
pending requests for that uid. `clear` is N removes plus ONE settle; `load` is clear plus N inserts
plus one settle. A removal derived from absence-in-the-view would be the engine-observes-the-graph
mirror this file rejects. `drain` stays a PULL, woken by the report-side notify.

**Engines stay in the manager's process** (2026-09-15, after a process per engine was written
out). What needs isolation is isolated per node — a Python node in a subprocess, a Rust node built
after boot in a host child — and the one crasher an engine boundary would not fix is a VST3
instance, which is a plugin-hosting question (`audio-engine.md`).

**A type id is `engine:Name`.** Two engines may offer one name; the record always stores the
qualified id, a bare name resolves only while one engine offers it, and the palette derives the
engine from the id. Adding an engine is one line at the composition root plus its library; nothing
engine-specific enters the graph.

**Settle carries a touched set, because a settled view has no delta.** Link and topology collapse
into settle bare; param and expression edits do not, because a `SetParam` is a write with effects
and an engine-side last-shipped copy of every record would be a third mirror. The one op path
records `Touched`, including the bindings a batch INVALIDATED. One settle per batch, from settled
state. The drain-side settle must not deliver while a batch is open (`hold_settle`/`release_settle`
live on the graph because the drain is another thread), and a `Touched` entry naming a node the
batch also removed is dropped at settle.

**Only the health projection crosses the seam.** `Ack` and `Ready` are consumed inside the engine;
the shared `Status` keeps the six health variants and the signal wire's `WireStatus` nests it
(`goofi-host/src/runtime/wire.rs`).

**Nothing polls to discover; a clock only paces.** Cross-engine delivery is latest-wins everywhere
— engines do not run in sync, so a queue between them lies about time. An in-order crossing is an
explicit BRIDGE node owned by the scheduled engine (`SignalIn`). Modulation crosses as a reference
at control rate.

**Cross-engine wiring is derived names from settled state, never a protocol.** After a batch every
engine reads the same settled `GraphView`; the producer ensures a publisher under the derived name
and rings the consumers' doorbells, the consumer subscribes to the same name, and `open_or_create`
is the rendezvous. Eventually convergent, not ack-phased: a mis-ordered settle costs at worst one
missed wake on a continuous stream.

**A scheduled engine's clock thread has no doorbells**; it drains its boundary before each tick.
Its control half IS doorbell-driven, woken like a signal node (`audio-engine.md`). No intermediate
engine proxy channel.

**Generations are PROCESS-LIFETIME and live on the runtime** with `instance`, riding `GraphView`
as resolver inputs: minted at settle, when a birth at a uid is inserted, surviving `clear()` and
`load_doc`, never entering the archive — or a reloaded uid re-opens its predecessor's stale service names.

**Node state splits by WRITER**: the record (op-written) and `Health` (drain-written) sit beside
each other on the graph; birth is construction, so a rebirth's Health is a fresh struct. Accepted
cost: a standing error reads young after a restart. A refreshable `Str` param's live options are an
OVERLAY beside Health, and the drain never writes the record.

**`GraphView` presents port-resolved leaf-to-leaf edges**, computed once at the settle point; a
port with nothing behind it resolves to NO edge. Never raw links, or every engine re-implements the
relay walk.

**Each engine owns its library and its scan.** A node source root holds one folder per engine,
`nodes_<engine id>`; the graph keeps the merged palette view, the unavailable overlay and the
provenance, and normalizes a caller's params against the owning engine's declaration.

**The rejected alternative.** "Each engine owns its node set and observes the graph" is a mirror.
Engines as enum variants inside `Graph` put iceoryx2 and a device library in one crate with the
model. A stateful cross-engine broker is rejected: derived names need no negotiation.

**`goofi-codec` stays beside `goofi-transport`, not merged.** The codec is a FORMAT contract —
pinned by a golden, mirrored by the frontend's TS decoder — where the transport changes with
iceoryx2; the pymod's FT-host build uses the codec and stays iceoryx2-free.

**No rename rides along.** Where two words exist for one thing, unify to the incumbent.

## Remaining

- **`Kind` has no exhaustive match.** `NodeEntry::leaf`/`leaf_mut` and `Graph::stub`
  (`goofi-graph/src/lib.rs`) use `_ =>`, so a fourth variant compiles silently classified "not a leaf".
  Replace them with explicit `Kind::Facade | Kind::Port(_)`.
- **`keys_touching`** (`goofi-signal/src/engine.rs:132`) is O(N × (links + bindings)) and runs once
  per node reaching `Ready`. A large patch will notice.
- **`Graph::contains`** means "is a running leaf", `exists` "is any node", `wirable` "leaf or port";
  `Command::precondition` picks a different one per variant. One pass to name them for what they
  answer.
- **`goofi-graph` depends on `goofi-view`** beside `goofi-core` and `goofi-node`. Decide whether the
  view vocabulary belongs below the graph or the dependency goes.
