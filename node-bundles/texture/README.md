# Texture

latent-lab's texture instruments as goofi nodes, each one move of the lab, so a patch can put a
signal on any knob between them. Masks say where a material goes, TextureTransfer or the graphics
nodes lay the material's own pixels there, and MaterialSpace picks the material from an archive by
how the IP-Adapter's encoder places it. The mapping modes that work pixel by pixel run on the GPU
through `graphics:Composite` and `graphics:Shade`. The three that need the whole frame at once run
here on the CPU.

| node | reads | gives | from the lab | runs |
|---|---|---|---|---|
| `AdaptiveMask` | a picture, and optionally a few words, a texture, a field, a region, a base mask | a mask, a preview, a status | `masks.py`, `segment.py` | inside goofi, its prompts in one shared worker |
| `PromptMask` | a picture and a few words | a mask, a field, a preview, a status | `segment.py` | own process, torch |
| `TextureTransfer` | a picture, a texture, and optionally a mask | the picture rebuilt from the texture's pixels (blend, guided, flow) | `composite.py` | inside goofi |
| `MaterialSpace` | a space file, and optionally a place | photographs, one photograph, their weights, a place vector | `ip_space.py`, `flights.py` | inside goofi |
| `MaterialEncoder` | a picture, or a folder to build a space from | a place vector, a status | `ip_space.py` | own process, torch |
| `SpaceDraw` | a place vector | a texture drawn from that place by SD 1.5 and the IP-Adapter | `ip_space.py` | own process, torch |

## One contract

Every node reads what comes down a wire the same way, so any of them plugs into any other and into
goofi's own nodes.

- A picture is [h, w, 3], or RGBA, or grey [h, w], in 0..1 or 0..255. A stack [n, h, w, 3] given
  where one picture is read gives its first.
- A mask is [h, w] in 0..1 or 0..255, or [1, h, w], or a picture whose first channel is read.
- A field is a map [h, w] of any range, where only the order of its values counts.
- A place is a vector of the space's width (1024 for CLIP ViT-H), where only its direction counts.
- Sizes never have to match. Each node brings what it reads to its own frame.

So `PromptMask.mask` feeds `AdaptiveMask.region` or `TextureTransfer.mask`, `MaterialSpace.picture`
feeds `TextureTransfer.texture` (or `graphics:SignalIn` for the GPU modes), `MaterialEncoder.place`
feeds `MaterialSpace.place`, and a depth map, a DINO similarity map, a shader through `GraphicsIn`
or an EEG map feeds `AdaptiveMask.field`.

## Light and heavy nodes

goofi runs a Python node inside its own process when the node's imports leave the free-threaded
interpreter free of the GIL, and in a Python process of its own otherwise. AdaptiveMask,
TextureTransfer and MaterialSpace import only numpy, scipy and PIL (`requirements.txt`), so they run
inside goofi at no memory cost of their own, and any number of them fit in a patch. The three model
nodes import torch (`requirements-gil.txt`) and each takes a process. With the CUDA build of torch,
measured on this laptop, a process commits about 1.6 GB on import, and a PromptMask about 5.5 to
6.4 GB once its models have cut a picture. Windows has to hold that within its commit limit (RAM
plus page file), and goofi.exe dies without a message when the limit is reached.

Pretrained models load on first use, from the Hugging Face cache when they are there.

| model | used by | size |
|---|---|---|
| `facebook/sam2.1-hiera-small` and `IDEA-Research/grounding-dino-tiny` | PromptMask by sam 2 | 0.9 GB |
| `CIDAS/clipseg-rd64-refined` | PromptMask by clipseg | 0.6 GB |
| `h94/IP-Adapter`, `models/image_encoder` (CLIP ViT-H) | MaterialEncoder | 2.5 GB |
| `stable-diffusion-v1-5/stable-diffusion-v1-5`, `h94/IP-Adapter` `models/ip-adapter_sd15.safetensors`, `latent-consistency/lcm-lora-sdv1-5`, `madebyollin/taesd` | SpaceDraw | 5 GB |

### CUDA

goofi's runtime Python comes with a CPU build of torch. On this laptop it carries the CUDA build of
the same version, 2.15.0+cu132, installed with the command below, and torch sees the RTX 4090
Laptop GPU. The cu132 build needs a driver for CUDA 13.2 or later. Close goofi first, since its
Python processes hold torch's files open. Run it again after anything reinstalls goofi's runtime,
since that brings back the CPU build.

