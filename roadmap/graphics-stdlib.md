# Graphics standard library gaps

A build order for the graphics bundle, after TouchDesigner's [TOP catalog](https://docs.derivative.ca/TOP).
Names are proposals; none is built. The bundle leaves this repo (`library.md`); the order travels
with it.

## Decisions needed before the first batch

- Audit alpha conventions across generators, filters, viewers, and recording. Shape scales RGB by
  coverage; Constant supplies independent RGB and alpha. New nodes must not add a third convention.
- Decide texture color space, channel selection, border modes, units, and invalid numeric results
  once. Preserve HDR values where the operation permits them.
- Add engine support for multiple internal passes only for a concrete consumer (Bloom, wide
  Gaussian Blur, Optical Flow). Do not hide large, unbounded loops in a per-pixel shader.
- Image and Text sources use the shared [Rust/Python graphics producer](../sdk/graphics.md).
- Test with real GPU sessions: opaque and transparent images, unequal dimensions, boundary values,
  mode changes. Check sampling cost at useful frame sizes.

## First batch: Reorder, Color, Transform/Fit, Switch/Mix, Mask

| Capability | Smallest useful scope | TOP reference |
|---|---|---|
| Reorder | Build RGBA from input channels, luminance, zero, or one; second input for alpha and channel packing. | Reorder |
| Color | Hue shift, saturation, value, monochrome; define the working color space. Level keeps gain/offset/gamma/invert. | HSV Adjust, Monochrome |
| Fit / Crop | Contain, cover, stretch, crop, pad to the common output size; alignment and border color. Extend Transform with independent X/Y scale, pivot, and flip. | Fit, Crop, Transform, Flip |
| Switch / Mix | Select a texture or crossfade two textures independent of coverage. Two inputs first; many-input selection needs shared engine support. | Switch, Cross |
| Mask | Replace or multiply alpha from a selected channel of another image; invert and remap the mask; keep foreground RGB. | Matte |

## Second batch: Function/Operation, Edge/Convolve, Image, Text, Remap/Pattern

| Capability | Smallest useful scope | TOP reference |
|---|---|---|
| Function | Per-channel abs, sign, power, root, log, exp, sin/cos, floor, ceil, round, fract; define invalid-domain results. Math stays the scale/range node. | Function |
| Operation | Raw channel add/subtract/multiply/divide/min/max with texture or scalar operand; explicit alpha policy and zero-divisor behavior. | Math, Function |
| Edge / Convolve | Shared neighborhood sampling; Sobel magnitude/direction, Laplacian, sharpen, emboss, small custom kernel. Presets share one node. | Edge, Convolve, Emboss |
| Image | Load a still image from the patch workspace, preserve alpha, report decode failures. | Movie File In |
| Text | Render a string with font, size, alignment, wrapping, foreground, background. Needs a font/raster upload path. | Text |
| Remap | Sample an image at absolute UV coordinates from another texture; explicit outside-frame behavior. Could be a Displace mode. | Remap |
| Pattern | Checker, grid, stripes, radial/angular coordinates. Extend Ramp/Shape where appropriate; Wave is a simulation, not this. | Ramp, Circle, Rectangle |

## After the foundations

| Capability | Proposed scope |
|---|---|
| Key | Luma and chroma keys, soft selection, spill suppression. |
| Morphology | Dilate and erode masks; opening/closing as compositions. |
| Channel Mix | A channel matrix; add only after Reorder proves insufficient. |
| Color space / Tone map | RGB↔HSV, linear↔sRGB, HDR-to-display; after the color contract is agreed. |
| Bloom | Bright-region extraction and multi-scale blur/add; a patch first, a node needs multi-pass. |
| Normal / Slope | Height-to-gradient and height-to-normal; share derivative kernels with Edge. |
| Corner Pin | Four-corner projective warp. |
| Lens / Polar | Lens distortion and cartesian↔polar; prefer Remap presets where they suffice. |
| Layout | Arrange several images in a row, column, or grid inside a texture. |
| Resample | Nearest, linear, a proper downsample filter, explicit border modes; a node only for a distinct resize stage. |
| Blur extensions | Bilateral blur and mask-driven radius, inside Blur. |
| Composite extensions | Hue, saturation, color, luminosity blend modes, inside Composite. |

## Engine-backed work

| Capability | Why it is more than a fragment shader |
|---|---|
| Cache / Delay / Hold | Bounded GPU frame history with capture, freeze, reset, indexed delay; one owner for allocation and advancement; no viewer-driven clock. |
| Analyze / Histogram | Min/max/mean and distributions need GPU reduction and a defined result format. |
| Sample / Texture-to-signal | Read pixels, rows, or regions into arrays through the transport; specify readback rate and cost; never viewer snapshots. |
| Media playback | Still images first; later seek, pause, speed, timestamps, image sequences through the existing source owner. |
| Optical Flow | Motion vectors from successive frames; needs history and a pyramid or multiple passes; after Cache. |
| External texture I/O | Screen capture and Spout/Syphon/NDI; optional, not blocking the basic bundle. |

## Not to be done

- No separate Add, Multiply, Over, Under, Invert, Limit, Circle, Rectangle, Movie File Out, or
  texture In/Out nodes merely to match TOP names: existing nodes cover those roles.
- General 3D rendering (cameras, lights, materials) and vendor camera integrations are not core 2D
  standard-library prerequisites.
