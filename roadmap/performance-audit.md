# Performance: remaining work

From the audit of 2026-10-22 (commit `e1149d1` holds its measurements and probes, under
`backend/goofi-tests/examples/`) and an external audit of the same tree. The rule every item
serves: signal, audio and graphics processing have equal protection from editor work,
persistence, recording and display work. Each fix lands with a fixture that fails on the broken
variant and passes on the fix, five runs each way; a test asserts the state reached, never a
rate or a latency.

A second audit on 2026-11-23 measured a scratch copy of the debug build (`cargo run`'s, workspace
crates at opt-level 1, node libraries release) with per-thread `schedstat` deltas, gdb SIGTRAP
stack samples and headless Chrome on the GPU (`--use-angle=vulkan`; without it SwiftShader takes
13 cores). Its baseline, for the steps below to compare against:

| case | goofi CPU (one core = 100%) | browser, GPU |
|---|---|---|
| empty patch, headless | 0.54%, 124 wakeups/s (graphics clock 60, follower 30, status drain 22) | — |
| empty patch, editor open | 0.64% | Chrome 5.7% |
| `psd-topomap`, editor open | 76% (the `Reduce` median alone 41%) | Chrome 35%, 665 KiB/s |
| `cycles.gfi`, editor open | 406% (graphics halves 282%, reducers 104%) | GPU 48%, 42 W |
| `cycles.gfi`, no browser | 34% | GPU 33%, 31 W |

Audio was not measured (it plays through the speakers; use the null sink). A release build was
not measured.

## Measure again

- The audit's probes on the new data plane: the hop (`hop_latency`), the producer cap table
  (30, 1 000 and uncapped), the audio control tick, the graphics tap lag (`graphics_lag`) and
  the viewer pace. Targets: a busy hop under 5 µs, a 1 kHz cap delivering 1 kHz, a 60 fps viewer
  cap delivering 60.
- The table above on a release build, and an audio patch on a null sink.

## Signal

