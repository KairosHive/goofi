# Textures: one frame on every plane, nodes told apart by their clock

Decided 2026-11-25. A node's engine says how it is scheduled, not which kinds it carries. The
graphics engine runs what the graphics tick runs; a node on its own pace is a signal node,
whatever it reads or makes.

## Decisions

- **One frame form.** A `TEXTURE` slot carries `Value::Texture` on every wire. `Texture::Pixels`
  is the CPU form (RGB or RGBA, `u8` or `f32`); step 3 adds the GPU form beside it. An array is
  never a texture frame, and `Texture::Render` (a host's shader program) is gone.
- **Any engine may produce a texture.** `AUDIO` stays the audio engine's alone. A signal node
  (Python on both tiers, Rust shipped and hosted) declares `TEXTURE` inputs and outputs.
- **The graphics engine runs shaders.** The detached host arm is gone: `Camera`, `VideoFile`,
  `NdiIn` and `Variable` are signal nodes with a `TEXTURE` output, and `goofi_graphics_sdk` is
  removed until the tick-synchronous kind needs one.
- **A shader's `TEXTURE` input has two feeds.** A wire from a graphics node is a plan edge, as it
  was. A wire from any other engine is subscribed, and its frame is uploaded into the consumer's
  own input texture, the way an `ARRAY` input is. Fan-out uploads once per consumer.
- **The tap publishes a texture.** A graphics output read by another engine goes out as
  `Texture::Pixels` in `Rgba32Float`. The 8-bit boxed frame for an unwired viewer stays an
  encoded array frame.
- **Every reader converts in one place.** `Pixels::to_array` gives the `[H, W, C]` array of values
  in 0..1. The bridge reducer converts on arrival (viewers, snapshots, followed variables), the
  recorder files a texture as video, and Python reads `data.numpy()`.
- **`# goofi: graphics` on a Python file is reserved** for the tick-synchronous kind. Until it
  lands the graphics engine refuses such a file with that reason.

## Remaining work

1. **Unification** (in progress): the decisions above, with the situations that own the surface.
2. **Frame-time share per graphics node.** Each shader stage is one render pass in the tick's one
   encoder, so a pass is timed with timestamp queries (`Features::TIMESTAMP_QUERY`,
   `timestamp_writes`). The node shows its pass time as a percentage of the frame period
   (`1 / graphics.fps`) where a signal node shows its updates per second. Nothing is shown on an
   adapter without timestamp queries.
3. **Tick-synchronous nodes with GPU frames.** A Python or Rust node the graphics tick runs,
   `TEXTURE` in and out, whose frames stay on the GPU. `Texture` gains a GPU form: a buffer the
   engine allocates with exportable memory and CUDA maps. Helpers convert a texture, a CUDA
   buffer (a torch tensor through `__cuda_array_interface__`) and a numpy array in every
   direction, and serve detached nodes too.
4. **`ai:Depth`** on `apple/DepthPro2` (S, B, L, H), image and live modes.

## Open questions

- **Cross-process import.** The Vulkan spec lets an opaque fd cross a socket; no NVIDIA document
  says `cuImportExternalMemory` takes one from another process. A probe proves it before step 3
  is designed: five runs, both directions, the wgpu adapter and CUDA device 0 on one GPU.
- **A late node on the tick.** Whether the tick waits for a tick-synchronous node that overruns
  the frame period, or draws its last frame and reports the overrun.
- **Synchronisation.** External semaphores need the device opened through
  `open_with_callback` (wgpu-hal does not enable `VK_KHR_external_semaphore_fd`); the other way
  is a submission wait on the node's side before it rings the child.
- **Platforms without CUDA.** macOS has no documented way to share a Metal buffer with PyTorch
  MPS; the CPU form is the fallback everywhere.
- **`AUDIO` from a signal node.** Not part of this work; `audio:SignalIn` is the crossing.
