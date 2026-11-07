"""AdaptiveMask against what it promises: coverage as a quantile, the lab's own masks, the same mask
at any working size, the three ways to combine, and no work when nothing changed.

    <goofi runtime>/python/gil/Scripts/python.exe tests/test_adaptive_mask.py
"""

import sys
import time

import numpy as np

import harness as h

MOD, INFO = h.load("adaptive_mask")
BEFORE = set(sys.modules)  # what loading the node imported, before the lab's own code is
PIC = h.picture()
TEX = h.texture()


def mask_of(img=PIC, tex=TEX, **over):
    n = h.node(MOD, INFO, **over)
    return h.run(n, image=img, texture=tex)["mask"]


def test_coverage_is_a_quantile():
    """With a hard edge and no snapping, `coverage` is the share of the frame taken, for every
    ranked source, on a dark copy of the picture as on the picture itself."""
    for source in ["light", "shadow", "edges", "flat", "saturation", "colour", "gradient", "spot", "growth",
                   "texture light", "texture dark"]:
        for img in (PIC, PIC * 0.3):
            for want in (0.2, 0.6):
                got = float((mask_of(img, mask__source=source, mask__coverage=want, edges__soft=0.0,
                                     edges__follow=0.0, mask__point=[0.33, 0.5]) > 0.5).mean())
                assert abs(got - want) < 0.03, f"{source}: asked {want:.0%}, took {got:.1%}"


def test_matches_the_lab():
    """One layer at the lab's reference frame (384 px) cuts the lab's own mask (`masks.build`)."""
    sys.path.insert(0, str(h.LAB))
    import masks

    img = h.picture(256, 384)
    tex = h.texture(256, 384)
    cases = [dict(mask="light"), dict(mask="shadow", coverage=0.3), dict(mask="edges", mask_scale=2.0),
             dict(mask="flat"), dict(mask="saturation", invert=True),
             dict(mask="colour (click)", points=[(0.33, 0.5, 1)]), dict(mask="gradient", angle=1.2),
             dict(mask="spot (click)", points=[(0.7, 0.4, 1)]), dict(mask="growth", mask_seed=3),
             dict(mask="light", mask_noise=0.4), dict(mask="shadow", grow=6.0), dict(mask="light", grow=-6.0),
             dict(mask="texture dark", follow=0.0), dict(mask="light", fade="across", fade_angle=2.0),
             dict(mask="light", fade="from the edges", fade_reach=0.3, fade_length=0.4)]
    names = {"colour (click)": "colour", "spot (click)": "spot"}
    for case in cases:
        p = dict(coverage=0.5, mask_scale=4.0, mask_noise=0.0, mask_seed=0, mask_soft=0.2, follow=0.6, amount=1.0,
                 invert=False, grow=0.0, fade="off")
        p.update(case)
        want = masks.build(p, img, tex, lambda: None)
        pt = list(p.get("points", [(0.5, 0.5, 1)])[-1][:2])
        got = mask_of(img, tex, mask__source=names.get(p["mask"], p["mask"]), mask__coverage=p["coverage"],
                      mask__point=pt, mask__angle=p.get("angle", 0.0), mask__invert=p["invert"], mask__grow=p["grow"],
                      edges__scale=p["mask_scale"], edges__noise=p["mask_noise"], edges__seed=p["mask_seed"],
                      edges__soft=p["mask_soft"], edges__follow=p["follow"], fade__fade=p["fade"],
                      fade__side=p.get("fade_angle", 0.0), fade__reach=p.get("fade_reach", 0.5),
                      fade__length=p.get("fade_length", 1.0), frame__work=0)
        gap = float(np.abs(got - want).mean())
        agree = float(((got > 0.5) == (want > 0.5)).mean())
        print(f"  {str(case):<70} mean gap {gap:.4f}, agree {agree:.1%}")
        assert gap < 0.02 and agree > 0.97, case


