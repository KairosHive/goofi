# Graphics standard library gaps

Reviewed 2026-09-10 against `node-bundles/graphics/`, the related signal nodes, and
TouchDesigner's [TOP catalog](https://docs.derivative.ca/TOP). This is a proposed
build order for goofi, not a plan to copy every TOP. Names below are proposals.

## Current coverage

The graphics bundle has Constant, Ramp, Shape, Noise, Transform, Displace, Lookup,
Level, Threshold, Composite, Blur, Math, Feedback, Tessellate, SignalIn, AudioIn,
Window, and simulation nodes. Composite now has 20 modes; Blur has six algorithms;
Math provides scale/shift, range mapping, bounds, and RGB/RGBA/alpha selection.

Important limits of that coverage:

- Transform has translation, rotation, uniform positive scale, and tiling. It has
  no separate X/Y scale, pivot, flip, or aspect-preserving fit.
- Common width/height already control output resolution. The missing capability
  is good resampling and fit policy, not a second pair of size controls.
- Level has gain, offset, gamma, and invert. It has no hue or saturation controls.
- Composite combines two images with alpha. Its arithmetic modes do not provide
  raw channel arithmetic independent of coverage. Its blend control is not a
  general crossfade between A and B.
- Lookup reads a one-dimensional palette from one channel. Displace applies
  relative RG offsets; neither provides an absolute two-dimensional UV lookup.
- Feedback supplies the previous tick. It does not provide a configurable frame
  history, indexed playback, or capture/hold controls.
- Camera/video input exists through `signal:Camera` and `graphics:SignalIn`.
  The camera node owns the device or file. Native texture input is an optimization
  and media-control project, not a reason to open the same device twice.
- `signal:Text` emits a string; it does not render text. Signal Switch and Select
  operate on arrays, not GPU textures.
- Texture video recording already exists in `goofi-record`. Window already handles
  display. Subpatches already have texture input/output ports.
- `harmonic-geometry:GeometryRender` draws its geometry format with a fixed view.
  A general scene renderer with cameras, lights, and materials remains separate.

## First: everyday image operations

| Priority | Capability | Smallest useful goofi scope | TouchDesigner reference |
|---|---|---|---|
| 1 | Reorder | Build RGBA from input channels, luminance, zero, or one. Support a second input for alpha and channel packing. | Reorder |
| 2 | Color | Hue shift, saturation, value, and monochrome; define the working color space. | HSV Adjust, Monochrome |
| 3 | Fit / Crop | Contain, cover, stretch, crop, and pad to the common output size; alignment and border color. Extend Transform with independent scale, pivot, and flip. | Fit, Crop, Transform, Flip |
| 4 | Switch / Mix | Select a texture or crossfade two textures independent of their coverage. Begin with two inputs; many-input selection needs shared engine support. | Switch, Cross |
| 5 | Mask | Replace or multiply alpha from a selected channel of another image; invert and remap the mask. Keep foreground RGB intact. | Matte |
| 6 | Function | Per-channel abs, sign, power, root, log, exp, sin/cos, floor, ceil, round, and fractional part. Define invalid-domain results. Math remains the scale/range node. | Function |
| 7 | Operation | Raw channel add/subtract/multiply/divide/min/max, with texture or scalar operand; explicit alpha policy and zero-divisor behavior. | Math, Function; signal Operation is the local naming precedent |
| 8 | Edge / Convolve | Shared neighborhood sampling; Sobel edge magnitude/direction, Laplacian, sharpen, emboss, and a small custom kernel. Presets can share one node. | Edge, Convolve, Emboss |
| 9 | Image | Load a still image from the patch workspace, preserve alpha, and report decode failures. Use existing workspace packaging. | Movie File In |
| 10 | Text | Render a string with font, size, alignment, wrapping, foreground, and background controls. Needs a font/raster upload path. | Text |
| 11 | Remap | Sample an image at absolute UV coordinates from another texture, with explicit outside-frame behavior. Could be a Displace mode if the interface stays clear. | Remap |
| 12 | Pattern | Checker, grid, stripes, and radial/angular coordinates for masks, tests, and UV maps. Extend Ramp/Shape where appropriate. Wave is a membrane simulation, not this generator. | Ramp, Circle, Rectangle |

