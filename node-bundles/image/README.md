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
the result for 2x. Set `model/weights` before selecting `runtime/state = start`.

The node requires an NVIDIA GPU and CUDA-enabled PyTorch in the standard
`.gfivenv` worker. The bundle declares PyTorch, but a generic package install
does not guarantee CUDA support. The locally tested Windows installation is:

```powershell
uv pip install --python .gfivenv/Scripts/python.exe "torch==2.11.0" --index-url https://download.pytorch.org/whl/cu128
.gfivenv/Scripts/python.exe node-bundles/image/tools/prepare_realesrgan.py --output .external/realesrgan-results/realesr-general-x4v3.pth
```

On Linux use `.gfivenv/bin/python`. The preparation script downloads the model
and verifies its SHA-256 checksum. Model weights are not installed by normal
goofi setup. This node does not need torchvision, basicsr, or the FluxRT runtime.

For lower-cost GPU scaling, use `graphics:Upscale` in the graphics bundle.
RealESRGAN's upstream license is in `REAL-ESRGAN-LICENSE.txt`.