```powershell
uv pip install --python "$env:LOCALAPPDATA\goofi\3.0.0\python\gil\Scripts\python.exe" "torch==2.15.0" --index-url https://download.pytorch.org/whl/cu132 --reinstall-package torch
```

The model nodes pick CUDA when torch reports it and fall back to the CPU otherwise. On the CPU, SAM 2
takes about 20 s a new picture.

## AdaptiveMask

One mask layer. `mask/by` picks what the layer is cut by.

| by | its menu | what is taken |
|---|---|---|
| segmentation | prompt, region | a model's cut-out of the words in `prompt`, or of a cut wired into `region` |
| picture | light, shadow, edges, flat, saturation, colour | statistics of the picture's own pixels |
| field | | any wired map, its highest values first |
| texture | light, dark | the wired `texture`'s luminance, laid over the frame |
| shape | gradient, spot, growth | geometry, toward `angle`, around `spot_at`, or fractal noise |

All but segmentation keep the share of the frame `coverage` asks for, as a quantile of this frame,
so 0.3 is the lightest, highest or nearest 30% whatever the exposure. `follow` snaps the edge to the
picture's contours, `noise` frays it, `grow` moves it. One node's `mask` into the next node's `base`
with `combine` (and, or, but not) chains layers, and `fade` grades the result across its own extent.
`transition` glides any change of setting over that many seconds.

By `prompt`, Grounding DINO and SAM 2.1 (`sam 2`) or CLIPSeg (`clipseg`) cut out what the words
name. The prompt goes to one worker process shared by every AdaptiveMask in goofi,
`prompt_mask.py --serve` in goofi's GIL Python. The first prompt starts it and the model loads by
itself while the status reads "loading" (about 20 s for CLIPSeg, 20 to 60 s for SAM 2). The worker
commits about 1.8 GB with CLIPSeg on the CPU and about 6 GB with SAM 2 on CUDA, once for all masks.
It leaves when goofi closes or after ten minutes without a prompt, and logs to
`.goofi/plugin-data/texture/prompt-worker.log`.

## PromptMask

The same prompt models as a node of their own, with the model's soft map as `field` beside the
mask. It takes a process of its own, so a prompt inside AdaptiveMask is the lighter way to cut
several things.

## TextureTransfer

- blend. A mixed-gradient Poisson solve, the stronger edge winning at each pixel. `wmix` leans
  toward the picture.
- guided. The texture's tones poured into the picture's forms by a guided filter, `radius`, `eps`
  and `detail` setting the fit's size, how strong an edge holds, and how much grain returns.
- flow. The texture combed along the picture's contours, `length`, `smear`, `steps` and `reach`.

A wired `mask` keeps the picture outside it, `amount` sets how much replaces it inside, and `zoom`
and `centre` move the material under the picture, in mirror past 0..1. At 512 px blend takes 76 ms,
guided 56 ms and flow 128 ms on the CPU. Against latent-lab's `composite.py` at 384 px the three
modes match to a mean gap below 0.0001.

## From TextureMap

The other seven modes of latent-lab's mapping run on the GPU. A picture or a mask enters through
`graphics:SignalIn` (texture mode, colormap gray for a mask), and goes back to arrays through
`signal:GraphicsIn`. `graphics:Transform` with tiling mirror takes `zoom` and `centre`, and
Composite's `mask` input with `blend` takes the mask and `amount`.

| mode | graphics nodes |
|---|---|
| chroma | Composite `color`, A the texture, B the picture |
| grain | Composite `luminosity`, A the texture, B the picture |
| relief | Shade `relief`, height the picture, input the texture |
| displace | Shade `slopes` from the picture into Displace's `map`, the texture as Displace's input, then Composite `multiply` with the picture for its shading |
| waterline | Threshold on the picture (below white, above clear, `soft` the feather) into Composite's `mask`, Composite `over`, A the texture, B the picture |
| bands | Blur the texture and Composite `subtract` it from the texture (its fine scales), Math `multiply` for grain, Composite `add` onto the picture through Blur |
| veins | Blur the texture, Composite `subtract` the texture from it (its cracks), Math to scale and clamp, Level `invert`, Composite `multiply` onto the picture |

