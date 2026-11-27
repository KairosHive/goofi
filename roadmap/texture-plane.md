# Textures: one frame on every plane, nodes told apart by their clock

Decided 2026-11-25. A node's engine says how it is scheduled, not which kinds it carries. The
graphics engine runs what the graphics tick runs; a node on its own pace is a signal node,
whatever it reads or makes.

## Decisions

- **One frame form.** A `TEXTURE` slot carries `Value::Texture` on every wire. `Texture::Pixels`
  is the CPU form and the one a wire carries. `Texture::Shared` exists only between the graphics
  engine and the child of a node its tick calls; every other reader refuses it.
- **The graphics engine runs what its tick runs.** A detached node is a signal node. A Python
  class on `goofi.GraphicsNode`, or a Rust file against `goofi_sdk::graphics`, is a stage the
  render thread calls and waits for.
- **The tick waits for a node.** A node that overruns the frame period holds the frame, as a slow
  shader does, and its share of the frame says so. Its `setup` runs off the tick.
- **A tick node runs in a child process.** The libraries that use CUDA need the interpreter with
  the GIL, so there is one tier. The node's half builds it and runs it; the render thread and the
  half meet at the node's place in the tick (`half::Meeting`).
- **A shader input fed from another engine uploads per consumer.** Fan-out from one signal
  texture to several shaders uploads once for each. A GPU frame makes that a GPU copy.
- **`AUDIO` stays the audio engine's alone.** `audio:SignalIn` is the crossing.
- **A tick node's GPU frame is a shared buffer.** `Texture::Shared` names a Vulkan buffer whose
  memory the node's child maps with CUDA (`goofi-graphics/src/share.rs`, `goofi.gpu`). The engine
  copies a stage's output into it and the node's picture out of a second one; an image cannot be
  mapped as a tensor, so one GPU copy each way stays. The child joins the engine's socket only
  when its CUDA driver has the engine's device; anywhere else the pictures cross as pixels.
- **Synchronisation is two waits.** The render thread waits for its submission before it calls
  the node, and the child waits for its CUDA context before it answers. No semaphore crosses.
- **An 8-bit picture stays 8-bit until the GPU.** `uint8` pixels are uploaded as `Rgba8Unorm`:
  a shader samples that texture directly, and a tick node's output is drawn from it into the
  stage's `Rgba16Float` output by one pass. A tick node's `uint8` RGBA picture goes into a
  shared buffer of byte texels with one copy from the CPU (`goofi.gpu.host`).
- **A picture source is a graphics node.** `graphics:Camera`, `graphics:VideoFile` and
  `graphics:NdiIn` run on the tick. A source reads its device or decodes on a thread of its own
  and the tick takes the newest frame; `process` never waits.
- **CUDA maps Vulkan memory from another process.** Probed 2026-11-25 on an RTX 4090 Laptop
  (driver 620.70.06, torch 2.14.1+cu130), five runs of five: a device-local buffer's memory,
  exported as an opaque fd and sent over a Unix socket (`SCM_RIGHTS`) to a Python process that
  already ran CUDA, was mapped with `cuImportExternalMemory` and read and written through a
  torch tensor at the mapped pointer. Content was checked in both directions; a submission wait
  on the Vulkan side and `torch.cuda.synchronize()` on the other were the only synchronisation.

## Remaining work

1. **A texture as a value.** A bare expression read of a texture output is refused
   (`SlotType::is_value`), and a machine's expression reads one as `None`.
2. **The pixel form of a tick node is slow for a large picture.** It is the form a machine
   without CUDA has. Measured 2026-11-25 (debug profile, RTX 4090 Laptop, CUDA hidden from the
   child): `graphics:VideoFile` at 1440x1920 holds 38 to 64% of a 60 Hz frame, against 10 to
   13% through shared memory. A node that read a 1920x1080 shader output took 268 to 288 ms a
   tick; that was before the codec stopped packing a texture twice, and is not measured since.
   What is left: an input's readback converts `f16` to `f32` on the CPU, and each direction
   copies the frame into the exchange and out of it.
3. **`signal:NdiOut` as a reader of the engine.** It reads a texture as `float32` pixels and
   makes bytes of them on the CPU. The engine's 8-bit readback (what the recorder takes) would
   give it the bytes; a tick node would hold the frame for the conversion instead.

## Open questions

- **Which model nodes move to the tick.** The picture models (`DinoDecode`, `RealEsrgan`,
  `FluxRt`, `Segment`, the texture bundle) take and give pictures as `ARRAY`, have a `status`
  output beside the picture, and take 50 ms to seconds a picture. A tick node has one output
  and holds the frame while it runs.
- **A node that stalls.** A param edit that makes a node reload a model holds the tick for as
  long as the reload takes. Whether such a node should answer with its last picture meanwhile.
- **A node that writes its target in place.** `goofi.Texture(tensor)` copies the tensor into the
  shared target. A node that renders straight into the target would save that copy.
- **Platforms without CUDA.** macOS has no documented way to share a Metal buffer with PyTorch
  MPS; the CPU form is the fallback everywhere.
- **The viewer of a camera.** The bridge reducer turns an 8-bit texture into `f32` before it
  reduces it for a viewer. Reduce the pixels in their own depth if a profile shows the cost.
