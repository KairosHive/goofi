# Textures: one frame on every plane, nodes told apart by their clock

Decided 2026-11-25. A node's engine says how it is scheduled, not which kinds it carries. The
graphics engine runs what the graphics tick runs; a node on its own pace is a signal node,
whatever it reads or makes.

## Decisions

- **One frame form.** A `TEXTURE` slot carries `Value::Texture` on every wire. `Texture::Pixels`
  is the CPU form; the GPU form is a second variant beside it, and every reader that
  converts through `Pixels::to_array` today then reads the GPU form through one readback.
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
- **CUDA maps Vulkan memory from another process.** Probed 2026-11-25 on an RTX 4090 Laptop
  (driver 620.70.06, torch 2.14.1+cu130), five runs of five: a device-local buffer's memory,
  exported as an opaque fd and sent over a Unix socket (`SCM_RIGHTS`) to a Python process that
  already ran CUDA, was mapped with `cuImportExternalMemory` and read and written through a
  torch tensor at the mapped pointer. Content was checked in both directions; a submission wait
  on the Vulkan side and `torch.cuda.synchronize()` on the other were the only synchronisation.

## Remaining work

1. **GPU frames for a tick node.** Today a tick node's input is read back to the CPU and its
   picture is uploaded. `Texture` gains a GPU form: an exportable Vulkan buffer the engine copies
   a stage's output into, sent to the child as an fd over a Unix socket and mapped by CUDA. The
   node reads it as a torch tensor at the mapped pointer and writes its picture into a second
   such buffer, which the engine copies into the stage's output. The CPU form stays for a device
   that cannot share and for a child without CUDA.
2. **Helpers.** `goofi.Texture` converts a texture, a CUDA buffer (anything with
   `__cuda_array_interface__`) and a numpy array in every direction, for a tick node and a
   signal node alike. `numpy()` answers a read-only array today; make it writable.
3. **`ai:Depth`** on `apple/DepthPro2` (S, B, L, H), image and live modes.
4. **A Rust node the tick calls**, against `goofi_graphics_sdk`.
5. **A texture as a value.** A bare expression read of a texture output is refused
   (`SlotType::is_value`), and a machine's expression reads one as `None`.

## Open questions

- **A node that stalls.** A param edit that makes a node reload a model holds the tick for as
  long as the reload takes. Whether such a node should answer with its last picture meanwhile.
- **Synchronisation.** External semaphores need the device opened through
  `open_with_callback` (wgpu-hal does not enable `VK_KHR_external_semaphore_fd`); the other way
  is a submission wait on the node's side before it rings the child.
- **Platforms without CUDA.** macOS has no documented way to share a Metal buffer with PyTorch
  MPS; the CPU form is the fallback everywhere.
- **The viewer of a camera.** The bridge reducer turns an 8-bit texture into `f32` before it
  reduces it for a viewer. Reduce the pixels in their own depth if a profile shows the cost.
