# Engines in the browser: one crate, two hosts

Decided 2026-09-16, out of the performance audit and the render-surface work
(`viewer-render-surface.md`). The graphics and audio engines will also run IN THE BROWSER, from
the same Rust crates the backend runs, compiled to WebAssembly. The signal engine stays in the
backend. Nothing is mirrored: an engine is one crate with a host seam, and the browser is a second
implementation of that seam, never a second implementation of the engine.

## The principle, and what it is not

It is not derivation and needs no generator. wgpu already runs one code base on Vulkan, Metal,
DX12 and WebGPU, and WGSL runs unchanged on WebGPU; the Web Audio API runs a wasm graph inside an
`AudioWorklet` exactly as cpal runs one inside its callback. What an engine needs from the
platform — frames in, output out, a clock, sources, producers — is a trait with two
implementations. Everything that plans, allocates, evaluates and renders compiles for both
targets from one file.

The codebase is already shaped for it. Every process, thread, scratch path and device is minted
through `goofi_core::{child, worker, registry}`, every port through `goofi_transport`, every
dynamic library through `goofi_build`. That funnel IS the seam; today it has one implementation.

## Per-engine verdict

| engine | what it is | native primitive | web equivalent | verdict |
|---|---|---|---|---|
| graphics | a plan, executed on one thread | wgpu, fragment shaders only, four formats | wgpu on WebGPU, canvas present | **cheap** — `gpu.rs`, `plan.rs`, `shader.rs`, `resources.rs`, `half.rs` compile as they are; ~1 k lines of seam in `lib.rs`, `runtime.rs`, `producer.rs`, `scan.rs` |
| audio | a plan, executed in a callback on its own clock | cpal, `rtrb` rings, Rust nodes as dylibs, VST3, midir | an `AudioWorklet` running the wasm graph at 128-sample quanta, `getUserMedia` in, Web MIDI | **medium** — a path web DAWs walk; nodes must load as `.wasm`; VST3 never crosses |
| signal | a RUNTIME: self-scheduling node threads, iceoryx2 latest-wins memory, child processes, an in-process and a subprocess Python tier, LSL/OSC/serial/BLE/MIDI devices | all of the above | wasm threads need `SharedArrayBuffer`; no processes; Python only as Pyodide at 3–10× and ~10 MB; LSL and OSC are UDP and cannot exist in a page; Web Serial/Bluetooth/MIDI are Chrome-only | **backend only** — the runtime is most of the engine, and it is the part that does not port |

The distinction that decides it: **graphics and audio are executors of a plan; signal is a
runtime.** An executor moves behind a seam. A runtime would be redesigned around workers, for a
subset of devices, and that is a different product.

## Which product this is

Two things hide behind "engines in the browser":

- **Placement.** The backend stays the manager and owns the document. An engine may ALSO run in
  the browser, as a projection of that document, one direction. Value now: native-resolution
  graphics on the tablet with no readback, and audio playing on the tablet's own speakers while
  the backend runs the analysis — a phone becomes an output device by opening the page.
- **Standalone.** Manager and engines in wasm, no backend: goofi as a static page. Distribution
  and onboarding value, but it drags `goofi-graph`, commands, undo, sessions and autosave through
  the same seam, and without a signal engine it is not goofi.

This entry is **placement**. Standalone is not planned; it becomes a question again only if a
browser subset of the signal engine is ever wanted, and every seam cut here would serve it then.

## Rules

- **A browser engine is a projection.** It reads the document replica the page already holds and
  the EVALUATED param values from `/params/{node}`; it never writes, never feeds the backend, and
  is never the recording or the window. Expressions are evaluated once, in the backend. Two
  engines rendering one document differ in clock and seed; that is a property of a preview, and
  it is documented rather than chased.
- **Mode is a capability of the open patch, reported by the manager**: `backend`, `browser` or
  `both`, per engine, from the node types the patch holds. A patch with a Python producer or a
  `graphics:Window` offers `backend` and `both`, never `browser`. There is no per-node placement:
  streaming one node's texture into a browser sub-graph is where the design would stop being
  small, and it is refused.
- **One entry ABI, two artifacts.** `goofi_build` emits `.wasm` beside `.so` for a node crate,
  against the same SDK symbols; the page loads it with `WebAssembly.instantiate`. Porting shipped
  nodes by hand would be mirroring under another name.
