# Graphics node sources

Graphics nodes can use WGSL, Rust, or Python. All outputs use the same GPU texture
resources, downstream shader connections, windows, recording, and readback.

## Python

Put `# goofi: graphics` on the first line of the file. Declare one `TEXTURE` output
named `out`. Return `goofi.Texture(pixels)` from `process`:

```python
# goofi: graphics
import goofi
import numpy as np

class Image(goofi.Node):
    OUTPUTS = {'out': goofi.DataType.TEXTURE}
    PRODUCER = True

    def process(self):
        return goofi.Texture(np.ones((64, 128, 3), dtype=np.uint8) * 255)
```

Pixels must have shape `[height, width, 3 or 4]` and dtype `uint8` or little-endian
`float32`. Row zero is the top row. Alpha is straight. RGB values use the patch's
working color space. RGB input receives alpha 1. Python makes non-contiguous arrays
contiguous. Dimensions must be within 1 through 8192.

Automatic Python tier selection applies: safe imports use embedded free-threaded
Python; other imports use a subprocess. Both tiers use the same texture contract.
Capture and processing run on a host worker. A viewer is not required to run it.

## Rust

Use `goofi_graphics_sdk` with the same `Node` lifecycle as `goofi_signal_sdk`.
Declare one `SlotType::Texture` output named `out`. Submit a validated frame:

```rust
let pixels = Pixels {
    width: 1, height: 1, stride: 3,
    format: PixelFormat::Rgb8,
    bytes: vec![255, 0, 0],
};
out.set("out", Data::texture(Texture::Pixels(pixels), Meta::new())?);
```

Import `Pixels`, `PixelFormat`, and `Texture` from `goofi_graphics_sdk`, and `Data`
and `Meta` from `goofi_core`. Rust pixels support RGB/RGBA U8 and F32. The stride
includes padding; the byte count must equal stride times height.

A native producer can submit GPU work with
`Texture::Render { width, height, source }`. The source defines
`fn shade(uv: vec2f) -> vec4f`. It uses the regular graphics uniforms and writes
into the engine-owned output target. The engine compiles and submits this program
on its existing device. No wgpu object or pointer crosses the dynamic-library ABI.
This command supports render programs; arbitrary native GPU handles and external
CUDA/Vulkan/Metal imports are not part of this API.

Host nodes currently accept single CPU-frame inputs. Texture processing uses
shader nodes. Scheduling, params, pulse, refresh, and error handling use the shared
host runtime. There is one pending frame per producer; a new frame replaces an
unconsumed one. The last valid GPU texture remains until another frame arrives.

## Resource ownership

The graphics engine owns output textures, upload textures, state buffers, and
readback buffers. Host workers submit owned pixels or GPU programs. They never
allocate a separate GPU device. Natural output dimensions follow each submission;
zero common width/height inherits that size. Explicit dimensions request resizing.
A size change updates downstream inherited dimensions at graph settlement.

CPU frames require an upload. A matching-size frame writes directly to the output;
it does not need a shader pass. Shader consumers then sample that same resource.
GPU programs render directly into it. Readback happens only for consumers that
request CPU pixels, viewers, snapshots, or recording.

`graphics:Camera` captures devices and video files. Connect it directly to graphics
nodes. Use `signal:GraphicsIn` when a CPU consumer needs pixels. That crossing uses
shared GPU readback at the graphics clock and works without an open viewer.
