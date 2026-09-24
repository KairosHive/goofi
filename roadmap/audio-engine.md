# Audio engine

Designed with the user 2026-08-24/25 against measurements on the target machine, and in full on
2026-09-02. The seam that lets a second engine exist — the `Engine` trait and the settle point —
is `multi-engine-graph.md`; the structural redesign is `backend-architecture.md`. What is here is
audio: the decisions that bind the work ahead, and the work itself.

## What it is for

Real-time generative audio through modular synthesis, modulated by everything else in the patch. It
is not a DAW and does not compete with one. The nearest neighbour is VCV Rack — a graph of modules
with CV, where a plugin host is one module among many — and it is a neighbour, not a template. The
consequence that shapes the most decisions below: a node reload is a discrete authoring event, and
a click or a short gap at one is acceptable.

The signal plane is latest-wins with no queue, one self-paced thread per node, and no scheduler. A
synth needs every sample in order, so none of that carries over. The audio engine is synchronous,
centrally scheduled, and in-process: one block of `BLOCK = 64` frames at the device rate, every
node visited once in topological order, buffers passed as slice indices into one arena. No iceoryx2
on the audio path.

## Decisions that bind

**Every audio node implements one goofi trait, and a plugin format is an adapter behind it.** The
plan compiler, the Kahn sort, the arena, the watchdog and the viewer tap see one trait and no
format enum. Audio I/O is the engine's own — a device is never a node's to open — and it stands
behind the same trait. Forking a shipped node is a copy.

**No CLAP for the builtins** (locked 2026-08-25, reversed 2026-09-02). CLAP params are f64-only, so
every string param, every `refresh` param and every host-resolved resource rode a vendor extension
— a goofi ABI wrapped in CLAP, growing with every modular need CLAP has no answer for. Every
modular environment — VCV Rack, Max, Pure Data, Bitwig's Grid — owns its module contract and hosts
a plugin as one special module. Hosting a CLAP-only plugin is deferred, not lost: an adapter behind
the same trait, only if a CLAP-only plugin ever matters; nearly every plugin that ships CLAP ships
VST3. If one is built, it is written over `clap-sys`, reading `clap-validator`'s `src/plugin/`;
`clack-host` is a one-person project. Exporting a goofi node to a DAW through `clap-wrapper` is
dropped as a goal.

**`process(&mut Block)` allocates, locks and blocks on nothing.** A channel is `&[f32; BLOCK]`, a
fixed-size array the compiler bounds and vectorizes; the rate arrives at `prepare` and a second
copy in the block would be a duplicate.

**A port carries a signal with no default; a param carries a value with a default.** An audio
output feeds an audio input, or an ARRAY input through the engine's crossing, and nothing but audio
feeds an audio input. No new `Value` variant and no new GOOF dtype: audio buffers never cross
iceoryx2 or the wire. An unwired input reads one shared silent region: present, silent, never an
error. A `multi` input sums its wires at the jack. No node carries a CV port, because every
modulatable quantity is a param, and exactly one source holds per param.

**Every signal is audio-rate numbers in a standard range.** Bipolar in `[-1, 1]`, unipolar — a
gate, a velocity, an envelope — in `[0, 1]`; a gate is HIGH at `>= 0.5` and a trigger is its rising
edge; pitch is volts per octave, zero at C4, unbounded. A gate arrives at a `Bool` param and there
is no event type anywhere. Polyphony is channels.

**Channel counts are inferred, never configured.** `channels` is evaluated once per node inside
the Kahn loop, so there is no fixed point. The count is dynamic per block, not per instance: a node
is prepared once for `MAX_CHANNELS` and reads `port.channels` each block, so a wiring change
re-plans and resets nothing. A layout tag (`Speakers`, `Bins`) is not carried until the first node
that needs one.

**The audio device belongs to the engine, not to a node.** The device callback is the clock;
without a device the clock is external (`AudioEngine::drive(frames)`). The external clock owns no
hardware in either direction, one rule at one owner (`Clock::owns_devices`), because `drive`
renders at the caller's speed and no stream has a timeline it can meet. goofi asks the machine's
sound server by default and never picks a host itself: every cpal host is offered and the device
NAME carries which (`PipeWire: …`, `ALSA: …`), so no second param can fall out of step with the
first.

**The device cache is filled once, at engine construction, in both directions.** Not lazily and
never when a name is resolved to open: that enumerates ASIO on the clock thread while VST3 plugins
instantiate, which died with `STATUS_HEAP_CORRUPTION` every time. A driver another application
holds at that moment is genuinely unavailable.

