# 06. Runtime modes

The graph, the nodes and the engines are a library that allows execution (parts 03 to 05). A
runtime mode is a scheduler over that library: it decides when a node runs and what it is handed.
There are two modes. Both run the same graph and the same nodes, unchanged.

| | Live mode | Kahn mode |
| --- | --- | --- |
| Program | goofi-pipe | goofi-data |
| Time | The patch clock follows the wall clock. | The patch clock is data time. |
| Cable | One cell: latest wins. | A bounded queue: every frame, in order. |
| Slow consumer | Skips frames. | Holds its producer back. |
| Purpose | A person plays the patch now. | A recipe runs over a dataset, lossless. |
| Result | Depends on timing. | The same on every run. |

## 6.1 What both modes share

- **Events per slot.** Each input slot has a coupling, declared by the node and changed per
  instance by the user:
  - `each`: every frame that arrives on the slot is an event that runs the node.
  - `zip`: the slot belongs to a group; one event occurs when every slot of the group holds a
    fresh frame.
  - `hold`: the slot never causes an event; the node reads the value it holds.
- **Timers.** A node with `autotrigger` runs at its frequency without an input event. The common
  params of a node (part 04.3) set the frequency and a maximum rate.
- **A pulse** runs a node once on demand.
- **One run, one settle.** A run reads its inputs, calls the node, publishes the outputs, stamps
  each frame, and then clears the inputs that the node declares as cleared. Input clearing takes
  effect after a successful run and before the next drain of the inputs.
- **Stamps.** Every frame carries the stamps of the run that made it (part 02.6). Stamps are
  never inherited.
- **A change of the patch** reaches a running node at a settle (part 03.8): new wiring, new
  param sources and new armed outputs are applied whole between two runs.
- **A failure** in a run goes to the error channel of the node. The node stays in the graph and
  runs again at its next event.
- **A feedback edge** is legal. The mode knows which edges close a loop.

## 6.2 The live mode

- There is no global tick. Each signal node waits for its own events and its own timer. Audio
  and graphics run on clocks of their own (part 05).
- A cable holds the newest frame only. A node that is slower than its producer sees the newest
  frame at each run and never a backlog.
- An `each` or `zip` event with several frames waiting delivers the newest.
- A node never blocks its producer, and a producer never waits for a consumer.
- The maximum rate of a node caps how often it runs; events that arrive faster coalesce.
- Node processing never runs under the graph lock. An edit of the patch does not wait for a node.
- A node hosted in another process (a Python subprocess node, a Rust node built after the start)
  behaves as an in-process node: it schedules itself, and its death is seen and repaired by the
  session (part 05.1).
- The clock of the live mode can be driven by a test harness in place of the wall clock.

## 6.3 The Kahn mode

### 6.3.1 The guarantee

- A patch, a sample of a dataset (streams of frames with their instants) and a seed go in. The
  output streams come out the same, frame for frame, however fast or slow the run executes, and
  again at a later date.
- Equality is bit for bit on one machine and one binary. The graphics plane is equal within a
  tolerance across machines, because the reduction order of a GPU can differ.
- A node that reads the wall clock, a device or an unseeded library on its own is outside the
  guarantee. The SDK gives the deterministic paths: the patch clock and a seeded random
  generator.

### 6.3.2 The rules

1. **Time is data time.** The patch clock does not advance by itself. The time of a run is the
   instant of the event that caused it. Stamps and the `t` of an expression carry that instant.
   A timer keeps its meaning in data time: a 10 Hz timer emits at instants 0.1 s apart.
2. **A cable is a queue.** It carries every frame in order. A producer ahead of its consumer waits
   and never drops.
3. **A node runs once per event, in instant order.** With several `each` slots, the node takes
   the events in the order of their instants, not of their arrival. Equal instants are ordered by
   the declared order of the slots.
4. **A `hold` slot is read by instant.** At an event at instant t, a hold slot gives the latest
   frame with an instant ≤ t.
   A param source that reads a node, a variable or a playhead is the same read: it gives the
   value at the instant of the event.
5. **A feedback edge is one frame of delay.** At instant t it gives the latest frame of the
   producer with an instant < t.
6. **Randomness is seeded per run and per node.** A repetition block with n seeds gives n
   reproducible samples.
7. **Sources are the streams of the dataset**, and timers in data time. A stream ends, the end
   propagates, and the run finishes.

### 6.3.3 Progress

- Each queue carries, beside its frames, the progress of its producer: the instant up to which no
  further frame follows. The producer advances it after every run, whether or not the run
  emitted a frame, and sets it to the end when its stream closes.
- A node takes its earliest pending event at instant t when the progress of every input has
  reached t. A feedback input needs progress only to the previous instant of the node.
- A node without a pending event forwards the lowest progress of its inputs, so a consumer
  downstream does not starve.
- The graph is acyclic apart from feedback edges, and every node advances its progress, so no
  node waits forever.

### 6.3.4 Engines, machines, hosted nodes

- The audio engine is one node of the network with a periodic schedule in data time: it renders
  block k at k·B/sr when the progress of its inputs has reached that instant.
- The graphics engine is one node of the network: it renders frame n at n/fps the same way.
- The signal-plane inputs of an engine are `hold` reads at the instant of the block or frame.
  The outputs of an engine are frames with instants.
- The state machines of a patch (part 03.6) are a node of the network, stepped at their intervals
  in data time.
- A hosted node is the same node; the mode owns the scheduling on whichever side it runs.

### 6.3.5 Replication

- A stateless node can run as k workers on one queue. The outputs are put in frame order again
  before the next queue.
- The scheduler of goofi-data (part 10.4.2) sets k, the depth of each queue and the number of
  network instances.

## 6.4 The interface of coupling

- A node in the editor has a vertical bar on its left side with one control per input slot. The
  control sets the coupling: `each`, `zip` with its group, or `hold`.
- The manifest of a node declares the default coupling of each slot.
- The same setting drives both modes.

## 6.5 Verification

- A situation runs one patch over one recording twice at different speeds and asserts equal
  outputs.
- A model of the rules was run against random graphs and random schedules (part 12.4 holds the
  result).

## 6.6 Open decisions

1. The default coupling of an input slot.
2. The depth of a queue: fixed, or set by the scheduler from its memory limit.