Color controls follow the role of [HSV Adjust](https://derivative.ca/UserGuide/HSV_Adjust_TOP).
[Function](https://derivative.ca/UserGuide/Function_TOP) separates nonlinear channel
functions from range mapping. [Convolve](https://derivative.ca/UserGuide/Convolve_TOP)
shows how several neighborhood filters can share one kernel interface.
[Reorder](https://derivative.ca/solr/reorder-top) and
[Channel Mix](https://derivative.ca/UserGuide/Channel_Mix_TOP) are distinct: channel
selection is a first step; arbitrary weighted channel mixing can follow it.

## Next: masks, color, and spatial effects

| Capability | Missing behavior and proposed scope |
|---|---|
| Key | Luma and chroma keys, soft selection, and spill suppression. Threshold currently emits grayscale and preserves the original alpha. |
| Morphology | Dilate and erode masks; opening/closing as compositions. Spatial min/max is different from Composite's comparison of two images. This is a goofi proposal, not a claim of a same-named TOP. |
| Channel Mix | A channel matrix for grayscale weights, tinting, and channel reconstruction. Add after Reorder proves insufficient. |
| Color space / Tone map | RGB↔HSV and linear↔sRGB conversions, plus HDR-to-display tone mapping. Agree on the texture color contract before implementation. |
| Bloom | Bright-region extraction and multi-scale blur/add. Start as a reusable patch; a fast node needs internal multi-pass rendering. |
| Normal / Slope | Height-to-gradient and height-to-normal output for displacement and lighting. Share derivative kernels with Edge. |
| Corner Pin | Four-corner projective warp for fitting an image to a surface. |
| Lens / Polar | Lens distortion and cartesian↔polar transforms. Prefer Remap presets when they can express the same behavior. |
| Layout | Arrange several images in a row, column, or grid inside a texture. This is separate from browser panel layout. |
| Resample | Nearest, linear, and a proper downsample filter; explicit border modes. Common controls should own shared sampling rules. A node is useful only for a distinct resize stage. |
| Blur extensions | Edge-preserving bilateral blur and mask-driven radius. Keep them in Blur. Faster wide Gaussian blur needs multiple passes; fixed sparse sampling is not an exact large kernel. |
| Composite extensions | Hue, saturation, color, and luminosity blend modes. Keep these in Composite. |

The TOP catalog names Key, Normal Map, Slope, Bloom, Corner Pin, Lens Distort,
Layout, and Tone Map counterparts. The grouping and priorities here are goofi
implementation choices. [Remap](https://derivative.ca/UserGuide/Remap_TOP) and
[Resolution](https://derivative.ca/UserGuide/Resolution_TOP) inform the coordinate
and resampling work.

## Engine-backed work

| Capability | Why it is more than another fragment shader |
|---|---|
| Cache / Delay / Hold | A bounded GPU frame history with capture, freeze, reset, and indexed delay. One owner for allocation and frame advancement; no viewer-driven clock. |
| Analyze / Histogram | Min/max/mean, image statistics, and distributions need GPU reduction and a defined result format. A compact signal result is useful for biosignal/visual feedback. |
| Sample / Texture-to-signal | Read selected pixels, rows, or regions into arrays through the existing transport. Specify readback rate and cost; do not use viewer snapshots as engine input. |
| Media playback | Still images first; later seek, pause, speed, timestamps, and image sequences through the existing source owner. Reuse video recording for output. |
| Optical Flow | A motion-vector field from successive frames; requires history and usually a pyramid or multiple passes. Useful after Cache and reduction/multi-pass work. |
| External texture I/O | Screen capture and Spout/Syphon/NDI have platform, resource, and dependency requirements. Keep these optional rather than blocking the basic bundle. |

[Cache](https://derivative.ca/UserGuide/Cache_TOP) is the reference for freeze and
indexed history. [Analyze](https://derivative.ca/UserGuide/Analyze_TOP) is the
reference for reducing images to small results. Their goofi implementations must
follow the existing engine and transport ownership rules.

## Shared requirements and build order

- Audit alpha conventions across generators, filters, viewers, and recording.
  Shape currently scales RGB by coverage, while Constant supplies independent
  RGB and alpha. New nodes must not add a third convention.
- Decide texture color space, channel selection, border modes, units, and invalid
  numeric results once. Preserve HDR values where the operation permits them.
- Add engine support for multiple internal passes only for a concrete consumer.
  Do not hide large, unbounded loops in a per-pixel shader.
- First implementation batch: Reorder, Color, Transform/Fit, Switch/Mix, Mask.
  Second batch: Function/Operation, Edge/Convolve, Image, Text, Remap/Pattern.
  Build advanced effects and temporal analysis after those foundations.
- Use real GPU sessions with opaque and transparent images, unequal dimensions,
  boundary values, and mode changes. Check sampling cost at useful frame sizes.
- Do not add separate Add, Multiply, Over, Under, Invert, Limit, Circle, Rectangle,
  Movie File Out, or texture In/Out nodes merely to match TOP names. Existing
  nodes or shared capabilities already cover those roles, subject to the limits
  listed above. General 3D rendering and vendor camera integrations are not core
  2D standard-library prerequisites.
