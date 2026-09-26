# Audio engine

The structural redesign is `backend-architecture.md`.
The audio node contract is `sdk/README.md` and the code.

## Remaining

- **Process isolation for native plugin processing.** Only the VST3 scan runs in a child
  (`goofi vst3-scan`); `vst3/node.rs` calls the processor on the device thread, so a plugin that
  crashes at `setupProcessing` or its first block, or a CPU-limit signal under the RTKit fallback,
  takes the server with it. Keep the device callback bounded, with a defined fallback for late
  worker output. Measure the added buffering latency and deadline misses under CPU load before
  choosing the worker scheduling and buffer policy. Remove the `audio_thread_priority` git patch
  (`dav0dea`, `goofi-direct-first`, `Cargo.toml [patch.crates-io]`) when upstream provides
  direct-first Linux promotion.
- **A buffer size request.** goofi asks for none (no `BufferSize` in `goofi-audio`); the host
  default costs 2 s of buffer on pipewire-pulse. `Fixed(1024)` would make it ~43 ms end to end.
  Provable only from outside the process, by recording the sink back: `session status` reports
  what goofi did, not what was heard, and this host never surfaces the server's underflow.
- **A device switch, a rate change and a stream loss run only under `Clock::Device`**, which only
  `goofi-cli` constructs; every test uses `Clock::External`. A rate change and a stream loss
  (`DeviceNotAvailable` in the error callback) are unexercised. Windows and macOS callbacks are
  unmeasured, as is RT priority against the control thread's graph lock under a knob drag.
- **A machine with no output device** faults every `AudioOut` and renders nothing. A real-time
  self-clock for a device-less machine is open; graphics' `RenderClock::Timer` is the precedent.
- **The `codes` mutex** (`goofi-python/src/inproc/expr.rs`, `Mutex<HashMap>`) is taken on every
  evaluation from every node thread. It wants to be read-mostly (written only on compile and
  release). Settle it with a threaded measurement before promising a high expression rate; a
  reference bypasses it.
- **Three avoidable evaluator costs** in the same file: the locals dict is rebuilt every eval,
  `numpy` is imported on every array conversion, and `PyBytes::new` copies an array an
  `Arc<[u8]>` could alias.
- **The OS mixer labels goofi `cpal-pulseaudio-<pid>`**; cpal exposes no way to set it. Upstream.
- **`MidiIn` reads notes, bend and the sustain pedal** (`nodes/midi_in.rs`). Other CCs and
  aftertouch are absent, and a note-on for a held note moves its voice's velocity, not its
  envelope. A note lands at block start, up to 1.3 ms late; sample-accurate placement needs the
  port's timestamps correlated with the device clock, built when a measurement asks.
- **Windows latency.** cpal's WASAPI is shared-mode only, floor ~10 ms; `IAudioClient3` reaches
  2.66 ms and cpal does not use it. ASIO is compiled in (`goofi-audio` feature `asio`) and is the
  way past that floor. What the Steinberg licence bars is a redistributed binary: a release
  question. If the answer is no: bind the driver as the COM object it is, `CoCreateInstance` on
  the CLSID under `HKLM\SOFTWARE\ASIO`, no SDK in the tree.
- **A sample format cpal adds is refused until a device reports it.** `SampleFormat` is
  `#[non_exhaustive]`; DSD is the one refusal left. Untestable, because no test opens a device.
- **macOS signing**: `com.apple.security.cs.disable-library-validation`, arriving with the first
  notarized release.
- **A control-rate param referencing an audio output** receives a `[C, T]` frame and the bare
  reference rule wants one element. Open: the rule takes the last sample, or a node sits between.
- **What a spectral port does to a port's width** when a `Bins` layout arrives. No layout tag
  exists until a node needs one.
- **Drift between two devices** for `AudioIn`, and between hosts in one patch (a WASAPI capture
  beside an ASIO output is allowed): measure before any correction is built.
- **A canvas affordance for references**: nothing draws a reference on the canvas.
- **Watchdog tuning.** `OVERRUNS = 8` blocks over `BUDGET` (`runtime.rs`); a node that runs at
  exactly the budget flaps in and out; neither number has been tuned on a device.
- **The shipped set, the owner's to settle.** Shipped today: `AudioIn`, `AudioOut`,
  `AudioPlayback`, `MidiIn`, `SignalIn`, plus VST3. Candidates: `LFO` (or `Osc` at a low pitch?),
  `Clock` (or `Osc` square?), `Seq`, `HzToOct`, `MidiCC` (one CC number per node, one output;
  decided, deferred), `Sampler` (needs the resource door). `Mix` and `Offset`/`Scale` look
  redundant while the jack sums and a reference replaces a literal. Whether `Limiter` belongs at
  the jack, since `Filter` at a high `q` peaks at `q` times its input. The rule is orthogonality:
  each node proves a seam or closes a gap nothing else composes to.
- **VST3 residuals**: bus arrangements are the plugin's defaults (`vst3/node.rs` `arrange`), never
  the patch's choice; output parameter changes and output events are not read
  (`outputParameterChanges`, `outputEvents` are null). A bundle replaced in place keeps its old
  code until goofi restarts (`dlopen` answers one handle per path; `vst3/module.rs`); the scan
  cache is never pruned; a class the host cannot describe is dropped without a row, and a bundle
  that yields none greys under its file name. Not yet proven on a real JUCE editor on Windows or
  macOS.
- **The DSP crate allowlist is `libm` alone.** `fundsp`, `biquad`, `realfft` and `rustfft` join
  when a shipped node needs one, which re-keys every audio artifact once.

## Not to be done

- **CLAP for the builtins.** CLAP params are f64-only; every string or `refresh` param would ride
  a vendor extension. Hosting a CLAP-only plugin is an adapter behind the same trait, only if one
  ever matters; if built, over `clap-sys`, reading `clap-validator`'s `src/plugin/`. Exporting a
  goofi node through `clap-wrapper` is dropped.
- **Python on the audio clock.** Every real-time neural audio model has left Python; `nam-rs` and
  `tract` cover Rust.
- **wasmtime.** Parity with native for scalar DSP and a recoverable memory error, against a second
  toolchain target, 13.5 MB, a `static mut`-in-linear-memory trap shared between two instances of
  one module, a 160 ns per-node page-walk tax, a store-wide epoch deadline, an instance leak with
  a 10,000 ceiling, and a trap handler unproven on a `SCHED_FIFO` thread.
- **Liveness-based arena reuse**, until a measurement asks.
- **A crossfade at node reload.** A reload is a discrete authoring event; a click is acceptable.
- **goofi as a plugin inside a DAW.** Cross-engine modulation needs the signal plane, so it may be
  a different product. The door stays open: the engine depends on nothing above
  `goofi-transport`, so an external block callback can drive it.
