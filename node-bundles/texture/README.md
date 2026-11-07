# Texture

Masks that say where a material goes, and the transfer of a material into a picture, from
latent-lab's texture instruments (`masks.py`, `segment.py`, `composite.py`). The modes that work
pixel by pixel run on the GPU through `graphics:Composite` and `graphics:Shade`. The three that need
the whole frame at once run here on the CPU.

| node | reads | gives | runs |
|---|---|---|---|
| `AdaptiveMask` | a picture, and optionally a few words, a texture, a field, a region, a base mask | a mask, a preview, a status | inside goofi, its prompts in one shared worker |
| `PromptMask` | a picture and a few words | a mask, a field, a preview, a status | own process, torch |
| `TextureTransfer` | a picture, a texture, and optionally a mask | the picture rebuilt from the texture's pixels | inside goofi |

A picture is [h, w, 3], RGBA or grey, in 0..1 or 0..255, and a stack gives its first. A mask is
[h, w] in 0..1. Sizes never have to match.

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
