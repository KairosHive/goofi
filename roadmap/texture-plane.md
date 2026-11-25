# Textures: one frame on every plane, nodes told apart by their clock

Decided 2026-11-25. A node's engine says how it is scheduled, not which kinds it carries. The
graphics engine runs what the graphics tick runs; a node on its own pace is a signal node,
whatever it reads or makes.

## Decisions

- **One frame form.** A `TEXTURE` slot carries `Value::Texture` on every wire. `Texture::Pixels`
  is the CPU form; the GPU form of step 2 is a second variant beside it, and every reader that
  converts through `Pixels::to_array` today then reads the GPU form through one readback.
- **The graphics engine runs what its tick runs.** A detached node is a signal node. A Python
  file marked `# goofi: graphics` and a Rust file against `goofi_graphics_sdk` are refused until
  the tick-synchronous kind of step 2 exists; the header and the crate name are kept for it.
- **A shader input fed from another engine uploads per consumer.** Fan-out from one signal
  texture to several shaders uploads once for each. A GPU frame makes that a GPU copy.
- **`AUDIO` stays the audio engine's alone.** `audio:SignalIn` is the crossing.

## Remaining work

1. **Frame-time share per graphics node.** Each shader stage is one render pass in the tick's one
   encoder, so a pass is timed with timestamp queries (`Features::TIMESTAMP_QUERY`,
   `timestamp_writes`). The node shows its pass time as a percentage of the frame period
   (`1 / graphics.fps`) where a signal node shows its updates per second. Nothing is shown on an
   adapter without timestamp queries.
2. **Tick-synchronous nodes with GPU frames.** A Python or Rust node the graphics tick runs,
   `TEXTURE` in and out, whose frames stay on the GPU. `Texture` gains a GPU form: a buffer the
   engine allocates with exportable memory and CUDA maps. Helpers convert a texture, a CUDA
   buffer (a torch tensor through `__cuda_array_interface__`) and a numpy array in every
   direction, and serve detached nodes too.
3. **`ai:Depth`** on `apple/DepthPro2` (S, B, L, H), image and live modes.
4. **A texture as a value.** A bare expression read of a texture output is refused
   (`SlotType::is_value`), and a machine's expression reads one as `None`.

## Open questions

- **Cross-process import.** The Vulkan spec lets an opaque fd cross a socket; no NVIDIA document
  says `cuImportExternalMemory` takes one from another process. A probe proves it before step 2
  is designed: five runs, both directions, the wgpu adapter and CUDA device 0 on one GPU.
- **A late node on the tick.** Whether the tick waits for a tick-synchronous node that overruns
  the frame period, or draws its last frame and reports the overrun.
- **Synchronisation.** External semaphores need the device opened through
  `open_with_callback` (wgpu-hal does not enable `VK_KHR_external_semaphore_fd`); the other way
  is a submission wait on the node's side before it rings the child.
- **Platforms without CUDA.** macOS has no documented way to share a Metal buffer with PyTorch
  MPS; the CPU form is the fallback everywhere.
- **The viewer of a camera.** The bridge reducer turns an 8-bit texture into `f32` before it
  reduces it for a viewer. Reduce the pixels in their own depth if a profile shows the cost.