def test_same_mask_at_any_working_size():
    """Pixel knobs scale with the frame: a 256 and a 768 working frame cut the same mask. The
    picture here is fractal, detail at every scale and no pixel noise: noise one pixel wide is a
    different field at every size, and a flat sky is one long tie a quantile cuts anywhere."""
    clean = h.texture(768, 1152, seed=5)
    for source in ["light", "edges", "flat", "growth"]:
        a = mask_of(clean, mask__source=source, frame__work=256, edges__noise=0.3)
        b = mask_of(clean, mask__source=source, frame__work=768, edges__noise=0.3)
        agree = float(((a > 0.5) == (b > 0.5)).mean())
        assert agree > 0.93, f"{source}: 256 and 768 agree on {agree:.1%}"


def test_combine():
    """`and`, `or` and `but not` meet a wired base the way their names say."""
    base = (np.arange(PIC.shape[1])[None, :] < PIC.shape[1] / 2).astype(np.float32).repeat(PIC.shape[0], 0)
    layer = (mask_of(mask__source="light", edges__soft=0.0, edges__follow=0.0) > 0.5)
    for how, rule in (("and", lambda b, l: b & l), ("or", lambda b, l: b | l), ("but not", lambda b, l: b & ~l)):
        n = h.node(MOD, INFO, mask__source="light", edges__soft=0.0, edges__follow=0.0, mask__combine=how)
        got = h.run(n, image=PIC, texture=TEX, base=base)["mask"] > 0.5
        agree = float((got == rule(base > 0.5, layer)).mean())
        assert agree > 0.99, f"{how}: {agree:.1%}"


def test_region_and_field():
    """A wired region is taken as it comes with `keep_edge`, and a wired field of any range and any
    size is ranked like the picture's own fields, highest first, lowest with `invert`."""
    soft = np.clip(np.linspace(-0.5, 1.5, PIC.shape[1])[None].repeat(PIC.shape[0], 0), 0, 1).astype(np.float32)
    n = h.node(MOD, INFO, mask__source="region", mask__keep_edge=True)
    got = h.run(n, image=PIC, region=soft)["mask"]
    assert np.abs(got - soft).max() < 0.02
    rows = np.linspace(-40, 900, 96, dtype=np.float32)[:, None].repeat(144, 1)  # a quarter the size, any range
    for invert in (False, True):
        # invert takes what the source leaves out: coverage 0.25 inverted is the field's lowest 75%.
        n = h.node(MOD, INFO, mask__source="field", mask__coverage=0.25, mask__invert=invert, edges__soft=0.0,
                   edges__follow=0.0)
        got = h.run(n, image=PIC, field=rows)["mask"] > 0.5
        taken, left = (got[:10], got[-10:]) if invert else (got[-10:], got[:200])
        assert taken.all() and not left.any(), f"the field's {'lowest' if invert else 'highest'} rows are taken"
        assert abs(got.mean() - (0.75 if invert else 0.25)) < 0.03
    n = h.node(MOD, INFO, mask__source="light", mask__coverage=0.9, edges__soft=0.0, fade__fade="along the field")
    faded = h.run(n, image=PIC, field=rows)["mask"]
    assert faded[-20:].mean() > faded[:20].mean() + 0.5, "the fade runs along the field, full where it is highest"
    for source, wire in [("field", "field"), ("texture light", "texture"), ("region", "region")]:
        out = h.run(h.node(MOD, INFO, mask__source=source), image=PIC)
        assert out["mask"].max() == 0.0, f"{source} without its wire selects nothing"
        assert f"`{wire}`" in out["status"], f"{source} without its wire names it in the status"
    out = h.run(h.node(MOD, INFO, mask__source="region"), image=PIC, base=soft)
    assert np.abs(out["mask"] - soft).max() < 0.02, "an unwired layer passes its base through"
    out = h.run(h.node(MOD, INFO, mask__source="region", mask__keep_edge=True), image=PIC, region=soft[None])
    assert np.abs(out["mask"] - soft).max() < 0.02, "a region [1, h, w] reads as [h, w]"


def test_pictures_any_way_they_come():
    """A picture as 0..255 bytes, with alpha, or as the first of a stack cuts the same mask."""
    want = mask_of(PIC, mask__source="saturation")
    as_bytes = (PIC * 255).round().astype(np.uint8)
    with_alpha = np.concatenate([PIC, np.ones_like(PIC[..., :1])], -1)
    stack = np.stack([PIC, PIC[::-1]])
    for name, img in (("bytes", as_bytes), ("alpha", with_alpha), ("stack", stack)):
        got = mask_of(img, mask__source="saturation")
        agree = float(((got > 0.5) == (want > 0.5)).mean())
        assert agree > 0.995, f"{name}: {agree:.1%}"