- **An in-process native node pays the codec on every run.** `CodecNode` encodes the request,
  the library decodes it (copying every input's samples in `gather`), encodes the reply into an
  unreserved `Vec`, and the host parses the meta again; `encoded_len` packs each meta a second
  time. Meta travels as `rmpv::Value` trees, and `with_stamps` clones an `IndexMap<String, _>`
  per output. Measured on ten chained `Math` nodes: about 190 µs a run at 200 Hz and 77 µs at
  1 780 Hz, the same for 1 value and 320; in 65 busy stack samples `abi::call` is 43%, mostly
  meta encode and decode and malloc/free. Pack the meta once, make the stamps typed fields with
  `source` an `Arc` shared across outputs, and pass frames in-process by handle, not by codec.
- An autotrigger node whose `max_frequency` evaluates to 0 has `next_wake` in the past and runs
  back to back (`goofi-runtime/src/host.rs`). Found by reading; prove it with a fixture, then put
  a floor on the period or refuse the configuration.
- The settle still walks every node for its view, every link for its edges and compiles both
  plans whole; the status beat, the live fill and the halves recompute on their clocks:
  `roadmap/reactivity.md` holds the measurements and the remaining work.
- A Python node copies an array three times out (`tobytes`, the extract, the cast) and once in.
  Read the array through the buffer protocol; an empty array has no byte view to cast.
- `HostExecutor::source` rebuilds and remaps the whole upstream lineage on every emission; a
  32-node chain stays slow uncapped. Measure the lineage's share of a deep hop before changing it.

## Audio

- Growth on the callback: `render_block` grows the fifo, `Inbox::fill` resizes on a channel
  count that grew, `output_stream` resizes scratch when the device buffer grows, and `apply`
  discards a failed `outbox.push`. Reserve off the callback; verify a reachable full outbox
  before treating the drop as a defect.
- `Pool::run` waits for every helper with no bound, and the overrun check runs after `process`
  returns: the clock cannot finish a block while a helper is descheduled. Measure the pool's
  crossover for cheap nodes, deep chains and wide graphs before adding helpers.
- Any level of two or more stages unparks helpers, with no cost threshold, and each spins 200 µs
  (`pool.rs` `SPIN`) after the callback's last level: an estimated 3.75% of a core per helper at
  256 frames, 15% at 64. Dispatch a level only above a cost threshold; park the helpers at the
  end of `render_into`.
- Every output of every stage is written to its tap ring and its record ring each block, read or
  not, and the control half drains both before it checks `readers` and `recorded`
  (`control.rs`). Gate each output by an atomic the half sets from its readers and the armed set.
  A MidiIn at 4 voices holds about 4.8 MB of rings (estimate).
- A bound functional runs at 17 points every tick even when no block moved; skip when the first
  block, the epoch and the code are unchanged.
- Every control-rate param gets a 64-sample region and a glide check each block (a VST3 plugin
  declares up to 4 096): a source that writes the scalar strip alone, and a per-slot "moved"
  generation.
- Open: whether the device stream should stop when the plan holds only sinks with no input.
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
- The device opens at boot on the `HighPerformance` adapter (`gpu.rs`), with no graphics node,
  `--headless` included: on a hybrid laptop the discrete GPU stays powered. Open it at the first
  graphics node or shader compile; measure the idle power before and after.
- Every ARRAY arrival (`Upload::of`) and every host `Pixels` frame (`Upload::pixels`) is
  converted to f16 on the CPU whether or not its stage is drawn. Keep the `Arc`, convert at draw,
  or upload f32 and RGBA8 and convert in the existing blit.
- Every library shader's pipeline is built at boot, one thread each, with no `PipelineCache`:
  validate with naga at the scan, build at the first `insert` of the type.
- An occluded window's stage is still drawn and submitted every tick; count it as no reader.

## Manager and bridge

The 10 ms half wake, the status beat's walk, the live fill, the follower pass and the MIDI resync
are in `roadmap/reactivity.md` (B, D). The 2026-11-23 measure of the first: 99 wakeups/s and
0.5 to 0.7% of a core per idle graphics node. Not covered there:

- The status beat reads `config.toml` and every agent record from disk every 500 ms
  (`Agents::offered`, `Agents::roster`). Keep both in memory; re-read on a watch event or an
  agent op.
- The MIDI probe opens a new ALSA sequencer client every 2 s, and "Midi Through" is always
  listed, so the no-device back-off never applies. Keep one client and follow `System:Announce`.
- Every variable value move wakes `goofi-machines`, a machine or not (`machines.rs` watch).
  Send only names a machine reads.
- A variable entry preview bumps the store `version`, so each drag frame redelivers every
  binding to every engine; `set`, `apply_change` and `set_expression` bump it before they
  validate. Bump on a configuration change only, after validation, with a separate counter for
  history.
- A timeline edit re-serializes the whole `machines` and `timeline` roots, a drag preview sends a
  track's whole note or key array, and `Timeline::new` recompiles every curve track. Project each
  track and machine as its own record; cache the functionals by source text.
- Undo history has no bound, and each entry clones the variable store with every value it held.
  Cap the past; share the store as a persistent map.
- `/control` clones each event `String` per subscriber and formats it with none: a shared
  `Bytes`, and no text without a receiver.
- Verify whether autosave loops on its own `variables/*.npy` writes when a wide driven variable
  moves on a dirty patch.

## Frontend

The owner's-display trace is `roadmap/frontend-performance.md`. From reading `frontend/src`:

- Each viewer posts its own `drawn` message outside the flush batch, and autoscaled tick labels
  change every frame. Fold them into the batch; round the range with hysteresis.
- `MachineInspector` runs a rAF loop while a machine has a playhead; the sweep can be a CSS
  animation paused by `clock.playing`.
- plotluck redraws every plot of a pane and blits the whole atlas with `'copy'` when one plot
  changes: dirty plots and dirty rects, upstream.
- The worker's paint loop lets go after 30 flushes with nothing to do and re-arms two animation
  frames late, because the first one after an idle gap fires at once. Verify on a display that
  several streams slower than 2 Hz paint once a tick: the unit tests cannot see a vsync.

## Recording

- Measure the per-stream workers against a fast signal producer, an armed audio pair and a
  video at 1080p60: queue fill, drops and the manifest cadence.
