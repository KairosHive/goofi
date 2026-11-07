"""PromptMask: SAM 2 cuts what the words name with the drawn edge, CLIPSeg takes what is there and
nothing that is absent, and the mask into AdaptiveMask's `region` is reshaped as any region is.

    <goofi runtime>/python/gil/Scripts/python.exe tests/test_prompt_mask.py
"""

import sys

import numpy as np

import harness as h

AM, AI = h.load("adaptive_mask")
assert "torch" not in sys.modules, "AdaptiveMask imports no torch"
MOD, INFO = h.load("prompt_mask")
DISC = h.picture(seed=3)
H, W = DISC.shape[:2]
DRAWN = ((np.arange(W)[None] - W / 3) ** 2 + (np.arange(H)[:, None] - H / 2) ** 2) < (0.22 * H) ** 2


def test_sam_cuts_the_drawn_edge():
    """SAM 2 cuts the disc it is asked for along the disc's own edge. Grounding DINO also hands the
    only object in a picture to a word naming something absent ("a car" scores the disc 0.43), so
    presence alone is checked."""
    out = h.run(h.node(MOD, INFO, prompt__prompt="a yellow circle", prompt__by="sam 2"), image=DISC)
    got = out["mask"] > 0.5
    print(f"  {out['status'].split(' | ')[0]}, agrees with the drawn disc on {(got == DRAWN).mean():.1%}")
    assert out["mask"].shape == (H, W) and out["field"].shape == (H, W)
    assert (got == DRAWN).mean() > 0.98 and not got[5, W // 2]


def test_clipseg_takes_what_is_there():
    n = h.node(MOD, INFO, prompt__prompt="a yellow circle", prompt__by="clipseg")
    got = h.run(n, image=DISC)["mask"] > 0.5
    assert got[H // 2, W // 3] and not got[5, W // 2], "the disc is taken and the sky left"
    h.retune(n, INFO, prompt__prompt="a car", prompt__by="clipseg")
    assert (h.run(n, image=DISC)["mask"] > 0.5).mean() < 0.02, "an absent thing selects nothing"


def test_into_adaptive_mask():
    """PromptMask's mask into AdaptiveMask's region keeps the cut, and AdaptiveMask then grows it."""
    cut = h.run(h.node(MOD, INFO, prompt__prompt="a yellow circle"), image=DISC)["mask"]
    kept = h.run(h.node(AM, AI, mask__source="region", edges__follow=0.0), image=DISC, region=cut)["mask"]
    grown = h.run(h.node(AM, AI, mask__source="region", edges__follow=0.0, mask__grow=12.0), image=DISC,
                  region=cut)["mask"]
    agree = ((kept > 0.5) == (cut > 0.5)).mean()
    print(f"  region keeps the cut on {agree:.1%}, grown 12 px covers {(grown > 0.5).mean():.1%} "
          f"against {(cut > 0.5).mean():.1%}")
    assert agree > 0.99 and (grown > 0.5).mean() > (cut > 0.5).mean() + 0.02


def test_prompt_inside_adaptive_mask():
    """AdaptiveMask with by segmentation and a prompt starts the shared prompt worker, which loads
    SAM 2 by itself while the mask waits with a status, then cuts what PromptMask cuts.
    tests/test_prompt_worker.py runs the same in goofi's free-threaded Python with two masks."""
    import os
    import subprocess
    import time

    def committed():
        out = subprocess.run(["powershell", "-NoProfile", "-Command", f"(Get-Process -Id {os.getpid()}).PrivateMemorySize64"],
                             capture_output=True, text=True).stdout
        return int(out.strip()) / 2**30

    n = h.node(AM, AI, mask__by="segmentation", mask__segmentation="prompt", mask__prompt="a yellow circle",
               edges__follow=0.0)
    first = h.run(n, image=DISC)
    print(f"  first frame: {first['status']}")
    assert "loading" in first["status"] and first["mask"].max() == 0.0
    t = time.perf_counter()
    while True:
        time.sleep(1.0)
        out = h.run(n, image=DISC)
        if out is not None and "loading" not in out["status"]:
            break
        assert time.perf_counter() - t < 300, "the model loads within five minutes"
    got = out["mask"] > 0.5
    print(f"  loaded after {time.perf_counter() - t:.0f} s: {out['status']}")
    print(f"  agrees with the drawn disc on {(got == DRAWN).mean():.1%}, process now commits {committed():.2f} GB")
    assert (got == DRAWN).mean() > 0.98


def test_no_words_selects_nothing():
    out = h.run(h.node(MOD, INFO, prompt__prompt=" , "), image=DISC)
    assert out["mask"].max() == 0.0 and "no words" in out["status"]


if __name__ == "__main__":
    failed = 0
    for name, test in [(k, v) for k, v in globals().items() if k.startswith("test_")]:
        try:
            print(name)
            test()
            print("  ok")
        except AssertionError as e:
            failed += 1
            print(f"  FAILED: {e}")
    print("all passed" if not failed else f"{failed} failed")
    sys.exit(1 if failed else 0)
