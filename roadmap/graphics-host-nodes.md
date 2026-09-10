# Rust and Python graphics nodes

## Goal

A graphics node can be authored in WGSL, Rust, or Python. All three produce ordinary
TEXTURE outputs in the same graphics runtime. Move `signal:Camera` to
`graphics:Camera`, and connect it directly to Blur, Composite, Math, and Window.
Image loading and text rendering use the same mechanism.

This is a proposed runtime/SDK design, not an implemented feature. The user has
specified the capability and Camera move; the API details below remain proposals.

## What the code does today

- `goofi-node/src/describe.rs::engine_of` assigns every `.py` to signal and every
  `.wgsl` to graphics. Rust files select an engine through their SDK import.
- `goofi-graphics/src/scan.rs::Class` requires a shader pipeline. Graphics library
  entries always report shader isolation. `plan::Stage` and `runtime::tick` also
  require that pipeline before they can produce output.
- `goofi-build` supports the signal and audio Rust SDKs, but has no graphics SDK.
- Python discovery and both execution tiers use `goofi-signal-sdk::Node`.
  Python's DataType currently exposes ARRAY, STRING, and TABLE only.
- Camera captures CPU frames through OpenCV, converts them to RGB float arrays,
  and publishes ARRAY. SignalIn receives an array, converts it to RGBA16F, uploads
  it, and draws it into another output texture.
- The graphics runtime already owns the device, queue, targets, submission,
  downstream texture views, window output, and recording/viewer readback.
  `half::Upload` and `runtime::State::upload` provide the existing CPU upload path.

The change must reach these boundaries together. Moving the Python file or
changing its output label alone would not make it a graphics node.

## Ownership and execution

Keep `graphics` as the engine identity. Language and isolation describe how the
node runs; they do not select a different texture graph or public op vocabulary.

The graphics engine owns every published GPU texture. A completed output is
identified by the graph's node generation and output slot. Shader consumers bind
that resource directly, regardless of how the producer made its pixels.

Use a class/stage variant for shader execution and host-produced textures. Keep
allocation, output publication, downstream bindings, display, and recording shared
outside the variant. A host producer must not require a dummy shader pass to enter
the graph. Shader pipeline compilation errors must not block unrelated host stages.

Blocking capture, decode, Python, and CPU processing run on a worker, outside the
graph lock and render thread. Each producer has one latest-frame mailbox. The
render thread consumes complete submissions at a frame boundary and keeps the
previous valid texture when no new frame is available. A slow producer must not
stall the graphics clock. Viewer demand controls readback, not capture scheduling.

Publish dimensions and frame content together. When a source changes size, adopt
its size and update downstream inherited sizes through one settled transition.
Explicit common width/height still override natural size. Reuse a target while its
descriptor stays the same; retire replaced textures after in-flight GPU work can
no longer use them. The graph remains the owner of instance generations.

## Producer API

Provide one validated texture-output contract, with two ways to populate it:

1. CPU frame submission, available to Rust and both Python tiers. A frame declares
   dimensions, pixel format, row stride, origin, alpha convention, timestamp, and
   owned bytes. The host validates the complete layout before it reaches wgpu.
   Start with RGB/RGBA U8 and F32 input, converted to the engine's texture format.
   Do not infer TEXTURE merely because an array has three dimensions.
2. Engine GPU work, for native producers. The host supplies a scoped output target
   and encoding operations on the existing device/queue. Submission remains the
   render runtime's job. This supports native compute/render work without creating
   a second device or reading pixels back to the CPU.

Use an opaque, generation-checked texture identifier at the SDK boundary. Rust
node libraries are cdylibs: do not pass Rust-layout wgpu objects through their C
ABI. Define host callbacks or an opaque command interface for the native GPU path.
The same ownership rules apply if Python later exposes these operations through
the Rust extension.

A Python subprocess sends a frame descriptor and bytes through the existing
shared-memory transport. It does not receive a Rust pointer or own a GPU handle.
Embedded Python uses the same validation and marshalling rules. Extract reusable
Python discovery, lifecycle, params, and execution support from its current
signal-only binding rather than creating another Python implementation.

CPU submission necessarily includes an upload. It gives Camera the same downstream
GPU texture resources as shaders, but does not claim zero-copy camera capture.
External CUDA/Vulkan/Metal texture imports need explicit device compatibility,
synchronization, and lifetime contracts. Add such an import for a concrete user;
do not describe a NumPy upload as GPU memory sharing. wgpu provides
[CPU texture writes](https://docs.rs/wgpu/latest/wgpu/struct.Queue.html#method.write_texture)
and separate [native texture access](https://docs.rs/wgpu/latest/wgpu/struct.Texture.html#method.as_hal).

## Discovery and authoring

- Add `goofi-graphics-sdk` to the existing Rust build/cache/versioned ABI flow.
- Add explicit graphics engine intent and TEXTURE output support to the shared
  Python introspection schema. A proposed declaration is `ENGINE = 'graphics'`.
  Probe and catalog routing must use one parsed result. Do not add a second regex
  parser that guesses engine identity from a class body.
- Update discovery, source editing, workspace paths, bundle embedding, unavailable
  type reporting, and restart/reload together. A `.py` suffix must no longer force
  signal identity anywhere in these paths.
- Preserve Python's automatic in-process/subprocess selection and its shared
  marshalling interface. Rebuild both installed wheels after the API changes.
- Reuse normal params, expressions, pulse/refresh, errors, and lifecycle reporting.
  A host graphics class reports its real isolation; it does not report shader.
- Initially expose the same single `out` texture as current graphics shaders.
  Additional output support must update the shared plan, not become a host-only
  special case.

## Camera and consumers

Move Camera into the graphics bundle and declare a TEXTURE output. Keep one
capture owner, the existing camera/file choice, mirror, loop, and rate controls.
Submit native U8 frames where possible instead of expanding every camera frame to
float32 before transport. Preserve top-row origin and channel order explicitly.

Remove `signal:Camera`; update checked-in callers, examples, tests, and documentation
in the same change. No alias is required under the pre-launch policy. Keep
SignalIn for actual array-to-image and plot conversion.

Computer-vision consumers still need CPU images. Trace all Camera users and provide
an explicit texture-to-array crossing where needed, through the same readback and
transport mechanism. Do not retain a second Camera or rely on an open viewer to
supply the array. Readback cost and cadence must be explicit.

## Validation and implementation order

1. Settle the frame descriptor, alpha/color contract, and discovery routing. Add
   malformed-size/stride/format and wrong-engine-output fixtures.
2. Extract the shared host-node lifecycle and add the Rust graphics SDK plus the
   Python texture-output adapter. Exercise the real dynamic-library path and both
   Python tiers; no camera hardware in tests.
3. Integrate host output stages with the existing texture allocator and render
   plan. A Rust fixture and a Python fixture each feed Blur → Math → Composite.
   Verify exact channels, natural size, resize, and no extra output draw/copy when
   the submitted format and size already match the target.
4. Test latest-wins delivery, stalled producer, no-viewer operation, restart,
   deletion during submission, generation rejection, source failure/recovery,
   recording, snapshots, and resource release. Reclaim IPC through iceoryx2.
5. Move Camera with a synthetic capture backend or generated video fixture. Update
   its array consumers and remove the old registration atomically.
6. Add a native GPU producer fixture that writes through the shared host target.
   Verify submission order and that downstream shaders sample the same resource.
   CPU producers and native GPU producers are both required for the full design;
   an upload-only milestone must be reported as such.

This precedes the Image/Text source work in `graphics-stdlib.md`. Shared frame and
alpha rules also inform future sampling, media decode, and GPU interoperability.
