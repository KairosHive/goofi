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

A picture source that follows the frames of the patch (`graphics:Camera`, `graphics:VideoFile`,
`graphics:NdiIn`) is a graphics node instead: see "A Python node the graphics tick calls".

## A signal node that reads a texture

Declare a `TEXTURE` input. The frame's `data` is a `goofi.Texture`:

```python
INPUTS = {'image': goofi.InputSlot(goofi.DataType.TEXTURE)}

def process(self, image):
    pixels = image.data.numpy()   # [height, width, 3 or 4], uint8 or float32
```

In Rust, `frame.as_texture()` gives the texture, and `frame.pixels_as_array()` gives the
`[H, W, C]` array of values in 0..1. `signal:GraphicsIn` is that crossing as a node.

## Textures, CUDA and numpy

In Python, a picture has three forms, and each one converts to each other one:

| from | to a texture | to numpy | to CUDA |
| --- | --- | --- | --- |
| a `goofi.Texture` | | `texture.numpy()` | `texture.cuda()`, `texture.torch()` |
| a numpy array | `goofi.Texture(array)` | | `goofi.gpu.cuda(array)` |
| CUDA memory | `goofi.Texture(tensor)` | `goofi.gpu.numpy(tensor)` | |

- CUDA memory is a torch tensor on a CUDA device, a cupy array, or any object with
  `__cuda_array_interface__`. `texture.cuda()` and `goofi.gpu.cuda(array)` give such an object;
  its `.torch()` is a tensor on the same memory.
- `texture.numpy()` gives an array of its own that the node can write.
- `texture.torch()` gives a tensor `[height, width, channels]`. `texture.torch('cuda')` moves it.
- `goofi.Texture(tensor)` takes `[height, width, 3 or 4]`, `uint8` or a float type. A plane
  `[height, width]` is grey.
- `texture.shared` says whether the picture is in device memory that the node shares with the
  graphics engine. That is so only in a node the graphics tick calls (see below). In a signal
  node, each conversion to or from CUDA is one copy between the CPU and the GPU.
- CUDA support needs Linux and the NVIDIA driver (`libcuda.so.1`). torch is needed only for
  `torch()`.

## How a texture crosses

- A shader's output that another engine reads is read back from the GPU and sent as `RGBA`
  `float32` pixels. Readback happens only for a reader: a consumer on another engine, a viewer, a
  snapshot or a recording.
- A signal node's texture that a shader reads is uploaded into that shader's own input texture.
  There is one pending frame per input; a new frame replaces one not yet uploaded, and the last
  uploaded texture stays until another frame arrives. `uint8` pixels go to the GPU as bytes, and
  the GPU reads them as 0..1; `float32` pixels are converted to `float16` on the CPU.
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

With torch, the same node keeps the picture on the GPU where the machine shares memory, and
works on the CPU where it does not:

```python
    def process(self, image):
        if image is None:
            return None
        pixels = image.data.torch()          # [height, width, 4]; no copy where it is shared
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
- A source has no input: `process(self)` runs at each tick while something reads the node.
  Do not wait in it. Read a device or decode a file on a thread of the node's own, and return
  the newest frame, or `None` when there is no new one (`graphics:Camera`, `graphics:VideoFile`).
- Return `uint8` `RGBA` where the source has 8-bit pictures (`cv2.COLOR_BGR2RGBA`). The bytes
  cross with no conversion on the CPU, and where the node shares memory with the engine (next
  point) one copy puts them on the GPU.
- On Linux, when the graphics device is an NVIDIA GPU and the node's interpreter finds the CUDA
  driver for the same GPU, the pictures stay on the GPU. The engine copies a shader's output
  into a buffer whose memory CUDA maps in the node's process: `image.data.torch()` is a tensor
  on that memory with no copy, `RGBA` `float16`. `goofi.Texture(tensor)` copies the result,
  CUDA to CUDA, into a second such buffer, which the engine copies into the node's output. The
  first picture of each new size crosses as pixels. `session status` counts these nodes as
  `graphics.shared`.
- The memory behind an input holds that picture for one `process` call. Copy the tensor to keep
  it for a later call.
- `process` that returns `None` keeps the last picture. A raise is the node's error, and the
  last picture stays.
- The node runs in a child process of the subprocess interpreter. Load a model in `setup`: the
  tick passes the stage by until `setup` is done.

A Rust file against `goofi_graphics_sdk` is refused: that name is kept for a Rust node the tick
calls (`roadmap/texture-plane.md`).
