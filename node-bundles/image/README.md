# Image

General image loading and processing, independent of the FluxRT runtime.

## ImageFile

Set `file/path` to an absolute local PNG, JPEG, WebP or other Pillow-supported
image path. The node emits RGB float32 data in 0..1. `image/max_size` limits the
longest edge without enlarging the image. File changes reload automatically;
`file/reload` forces a reload. NumPy is supplied by goofi; Pillow is declared in
this bundle's requirements. No model downloads or CUDA runtime are needed.

## RealESRGAN

Restore RGB images with the general-x4v3 model. Select `upscale/scale` 2 or 4,
and `model/precision` fp16 or fp32. The model computes 4x internally, then reduces
the result for 2x.

With `model/weights` empty, the first start downloads the published checkpoint
into `$GOOFI_HOME/.goofi/models/realesr-general-x4v3.pth` and verifies its
SHA-256 checksum. Later starts read that file. Set `model/weights` to an
absolute path to use another copy of the same checkpoint.

The node requires an NVIDIA GPU and CUDA-enabled PyTorch in goofi's GIL
interpreter. The bundle declares PyTorch, and normal setup installs its CPU
build. On Windows, install the CUDA build from the repository root:

```powershell
$py = "$env:LOCALAPPDATA\goofi\3.0.0\python\gil\Scripts\python.exe"
uv pip install --python $py "torch==2.15.0+cu132" --index-url https://download.pytorch.org/whl/cu132
```

This node does not need torchvision, basicsr, or the FluxRT runtime.

For lower-cost GPU scaling, use `graphics:Upscale` in the graphics bundle.
RealESRGAN's upstream license is in `REAL-ESRGAN-LICENSE.txt`.

## Vectorize

Trace an image into vector shapes on every frame, in native Rust. The method is
the symbol vectorizer of delyrism. `image` takes an [H, W] or [H, W, C] frame in
0..1, or a texture read back by `signal:GraphicsIn`, so `graphics:Camera`
connects to it through `GraphicsIn`.

`vectorize/mode` sets the trace.

- `regions` reduces any picture to `colors` colours with k-means in Lab and
  outlines each region of one colour. Regions smaller than `speckle` working
  pixels join their neighbour. The largest regions are drawn first, so a
  region inside another sits on top of it and neighbours meet without gaps.
- `ink` reads a drawing on paper. It divides out the paper's colour, keeps ink
  by chroma and darkness with hysteresis, drops specks and shapes that touch
  the frame, and outlines each ink colour.
- `lines` reads the same ink as pen paths. Each colour is thinned to a
  skeleton, chains that continue through a junction join into one stroke, and
  each stroke keeps the pen width measured along it. A region thicker than
  `fill` pen widths is outlined as a fill, and a small blob is a dot.

Every edge is smoothed by `smooth` and fitted with cubic Béziers within
`detail` working pixels, split at corners. The node draws the curves back into
`out` as RGBA at `resolution` (0 is the input's size). `out` is opaque in
`regions` and has a clear ground in `ink` and `lines`. `svg` is the same
drawing as an SVG document. `paths` lists the drawn paths as rows of x, y,
path index and palette index, with x and y in 0..1 of the frame and y down.
`palette` holds the colours as [K, 3] RGB. Each frame starts k-means from the
previous palette, so colours hold steady on video.

`size` is the longest side of the working copy and sets the speed. At 512 a
frame takes about 10 to 25 ms on eight threads, and a busy photo takes
the longest. At 1024 a drawing keeps parallel strokes apart that merge at 512.