**Which channels a node uses is a param, one-based, spelled like the front panel** (`all`, `1`,
`1-2`, `1,3-4`; order kept, repeats allowed, so `4-3` swaps a pair). One parser (`chanmap`) serves
`AudioIn` and `AudioOut`. A selection that does not parse is a FAULT and plays nothing, never a
fallback to the whole device: on a live rig, signal on channels the patch kept clear is the
expensive direction to be wrong in.

**Priority is why the host choice is not cosmetic.** cpal's `realtime` promotes the callback on
alsa, pipewire and jack; the `pulseaudio` host has no call to it, and `audio_thread_priority`
without dbus is a no-op that answers Ok. Linux pays the pipewire, jack and dbus dev packages for
that; Windows takes MMCSS from the same feature.

**A device name is tried once, under a two-second ceiling, because the open runs under the graph
lock.** One that will not open faults the agreeing `AudioOut`s with cpal's reason and the previous
clock stands. The old stream is closed and waited for before the new one opens, and the new one
plays only once the runtime is cut to its rate and width.

**The audio thread owns one `Runtime`: a slab of instances, the plan, the arena, the atomics.**
The plan holds slab indices, so a topology edit is a new order over the same instances. Every
change is a pointer move over an `rtrb` ring, and every retired box returns on a second ring to be
dropped off-thread. The plan crosses as an owned value, not `ArcSwap`: it holds per-node staging
and needs `&mut`. The runtime sits behind a mutex for one reason — a clock swap — which the
callback `try_lock`s. No liveness-based arena reuse until a measurement asks.

**Topological order, Kahn, ties broken by `Uid`** — never `IndexMap` order, which moves across
save, load, undo and paste. A loop closes only through a node whose type answers
`feedback() == true`: one block of delay, no cycle-breaking heuristic. A cycle with no such node is
excluded and named through the fault channel.

**One node, two halves; the control half owns everything that touches the OS.** An expression is
Python and cannot evaluate on the audio thread, so every boundary consumer of this engine is a
control-half thread, woken like a signal node is, that stores into the param's atomic; the audio
thread reads it at its next block start. Latest wins, no event list, no queue. The control half is
the shared `goofi-control` crate — one thread per node, a 10 ms tick — and `backend-architecture.md`
§4 owns whether that shape stays.

**One tap serves every reader of an audio output**: a plain publish on the derived name, keyed on
the data service's subscriber count. No reducer arm, no bridge reaching into the engine, and no
plan recompile when a tap opens or closes.

**An in-order signal→audio crossing is the engine's, and `SignalIn` exposes it.** It is for ORDERED
samples — sonification, playback; a control value or a gate from the signal plane is a reference
into a param, never this. A live source (`AudioIn`) DROPS what piled up because a late period is
latency; a file (`AudioPlayback`) KEEPS it because dropping there is a skip through the file.
`AudioPlayback` is built into the engine because a free-text `Str` reaches no loaded node.

**MIDI is a node that emits signals, and no engine mechanism knows it exists.** `AudioIn`, `AudioOut`
and `MidiIn` stay compiled into the engine because their control halves own OS handles; every other
shipped node is a file like any authored node. `MidiCC` — one CC number per node, one output — is
decided and deferred.

**A node reload is a discrete event, and it does not crossfade.** A reload is the graph's own
restart at a block boundary, state blob flushed and reloaded across it. So is a device switch.
Reinstantiate per node, never per graph.

**A plugin's editor is a window on the machine goofi runs on, and the parameter list stays the
portable UI.** goofi opens no window of its own; a VST3 editor is the plugin's, so it appears on the
server's desktop and nowhere else (`goofi-window`, on the process main thread, because a JUCE
plugin holds the thread that loaded it). The cost is named: a plugin whose value IS its editor is
degraded to a parameter list everywhere but that desktop — which is also the only route another
node can modulate.

**VST3 hosting is one more implementor of the trait, over the MIT `vst3` bindings**, each bundle
scanned in a child `goofi vst3-scan` so a plugin that crashes at load is greyed and named. MIDI is
VST3's language, not goofi's: an event input is `gate`, `pitch` and `velocity` params.

**Opaque per-node state lives in `workspace/.goofi/state/<uid_hex>/<Type>`**, written by the
engine when a box comes back from the runtime and at `session save`. A goofi node's blob never
carries a param value, so the `.gfi` record is the one authority; a VST3 plugin's load is blob
first, then the param record on top.

