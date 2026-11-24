# Triggers

A trigger is an event a node runs on: it happened once, at an instant, and every node that
listens runs once for it. goofi has no such thing today. A signal node runs on `autotrigger`,
on a frame at a triggering input, or on the `common.trigger` pulse (`goofi_runtime::common`);
a state machine says what happened through variable frames (`<playhead>.state`, `.prev`,
`.progress`, `.arrived`) and `st().arrivals` and `st().departures` count what happened AT the
instant they are read. A node that wants to act on a transition must read those on the tick
that carries it: a node slower than the machine misses a short state, and a node faster than
it sees one transition on several ticks. The same holds for a threshold crossing in a signal
and for a MIDI note: the node that wants the edge has to be the one looking when it passes.
The TouchDesigner model is the reference: a cook is caused, and a pulse is delivered to every
listener exactly once.

## Decisions

- A transition is an event the machine thread emits as it happens, not a value a reader
  samples. The frames it publishes today stay, for readers that want the state as a value.
- A trigger is delivered to each listener once, in order, whatever the listener's rate; a node
  that runs for a trigger runs once per trigger it was sent, not once per tick it is on.
- Triggers ride the transport (`goofi_transport`), not the variable bus: a queue per listener,
  bounded and oldest-dropped like the recorder's, never a slot (a slot holds the latest and
  would merge two pulses into one).
- The trigger a transition fires carries the playhead, the states left and entered, and the
  patch instant. The `machine event` op (`roadmap/state-machine.md`, item 2) fires a transition
  from outside by the same path; `event {name}` and `always` triggers consume one.

## Remaining work

1. A trigger kind on the signal plane: an input slot a node declares as a trigger listener,
   with the Python and Rust SDK form (`sdk/README.md`) and `nd().on(...)` in expressions.
2. The machine thread (`goofi_bridge::machines`) emits a trigger per transition and per
   arrival; a state's `interval` due is one too.
3. Sources beyond the machine: a threshold edge on a signal (`rising`, `falling`), a MIDI note
   on the bus, a `trigger` op for the CLI and the agent harness.
4. Each delivery is observable: a trigger counts in `node stats`, a drop is a gap in its index,
   and the recorder can arm a trigger stream.

## Open

- Whether an audio node consumes a trigger at the block or at the sample: the transport hands
  the audio thread a queue it drains at the block, and a sample-exact onset is the state
  machine's "exact step onsets" question (`roadmap/state-machine.md`), not a new one.
- Whether a trigger carries a payload (the machine's attribute values at the instant) or only
  its identity, with the listener reading values off the bus as it runs.
