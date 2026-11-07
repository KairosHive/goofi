"""MaterialSpace's map mode on a small synthetic space, so it runs anywhere: the map's coordinates
span the archive, the nearest photographs follow the position, `place` sits at the position on the
map's axes and moves continuously, a concepts map reads a concept book, only photographs on disk
are handed on, and `transition` glides the position and then goes quiet.

    <goofi runtime>/python/gil/Scripts/python.exe tests/test_material_map.py
"""

import json
import pathlib
import sys
import tempfile
import types

import numpy as np
from PIL import Image

import harness as h

MOD, INFO = h.load("material_space")
DIM, PHOTOS, CROPS = 64, 40, 3


def make_space():
    """A space of 40 photographs, 3 crops each, in 2 categories, with its photographs on disk except
    the last 5, and a concept book of 2 directions."""
    root = pathlib.Path(tempfile.mkdtemp(prefix="map-space-"))
    rng = np.random.default_rng(1)
    centres = rng.standard_normal((PHOTOS, DIM)).astype(np.float32)
    emb = np.repeat(centres, CROPS, 0) + 0.05 * rng.standard_normal((PHOTOS * CROPS, DIM)).astype(np.float32)
    emb /= np.linalg.norm(emb, axis=1, keepdims=True)
    owner = np.repeat(np.arange(PHOTOS), CROPS)
    tag = (owner >= PHOTOS // 2).astype(np.int64)
    files = [f"cat{t}/{i:05d}_P.jpg" for i, t in zip(range(PHOTOS), tag[::CROPS])]
    c = emb - emb.mean(0)
    axes = np.linalg.svd(c, full_matrices=False)[2][:8].astype(np.float32)
    spread = (c @ axes.T).std(0).astype(np.float32)
    np.savez(root / "space.npz", emb=emb, owner=owner, tag=tag, files=np.array(files), names=np.array(["cat0", "cat1"]),
             centre=emb.mean(0), axes=axes, spread=spread, means=np.stack([emb[tag == k].mean(0) for k in (0, 1)]),
             norm=np.float32(1.0), preset=np.array("test"))
    photos = root / "photos"
    photos.mkdir()
    for i, f in enumerate(files[:-5]):
        Image.fromarray(np.full((24, 36, 3), i * 5, np.uint8)).save(photos / f"{i:02d}_{pathlib.Path(f).name}")
    book = {"one": {"direction": axes[0].tolist()}, "two": {"direction": axes[2].tolist()}, "bad": {"direction": [1.0]}}
    (root / "concepts.json").write_text(json.dumps(book))
    return root


ROOT = make_space()
BASE = dict(space__space=str(ROOT / "space.npz"), space__photos=str(ROOT / "photos"), frame__long=128,
            walk__mode="map", walk__dims=2)


def axes(values):
    """A position as the node's axis params."""
    return {f"walk__axis{i + 1}": float(v) for i, v in enumerate(values)}


def run(**over):
    n = h.node(MOD, INFO, **{**BASE, **over})
    return n, h.run(n)


def test_map_spans_the_archive():
    n, out = run()
    m = n.space.map_of("principal")
    c = m["coords"][:, :2]
    assert abs(np.percentile(c, 1, 0) - 0).max() < 1e-4 and abs(np.percentile(c, 99, 0) - 1).max() < 1e-4
    assert out["map"].ndim == 3 and out["map"].shape[2] == 3
    assert "35 of 40 on disk" in out["status"], out["status"]


def test_nearest_follow_the_position():
    n, _ = run()
    m = n.space.map_of("principal")
    for target in (3, 17, 30):
        pos = [float(x) for x in m["coords"][target, :2]] + [0.5] * 6
        h.retune(n, INFO, **{**BASE, **axes(pos)})
        out = h.run(n)
        assert out["pictures"].shape[0] == 4
        assert out["weights"][0] == out["weights"].max() and abs(float(out["weights"].sum()) - 1) < 1e-5
        assert f"{target:05d}_P" in out["status"].split(" | ")[-2].split(",")[0], out["status"]


def test_place_sits_at_the_position_and_moves_continuously():
    n, _ = run(walk__dims=3)
    sp = n.space
    m = sp.map_of("principal")
    for pos in ((0.2, 0.7, 0.4), (0.9, 0.1, 0.5)):
        _, _, v = sp.on_map(m, pos, 4, 0.08)
        got = (m["axes"][:3] @ v - m["lo"][:3]) / m["span"][:3]
        assert np.abs(got - pos).max() < 1e-4, got
    for step in (0.01, 0.002):
        vs = [sp.on_map(m, (x, 0.5, 0.5), 4, 0.15)[2] for x in np.arange(0.1, 0.9, step)]
        d = np.linalg.norm(np.diff(vs, axis=0), axis=1)
        print(f"  step {step}: largest move of place {d.max():.4f}")
        if step == 0.01:
            coarse = d.max()
    assert d.max() < coarse / 3, "a finer step moves the place less: no jumps"


def test_concepts_map_reads_the_book():
    n, out = run(walk__basis="concepts")
    assert "one 0.50 two 0.50" in out["status"], out["status"]
    m = n.space.map_of("concepts", n.book(n.source()))
    assert m["names"] == ["one", "two"], "a direction of the wrong length is left out"


def test_only_photographs_on_disk_are_handed_on():
    n, _ = run()
    m = n.space.map_of("principal")
    pos = [float(x) for x in m["coords"][PHOTOS - 1, :2]] + [0.5] * 6  # a photograph missing from disk
    h.retune(n, INFO, **{**BASE, **axes(pos)})
    out = h.run(n)
    assert f"{PHOTOS - 1:05d}_P" not in out["status"]


def test_transition_glides_then_goes_quiet():
    clock = [50.0]
    real = MOD.time
    MOD.time = types.SimpleNamespace(monotonic=lambda: clock[0], perf_counter=real.perf_counter)
    try:
        n, _ = run(walk__transition=1.0, **axes([0.1] * 8))
        h.retune(n, INFO, **{**BASE, "walk__transition": 1.0, **axes([0.9] * 8)})
        seen = []
        for _ in range(5):
            out = h.run(n)
            seen.append(float(out["status"].split(" at ")[1].split()[2]))
            clock[0] += 0.25
        print("  axis 1 on the way", seen)
        assert seen[0] == 0.1 and seen[-1] == 0.9 and abs(seen[2] - 0.5) < 0.01
        assert all(b > a for a, b in zip(seen, seen[1:]))
        assert h.run(n) is None
    finally:
        MOD.time = real


def clocked(test):
    """Run `test(clock)` with the node's clock in the test's hand."""
    clock = [50.0]
    real = MOD.time
    MOD.time = types.SimpleNamespace(monotonic=lambda: clock[0], perf_counter=real.perf_counter)
    try:
        test(clock)
    finally:
        MOD.time = real


def test_tour_moves_by_itself_and_blends_stops():
    def go(clock):
        n, first = run(walk__mode="tour", walk__stops=6, walk__pace=60.0)
        seen = [first["picture"]]
        for _ in range(8):
            clock[0] += 0.125
            out = h.run(n)
            assert out is not None, "a paced tour keeps moving"
            seen.append(out["picture"])
        steps = [float(np.abs(a - b).mean()) for a, b in zip(seen, seen[1:])]
        print("  picture change per eighth of a stop", [round(x, 3) for x in steps])
        assert max(steps) < 0.5 * sum(steps), "the picture moves in small steps, with no jump"
        assert "stop 2 to 3" in out["status"] or "stop 1 to 2" in out["status"], out["status"]
    clocked(go)


def test_map_crossfade_has_no_jump():
    """Walking across the map, the blended picture changes continuously, even as photographs enter
    and leave the set: a step ten times finer changes it about ten times less, everywhere."""
    n, _ = run(walk__count=3, walk__reach=0.2)
    largest = []
    for step in (0.01, 0.001):
        seen = []
        for x in np.arange(0.5003, 0.6, step):  # off the starting 0.5, so the first is new
            h.retune(n, INFO, **{**BASE, "walk__count": 3, "walk__reach": 0.2, **axes([x, 0.5])})
            seen.append(h.run(n)["picture"])
        largest.append(max(float(np.abs(a - b).mean()) for a, b in zip(seen, seen[1:])))
    print(f"  largest step at 0.01 {largest[0]:.4f}, at 0.001 {largest[1]:.4f}")
    assert largest[1] < largest[0] / 4


def test_wander_moves_the_map_by_itself():
    def go(clock):
        n, first = run(walk__wander=0.3, walk__pace=6.0)
        out = None
        for _ in range(5):
            clock[0] += 1.0
            out = h.run(n)
            assert out is not None
        assert out["status"].split(" | ")[0] != first["status"].split(" | ")[0]
    clocked(go)


def test_a_switched_choice_crossfades():
    def go(clock):
        n, before = run(walk__transition=1.0, walk__count=2)
        h.retune(n, INFO, **{**BASE, "walk__transition": 1.0, "walk__count": 2, "walk__mode": "tour"})
        first = h.run(n)
        assert np.abs(first["picture"] - before["picture"]).max() < 1e-5, "the crossfade sets off from what was shown"
        clock[0] += 0.5
        mid = h.run(n)
        clock[0] += 0.6
        end = h.run(n)
        assert "in transition" in mid["status"] and "in transition" not in end["status"]
        d1 = float(np.abs(mid["picture"] - before["picture"]).mean())
        d2 = float(np.abs(end["picture"] - before["picture"]).mean())
        assert 0.2 * d2 < d1 < 0.8 * d2, (d1, d2)
    clocked(go)


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