**One node editor panel for every engine.** Engines are told apart by slot colour; a frontend
branch on which engine a node belongs to is a defect.

**No Python on the audio clock, and not as a narrow exception either.** Every real-time neural
audio model has already left Python; `nam-rs` and `tract` cover the Rust side. PEP 703's collector
pauses only attached threads, so a pure-Rust audio thread is safe beside the in-process tier.

**wasmtime is rejected.** Parity with native for scalar DSP and a recoverable memory error, against:
a second toolchain target, 13.5 MB, a `static mut`-in-linear-memory trap that shares state between
two instances of one module, a 160 ns per-node page-walk tax, a store-wide epoch deadline, an
instance leak with a 10,000 ceiling, and a trap handler unproven on a `SCHED_FIFO` thread.

**The engine depends on nothing above `goofi-transport`**, so no iceoryx2 thread or tokio reaches
the DSP path and an external block callback can drive it. The SDK carries both sides of the
boundary — the trait, the vtable, `export!` and `Loaded` — and depends on `goofi-node` alone. No
DSP crate in the engine or the shipped nodes.

**The shipped set's rule is orthogonality**: each node proves a seam or closes a gap nothing else
composes to.

## Authored nodes: the rules an audio thread forces

One file, `workspace/nodes_audio/<Name>.rs` — `impl AudioNode`, safe Rust — and goofi generates the
crate, builds it with the shared `goofi-build`, and loads it while audio runs.

- **The manifest crosses as data, never as a Rust struct.** Only the vtable crosses as code.
- **A version symbol is checked before anything else.** A stale artifact is a refusal, never a crash.
- **The build runs outside the graph lock**, and its allowlist is what an audio node may import:
  `libm` alone. `fundsp`, `biquad`, `realfft` and `rustfft` would cost minutes in every node crate
  and goofi's own build for a capability no shipped node uses; they join when a node needs one,
  which re-keys every audio artifact once — accepted.
- **Reload is `library refresh`, and nothing watches a file.** A build that fails leaves the stamp
  unchanged: the instances keep the old artifact and the palette row turns UNAVAILABLE with
  rustc's output.
- **Open with `RTLD_NOW`**: the lazy default runs the PLT resolver on the audio thread.
- **A per-generation unique filename is mandatory.** Rebuilding to a fixed path and re-loading
  returned the identical vtable pointer and the old behaviour.
- **Never `dlclose`.** Live vtables, `&'static` data, TLS destructors and `atexit` registrations pin
  a library. Measured: 338 kB of address space per reload — thousands of edits before a gibibyte.
  Sweep at next start.
- **`#![forbid(unsafe_code)]` in the template, plus the allowlist as a stated policy.** The lint
  does not reach dependencies, and `RUSTFLAGS="-Funsafe_code"` is a silent no-op under
  `--cap-lints allow`.
- **`catch_unwind` in the SDK's shim, never in the author's code.** Catch once, zero that node's
  output, drop it from the plan, surface `NodeFault::Process`; `node restart` brings it back. Never
  retry in place — a node that panics panics 750 times a second.
- **A watchdog, not a per-node budget.** It blames a node by its OWN duration — `OVERRUNS = 8`
  blocks in a row over `BUDGET = 4` blocks of wall time takes it out of the plan — and never skips
  a neighbour, because a skip after a deadline lands on the victim rather than the culprit.

## Measured, so it is not re-argued

- A C-ABI vtable call over a native trait call: +8.3 ns at 0 events, ~2.15 ns marginal per event —
  0.13% of a block for a hundred nodes. The ABI cost is not an argument in either direction.
- Wasm versus native: parity for scalar DSP, 2.09× for a 1024-point spectral node, 3.22× for a bare
  FFT.
- Per-edge buffer copies never become measurable at any chain length.
- `PyExprEvaluator::eval`: 509 ns for one scalar variable, ~4.6 µs fixed overhead for any array
  variable, 77 µs for a 1 MiB frame. Single-threaded; the harness was not kept.

## Remaining

- **Process isolation for native plugin processing.** Only the scan runs in a child; a plugin that
  crashes at `setupProcessing` or its first block, or a CPU-limit signal under the RTKit fallback,
  takes the server with it. Keep the device callback bounded, with a defined fallback for late
  worker output. Measure the added buffering latency and deadline misses under CPU load before
  choosing the worker scheduling and buffer policy. Remove the `audio_thread_priority` git patch
  (`dav0dea`, `goofi-direct-first`) when upstream provides direct-first Linux promotion.
