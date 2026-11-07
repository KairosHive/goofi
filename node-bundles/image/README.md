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
