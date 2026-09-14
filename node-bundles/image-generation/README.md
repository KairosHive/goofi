# Image generation

FluxRT edits an RGB image stream with a prepared FluxRT runtime. It defaults to
320 x 320, int8 weights, two steps, and RIFE interpolation. General image loading
and RealESRGAN are in the [image bundle](../image/README.md). The graphics bundle
provides the shader Upscale node with linear, FSR1, and NIS modes.

## Setup

Run normal goofi setup first. FluxRT additionally needs an NVIDIA GPU, CUDA
PyTorch, and the prepared source and model weights. Normal setup does not
download these models. From the repository root on Windows:

```powershell
uv pip install --python .gfivenv/Scripts/python.exe "torch==2.11.0" "torchvision==0.26.0" --index-url https://download.pytorch.org/whl/cu128
.gfivenv/Scripts/python.exe node-bundles/image-generation/tools/prepare.py --python .gfivenv/Scripts/python.exe --root .external/FluxRT
```

On Linux use `.gfivenv/bin/python`. The preparation script checks the pinned
FluxRT revision, applies `tools/runtime.patch`, installs its requirements, and
downloads the checkpoint components. The patch uses Safetensors pread loading
and disables unused runtime integrations. Use only opencv-contrib-python in
this environment; do not install competing OpenCV distributions.

Restart goofi or refresh the library after setup. Set `model/root` to the
absolute prepared checkout path, then select `runtime/state = start`.

## Inputs and outputs

- `source`: live RGB float32 data in 0..1, shape [height, width, 3].
- `reference`: optional captured RGB reference with the same data format.
- `image`: generated RGB float32 data in 0..1.
- `status`: loading state, generation rate, and delivered output rate.

For graphics input, connect a texture through GraphicsIn in pixels mode, then
Select RGB channels (`axis = 2`, `include = 0:3`) into `source`. ImageFile can
supply `reference`. Connect the image output through graphics SignalIn for GPU
upscaling. Supply a new source frame after a node restart.

## Controls

| Group | Controls |
|---|---|
| runtime | Start loads and generates; Pause keeps the model; Stop releases it. |
| model | Prompt A, prompt B, their embedding mix, seed, steps, and guidance. A mix of 0 selects A; 1 selects B. Guidance above 1 adds a second model pass per step. |
| source | Output dimensions, source attention influence, and freeze. Stop and start after changing dimensions. |
| reference | Enable, capture, influence, and main_image. Source and reference identities stay fixed; main_image selects which is transformed as the main image. |
| smoothing | time smooths numeric control changes. output_fps sets paced image playback; 0 keeps RIFE-only output. |
| transition | Prompt/seed/reference blend duration, cut, and RIFE frames: 1 disables RIFE; 4 or 8 enables it. |
| consistency | Feedback from the previous output, optional motion alignment, and reset. Feedback resets model caches and can reduce generation speed. |

A reference is image conditioning, not pure style transfer: it can contribute
objects and layout as well as color and texture. Source and reference influence
are attention weights, not denoising percentages. Prompt instructions should
state which image structures to preserve.

## Playback and memory

A positive `smoothing/output_fps`, such as 30, blends between the selected RIFE
frames without extra model passes. Status identifies this as `rife + blend`.
It can soften detail or produce ghosting. One active batch and one replaceable
pending batch bound the buffer. Playback adds about one generation batch of
latency; slow generation can still cause gaps. Keep `common/max_frequency`
above the target. Downstream nodes and viewers can deliver fewer frames.

Delivered FPS is measured over two seconds. Generated FPS includes inference
and RIFE work. The inspector update rate is node polling, not image FPS.

On Windows, `runtime/memory_budget` defaults to an estimated 18.5 GiB total
worker commit. Startup subtracts the worker's current private commit, with at
least 2 GiB free required. If headroom is insufficient, status shows Waiting
for memory and retries automatically. Stop cancels the wait. Set the budget
before Start; increasing it requires more free memory. This check neither
reserves memory nor guarantees protection from later system memory exhaustion.

## Tests

The public-session tests are `fluxrt`, `image_file`, and `upscaling` in
`backend/goofi-tests/tests`. The optional `upscaling_fluxrt` test additionally
needs CUDA and model weights, supplied through `GOOFI_FLUXRT_ROOT`,
`GOOFI_REALESRGAN_WEIGHTS`, and `GOOFI_UPSCALE_RESULTS`. No example patches are
required by these tests.
