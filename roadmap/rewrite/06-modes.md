# 06. Runtime modes

The graph, the nodes and the engines are a library that allows execution (parts 03 to 05). A
runtime mode is a scheduler over that library: it decides when a node runs and what it is handed.
There are two modes. Both run the same graph and the same nodes, unchanged.

| | Live mode | Kahn mode |
| --- | --- | --- |
| Program | goofi-pipe | goofi-data |
| Time | The patch clock follows the wall clock. | The patch clock is data time. |
| Cable | One cell: latest wins. An event cable is a queue. | A bounded queue: every frame, in order. |
| Slow consumer | Skips frames. | Holds its producer back. |
| Purpose | A person plays the patch now. | A recipe runs over a dataset, lossless. |
| Result | Depends on timing. | The same on every run. |

## 6.1 What both modes share

- **A trigger** is what causes one run of a node: a frame on an `each` slot, a complete `zip`
  group, a tick of the timer of the node, or an event (part 03.7) on an event slot, on a pulse
  param or on the common `trigger` param.
- **Triggers per slot.** Each input slot has a coupling, declared by the node and changed per
  instance by the user:
  - `each`: every frame that arrives on the slot is a trigger that runs the node.
  - `zip`: the slot belongs to a group; one trigger occurs when every slot of the group holds a
    fresh frame.
  - `hold`: the slot never causes a trigger; the node reads the value it holds.
- **Timers.** A node with `autotrigger` runs at its frequency without an input trigger. The
  common params of a node (part 04.3) set its rate: the frequency of its timer and the cap on
  its runs.
- **The common `trigger` param** runs a node once per event that it receives (part 03.7.4).
- **Events are never merged.** An event slot has the coupling `each` by default and delivers
  every event, in both modes. A `hold` slot of kind event gives each event once.
- **One run, one settle.** A run reads its inputs, calls the node, publishes the outputs, stamps
  each frame, and then clears the inputs that the node declares as cleared. Input clearing takes
  effect after a successful run and before the next drain of the inputs.
- **Stamps.** Every frame carries the stamps of the run that made it (part 02.6). Stamps are
  never inherited.
- **A change of the patch** reaches a running node at a settle (part 03.9): new wiring, new
  param sources and new armed outputs are applied whole between two runs.
- **A failure** in a run goes to the error channel of the node. The node stays in the graph and
  runs again at its next trigger.
- **A feedback edge** is legal. The mode knows which edges close a loop.

## 6.2 The live mode

- There is no global tick. Each signal node waits for its own triggers and its own timer. Audio
  and graphics run on clocks of their own (part 05).
- A cable holds the newest frame only. A node that is slower than its producer sees the newest
  frame at each run and never a backlog.
- An `each` or `zip` trigger with several data frames waiting delivers the newest. Events are
  the exception: each event is a trigger of its own and waits in a queue (part 03.7.3).
- A node never blocks its producer, and a producer never waits for a consumer.
- The rate of a node caps how often it runs. Data triggers that arrive faster coalesce; events
  wait and do not coalesce.
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

The rules below include the corrections that a model of the rules made necessary (part 12.4).

1. **Time is data time.** The patch clock does not advance by itself. The time of a run is the
   instant of the trigger that caused it. Stamps and the `t` of an expression carry that instant.
   A timer keeps its meaning in data time: a 10 Hz timer emits at instants 0.1 s apart.
2. **A cable is a queue.** It carries every frame in order. A producer ahead of its consumer waits
   and never drops.
3. **A node runs once per trigger, in instant order.** With several `each` slots, the node takes
   the triggers in the order of their instants, not of their arrival.
4. **Equal instants have a total order.** Triggers at one instant run in the declared order of
   the slots, then in queue order within a slot. A `zip` group counts at its first slot.
5. **A `hold` slot is read by instant.** At a trigger at instant t, a hold slot gives the latest
   frame with an instant ≤ t. The read waits until the progress of that input has reached t.
   A param source that reads a node, a variable or a playhead is the same read: it gives the
   value at the instant of the trigger. A read of an event at instant t takes the pending
   events with an instant ≤ t, one per read (part 03.7.7).
6. **A `zip` trigger is defined by instant.** It occurs at the instant at which the last slot of
   the group gets a frame newer than the previous zip trigger. Each slot gives its latest frame
   at or before that instant, and the frames at or before it are consumed.
7. **A feedback edge is one frame of delay.** At instant t it gives the latest frame of the
   producer with an instant < t. The read waits until the progress of the edge says that no
   frame before t follows.
8. **Randomness is seeded per run and per node.** A repetition block with n seeds gives n
   reproducible samples.
9. **Sources are the streams of the dataset**, and timers in data time. A stream ends, the end
   propagates, and the run finishes. A timer and an engine end at the horizon of the run: the
   last instant of the streams of the dataset.

### 6.3.3 Progress and back-pressure

- Each queue carries, beside its frames, the progress of its producer: a bound on the next
  frame. It has two forms: "no further frame before t" and "no further frame at or before t".
- A node publishes "none before t" when it takes a trigger at t, before it sends a frame. It
  publishes "none at or before t" when no other trigger at t can occur. A node without a pending
  trigger forwards the lowest bound of the inputs that can trigger it: the instant at the head of
  a queue, else the progress of the input. A node sets the progress to the end when its stream
  closes.
- The progress of a timer or an engine is its next tick. It does not depend on a slow `hold`
  input.
- A node takes its earliest pending trigger at instant t when the progress of every input has
  reached t.
- **A sender never waits inside one instant.** The bound of a queue counts instants: a frame
  with the instant of the newest frame in the queue is always admitted.
- **A `hold` queue and a feedback queue drain by progress.** The node removes the frames it can
  no longer need: all but the latest at or before the lower bound of its next trigger.
- **A slot that can cause no further trigger discards its frames.** A node whose streams have
  ended discards all of its inputs. A `zip` group ends when one of its slots has ended and is
  empty, and then discards the frames of its other slots.
- With these rules the graph has no deadlock and no dependence on the schedule, for every queue
  depth from 1.

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

- A stateless node can run as k workers on one queue. The outputs are put in the order of the
  input triggers again before the next queue.
- The stage that restores the order owns the progress of the replicated node: it is the bound of
  the oldest trigger that has not left.
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
- A model of the rules was run against random graphs and random schedules. Part 12.4 holds the
  result. The cases of the model that failed under the first wording of the rules become
  fixtures of the situation.

## 6.6 Open decisions

1. The default coupling of an input slot.
2. The depth of a queue: fixed, or set by the scheduler from its memory limit. A burst of
   frames at one instant is not bounded by the depth (6.3.3); its memory cost is not measured.
3. Pairing by index. The model shows that a `zip` that pairs the n-th frame of each slot
   deadlocks on bounded queues when the slots differ in rate, and that a `zip` by instant does
   not. A recipe pairs the samples of a tuple by index (part 10.4.1); the two sides then have
   equal counts by construction. Decide whether pairing by index exists as a coupling of its
   own for that case.
4. Not covered by the model: real threads (a torn read of a frame and its progress), instants
   as floating point numbers, a feedback slot with `each` coupling, replication of a node with
   a `hold` or feedback input, several `zip` groups on one node, hosted nodes and the machines.