- **No engine logic in the glue.** The `wasm-bindgen` layer owns the canvas, the worklet node,
  the sockets and the calls into the engine — nothing else.
- **WebGPU is required for a browser engine, and only there.** The mode is optional and
  feature-detected; the render surface every user needs stays WebGL2 (`viewer-render-surface.md`).
  wgpu's WebGL2 backend cannot share a context with anything and lacks float render targets.
- **Cross-origin isolation is a server decision.** `SharedArrayBuffer` — wanted the moment the
  audio worklet shares a latest-wins buffer with the page — needs COOP/COEP on the page, which
  forbids embedding cross-origin resources without CORP. Check the plugin panel's assets and the
  fonts before turning it on; a page that does not need it does not set it.

## Phase 1: graphics

The engine crate gains a `Host` trait with the platform in it and nothing else:

- `frame(uid, slot) → Option<Frame>` — natively a `goofi_transport` subscriber, in the page the
  latest frame the `/data` socket for that `SignalIn` delivered, at full resolution and `f16`
  where the producer allows it (`data-plane-bandwidth.md`).
- `present(uid, texture)` — natively `goofi_window`, in the page the canvas the mode was opened on.
- `now()` — the native clock, or `performance.now()` driven from `requestAnimationFrame`.
- `source(path) → String` — `std::fs`, or a fetch of the patch's shader files from a route the
  bridge serves out of the workspace.
- `producer(kind)` — `goofi_build` + dlopen + Python natively; in the page `None`, which is what
  makes the capability rule answer `backend` for such a patch.

The graph subset is derived from the replica: graphics nodes, links among them, params. The
recorder and `signal:GraphicsIn` stay backend concerns and make the same capability answer.
Presentation goes to a canvas of its own first; once the render surface has a WebGPU backend, the
engine's output texture is drawn into a viewer rect on the shared device, zero-copy, which is what
a `graphics` viewer kind in the browser should be. Costs: ~1 MB gzipped of wgpu-on-wasm served
once from `frontend/build`; `Rgba32Float` sampling needs `float32-filterable`; and the seam is a
refactor of the engine's clock and shutdown order, so it comes with a `goofi-tests` situation
that drives both hosts through the test clock.

## Phase 2: audio

The same seam, with the worklet in place of cpal: the plan runs in an `AudioWorkletProcessor`
per 128-sample quantum, `getUserMedia` stands in for `audio_in`, the device output for
`audio_out`, Web MIDI for `midir` where the browser has it. Inputs cross the same doors graphics
uses — `SignalIn` from `/data`, `GraphicsIn` only when graphics runs in the browser too, on the
same device. VST3 modules are native code and make the patch `backend`-only. Nodes load as the
`.wasm` artifacts `goofi_build` already emits by then. The control ring (`control.rs`) is the
part to read first: it is where the native engine's threading shows, and where the worklet's
message port and a shared buffer replace it.

## Signal: backend only

Decided, not deferred. What a browser signal engine could offer is synthesized and derived
signals, microphone, and Chrome-only device APIs — with no LSL, no OSC, no subprocess tier and a
slow Python — behind a runtime rewritten around workers. That is neither the product nor a step
towards it. The signal engine's job in this design is to feed the browser engines through `/data`,
and the bandwidth track is what makes that feed cheap enough.

## Order

1. The graphics `Host` seam, native only, with the two-host test situation — no behaviour change.
2. Graphics in the browser, `browser` and `both` modes, own canvas, capability reported by the
   manager, the mode switch in the UI reading that capability.
3. `goofi_build` emitting `.wasm` artifacts; the render surface's WebGPU backend; the graphics
   output drawn into viewer rects.
4. The audio seam and the worklet.

## Open

- Where the mode switch lives in the UI and whether it is per patch or per client — a tablet
  wants `browser` while the desktop beside it wants `backend`, so it is likely per client, held
  with the viewpoint rather than in the document.
- Whether `/params` at 20 Hz is a smooth enough source for a param a signal drives at frame rate;
  the period is a constant and the socket is per node, so it can be tuned without a new door.
- Sub-patches whose members span engines: the capability is computed over the whole patch, and
  a graphics sub-graph inside a signal sub-patch is still graphics.
- Whether user Rust host nodes in `.wasm` can share the wgpu device with the engine: a wasm
  module holds JS handles, not GL objects, so the glue passes textures by id. Plausible, untested.
