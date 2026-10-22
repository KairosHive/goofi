# Performance audit, 2026-10-22

Measured on a release build, on a 4-vCPU Xeon VM: Linux 6.18 at `CONFIG_HZ=250`, no real-time
privilege (`ulimit -r 0`), no audio device, Mesa lavapipe for the GPU. Every number below comes from
a probe run in that session. The probes are `backend/goofi-tests/examples/{hop_latency, audio_cost,
graphics_lag, op_cost, op_parts}.rs` beside the older `py_latency` and `backend_parity`. The user's hardware
differs; the mechanisms do not. What was not verified is listed at the end.

## 1. Every timed wait is quantised to kernel ticks

This is the limiter on the signal plane, and it reaches audio, graphics and the viewers too.

`goofi_transport::wait_within` is `Listener::timed_wait_all`, and iceoryx2's unix-datagram listener
implements that with `SO_RCVTIMEO` on the socket (`iceoryx2-bb-posix/src/unix_datagram_socket.rs`,
`set_timeout`). The kernel converts a receive timeout to jiffies, rounds up, and wakes at the next
tick. `poll`, `ppoll` and `nanosleep` use high-resolution timers and do not. Measured on this kernel:

| asked | `SO_RCVTIMEO` recv | `poll` | `nanosleep` |
| --- | --- | --- | --- |
| 0.2 ms | 8.0 ms | 1.1 ms | 0.3 ms |
| 1 ms | 8.0 ms | 1.1 ms | 1.1 ms |
| 4 ms | 8.0 ms | 4.2 ms | 4.2 ms |
| 10 ms | 16.0 ms | 10.2 ms | 10.1 ms |
| 33.3 ms | 40.0 ms | 34.2 ms | 33.5 ms |

A producer's cap is paid through this wait (`goofi-runtime/src/lib.rs`, `park`). The chain probe
measured what the cap delivers on a booted instance with the evaluator installed:

| `signal.default_ufreq` | frames/s at the producer |
| --- | --- |
| 30 (the patch default) | 25 |
| 1000 | 117 to 123 |
| 1e6 | 31 809 (one hop, len 64) |

