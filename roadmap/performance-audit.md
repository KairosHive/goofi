# Performance: remaining work

From the audit of 2026-10-22 (commit `e1149d1` holds its measurements and probes, under
`backend/goofi-tests/examples/`) and an external audit of the same tree. The rule every item
serves: signal, audio and graphics processing have equal protection from editor work,
persistence, recording and display work. Each fix lands with a fixture that fails on the broken
variant and passes on the fix, five runs each way; a test asserts the state reached, never a
rate or a latency.

## Measure again

- The audit's probes on the new data plane: the hop (`hop_latency`), the producer cap table
  (30, 1 000 and uncapped), the audio control tick, the graphics tap lag (`graphics_lag`) and
  the viewer pace. Targets: a busy hop under 5 µs, a 1 kHz cap delivering 1 kHz, a 60 fps viewer
  cap delivering 60.

## Signal

- `settle` returns before `derive_bindings` when nothing was touched and no engine is dirty;
  it derives only the sources a touched node owns; an engine's settle visits the touched nodes
  and their neighbours only. Measured: a param edit plus settle cost 10 ms at 200 nodes.
- `Graph::replica` runs on every op and rebuilds every node's record (550 µs a node at 200
  nodes, 133 ms an op). Keep the projection as state and patch it per command; cache a node's
  record and invalidate it on its own edit. Measure which of `typed`, `node_type` and `scope_of`
  carries the cost first. Target: a flat sub-millisecond op at 500 nodes.
- `Buffer` memmoves its whole window on every frame: a ring, so a frame costs one copy.
- A Python node copies an array three times out and once in, rebuilds its `params` namespaces
  every call and reads `_is_gil_enabled` twice a call: cache the namespaces, rebuild on
  `on_param_changed` alone, one GIL read per tick.
- `HostExecutor::source` rebuilds and remaps the whole upstream lineage on every emission; a
  32-node chain stays slow uncapped. Measure the lineage's share of a deep hop before changing it.
- The shipped example patches bind `max_frequency` to `variables.system.default_ufreq`, a name
  no variable has: rename to `signal.default_ufreq`.

## Audio

- `remove` and `persist` take the runtime lock the device callback renders under; the callback
  plays silence for the period (`contended`). Let the callback drop a removed box through the
  outbox; read states for a save through savers, or copy the slab's states on the callback.
- Growth on the callback: `render_block` grows the fifo, `Inbox::fill` resizes on a channel
  count that grew, `output_stream` resizes scratch when the device buffer grows, and `apply`
  discards a failed `outbox.push`. Reserve off the callback; verify a reachable full outbox
  before treating the drop as a defect.
- `Pool::run` waits for every helper with no bound, and the overrun check runs after `process`
  returns: the clock cannot finish a block while a helper is descheduled. Measure the pool's
  crossover for cheap nodes, deep chains and wide graphs before adding helpers.
- Measure `contended`, `late` and the pool waits on a real device, with and without the
  real-time grant; only audio asks for one, so the equal-protection rule is not yet met for
  signal workers and the render clock.
- The device open and a node's birth run through `ready::prepare` and land at a settle; verify
  on a real device that the stream keeps playing through a device switch and a VST3 birth.

## Graphics

- The viewer's serve is a pace grid: serve on the frame's arrival with the cap as a floor
  between serves. The `Tap` ring holds one slot where `Screen` and `Record` hold two: give it
  two. Let the half's wake be the readback's own (a bell from the render thread when a slot is
  ready), not a tick.
- `library refresh` validates every shader with naga on the op thread: 170 to 370 ms per refresh
  with one 1 600-line shader (`compile_stall` probe). Validate through `ready::prepare` and
  report the refusal at the settle after.
- Submission is not completion: `tick_max_us` excludes `drain_inbox`, nothing budgets GPU
  completion, and `Chain::configure` can fall back to `Fifo` presentation. Verify the mode and
  adapter on the user's display; time completion apart from submission.
- An f32 viewer declaration on a 512² stage delivered no frame in 180 s on lavapipe; not
  reproduced on a GPU.

## Recording

- Measure the per-stream workers against a fast signal producer, an armed audio pair and a
  video at 1080p60: queue fill, drops and the manifest cadence.
