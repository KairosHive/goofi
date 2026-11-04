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

- The settle still walks every node for its view, every link for its edges and compiles both
  plans whole; the status beat, the live fill and the halves recompute on their clocks:
  `roadmap/reactivity.md` holds the measurements and the remaining work.
- `Buffer` memmoves its whole window on every frame: a ring, so a frame costs one copy.
- A Python node copies an array three times out and once in, rebuilds its `params` namespaces
  every call and reads `_is_gil_enabled` twice a call: cache the namespaces, rebuild on
  `on_param_changed` alone, one GIL read per tick.
- `HostExecutor::source` rebuilds and remaps the whole upstream lineage on every emission; a
  32-node chain stays slow uncapped. Measure the lineage's share of a deep hop before changing it.
- The shipped example patches bind `max_frequency` to `vars.system.default_ufreq`, a name
  no variable has: rename to `signal.default_ufreq`.

## Audio

- Growth on the callback: `render_block` grows the fifo, `Inbox::fill` resizes on a channel
  count that grew, `output_stream` resizes scratch when the device buffer grows, and `apply`
  discards a failed `outbox.push`. Reserve off the callback; verify a reachable full outbox
  before treating the drop as a defect.
- `Pool::run` waits for every helper with no bound, and the overrun check runs after `process`
  returns: the clock cannot finish a block while a helper is descheduled. Measure the pool's
  crossover for cheap nodes, deep chains and wide graphs before adding helpers.
- Measure `contended`, `late` and the pool waits on a real device, with and without the
  real-time grant. `contended` now counts only a device open, a retune and a full message ring.
- The device open and a node's birth run through `ready::prepare` and land at a settle; verify
  on a real device that the stream keeps playing through a device switch and a VST3 birth.

## Graphics

- The `Tap` ring holds one slot where `Screen` and `Record` hold two: give it two. Every tap
  lands in its own mapped buffer; one staging buffer per tick would be one map for all.
- A tap is read back at `system.viewer_fps`, not at the display rate a page declares below it:
  a 24 Hz display under the 30 fps cap downloads six frames a second it never paints.
- Only audio asks for a real-time grant and a seat on the performance cores: the render clock
  and the signal workers compete with the bridge's encoders at ordinary priority. Move the grant
  to the supervisor and give the render clock one below audio's.
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
