"""AdaptiveMask's prompts through the shared worker, run in goofi's own free-threaded Python as goofi
runs the node: two prompting masks share one worker process, wait while it loads, cut what the words
name, the masks' own process stays light, and the worker leaves when its parent does.

    <goofi runtime>/python/ft/Scripts/python.exe tests/test_prompt_worker.py [sam 2 | clipseg] [cpu]

`clipseg cpu` is the light run, about 1.5 GB in the worker, for a machine already near its commit limit.
"""

import os
import subprocess
import sys
import time

if "cpu" in sys.argv:
    os.environ["CUDA_VISIBLE_DEVICES"] = "-1"  # the worker inherits this and keeps torch off the GPU (Windows drops "")

import numpy as np

import harness as h


def ps(command):
    return subprocess.run(["powershell", "-NoProfile", "-Command", command], capture_output=True, text=True).stdout


def committed(pid):
    return int(ps(f"(Get-Process -Id {pid}).PrivateMemorySize64").strip()) / 2**30


def workers():
    """Worker processes as (pid, parent). On Windows a venv's python.exe is a launcher that starts the
    real interpreter, so one worker shows as a launcher and its child."""
    out = ps("Get-CimInstance Win32_Process -Filter \"Name='python.exe'\" | Where-Object { $_.CommandLine -match "
             "'prompt_mask.py --serve' } | ForEach-Object { \"$($_.ProcessId) $($_.ParentProcessId)\" }")
    return [tuple(int(x) for x in line.split()) for line in out.splitlines() if line.strip()]


def trees(found):
    pids = {p for p, _ in found}
    return [p for p, parent in found if parent not in pids]


def main():
    assert not sys._is_gil_enabled(), "run this in goofi's free-threaded Python"
    model = "clipseg" if "clipseg" in sys.argv else "sam 2"
    AM, AI = h.load("adaptive_mask")
    disc = h.picture(seed=3)
    H, W = disc.shape[:2]
    drawn = ((np.arange(W)[None] - W / 3) ** 2 + (np.arange(H)[:, None] - H / 2) ** 2) < (0.22 * H) ** 2
    before = {p for p, _ in workers()}
    a = h.node(AM, AI, mask__by="segmentation", mask__segmentation="prompt", mask__prompt="a yellow circle",
               mask__model=model, edges__follow=0.0)
    b = h.node(AM, AI, mask__by="segmentation", mask__segmentation="prompt", mask__prompt="the sky",
               mask__model=model, edges__follow=0.0)
    first = h.run(a, image=disc)
    print("first frame:", first["status"])
    assert "loading" in first["status"] and first["mask"].max() == 0.0
    t = time.perf_counter()
    done = {}
    while len(done) < 2:
        time.sleep(1.0)
        for name, n in (("disc", a), ("sky", b)):
            out = h.run(n, image=disc)
            if out is not None and "loading" not in out["status"]:
                done[name] = out
        assert time.perf_counter() - t < 600, "the worker loads within ten minutes"
    print(f"both cut after {time.perf_counter() - t:.0f} s")
    for name, out in done.items():
        print(f"  {name}: {out['status']}")
    got = done["disc"]["mask"] > 0.5
    print(f"  the disc agrees with the drawn one on {(got == drawn).mean():.1%}")
    assert (got == drawn).mean() > (0.98 if model == "sam 2" else 0.9)
    sky = done["sky"]["mask"] > 0.5
    print(f"  the sky takes the top {sky[5, W // 2]}, the disc centre {sky[H // 2, W // 3]}")
    if model == "sam 2":
        assert sky[5, W // 2] and not sky[H // 2, W // 3], "the sky is taken and the disc left"
    new = [(p, q) for p, q in workers() if p not in before]
    print(f"worker processes started: {len(new)} in {len(trees(new))} tree, committing "
          f"{sum(committed(p) for p, _ in new):.2f} GB; the masks' own process {committed(os.getpid()):.2f} GB")
    assert len(trees(new)) == 1, "two prompting masks share one worker"
    print("all passed")


if __name__ == "__main__":
    if "--leaves" in sys.argv:  # the second half, after the first test process has ended
        print("workers left:", workers())
        sys.exit(0)
    main()
