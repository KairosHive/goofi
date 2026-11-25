# Textures

A `TEXTURE` slot carries a picture. A node's engine says how the node is scheduled, not which
kinds it carries: a shader runs on the graphics tick, and a Python or Rust node that makes or reads
a picture is a signal node on its own pace.

## Shaders

A `.wgsl` file is a graphics node. The graphics engine owns its output texture, its upload
textures, its state buffers and its readback buffers. Between two shaders a texture stays on the
GPU. Read `graphics:Life` for state kept between ticks.

## A signal node that makes a texture

Declare a `TEXTURE` output and return `goofi.Texture(pixels)` from `process`:

```python
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

Return `(goofi.Texture(pixels), {"sfreq": fps})` to state the frame rate. The recorder files an
armed texture output as video at that rate, and refuses a texture that states none.

In Rust, use `goofi_signal_sdk` and declare a `SlotType::Texture` output:

```rust
let pixels = Pixels {
    width: 1, height: 1, stride: 3,
    format: PixelFormat::Rgb8,
    bytes: vec![255, 0, 0],
};
out.set("out", Data::texture(Texture::Pixels(pixels), Meta::new())?);
```

Import `Pixels`, `PixelFormat` and `Texture` from `goofi_core::texture`, and `Data` and `Meta`
from `goofi_core`. The stride includes padding; the byte count must equal stride times height.

`signal:Camera` captures devices and video files this way. Connect it directly to a shader.

## A signal node that reads a texture

Declare a `TEXTURE` input. The frame's `data` is a `goofi.Texture`:

```python
INPUTS = {'image': goofi.InputSlot(goofi.DataType.TEXTURE)}

def process(self, image):
    pixels = image.data.numpy()   # [height, width, 3 or 4], uint8 or float32
```

In Rust, `frame.as_texture()` gives the texture, and `frame.pixels_as_array()` gives the
`[H, W, C]` array of values in 0..1. `signal:GraphicsIn` is that crossing as a node.

## How a texture crosses

- A shader's output that another engine reads is read back from the GPU and sent as `RGBA`
  `float32` pixels. Readback happens only for a reader: a consumer on another engine, a viewer, a
  snapshot or a recording.
- A signal node's texture that a shader reads is uploaded into that shader's own input texture.
  There is one pending frame per input; a new frame replaces one not yet uploaded, and the last
  uploaded texture stays until another frame arrives.
- A shader whose `common` width or height is zero follows the first texture input that is fed: a
  shader's size, or the size of the frame a signal node sent.

A file that starts with `# goofi: graphics`, or a Rust file against `goofi_graphics_sdk`, is
reserved for a node that the graphics tick runs. The graphics engine refuses it until that kind
exists (`roadmap/texture-plane.md`).
