# Node triggering and the lossless mode

A signal node runs today on `autotrigger`, on a frame at a `trigger_process` input, or on the
`common.trigger` pulse (`goofi_runtime::common`). A wire is a slot, latest wins; a node parks on
its door and takes the newest frame on every wire; `cx.now`, the stamps and every `t` read the
wall-based patch clock; audio and graphics run on clocks of their own, driven by hand under
`Clock::External`; the machines thread reads `time.now()`. Every one of these lets the wall clock
decide what a node sees, so a patch run twice over the same data is not the same run. goofi-pipe
needs a lossless mode, where no packet is ever dropped and every pipe between nodes is a queue,
and goofi-data reuses the same node graph for its recipes (`goofi-data.md`). Status: planning,
decided 2026-11-13 and 2026-11-16. `triggers.md` is the separate item for events delivered once
to each listener.

## Decisions

### The guarantee

- **Lossless and deterministic.** A patch, a sample of a dataset (streams of frames with their
  instants) and a seed go in; the output streams come out the same, frame for frame, however fast
  or slow the run executes, and again later. Equality is bit for bit on one machine and one
  binary. A GPU's reduction order can differ between machines, so the graphics plane is equal
  within tolerance across machines. A node that reads the wall clock behind goofi's back, a
  device, or an unseeded library is its author's non-determinism; the SDK offers `c.now` and a
  seeded `c.rng()` as the deterministic paths.

### The rules of the lossless mode

1. **Time is data time.** The patch clock does not advance by itself. `cx.now` of a run is the
   instant of the frame that caused it; `procstart`, `procend` and `t` in an expression carry that
   instant. `max_frequency` and `autotrigger` keep their meaning in data time: an LFO at 10 Hz
   emits at data instants 0.1 s apart. A node that reads only the patch clock is therefore
   deterministic in this mode.
2. **A wire is a queue that carries every frame, in order.** No overwrite; a producer ahead of
   its consumer waits (back-pressure) and never drops. goofi-pipe's live mode keeps the slot.
3. **A node runs once per trigger event, in instant order.** A setting per input slot says what
   an event is: `each` (every frame on this slot is an event), `zip` (one event when every slot of
   the group holds a fresh frame), `hold` (never an event; the slot is read). With several `each`
   slots the node processes their frames in instant order, not arrival order.
4. **A `hold` input is read by instant.** At an event at instant t, a hold slot holds the latest
   frame with instant ≤ t.
5. **A feedback edge is one frame of delay.** At instant t it delivers the producer's latest frame
   with instant < t. `goofi_runtime::schedule` already knows which nodes close a loop.
6. **Randomness is seeded per run and per node.** One generator each, from the run's seed and the
   node's uid. A repetition block with n seeds is n reproducible samples.
7. **Sources are the dataset's streams**, plus timers in data time. A stream ends, the end
   propagates, and the run finishes.

### Execution: a Kahn network

- **Threads and queues, as the live mode.** Each node is a thread on its own door; each wire is
  a bounded queue in the transport's queued mode; the sender parks when the queue is full.
- **Progress.** Every queue carries, beside its frames, the producer's progress: the instant up to
  which no further frame will follow, advanced after every run whether or not it emitted, and set
  to the end at the close of the stream. A node takes its earliest pending event at instant t
  once every input's progress has reached t; a feedback input needs progress to the node's
  previous instant only. The graph is acyclic apart from feedback edges and every node advances
  its progress when it runs, so no node waits forever.
- **Audio and graphics are Kahn nodes of the same network.** Each engine is one node with a
  periodic event schedule in data time: the audio engine renders block k at k·B/sr, the graphics
  engine renders frame n at n/fps, each once its inputs' progress has reached that instant. Their
  signal-plane inputs are `hold` reads at the block or frame instant; their outputs are frames
  with instants. Audio and graphics effects thus apply over datasets deterministically.
- **The machines thread** is a node of the network too, stepped at its intervals in data time.
- **Children.** A Python subprocess node or a hosted Rust node is the same node over the exchange;
  the lossless mode holds whatever side of the exchange the loop runs on.

### The per-slot interface

- **A setting per input slot.** The node gets a new vertical bar on its left side that exposes a
  setting per input slot: `each`, `zip` (with its group) or `hold`. The same setting drives the
  live mode: `each` and `zip` are the triggering slots, delivered latest wins; `hold` is a held
  reference input. `trigger_process` goes; the manifest declares the default per slot.
- **One modification to the node UI, one tweak to the backend.** The bar is a goofi-pipe UI
  modification to the nodes; the backend's node triggering logic takes a small tweak.

## Remaining work

1. The coupling setting per slot in the manifest, the SDK and the ops; `trigger_process` removed
   with every reader; the left-side bar.
2. The queued mode of the transport: a bounded queue whose sender parks, with the progress word.
3. The data clock: `timing::Time` driven by the frames' instants in the lossless mode; the host
   executor's `next_wake` and rate cap in data time; instant-ordered delivery across `each`
   slots; the hold read by instant; the feedback delay.
4. The engines as Kahn nodes: audio blocks and graphics frames at data instants, their inputs
   read by instant.
5. `c.rng()` in the SDK, seeded per run and node; `Noise` and the machines' draws on it.
6. A situation: one patch over one recording, twice at different speeds, the outputs equal.

## Open

- The right default for input slot coupling.
- The queue depth per wire: fixed, or set by goofi-data's scheduler from its memory limit.
