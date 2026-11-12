"""A sheet of AdaptiveMask previews on a real photograph, one tile per recipe, to judge by eye.

    <goofi runtime>/python/gil/Scripts/python.exe tests/look_mask.py PICTURE TEXTURE OUT.png
"""

import sys
import time

import numpy as np
from PIL import Image, ImageDraw

import harness as h

MOD, INFO = h.load("adaptive_mask")

RECIPES = [
    ("light 0.4", dict(mask__source="light", mask__coverage=0.4)),
    ("shadow 0.3, ragged", dict(mask__source="shadow", mask__coverage=0.3, edges__noise=0.5)),
    ("edges 0.25", dict(mask__source="edges", mask__coverage=0.25)),
    ("flat 0.4", dict(mask__source="flat", mask__coverage=0.4)),
    ("saturation 0.3", dict(mask__source="saturation", mask__coverage=0.3)),
    ("colour at the seed disc", dict(mask__source="colour", mask__point=[0.72, 0.38], mask__coverage=0.25)),
    ("growth, seed 4", dict(mask__source="growth", mask__coverage=0.45, edges__seed=4, edges__scale=10)),
    ("texture dark 0.4", dict(mask__source="texture dark", mask__coverage=0.4)),
    ("light, fade from the top", dict(mask__source="light", mask__coverage=0.6, fade__fade="across",
                                      fade__side=0.0, fade__reach=0.5, fade__length=0.6)),
    ("light, fade from the edges", dict(mask__source="light", mask__coverage=0.6, fade__fade="from the edges",
                                        fade__reach=0.3, fade__length=0.5)),
]


def main(picture, texture, out):
    img = h.read(picture, 768)
    tex = h.read(texture, 768)
    tiles = []
    for label, over in RECIPES:
        n = h.node(MOD, INFO, **over)
        t = time.perf_counter()
        got = h.run(n, image=img, texture=tex)
        tiles.append((f"{label}  ({(time.perf_counter() - t) * 1000:.0f} ms)", got["preview"]))
        print(label, "|", got["status"])
    # PromptMask's cut into `region`, then a second layer: the flower SAM 2 cuts out, but not its shadows.
    pm, pi = h.load("prompt_mask")
    for words, by in (("leaf", "sam 2"), ("flower", "clipseg")):
        cut = h.run(h.node(pm, pi, prompt__prompt=words, prompt__by=by), image=img)["mask"]
        got = h.run(h.node(MOD, INFO, mask__source="region"), image=img, region=cut)
        tiles.append((f"PromptMask {words} ({by}) into region", got["preview"]))
    flower = h.run(h.node(pm, pi, prompt__prompt="flower"), image=img)["mask"]
    first = h.run(h.node(MOD, INFO, mask__source="region"), image=img, region=flower)["mask"]
    chain = h.run(h.node(MOD, INFO, mask__source="shadow", mask__coverage=0.35, mask__combine="but not"),
                  image=img, base=first)
    tiles.append(("flower but not shadow (2 nodes)", chain["preview"]))
    # Any map on `field`: here a vertical ramp standing in for a depth map, highest at the bottom.
    ramp = np.linspace(0.0, 1.0, img.shape[0], dtype=np.float32)[:, None].repeat(img.shape[1], 1)
    near = h.run(h.node(MOD, INFO, mask__source="field", mask__coverage=0.35), image=img, field=ramp)
    tiles.append(("field: a ramp, highest 35%", near["preview"]))
    cols = 4
    th, tw = tiles[0][1].shape[:2]
    k = 300 / tw
    th, tw = round(th * k), 300
    sheet = Image.new("RGB", (cols * tw, -(-len(tiles) // cols) * (th + 18)), "white")
    draw = ImageDraw.Draw(sheet)
    for i, (label, a) in enumerate(tiles):
        x, y = (i % cols) * tw, (i // cols) * (th + 18)
        sheet.paste(Image.fromarray((np.clip(a, 0, 1) * 255).astype(np.uint8)).resize((tw, th), Image.LANCZOS), (x, y))
        draw.text((x + 3, y + th + 3), label, fill="black")
    sheet.save(out)
    print("wrote", out)


if __name__ == "__main__":
    main(*sys.argv[1:4])