On a `HZ=1000` desktop kernel the same mechanism costs one to two milliseconds per wait, so a
1 kHz cap would deliver about 500 Hz and the 30 Hz default about 29 Hz; that is an estimate from
the rounding rule, not a measurement. On `HZ=250` (Ubuntu's generic kernel) it is what the table
shows. The same wait paces:

- the audio control half's 10 ms tick (`Executor::next_wake` default `TICK`): 16 ms here, so taps,
  recording drains and breakpoints land every 16 ms;
- the graphics half's 10 ms tap tick (`half.rs`): the readback a 60 Hz render made waits up to 16 ms;
- the reducer's serve grid at the viewer cap (`reducer.rs`, `Pace` through `wait_within`): a 60 fps
  cap delivered 42 to 46 frames/s, measured below;
- the recording drain's 20 ms ceiling and a pulse's one-tick lowering.

Fix: wait on the listener's file descriptor with `ppoll` (the listener is `FileDescriptorBased`)
or iceoryx2's `WaitSet`, then `try_wait_all`. One change in `wait_within` corrects every path.
Keep `blocking_wait_all` where no deadline exists; it is event-driven and costs nothing.

## 2. The producer cap itself

Every producer's `common.max_frequency` is bound to `variables.signal.default_ufreq`, 30 Hz by
default (`goofi-runtime/src/common.rs`). `LslIn`, `OscIn` and every `PRODUCER = True` Python node
are producers, so an EEG chain runs at 30 ticks a second and every downstream node with it,
whatever the stream's rate. That is a design choice, not a defect, but it is the first thing a
user meets as "the limiter", and the binding only acts once the evaluator is installed: on a bare
`Graph` (the old examples) the literal 0 stands and the producer spins at full speed.

Every shipped example patch binds `max_frequency` to `variables.system.default_ufreq`, a name no
variable has (`psd-topomap`: 10 nodes, `connectivity-spectrogram`: 8, `harmonic-observatory`: 1).
Each shows a binding error and runs on its literal. Rename them to `signal.default_ufreq`.

## 3. Settle and the control plane scale with the patch

`Graph::settle` runs `derive_bindings` for every leaf's every source BEFORE its early return, and
each engine's `settle` rebuilds `desired_of` for every live node and deep-compares it. The bridge's
status drain calls `drain_status` (which settles) at least every 50 ms, and every op settles,
projects the whole document to JSON (`replica`) and diffs it. Measured, driven directly:

| echo nodes | quiet `drain_status` | param edit + settle | restart + settle |
| --- | --- | --- | --- |
| 10 | 17 µs | 179 µs | 5.0 ms |
| 50 | 121 µs | 522 µs | 5.4 ms |
| 200 | 2.2 ms | 10.1 ms | 20.4 ms |

A knob drag at 60 ops/s on a 200-node patch spends 600 ms of every second under the graph lock in
settle alone. The whole op, through `/control`, is in the table below (`op_cost`). Fixes: return
from `settle` before `derive_bindings` when nothing was touched and no engine is dirty; derive
only the sources a touched node owns; let an engine's settle visit only the nodes the batch
touched plus the nodes wired to them, since `Desired` is already per node.

## 4. Audio

The device callback renders under `runtime.try_lock()` and plays SILENCE for the whole period when
the lock is held (`goofi-audio/src/lib.rs`, `output_stream`), counted as `contended` in
`session status`. The control side holds that lock in `remove` (the comment accepts the click),
`persist` (every save, every SDK node's `save()` under the lock), `retune` (device open), `send`
when the 4096-entry message ring is full, and `state_cost`. Each is a click at an authoring event.
Measured through the op, an upper bound on the hold:

| plan | `node remove` op | `node param edit` op |
| --- | --- | --- |
| 3 nodes | 0.64 ms | 0.52 ms |
| 10 nodes | 1.9 ms | 0.76 ms |
| 34 nodes | 2.2 ms | 0.84 ms |
| 82 nodes | 5.3 ms | 3.2 ms |

A period of 256 frames at 48 kHz is 5.3 ms, so a remove on a large plan can cost a whole period.
Fix: `remove` should let the callback drop the box through the outbox instead of taking the lock
(the comment already names the alternative), and `persist` should read states through savers the
way plugins do, or copy the slab's states on the callback and hand them out.

The render itself is cheap. One 64-frame block of the whole plan, on the external clock:

| plan | block p50 | p99 | max | worst node |
| --- | --- | --- | --- | --- |
| 1 Osc chain, 3 nodes | 3.7 µs | 8.6 µs | 0.26 ms | Osc max 128 µs |
| 8 Osc chains, 10 nodes | 18.5 µs | 91 µs | 2.1 ms | Osc max 233 µs |
| 32 Osc chains, 34 nodes | 48 µs | 0.51 ms | 28 ms | Osc max 5.9 ms |
| 4 chains of Osc, Filter, Delay, Reverb, Gain, 22 nodes | 94 µs | 1.09 ms | 4.4 ms | Reverb max 4.1 ms |
| 16 such chains, 82 nodes | 0.40 ms | 2.77 ms | 6.5 ms | Reverb max 5.2 ms |

The budget per block is 1.33 ms. The p50 fits with room; the tail does not, and a node's own
`max` of 4 to 6 ms against a mean of 45 µs is a thread descheduled mid-block, not DSP. Without
real-time priority the clock thread and the pool helpers (`pool.rs`, spin 200 µs then park) lose
their slot to any other thread; with `SCHED_RR` 80 (`rt.rs`) they do not, and the user's `late`
and `contended` counters say which case a machine is in. The pulseaudio client thread and the
device-less self-clock stay as the roadmap lists them.

Smaller audio costs, all on the control thread, none on the callback: `drain_blocks` allocates per
tap per tick; `Inbox::enter` collects the whole frame into a `Vec<f32>` per crossing;
`breakpoints` runs 17 Python evaluations per functional param per tick (1 700 a second per param
once the tick is 10 ms), each under the evaluator's `table` mutex and `attach`.

## 5. Graphics

The tap a viewer sees travels: render at tick N; `map_async`; the readback is taken at tick N+1
(one frame, since `poll` runs once per tick before `take`); the half thread takes it on its
10 ms tick (16 ms here); it encodes (a full f32 frame unless the viewer declared `u8`) and
publishes (a second copy); the reducer wakes, forwards a `u8` frame or decodes, reduces and
encodes an f32 one; the serve waits for the next pace tick at the viewer cap; the socket; the
browser's worker decodes and paints on `requestAnimationFrame`. Measured with the engine on its
own 60 Hz clock and a constant-colour stage, edit to pixel at the viewer:

| stage | viewer | cap | delivered | gap p50 / p99 | edit to pixel p50 / p90 / max |
| --- | --- | --- | --- | --- | --- |
| 256² | u8 box 128 | 30 | 30.0 fps | 32 / 40 ms | 64 / 72 / 72 ms |
| 256² | u8 box 128 | 60 | 46.5 fps | 16 / 48 ms | 48 / 52 / 68 ms |
| 512² | u8 box 256 | 30 | 30.0 fps | 32 / 40 ms | 44 / 68 / 73 ms |
| 512² | u8 box 256 | 60 | 42.1 fps | 17 / 40 ms | 45 / 52 / 56 ms |

Where the lag comes from, in order: the viewer cap `system.viewer_fps` is 30 while the engine
renders 60, so a viewer shows every other frame and waits up to 33 ms (40 ms here) for its serve;
the readback is one tick behind by design; the half's tick adds 0 to 10 ms (16 here); the pace
grid and the half tick both pay finding 1. Raising the cap to 60 took 16 to 20 ms off the median
and still delivered only 42 to 46 fps, because the pace wait rounds to ticks. The `Tap` ring holds
ONE slot (`Want::depth`): a GPU that has not finished the copy by the next tick's `poll` skips
that tick's readback, which halves the tap rate under load; `Screen` and `Record` hold two.

Fixes: serve the viewer on the frame's arrival with the cap as a floor between serves, not a
grid the wait has to land on; take the readback through `device.poll` on the tick after submit
and give `Tap` two slots like the others; let the half's wake be the readback's own, not a tick
(a bell from the render thread when a slot is ready). An f32 viewer declaration on a 512² stage
delivered no frame in 180 s on lavapipe; not reproduced on a GPU, so listed under unverified.

## 6. What one hop costs on the signal plane

Per hop, a frame is encoded into a `Vec` (one body copy), copied into the iceoryx2 loan (two),
and decoded by `frame.to_vec()` on the consumer (three). `goofi_codec::encoded_len` and `Fill`
exist to encode straight into the loan, and `Runtime::tick` does not use them. The lineage meta
(`source`) is rebuilt per output per run and msgpack-encoded with the frame, and its size grows
with upstream depth. `Buffer` memmoves its whole window on every frame (`Vec::drain` from the
front) and copies it again into the output. A Python node copies an array three times on the way
out (`ascontiguousarray`, `tobytes`, `extract::<Vec<u8>>`, then the f32 cast) and once on the way
in (`PyBytes::new` then `frombuffer`), rebuilds its `params` namespaces on every call
(`apply_params`), and checks `sys._is_gil_enabled` twice per call.

Measured, per-hop latency as the consumer's run `time` minus the producer's `emit`, on a booted
instance (the status drain and the evaluator running):

| chain | producer rate | end to end p50 / p90 / p99 | per hop p50 |
| --- | --- | --- | --- |
| 1 hop, len 64, uncapped | 31 809 fps | 16 / 31 / 75 µs | 16 µs |
| 1 hop, len 65 536, uncapped | 6 486 fps | 86 / 125 / 198 µs | 85 µs |
| 4 hops, len 64, 30 Hz | 25 fps | 0.78 / 1.08 / 2.37 ms | 170 to 232 µs |
| 8 hops, len 64, "1000 Hz" | 119 fps | 1.58 / 1.96 / 3.40 ms | 165 to 223 µs |
| 8 hops, len 4096, "1000 Hz" | 119 fps | 1.86 / 2.69 / 4.84 ms | 182 to 254 µs |
| 8 hops, len 64, uncapped | 1 090 fps | 0.79 / 1.41 / 2.91 ms | 16 to 136 µs |

Two readings. A hop that is busy costs 16 µs; a hop that has to wake costs 170 to 250 µs on this
VM, which is the idle-to-run latency of a thread per node, paid once per hop. A chain of eight
paced nodes is therefore 1.6 ms behind its source here and some 0.3 to 0.5 ms on a desktop. An
uncapped producer runs flat out and starves the chain behind it: with eight hops the last node
emits 1 090 frames/s while the source emits 30 000. Throughput of the tiers, one node and an
eight-node fan-out of `x*2+1` on 1024 floats (`backend_parity`): native 22 378 and 8 009 frames/s
per node; in-process free-threaded Python 12 097 and 4 351; subprocess Python 4 812 and 1 927. A
Python node alone sustains 10 588 frames/s at 64 samples and 5 562 at 16 384 (`py_latency`); the
same numpy work on bare threads costs 32 µs a call, so about 60 µs of every call is the crossing,
the params rebuild and the stamps. No shipped bundle import re-enables the GIL on the
free-threaded interpreter (numpy, scipy, mne, antropy, specparam, biotuner, sklearn, numba, pylsl
checked), so the in-process tier runs in parallel; `cv2`, `mediapipe` and `torch` are the GIL
interpreter's and route to the subprocess tier as designed.

Fixes, in order of yield: encode into the loan (`Fill`) in `Runtime::tick` and keep the `Vec`
only for the record port; give `Buffer` a ring so a frame costs one copy, the output; cache the
`params` namespaces in a Python node and rebuild on `on_param_changed` alone; take the GIL check
out of the per-call path (the tripwire can read `_is_gil_enabled` once per tick). The wake
latency per hop is the price of a thread per node; it is bounded by the OS, and the only lever
short of a shared scheduler is to run a short chain of light nodes on one thread, which is a
design change and not proposed here.

## 7. Control plane per op

One op through the manager (`AppState::call`, the path `/control`, the CLI and MCP share) on N
echo nodes, with the status drain running. `session state` is a read, so it is the cost of the
transaction and the projection alone:

| echo nodes | `node param edit` p50 / p99 | `node edit` (move) p50 / p99 | `session state` |
| --- | --- | --- | --- |
| 10 | 1.2 / 5.3 ms | 0.5 / 1.8 ms | 0.2 ms |
| 50 | 8.7 / 25.6 ms | 5.0 / 24.6 ms | 0.2 ms |
| 200 | 133 / 285 ms | 126 / 291 ms | 15.6 ms |
| 500 | 743 / 1 249 ms | 572 / 965 ms | 144 ms |

This is superlinear: 50 to 200 nodes is 4× the nodes and 15× the time. At 200 nodes a knob drag
gets 7 ops a second and the graph lock is held for 130 ms of each; the status drain, the
reducers' `set_view_demand`, the recording drain's `resolve` and every socket's connect wait
behind it. The direct settle of §3 is 10 ms of the 133, so most of the op is elsewhere; a
breakdown is in the table below.

The pieces of one `node param edit`, each timed alone on the held graph (`op_parts`):

| echo nodes | whole op | `replica()` | its JSON text | `update_param` | `settle` | `diff_ops` |
| --- | --- | --- | --- | --- | --- | --- |
| 50 | 19.4 ms | 23.6 ms | 0.3 ms (17 KB) | 32 µs | 0.7 ms | 0.3 ms (1 op) |
| 200 | 146 ms | 110 ms | 5.8 ms (66 KB) | 33 µs | 45.5 ms | 0.9 ms (1 op) |

The command itself costs nothing. `Graph::replica` is the op: `document()` calls `fragment` on
every uid, and per node that is a fresh `typed()` of every param, `node_type()` and `scope_of()`,
550 µs a node at 200 nodes and growing with the patch. It is run on EVERY op, read or write, and
again by the status drain's `settle_and_project` path, to produce a diff that is one op long. The
settle on the live instance (45 ms) is the §3 settle with the drain's own settles contending.

Fixes: keep the projection as state and patch it per command, so `reconcile_root` diffs a
record and not the tree; cache a node's record and invalidate it on the node's own edit. First
measure which of `typed`, `node_type` and `scope_of` carries the 550 µs; none was timed alone
here. Measure `session state` again after each; the target is a flat sub-millisecond op at 500
nodes, which the command cost says is reachable.

## 8. Recording

One drain thread holds a subscriber per armed slot and hands each frame to a per-stream writer
lane (`goofi-record/src/writer.rs`, 1024 deep, a counted drop when full). The manifest is
rewritten once a second and each stream syncs once a second on its own lane, so a sync stalls one
stream. An audio output records per 64-frame block, so one armed output is 750 frames and 750
bells a second through the transport; that is the one path where the per-frame overheads of §6
are paid at the audio rate on the control side. No defect found; the 20 ms drain ceiling pays
finding 1 but only bounds a park.

## 9. Stale measurement tooling

`backend/goofi-tests/examples/latency.rs` adds a `Buffer` node on a bare graph, where no bundle is
registered, and panics. `py_latency` and `backend_parity` set `signal.default_ufreq` to 1e6 on a
bare graph, where the binding never acts, so their "uncapped" is accidental. The new probes boot
the harness and set the variable through the op.

## Remaining

- Replace the `SO_RCVTIMEO` wait in `wait_within` with a high-resolution one, then measure the
  cap table again; every pace in finding 1 follows.
- `settle`: early return before `derive_bindings`; engines visit touched nodes only.
- Audio: `remove` and `persist` off the runtime lock; measure `contended` on a device.
- Graphics: event-driven tap, two `Tap` slots, serve on arrival under the cap.
- Signal: encode into the loan; `Buffer` ring; Python params cache and one GIL check per tick.
- Examples: `variables.system.default_ufreq` to `signal.default_ufreq`; delete or fix `latency.rs`.

## Not verified here

A real audio device and its `contended`, `late` and pool-wait counters; real-time scheduling; a
hardware GPU (all graphics numbers are lavapipe's, and the f32-viewer stall is unexplained); a
browser trace of the worker's paint loop; the user's kernel tick rate, which sets the size of
finding 1 on their machine.
