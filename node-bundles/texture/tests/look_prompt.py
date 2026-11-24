"""Prompted masks on a real photograph, SAM 2 beside CLIPSeg, with what each takes and how long.

    <goofi runtime>/python/gil/Scripts/python.exe tests/look_prompt.py PICTURE OUT.png "flower" "leaf" ...
"""

import sys
import time

import numpy as np
from PIL import Image, ImageDraw

import harness as h

MOD, INFO = h.load("adaptive_mask")


def main(path, out, prompts):
    img = h.read(path, 768)
    tiles = []
    for by in ("sam 2", "clipseg"):
        n = h.node(MOD, INFO, mask__source="prompt", mask__by=by, mask__prompt=prompts[0])
        for words in prompts:
            h.retune(n, INFO, mask__source="prompt", mask__by=by, mask__prompt=words)
            t = time.perf_counter()
            got = h.run(n, image=img)
            ms = (time.perf_counter() - t) * 1000
            print(f"{by:<8} {words:<22} {got['status']}  ({ms:.0f} ms with the call)")
            tiles.append((f"{by}: {words}", got["preview"]))
    th, tw = tiles[0][1].shape[:2]
    k = 320 / tw
    th, tw = round(th * k), 320
    cols = len(prompts)
    sheet = Image.new("RGB", (cols * tw, 2 * (th + 18)), "white")
    draw = ImageDraw.Draw(sheet)
    for i, (label, a) in enumerate(tiles):
        x, y = (i % cols) * tw, (i // cols) * (th + 18)
        sheet.paste(Image.fromarray((np.clip(a, 0, 1) * 255).astype(np.uint8)).resize((tw, th), Image.LANCZOS), (x, y))
        draw.text((x + 3, y + th + 3), label, fill="black")
    sheet.save(out)
    print("wrote", out)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2], sys.argv[3:])
