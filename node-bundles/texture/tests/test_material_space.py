"""MaterialSpace with MaterialEncoder: a space built through the encoder's `build` pulse is a lab
space MaterialSpace finds by the same folder, the tour picks what the lab's films pick, `near` hands
on weights that mean nearness, and a photograph of the set placed by the encoder finds itself.

    <goofi runtime>/python/gil/Scripts/python.exe tests/test_material_space.py FOLDER
"""

import os
import sys
import tempfile
import time

import numpy as np

import harness as h


def main(folder):
    os.environ["GOOFI_HOME"] = tempfile.mkdtemp(prefix="goofi-home-")  # the built space lands here
    mod, info = h.load("material_space")
    assert "torch" not in sys.modules, "MaterialSpace imports no torch: goofi runs it in its own process"
    emod, einfo = h.load("material_encoder")
    n = h.node(mod, info, space__folder=folder, frame__long=256)
    print(h.run(n)["status"])
    enc = h.node(emod, einfo, build__folder=folder, build__crops=2)
    t = time.perf_counter()
    enc.pulse_build_build()
    while enc.job.is_alive():
        enc.job.join(5)
        print(" ", enc.note, flush=True)
    print(f"built in {time.perf_counter() - t:.0f} s: {enc.note}")
    assert enc.note.startswith("built"), enc.note

    path = mod.built_path(folder)
    assert path == emod.built_path(folder), "the encoder keeps the space where MaterialSpace looks"
    z = np.load(path)
    for key in ("emb", "owner", "tag", "files", "names", "centre", "axes", "spread", "means", "norm", "preset"):
        assert key in z.files, key
    print(f"space: {z['emb'].shape[0]} crops of {z['emb'].shape[1]}, {len(z['files'])} photographs, "
          f"{len(z['names'])} categories, typical length {float(z['norm']):.2f}")

    got = h.run(n)
    assert got["pictures"].shape == (6, 171, 256, 3), got["pictures"].shape
    print("tour:", got["status"])
    sys.path.insert(0, str(h.LAB))
    import flights

    space = {k: z[k] for k in z.files}
    places, files = flights.photographs(space, 6, np.random.default_rng(0))
    lab = {str(files[i]) for i in flights.loop_order(places / np.linalg.norm(places, axis=1, keepdims=True))}
    mine = set(n.result["pictures"][1]["space"]["files"])
    print(f"the lab's tour and the node's share {len(lab & mine)} of 6 photographs")
    assert lab == mine

    h.retune(n, info, space__folder=folder, walk__mode="near", walk__count=4, frame__long=256)
    near = h.run(n)
    w = near["weights"]
    print("near the centre:", near["status"])
    assert w.shape == (4,) and abs(float(w.sum()) - 1) < 1e-5 and (np.diff(w) <= 1e-7).all()
    moved = []
    for x in np.linspace(-2, 2, 9):
        h.retune(n, info, space__folder=folder, walk__mode="near", walk__axes=[float(x), 0.0, 0.0, 0.0])
        moved.append(next(f for f in h.run(n)["status"].split(" | ") if "_" in f or "/" in f))  # the names, not the on-disk count
    print("along the first axis:", " / ".join(m.split(", ")[0] for m in moved))
    assert len(set(moved)) > 2, "moving along the first axis changes the nearest photographs"

    h.retune(n, info, space__folder=folder, walk__mode="near", walk__count=4, frame__long=256, walk__blend="nearest")
    near = h.run(n)
    one = near["picture"]
    assert one.shape == near["pictures"].shape[1:] and np.array_equal(one, near["pictures"][0]), \
        "with blend nearest, `picture` is the nearest photograph, the first of the stack"
    print("picture, the nearest:", one.shape)

    h.retune(n, info, space__folder=folder, frame__long=256, walk__at=0.5)
    tour = h.run(n)
    assert np.array_equal(tour["picture"], tour["pictures"][3]), "`at` 0.5 of six stops hands on the fourth"
    print("tour at 0.5:", tour["status"].split(" | ")[0])

    h.retune(n, info, space__folder=folder, walk__mode="near", walk__count=3)
    vector = z["emb"][z["owner"] == 11].mean(0)
    by_place = h.run(n, place=vector)
    first = n.result["pictures"][1]["space"]["files"][0]
    print(f"a wired place (photograph 11's mean crop) finds {os.path.basename(first)} first, {by_place['status']}")
    assert first == str(z["files"][11]), "a wired place steers the selection"
    try:
        h.run(n, place=np.ones(10, np.float32))
    except ValueError as e:
        assert "1024" in str(e)
    else:
        raise AssertionError("a place of the wrong width says so")

    target = str(z["files"][7])
    placed = h.run(enc, picture=h.read(target, 512))
    print("encoder:", placed["status"])
    assert placed["place"].shape == (1024,)
    h.retune(n, info, space__folder=folder, walk__mode="near", walk__count=3)
    found = h.run(n, place=placed["place"])
    first = n.result["pictures"][1]["space"]["files"][0]
    print(f"{os.path.basename(target)} placed by the encoder finds {os.path.basename(first)} first, "
          f"weights {found['weights']}")
    assert first == target
    print("all passed")


if __name__ == "__main__":
    main(sys.argv[1])
