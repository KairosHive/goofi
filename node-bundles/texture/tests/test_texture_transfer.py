"""TextureTransfer against the lab's own texture mapping (`composite.py`) for its three modes, and
against what its controls promise: the mask keeps the picture outside it, the material moves under
`centre`, and nothing is redone when nothing changed.

    <goofi runtime>/python/gil/Scripts/python.exe tests/test_texture_transfer.py
"""

import sys
import time

import numpy as np

import harness as h

MOD, INFO = h.load("texture_transfer")
PIC = h.picture(256, 384)
TEX = h.texture(256, 384)


def mapped(img=PIC, tex=TEX, mask=None, **over):
    n = h.node(MOD, INFO, frame__work=0, **over)
    return h.run(n, image=img, texture=tex, mask=mask)["image"]


def test_matches_the_lab():
    """Every mode at the lab's reference frame (384 px), the texture covering the frame as the lab
    lays it, draws what `composite.py` draws."""
    sys.path.insert(0, str(h.LAB))
    import composite as cx

    c, t = PIC, TEX
    cases = [
        ("blend", dict(transfer__wmix=0.65), lambda: cx.blend(c, t, 0.65)),
        ("flow", dict(transfer__length=0.06, transfer__reach=3.0, transfer__smear=0.25, transfer__steps=8),
         lambda: cx.flow(c, t, 0.06, 3.0, 0.25, 8)),
        ("guided", dict(transfer__radius=14.0, transfer__eps=0.015, transfer__detail=0.35),
         lambda: cx.guided(c, t, 14.0, 0.015, 0.35)),
    ]
    for mode, over, lab in cases:
        want = np.clip(lab(), 0, 1)
        got = mapped(transfer__mode=mode, **over)
        gap = float(np.abs(got - want).mean())
        print(f"  {mode:<10} mean gap {gap:.4f}, worst 1% {np.quantile(np.abs(got - want), 0.99):.3f}")
        assert gap < 0.01, mode


def test_mask_keeps_the_picture():
    """Outside a wired mask the picture comes back as it was; inside, the transfer."""
    m = np.zeros(PIC.shape[:2], np.float32)
    m[:, : PIC.shape[1] // 2] = 1.0
    full = mapped(transfer__mode="guided")
    got = mapped(mask=m, transfer__mode="guided")
    right = slice(PIC.shape[1] // 2 + 4, None)
    left = slice(0, PIC.shape[1] // 2 - 4)
    assert np.abs(got[:, right] - PIC[:, right]).max() < 0.01
    assert np.abs(got[:, left] - full[:, left]).max() < 0.01


def test_material_moves():
    """`centre` slides the material: a shift of one frame-width share moves the texture by that
    share; past the photograph's edge it carries on in mirror. guided with no grain held back and
    a wide fit hands the texture's own pixels through."""
    over = dict(transfer__mode="guided", transfer__detail=1.0, transfer__radius=2.0, transfer__eps=0.08)
    a = mapped(material__centre=[0.5, 0.5], **over)
    b = mapped(material__centre=[0.55, 0.5], **over)
    dx = round(0.05 * PIC.shape[1])
    shifted = np.abs(a[:, dx:] - b[:, :-dx]).mean()
    still = np.abs(a - b).mean()
    print(f"  slid by {dx} px the material lines up to {shifted:.4f}, against {still:.4f} unslid")
    assert shifted < 0.5 * still, "a slid material lines up with itself"
    far = mapped(material__centre=[2.3, -1.4], material__zoom=0.5, **over)
    assert np.isfinite(far).all() and far.std() > 0.02


def test_masks_and_textures_any_way_they_come():
    """A mask as 0..255 bytes, as [1, h, w] or as a picture, and a texture as the first of a stack,
    give what a [h, w] mask in 0..1 and a single texture give."""
    m = np.zeros(PIC.shape[:2], np.float32)
    m[:, : PIC.shape[1] // 2] = 1.0
    want = mapped(mask=m)
    for name, mask in (("bytes", (m * 255).astype(np.uint8)), ("channels first", m[None]),
                       ("picture", np.repeat(m[..., None], 3, -1))):
        assert np.abs(mapped(mask=mask) - want).max() < 1e-4, name
    stacked = mapped(tex=np.stack([TEX, TEX[::-1]]), mask=m)
    assert np.abs(stacked - want).max() < 1e-4, "a stack of textures gives its first"


def test_knobs_show_under_their_mode():
    params = {p["name"]: p for p in INFO["params"] if p["group"] == "transfer"}
    assert params["mode"]["options"] == MOD.MODES
    for name, mode in (("wmix", "blend"), ("radius", "guided"), ("eps", "guided"), ("length", "flow"),
                       ("reach", "flow")):
        assert params[name]["show"] == [{"param": "mode", "any_of": [mode]}], name


def test_no_work_when_nothing_changed():
    n = h.node(MOD, INFO)
    assert h.run(n, image=PIC, texture=TEX) is not None
    assert h.run(n, image=PIC, texture=TEX) is None
    h.retune(n, INFO, transfer__mode="blend")
    assert h.run(n, image=PIC, texture=TEX) is not None


def test_speed():
    """How long each mode takes on a 512 working frame (printed, not asserted)."""
    img, tex = h.picture(), h.texture()
    for mode in MOD.MODES:
        n = h.node(MOD, INFO, transfer__mode=mode)
        h.run(n, image=img, texture=tex)
        t = time.perf_counter()
        for i in range(3):
            h.retune(n, INFO, transfer__mode=mode, mix__amount=0.9 + 0.03 * i)
            h.run(n, image=img, texture=tex)
        print(f"  {mode:<10} {(time.perf_counter() - t) / 3 * 1000:6.1f} ms")


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