- **A buffer size request.** goofi asks for none; the host default costs 2 s of buffer on
  pipewire-pulse. `Fixed(1024)` would make it ~43 ms end to end. Provable only from OUTSIDE the
  process, by recording the sink back: `session status` reports what goofi did, not what was heard,
  and this host never surfaces the server's underflow.
- **A device switch, a rate change and a stream loss run only under `Clock::Device`**, which no
  test constructs. A rate change and a stream loss are unexercised; a raw ALSA `hw` device refuses
  an `f32` stream and needs an `i16`/`i32` stream with a conversion. Windows and macOS callbacks
  are unmeasured, as is RT priority against the control thread's graph lock under a knob drag.
- **A machine with no output device** faults every `AudioOut` and renders nothing. A real-time
  self-clock for a device-less machine is open; graphics' `Clock::Timer` is the precedent.
- **The `codes` mutex** in `PyExprEvaluator::eval` is taken on every evaluation from every node
  thread; the benchmark above cannot see the contention. It wants to be read-mostly (the map is
  written only on compile and release). Settle it with a threaded measurement before promising a
  high expression rate; a reference bypasses it.
- **Three avoidable evaluator costs**: the locals dict is rebuilt every eval, `numpy` is imported on
  every array conversion, and `PyBytes::new` copies an array an `Arc<[u8]>` could alias.
- **The OS mixer labels goofi `cpal-pulseaudio-<pid>`**; cpal exposes no way to set it. Upstream.
- **`MidiIn` reads notes, bend and the sustain pedal.** Other CCs and aftertouch are absent, and a
  note-on for a held note moves its voice's velocity, not its envelope. A note lands at block
  start, up to 1.3 ms late; sample-accurate placement needs the port's timestamps correlated with
  the device clock, built when a measurement asks.
- **Windows latency.** cpal's WASAPI is shared-mode only, floor ~10 ms; `IAudioClient3` reaches
  2.66 ms and cpal does not use it. ASIO is compiled in and is the way past that floor. What the
  Steinberg licence bars is a REDISTRIBUTED binary — a release question. The way out if the answer
  is no: bind the driver as the COM object it is, `CoCreateInstance` on the CLSID under
  `HKLM\SOFTWARE\ASIO`, no SDK in the tree.
- **A sample format cpal adds is refused until a device reports it.** `SampleFormat` is
  `#[non_exhaustive]`; DSD is the one refusal left. Untestable, because no test opens a device.
- **macOS signing**: `com.apple.security.cs.disable-library-validation`, arriving with the first
  notarized release.
- **A control-rate param referencing an AUDIO output** receives a `[C, T]` frame and the bare
  reference rule wants one element. Whether the rule takes the last sample or needs a node in
  between is open.
- **Whether `MAX_CHANNELS = 16` is right**, and what a spectral port does to it when `Bins` arrives.
- **Drift between two devices** for `AudioIn`: measure before any correction is built.
- **A canvas affordance for references**: nothing draws a reference on the canvas.
- **Watchdog tuning.** A node that runs at exactly the budget flaps in and out; neither number has
  been tuned on a device.
- **The shipped set, the owner's to settle**: `LFO` (or `Osc` at a low pitch?), `Clock` (or `Osc`
  square?), `Seq`, `HzToOct`, `MidiCC`, `Sampler` (needs the resource door). `Mix` and
  `Offset`/`Scale` look redundant while the jack sums and a reference replaces a literal. Whether
  `Limiter` belongs at the jack, since `Filter` at a high `q` peaks at `q` times its input.
- **VST3 residuals**: bus arrangements are the plugin's defaults, fixed at scan; no controller is
  instantiated at runtime, so a plugin whose processor needs its controller's messages is degraded;
  macOS gets a null `CFBundleRef`; output parameter changes and output events are not read. A
  bundle REPLACED in place keeps its old code until goofi restarts (`dlopen` answers one handle per
  path); the scan cache is never pruned; a class the host cannot describe is dropped without a row,
  and a bundle that yields none greys under its FILE name; the inspector calls a plugin a Rust node.
  Not yet proven on a real JUCE editor on Windows or macOS.
- **Whether goofi should run as a plugin inside a DAW.** Deliberately not an item: cross-engine
  modulation needs the signal plane, so it may be a different product. The door stays open through
  the crate floor above.
