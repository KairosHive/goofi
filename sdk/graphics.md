# Textures

A `TEXTURE` slot carries a picture. A node's engine says how the node is scheduled, not which
kinds it carries. A graphics node runs on the graphics tick: a shader, or a Python node the tick
calls. A Python or Rust node that makes or reads a picture on its own pace is a signal node.

## Shaders

A `.wgsl` file is a graphics node. The graphics engine owns its output texture, its upload
textures, its state buffers and its readback buffers. Between two shaders a texture stays on the
GPU. Read `graphics:Life` for state kept between ticks.

Where a signal node shows its updates per second, a shader shows the time of its pass as a
percentage of the frame period: a pass of 1/120 s at 60 frames per second is 50%. A shader that
is not drawn shows 0%. An adapter without timestamp queries shows nothing
(`session status` has `graphics.timed`).

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

## A Python node the graphics tick calls

Put `# goofi: graphics` on the first line of a Python file in `nodes_graphics/`. The node is a
stage of the render plan, as a shader is:

```python
# goofi: graphics
import goofi

class Invert(goofi.Node):
    INPUTS = {'image': goofi.InputSlot(goofi.DataType.TEXTURE)}
    OUTPUTS = {'out': goofi.DataType.TEXTURE}

    def process(self, image):
        if image is None:
            return None
        pixels = image.data.numpy()          # [height, width, 4], float32
        return goofi.Texture(1.0 - pixels)
```

- At the node's place in the tick, the engine finishes the stages before it, calls `process`
  and waits for the answer. The picture the node returns is the output the next stages read, in
  the same tick. A node that takes 1/120 s at 60 frames per second shows 50%; a node that takes
  longer than the frame period holds every stage back, and shows more than 100%.
- The node has one output, a `TEXTURE`, and its output is the size of the picture it returns.
  It has no `common` width or height. An input takes one wire.
- An input that a shader feeds is read back from the GPU as `RGBA` `float32`. An input that
  another engine feeds is the frame that engine sent.
- `process` that returns `None` keeps the last picture. A raise is the node's error, and the
  last picture stays.
- The node runs in a child process of the subprocess interpreter. Load a model in `setup`: the
  tick passes the stage by until `setup` is done.

A Rust file against `goofi_graphics_sdk` is refused: that name is kept for a Rust node the tick
calls (`roadmap/texture-plane.md`).
