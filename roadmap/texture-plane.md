# Textures: one frame on every plane, nodes told apart by their clock

Decided 2026-11-25. A node's engine says how it is scheduled, not which kinds it carries. The
graphics engine runs what the graphics tick runs; a node on its own pace is a signal node,
whatever it reads or makes.

## Decisions

- **One frame form.** A `TEXTURE` slot carries `Value::Texture` on every wire. `Texture::Pixels`
  is the CPU form and the one a wire carries. `Texture::Shared` exists only between the graphics
  engine and the child of a node its tick calls; every other reader refuses it.
- **The graphics engine runs what its tick runs.** A detached node is a signal node. A Python
  file marked `# goofi: graphics` is a stage the render thread calls and waits for. A Rust file
  against `goofi_graphics_sdk` is refused; the crate name is kept for the Rust form.
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
- **CUDA maps Vulkan memory from another process.** Probed 2026-11-25 on an RTX 4090 Laptop
  (driver 620.70.06, torch 2.14.1+cu130), five runs of five: a device-local buffer's memory,
  exported as an opaque fd and sent over a Unix socket (`SCM_RIGHTS`) to a Python process that
  already ran CUDA, was mapped with `cuImportExternalMemory` and read and written through a
  torch tensor at the mapped pointer. Content was checked in both directions; a submission wait
  on the Vulkan side and `torch.cuda.synchronize()` on the other were the only synchronisation.

## Remaining work

1. **`ai:Depth`** on `apple/DepthPro2` (S, B, L, H), image and live modes.
2. **A Rust node the tick calls**, against `goofi_graphics_sdk`.
3. **A texture as a value.** A bare expression read of a texture output is refused
   (`SlotType::is_value`), and a machine's expression reads one as `None`.
4. **The pixel form of a tick node is slow.** At 1920x1080 a tick took 268 to 288 ms through
   pixels and 3.6 to 5.9 ms through shared memory (measured 2026-11-25, debug profile, RTX 4090
   Laptop). The pixel form converts `f16` to `f32` on the CPU and copies the frame through the
   codec each way. It is the fallback on a machine without CUDA; make it faster if such a
   machine needs a tick node.

## Open questions

- **A node that stalls.** A param edit that makes a node reload a model holds the tick for as
  long as the reload takes. Whether such a node should answer with its last picture meanwhile.
- **A node that writes its target in place.** `goofi.Texture(tensor)` copies the tensor into the
  shared target. A node that renders straight into the target would save that copy.
- **Platforms without CUDA.** macOS has no documented way to share a Metal buffer with PyTorch
  MPS; the CPU form is the fallback everywhere.
- **The viewer of a camera.** The bridge reducer turns an 8-bit texture into `f32` before it
  reduces it for a viewer. Reduce the pixels in their own depth if a profile shows the cost.