chroma and grain use the W3C colour and luminosity modes, so their light is weighted 0.3 / 0.59 /
0.11 where the lab uses YCbCr, and Composite's `blend` takes the place of `share`. Shade reads its
slope across `reach` and scales it by `gain`, where the lab normalises the slope by its frame's RMS,
so a soft picture needs a higher gain. bands and veins take their scale from Blur's radius, and
veins its depth from Math, where the lab normalises the cracks by their 99.5th percentile.

A TextureMap on blend, guided or flow becomes a TextureTransfer on the same mode with the same
knobs. One on chroma, grain, relief, displace, waterline, bands or veins becomes the chain in the
table above. The three texture examples in `examples/` are converted.

## MaterialSpace

A space is the cloud of CLIP ViT-H vectors of crops of every photograph in a set, the encoder the
IP-Adapter reads. `space/space` loads a space from the lab (`ip_space.py --out`). `space/folder`
reads the space a MaterialEncoder built from that folder, kept in
`.goofi/plugin-data/texture/spaces/`. Both are the same file format.

With both left empty, the node loads the four-star archive by itself. The space
(`space/texture-ip-sd15.npz` in `Kairos-Hive/latent-textures`, 2,133 photographs) comes from the
Hugging Face cache, or is downloaded once with the token in `HF_TOKEN` or the Hugging Face login.
Each photograph handed on is fetched from `Kairos-Hive/latent-textures-sets`
(`training-set-1600/`) the first time it is needed and kept in `.goofi/plugin-data/texture/hub/`.
Both repositories are private, so the account needs access. The first frame took 12 s with the
space cached, and four new photographs 3.6 s.

- tour. The most typical photograph of each of `stops` regions of the space, in the order of the
  shortest loop through them. This is how the lab picks the stops of its films. `at` picks the stop
  handed on alone.
- near. The `count` photographs nearest a place, and how near each is as `weights`. The place is a
  wired `place` vector, else the space's centre moved along its four main axes by `axes`, in units
  of the space's spread on each.
