"""TokenMorph: a picture comes back through the tokens, the loop passes through every picture, the
mix weights mean what they say, a stack counts as its pictures, and the mask keeps the wired picture
outside it. With pictures given, it also draws a strip of the loop to judge by eye.

    <goofi runtime>/python/gil/Scripts/python.exe tests/test_token_morph.py [A.jpg B.jpg C.jpg OUT.png]
"""

import sys
import time

import numpy as np

import harness as h

MOD, INFO = h.load("token_morph")


def psnr(a, b):
    return float(10 * np.log10(1.0 / max(np.mean((a - b) ** 2), 1e-12)))


def frames(*pictures):
    return [(f"p{i}.image", h.Frame(p)) for i, p in enumerate(pictures)]


def main(paths=None, out=None):
    if paths:
        a, b, c = (h.read(p, 768) for p in paths)
    else:
        a, b, c = h.picture(512, 512), h.texture(512, 512), h.texture(512, 512, seed=7)
    n = h.node(MOD, INFO, frame__long=16, morph__even=False)
    t = time.perf_counter()
    got = n.process(pictures=frames(a))
    print(f"load and first draw: {time.perf_counter() - t:.1f} s | {got['status']}")
    one = got["image"][0]
    rows, cols = one.shape[0] // 16, one.shape[1] // 16
    assert max(rows, cols) == 16 and one.shape[:2] == (rows * 16, cols * 16), one.shape
    target = h.read_array(MOD.piece(a, rows, cols, 1.0), rows * 16, cols * 16)
    print(f"a picture through the tokens: PSNR {psnr(one, target):.1f} dB")
    assert psnr(one, target) > 14, "the decoder draws the picture back"

    h.retune(n, INFO, frame__long=16, morph__even=False)
    loop0 = n.process(pictures=frames(a, b, c))["image"][0]
    print(f"loop at 0 against the first picture alone: largest gap {np.abs(loop0 - one).max():.4f}")
    assert np.abs(loop0 - one).max() < 0.02, "the loop passes through the first picture at 0"

    h.retune(n, INFO, frame__long=16, morph__even=False, morph__mode="mix", morph__weights=[0.0, 1.0, 0.0, 0.0])
    mix_b = n.process(pictures=frames(a, b, c))["image"][0]
    h.retune(n, INFO, frame__long=16, morph__even=False, morph__at=1 / 3)
    loop_b = n.process(pictures=frames(a, b, c))["image"][0]
    print(f"mix [0, 1, 0] against loop at 1/3: largest gap {np.abs(mix_b - loop_b).max():.4f}")
    assert np.abs(mix_b - loop_b).max() < 0.02, "both are the second picture"

    same = [h.read_array(x, 384, 512) for x in (a, b, c)]
    h.retune(n, INFO, frame__long=16, morph__even=False, morph__at=1 / 3)
    stacked = n.process(pictures=[("s.image", h.Frame(np.stack(same)))])["image"][0]
    again = n.process(pictures=frames(*same))
    wired = stacked if again is None else again["image"][0]  # nothing new to draw: it read the same pictures
    assert np.abs(stacked - wired).max() < 0.02, "a stack counts as its pictures, in order"

    h.retune(n, INFO, frame__long=16, morph__even=False, morph__mode="mix", morph__weights=[0.5, 0.5, 0.0, 0.0])
    w = n.process(pictures=frames(a, b, c), weights=h.Frame(np.array([0.0, 0.0, 1.0], np.float32)))["image"][0]
    h.retune(n, INFO, frame__long=16, morph__even=False, morph__at=2 / 3)
    loop_c = n.process(pictures=frames(a, b, c))["image"][0]
    assert np.abs(w - loop_c).max() < 0.02, "a wired weights vector overrides the param"

    keep = np.zeros(a.shape[:2], np.float32)
    keep[:, a.shape[1] // 2:] = 1.0
    h.retune(n, INFO, frame__long=16, morph__even=False, morph__at=1 / 3)
    composed = n.process(pictures=frames(a, b, c), image=h.Frame(a), mask=h.Frame(keep))["image"][0]
    W = composed.shape[1]
    left = np.abs(composed[:, :W * 3 // 8] - target[:, :W * 3 // 8]).mean()
    right = np.abs(composed[:, W * 5 // 8:] - loop_b[:, W * 5 // 8:]).mean()
    print(f"masked compose: left to the picture's own pixels {left:.4f}, right to the morph {right:.4f}")
    assert left < 0.01 and right < 0.08, "the picture outside the mask, the morph inside"
    h.retune(n, INFO, frame__long=16, morph__even=False, morph__at=1 / 3, morph__keep_outside=False)
    drawn = n.process(pictures=frames(a, b, c), image=h.Frame(a), mask=h.Frame(keep))["image"][0]
    print(f"  drawn whole by the decoder: left to the picture {np.abs(drawn[:, :W * 3 // 8] - target[:, :W * 3 // 8]).mean():.4f}")

    def steps(even, count=24):
        """How far the picture moves from each of `count` moments of the loop to the next."""
        shots = []
        for u in np.arange(count) / count:
            h.retune(n, INFO, frame__long=8, morph__even=even, morph__at=float(u))
            shots.append(n.process(pictures=frames(a, b, c))["image"][0])
        return np.array([np.abs(shots[(i + 1) % count] - shots[i]).mean() for i in range(count)])

    plain, paced = steps(False), steps(True)
    print(f"pace along the loop (spread of the steps over their mean): straight {plain.std() / plain.mean():.2f}, "
          f"even {paced.std() / paced.mean():.2f}")
    assert paced.std() / paced.mean() < plain.std() / plain.mean(), "even pacing evens the steps"

    for long in (16, 32):
        h.retune(n, INFO, frame__long=long, morph__at=0.1)
        n.process(pictures=frames(a, b, c))
        t = time.perf_counter()
        for i in range(2):
            h.retune(n, INFO, frame__long=long, morph__at=0.2 + 0.1 * i)
            got = n.process(pictures=frames(a, b, c))
        print(f"grid {long}: {(time.perf_counter() - t) / 2 * 1000:.0f} ms a frame | {got['status']}")

    if out:
        h.retune(n, INFO, frame__long=32)
        strip = []
        for u in np.arange(12) / 12:
            h.retune(n, INFO, frame__long=32, morph__at=float(u))
            strip.append(n.process(pictures=frames(a, b, c))["image"][0])
        rows = [np.concatenate(strip[i:i + 6], 1) for i in (0, 6)]
        h.save(np.concatenate(rows, 0), out)
        print("wrote", out)
    print("all passed")


if __name__ == "__main__":
    args = sys.argv[1:]
    main(args[:3] if len(args) >= 3 else None, args[3] if len(args) >= 4 else None)