def test_light():
    """The node imports neither torch nor any model: goofi can run it in its own process."""
    assert "torch" not in BEFORE, "AdaptiveMask imported torch"


def test_no_work_when_nothing_changed():
    """A held frame and unchanged params give no new frame; a moved knob does."""
    n = h.node(MOD, INFO)
    assert h.run(n, image=PIC) is not None
    assert h.run(n, image=PIC) is None
    h.retune(n, INFO, mask__coverage=0.3)
    assert h.run(n, image=PIC) is not None
    noisy = PIC.copy()
    noisy[100:110, 100:110] += 0.2
    assert h.run(n, image=noisy) is not None


def test_hold_settles():
    """With `hold`, a changed frame is reached in steps and then the node goes quiet."""
    n = h.node(MOD, INFO, frame__hold=0.5)
    h.run(n, image=PIC)
    h.retune(n, INFO, frame__hold=0.5, mask__coverage=0.2)
    steps = 0
    while h.run(n, image=PIC) is not None:
        steps += 1
        assert steps < 30
    assert steps > 3


def test_transition_glides_and_crossfades():
    """With `transition`, a moved number glides, the covered share passing through the values in
    between, and a switched source crossfades. Both land exactly where the setting says and then the
    node goes quiet."""
    import types as _types
    clock = [100.0]
    real = MOD.time
    MOD.time = _types.SimpleNamespace(monotonic=lambda: clock[0], perf_counter=real.perf_counter)
    try:
        n = h.node(MOD, INFO, frame__transition=1.0, frame__work=256, mask__coverage=0.2)
        h.run(n, image=PIC)
        h.retune(n, INFO, frame__transition=1.0, frame__work=256, mask__coverage=0.8)
        shares = [float((h.run(n, image=PIC)["mask"] > 0.5).mean())]  # the frame that sees the change
        for _ in range(4):  # a second of transition in four frames
            clock[0] += 0.25
            out = h.run(n, image=PIC)
            assert out is not None
            shares.append(float((out["mask"] > 0.5).mean()))
        print("  coverage on the way", [round(x, 2) for x in shares])
        assert all(b >= a - 0.01 for a, b in zip(shares, shares[1:])), "the share grows steadily"
        assert 0.3 < shares[2] < 0.7, "half way the boundary stands half way"
        assert abs(shares[-1] - 0.8) < 0.05
        assert h.run(n, image=PIC) is None, "quiet once arrived"

        still = h.node(MOD, INFO, frame__work=256, mask__coverage=0.8, mask__source="shadow")
        want = h.run(still, image=PIC)["mask"]
        before = h.run(h.node(MOD, INFO, frame__work=256, mask__coverage=0.8), image=PIC)["mask"]
        h.retune(n, INFO, frame__transition=1.0, frame__work=256, mask__coverage=0.8, mask__source="shadow")
        first = h.run(n, image=PIC)["mask"]
        assert np.abs(first - before).max() < 1e-4, "the crossfade sets off from the last mask"
        clock[0] += 0.5
        mid = h.run(n, image=PIC)["mask"]
        assert np.abs(mid - 0.5 * (before + want)).mean() < 0.02, "half way is an even mix"
        clock[0] += 0.5
        end = h.run(n, image=PIC)["mask"]
        assert np.abs(end - want).max() < 1e-4, "the crossfade lands on the new mask"
        assert h.run(n, image=PIC) is None
    finally:
        MOD.time = real


def test_speed():
    """How long a 512 working frame takes, per source (printed, not asserted)."""
    for source in ["light", "edges", "flat", "colour", "growth"]:
        n = h.node(MOD, INFO, mask__source=source, edges__noise=0.2)
        h.run(n, image=PIC)
        t = time.perf_counter()
        for i in range(5):
            h.retune(n, INFO, mask__source=source, edges__noise=0.2, mask__coverage=0.3 + 0.05 * i)
            h.run(n, image=PIC)
        print(f"  {source:<10} {(time.perf_counter() - t) / 5 * 1000:6.1f} ms")


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