- map. The archive as an n-dimensional map with `dims` axes (up to 8). `basis` principal takes the
  space's main axes, strongest first. `basis` concepts takes the named directions of a concept book
  (latent-lab's `concepts.json`, with humid, fractured, turbulent, organic, warm and colloidal), so
  each coordinate reads as how much of that concept. On every axis 0 and 1 are the archive's two
  ends (its 1st and 99th percentiles) and 0.5 its middle. `axis1` to `axis8` are where you stand,
  each a param of its own, so a signal per axis steers through the archive along axes that carry
  names. The `count` photographs nearest the position are handed on. `reach` sets how far the weight
  spreads around it. `place` weighs every photograph on disk by its distance, so it moves
  continuously with the position and sits at the position on every map axis.

The concept book is `walk/concepts` when set, else a `concepts.json` beside the space file, else
the one in a latent-lab clone beside goofi's (`../latent-lab`) or in the folder `LATENT_LAB` names.

The walk moves smoothly and can move by itself.

- `pace` moves it with no signal wired. A tour goes round at `pace` stops a minute. On a map the
  position swings `wander` wide around the axes, each axis on its own pair of slow sines, `pace`
  swings a minute.
- `picture` is a crossfade (`blend`). A tour blends each stop into the next as `at` passes between
  them, and `place` slerps between their places. A map or near blends the photographs handed on by
  their weights, and a photograph's weight reaches 0 just as it leaves the set, so the blend never
  jumps. `blend` set to nearest hands on one photograph.
- `transition` glides the map's axes, and a change of mode, basis, dims, stops, seed, count or blend
  crossfades `picture` and slerps `place` from where they stood.

The `map` output draws the map in every mode, one panel per pair of axes. Each photograph is a dot
in its category's colour, bright when it is on disk. The photographs handed on are ringed and the
place is a white cross. In tour and near the panels show the principal axes.

Only photographs found on disk are handed on, and the status counts them. A tour still divides the
whole archive into regions, as the lab does, and a stop whose photograph is missing is taken by the
nearest photograph on disk. The status says how many stops were replaced. In the concept book as
fitted, humid and fractured lie close to one line, so a map on both has one dimension fewer than it
appears to.

`pictures` is a stack of the photographs cropped to one frame, for a node that walks or mixes
several. `picture` is their crossfade, or the nearest alone, for any node that takes a picture,
such as TextureTransfer's texture. `place` is the vector at the length a real crop's vector has,
for an IP-Adapter, the lab's space GAN or another MaterialSpace. When the space's photographs have
moved, `space/photos` finds them by name, including copies numbered in front.

## SpaceDraw

A texture drawn from a place in the space, the hybrid the photographs around it share. A place is
the CLIP ViT-H image vector the IP-Adapter reads, so MaterialSpace's `place` wires straight into
`SpaceDraw.place`. Stable Diffusion 1.5 draws it through the SD 1.5 IP-Adapter, the LCM-LoRA brings
the drawing to `steps` steps (4 by default), and TAESD decodes it. A place between photographs, on a
map or between two tour stops, draws a material none of them is.

Each frame starts from the same noise, so a place that moves smoothly draws a texture that changes
smoothly. `drift` carries the noise on by itself along a slow great circle through the seeds, in
seeds a minute, so the layout flows while the material follows the place. `smooth` carries part of
the last frame into the next. `scale` sets how strongly the place steers, and `prompt` adds words
beside it. A MaterialSpace on a map with `pace` and `wander` set, its `place` into a SpaceDraw,
draws a texture that wanders through the archive's materials by itself.

With CUDA a 512 px drawing at 4 steps takes about 0.9 s for a new place, after 78 s loading the
models the first time. On the CPU a 256 px frame at 2 steps took 7 s. Three tour stops of the
four-star archive drew three distinct materials (rock, wet pebbles, foliage), and the place half way
between the first two drew a weathered stone that neither photograph shows.

## MaterialEncoder

The IP-Adapter's image encoder (CLIP ViT-H). A wired `picture` comes out as its `place`, the
centre square of the picture as one 1024-number vector, so a camera into a MaterialEncoder into
`MaterialSpace.place` hands on the archive's photographs nearest what the camera sees. `folder`
and the `build` pulse make a space from a folder of photographs (subfolders are categories),
`crops` crops of each at `detail`, in the background with its progress on the status.

## Patches

The picture and the material are `ImageFile` nodes, or `graphics:Camera` through
`signal:GraphicsIn` for a live picture.

1. A material where the picture is light, lit by a moving sun. Picture into `AdaptiveMask.image`
   and through `graphics:SignalIn` into `Shade.height` and `Composite.b`, material through SignalIn
   into `Shade.input`, Shade on relief into `Composite.a`, `AdaptiveMask.mask` through SignalIn
   (colormap gray) into `Composite.mask`, and an LFO on `shade/angle`.
2. What the words name. An AdaptiveMask on segmentation, prompt, with `leaf` in `prompt`, and its
   mask into `TextureTransfer.mask` or Composite's. A second AdaptiveMask on shadow with combine set
   to but not, its `base` from the first, takes the leaves' shadowed undersides out.
3. The archive as the material. `MaterialSpace.picture` into `TextureTransfer.texture`, and a slow
   LFO on `walk/at` (tour) or band powers on `walk/axes` (near) changes which photograph is laid in.
4. The material the camera is looking at. Camera through GraphicsIn into a MaterialEncoder, its
   `place` into `MaterialSpace.place` (near), and `MaterialSpace.picture` into
   `TextureTransfer.texture`.
5. Any map as a mask. A depth map, a DINO similarity map or a shader through `GraphicsIn` into
   `AdaptiveMask.field`, with `coverage` setting how much of it is taken.
6. The archive as one surface. MaterialSpace's `pictures` through the `ai` bundle's `DINO` in
   tokens mode, `signal:Morph` between the grids and `DinoDecode` drawing them back.
7. A material no photograph holds. MaterialSpace on a map with `pace` and `wander`, its `place`
   into SpaceDraw, the drawing into `TextureTransfer.texture`.

## Examples

`examples/markov-blanket/04-material-blanket.gfi`, in the `inference` bundle's examples, runs
AdaptiveMask, MaterialSpace and Composite together, steered by EEG.

## Measured

Against the lab's own code at its 384 px reference frame (`tests/`), on synthetic pictures.

| node | check | result |
|---|---|---|
| AdaptiveMask | 15 source and option cases against `masks.build` | mean gap at most 0.0002, 100% of pixels on the same side |
| AdaptiveMask | coverage asked against coverage taken, 11 sources, bright and dark | within 3 points |
| AdaptiveMask | 256 px against 768 px working frame | 98.5% to 99.7% agreement |
| TextureTransfer | blend, guided, flow against `composite.py` | mean gap below 0.0001 |
| PromptMask | SAM 2's cut of a drawn disc against the disc | 99.7% of pixels |
| PromptMask | its mask into AdaptiveMask's region | 99.9% of pixels kept |
| AdaptiveMask | two prompted masks in goofi's free-threaded Python, clipseg on the CPU | one worker, 1.8 GB, ready in 20 s, the disc on 98.8% of pixels |
| MaterialSpace | tour of 6 against the lab's `flights.photographs` | the same 6 photographs |
| MaterialSpace | a photograph of the set placed by MaterialEncoder | found first, weight 0.90 to 0.999 |
| SpaceDraw | three archive stops and a place between two | four distinct drawings, mean gap 0.16 between stops |

Times on this laptop (RTX 4090 Laptop GPU, Core Ultra 9 185H), 512 px working frame. The light
nodes were timed in goofi's free-threaded Python, where they run. A live frame is a new picture
each time, as from a camera.

| node | time a frame |
|---|---|
| AdaptiveMask | 48 to 65 ms |
| TextureTransfer, guided and blend | 56 ms and 76 ms |
| TextureTransfer, flow | 128 ms |
| PromptMask by sam 2, a new prompt on a held picture | 0.23 to 0.35 s |
| PromptMask by sam 2, live frame, `detect_at` 384, `redetect` 0.5 | 103 ms |
| PromptMask by clipseg, a new prompt | 76 to 99 ms |
| MaterialEncoder, a picture placed | 71 ms |
| MaterialEncoder build, 40 photographs at 2 crops each, with the model load | 26 s |
| SpaceDraw, 512 px, 4 steps, a new place | 0.9 s |

First uses load models once per node, about 20 s for SAM 2 and Grounding DINO, and 78 s for
SpaceDraw's pipeline.

## Tests

Run from this folder with goofi's runtime interpreter, which has the `goofi` module.

```powershell
$py = "$env:LOCALAPPDATA\goofi\3.0.0\python\gil\Scripts\python.exe"
& $py tests/test_adaptive_mask.py
& $py tests/test_texture_transfer.py
& $py tests/test_material_map.py
& $py tests/test_prompt_mask.py
& $py tests/test_material_space.py FOLDER_OF_PHOTOGRAPHS
& $py tests/look_mask.py PICTURE TEXTURE SHEET.png
& "$env:LOCALAPPDATA\goofi\3.0.0\python\ft\Scripts\python.exe" tests/test_prompt_worker.py clipseg cpu
```

`test_prompt_worker.py` runs in goofi's free-threaded Python, where AdaptiveMask runs. Without
`clipseg cpu` it loads SAM 2 on the GPU, which commits about 6 GB more.

`tests/harness.py` runs a node file as goofi does, its declaration read by `goofi.introspect` and
its params set as goofi sets them. The parity tests import the lab from a clone of
`antoinebellemare/latent-lab` beside goofi's (`../latent-lab`), or from the folder the `LATENT_LAB`
environment variable names. goofi's node scan reads only this folder's top level, so `tests/` adds
no nodes.

## Limitations

- Grounding DINO scores things a picture lacks as high as things it holds. On archive photographs
  "a car" scored 0.39 to 0.67 and real leaves 0.33 to 0.44, so a prompt for something absent cuts
  out the nearest object. The status shows each score. SAM 3, which checks whether a concept is
  present at all, is gated on Hugging Face.
- A live picture through SAM 2 runs at about 10 frames a second on this GPU, with Grounding DINO's
  own pass (160 to 210 ms) the slowest part. It runs as many small kernels, so half precision gave
  it no speed.
- The prompt worker answers one request at a time. While it loads a second model (clipseg after sam
  2), the masks already cutting wait for that load, 20 to 60 s on an idle machine and several
  minutes on one whose CPU is saturated.
- AdaptiveMask and TextureTransfer run on the CPU, 48 to 128 ms a frame at 512 px.
- The lab's other AI mask sources (person, subject, body part, objects clicked, intrinsic light and
  colour) are absent. A wired `region` takes goofi's own `Segment` output in their place.
- quilt, analogy, synth, carve and relit take seconds to minutes a frame in the lab and are absent
  from TextureTransfer.
- The GPU chains for chroma, grain, relief, displace, bands and veins approximate the lab's modes.
  The From TextureMap section names each difference.
- A lab space names its photographs by the paths they had when it was built. `space/photos` looks
  them up by name in another folder.
