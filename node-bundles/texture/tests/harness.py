"""Run a node file the way goofi's subprocess tier does: its declaration read by goofi's own
introspection, its params as the nested namespaces goofi sets, its inputs as frames with `.data`
and `.meta`. Run with goofi's runtime interpreter (the `gil` one), which has the `goofi` module."""

import importlib.util
import json
import os
import pathlib
import types

import numpy as np

import goofi

# The texture bundle, whose node files these tests load.
BUNDLE = pathlib.Path(__file__).resolve().parents[1]
# The lab is the latent-lab clone beside goofi's, or wherever LATENT_LAB points.
LAB = pathlib.Path(os.environ.get("LATENT_LAB", BUNDLE.parents[2] / "latent-lab")).expanduser()


class Frame:
    def __init__(self, data, meta=None):
        self.data = data
        self.meta = meta or {}


def load(name):
    """The node module and goofi's reading of its declaration."""
    path = BUNDLE / f"{name}.py"
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module, json.loads(goofi.introspect(str(path)))


def params(info, **over):
    """The declared defaults as goofi hands them to a node, with `group__name=value` overrides."""
    groups = {}
    for p in info["params"]:
        if p["kind"] == "num":
            v = p["default"] if len(p["default"]) > 1 else p["default"][0]
            if p.get("int"):
                v = [int(round(x)) for x in v] if isinstance(v, list) else int(round(v))
        elif p["kind"] == "pulse":
            v = None
        else:
            v = p["default"]
        groups.setdefault(p["group"], {})[p["name"]] = v
    for key, v in over.items():
        group, name = key.split("__")
        if name not in groups.get(group, {}):
            raise KeyError(f"no param {group}/{name}")
        groups[group][name] = v
    return types.SimpleNamespace(**{g: types.SimpleNamespace(**d) for g, d in groups.items()})


def sourced(module, over):
    """AdaptiveMask's source named as one word, `mask__source="shadow"` and `mask__point`, as the
    node's own family and menu take it."""
    if not hasattr(module, "as_params") or "mask__source" not in over:
        return over
    over = dict(over)
    source = over.pop("mask__source")
    over.update({f"mask__{k}": v for k, v in module.as_params(source).items()})
    if "mask__point" in over:
        over["mask__colour_at" if source == "colour" else "mask__spot_at"] = over.pop("mask__point")
    return over


def node(module, info, **over):
    cls = next(v for v in vars(module).values()
               if isinstance(v, type) and issubclass(v, goofi.Node) and v is not goofi.Node)
    n = cls()
    n.harness_module = module
    n.params = params(info, **sourced(module, over))
    n.setup()
    return n


def retune(n, info, **over):
    n.params = params(info, **sourced(getattr(n, "harness_module", None), over))


def run(n, **inputs):
    """One process call, frames wrapped; returns the outputs as {name: array or text} or None."""
    out = n.process(**{k: (None if v is None else Frame(v)) for k, v in inputs.items()})
    if out is None:
        return None
    return {k: (v[0] if isinstance(v, tuple) else v) for k, v in out.items()}


def picture(h=384, w=576, seed=0, grain=0.03):
    """A synthetic picture with forms, colour and grain: a lit disc, a dark bar, a gradient sky."""
    rng = np.random.default_rng(seed)
    y, x = np.mgrid[0:h, 0:w].astype(np.float32)
    sky = np.stack([0.35 + 0.4 * y / h, 0.5 + 0.3 * y / h, 0.85 - 0.2 * y / h], -1)
    img = sky.copy()
    disc = ((x - 0.33 * w) ** 2 + (y - 0.5 * h) ** 2) < (0.22 * h) ** 2
    img[disc] = [0.95, 0.8, 0.2]
    bar = (abs(x - 0.75 * w) < 0.06 * w) & (y > 0.2 * h)
    img[bar] = [0.15, 0.25, 0.1]
    img += rng.normal(0, grain, img.shape).astype(np.float32)
    return np.clip(img, 0, 1).astype(np.float32)


def texture(h=384, w=576, seed=1):
    """A synthetic material: fractal noise in two colours, with cracks."""
    rng = np.random.default_rng(seed)
    out = np.zeros((h, w), np.float32)
    for octave in range(5):
        cell = 64 / 2 ** octave
        g = rng.standard_normal((int(h / cell) + 2, int(w / cell) + 2)).astype(np.float32)
        yy = np.linspace(0, g.shape[0] - 2, h)
        xx = np.linspace(0, g.shape[1] - 2, w)
        iy, ix = yy.astype(int), xx.astype(int)
        fy, fx = (yy - iy)[:, None], (xx - ix)[None]
        out += 0.5 ** octave * (g[iy][:, ix] * (1 - fy) * (1 - fx) + g[iy + 1][:, ix] * fy * (1 - fx)
                                + g[iy][:, ix + 1] * (1 - fy) * fx + g[iy + 1][:, ix + 1] * fy * fx)
    out = (out - out.min()) / (out.max() - out.min())
    a, b = np.array([0.55, 0.35, 0.2], np.float32), np.array([0.85, 0.75, 0.6], np.float32)
    return (a + (b - a) * out[..., None]).astype(np.float32)


def read(path, long=None):
    from PIL import Image

    with Image.open(path) as im:
        im = im.convert("RGB")
        if long:
            k = long / max(im.size)
            im = im.resize((max(1, round(im.width * k)), max(1, round(im.height * k))), Image.LANCZOS)
        return np.asarray(im, np.float32) / 255.0


def read_array(a, h, w):
    """A picture array at another size, Lanczos."""
    from PIL import Image

    im = Image.fromarray((np.clip(a, 0, 1) * 255 + 0.5).astype(np.uint8)).resize((w, h), Image.LANCZOS)
    return np.asarray(im, np.float32) / 255.0


def save(a, path):
    from PIL import Image

    a = np.asarray(a)
    if a.ndim == 2:
        a = np.repeat(a[..., None], 3, -1)
    Image.fromarray((np.clip(a, 0, 1) * 255 + 0.5).astype(np.uint8)).save(path)
