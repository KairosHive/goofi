"""AdaptiveMask finds where a material goes, reading it off the picture itself.

latent-lab's mask (`masks.py`), one layer per node. A layer reads a SOURCE and keeps the share of
the frame that `coverage` asks for. Coverage is a quantile of the source's own distribution, so 0.3
is the darkest, nearest or most alike 30% of THIS frame, on a bright frame and a dark one, at 256 px
and at 4K. `follow` snaps the boundary to the picture's contours with a guided filter, so the mask
takes regions of the picture as they are drawn. `noise` erodes the boundary into a ragged front.
Chain nodes for more layers, one node's `mask` into the next one's `base`, met by `combine`. A
`fade` over the result grades the selection across its own extent.

`transition` sets how many seconds a change of setting takes to reach the mask. A number glides
from its old value to its new one along an eased curve, so a boundary travels to its new place. A
change of source, seed, invert, combine, keep_edge, fade kind, reverse or working size crossfades
from the last mask to the new one over the same time. A setting moved again mid-way glides on from
where it stands. `hold` acts on the incoming frames instead, smoothing a live picture's mask.

`by` picks what the layer is cut by. The families work in two different ways.

segmentation. A model decides what a thing IS and cuts it out. With `prompt`, a few words typed
here are cut out by SAM 2 (Grounding DINO finds a box for each thing named, SAM 2.1 cuts it) or by
CLIPSeg (a soft map that also knows surfaces such as wet or rough). With `region`, the cut comes
down the `region` wire, from PromptMask or goofi's Segment (a person). Either way the node reads no
statistic of the picture. It takes the cut-out as the layer and only reshapes its edge (softness,
follow, noise, grow), or keeps the edge as it came with `keep_edge`. `coverage` plays no part, since
the model already said where the thing is.

The other four families rank a map of numbers and keep the share `coverage` asks for, the highest
values first. They know nothing about what is in the picture, only how its values spread.
- picture. Statistics of the picture's own pixels. light, shadow, edges (local contrast), flat (low
  local variance), saturation, and colour (nearness to the colour under `colour_at`).
- field. A wired map of any range, such as a depth map, a DINO similarity map, a shader read back
  through GraphicsIn or an EEG map, its highest values first (invert for its lowest).
- texture. The light or the dark of a wired `texture` laid over the frame.
- shape. Geometry with no picture in it. gradient (rising toward `angle`), spot (around `spot_at`)
  and growth (fractal noise).

Pictures arrive as [h, w, 3] (or 4, or grey [h, w]), in 0..1 or 0..255. A stack [n, h, w, 3] is
read as its first picture. Maps and masks arrive as [h, w], or as a picture whose first channel is
read, at any size.

It runs inside goofi's own Python on numpy, scipy and PIL, with no process or memory of its own.
A prompt goes to one shared worker process that holds the models for every AdaptiveMask in goofi.
The first prompt starts it and it loads the model by itself (about 5 GB, once, however many masks
prompt). It leaves when goofi closes, or after ten minutes without a prompt. Every pixel knob is
set in a 384 px frame and scales with the frame it runs in, so a working size of 256 and one of
1024 cut the same mask.
"""

import hashlib
import math
import os
import pathlib
import pickle
import struct
import subprocess
import sys
import threading
import time
import types
from concurrent.futures import ThreadPoolExecutor

import numpy as np
from PIL import Image
from scipy import ndimage

import goofi

REF = 384
LUM = np.array([0.299, 0.587, 0.114], np.float32)
GOLD = np.array([0.79, 0.64, 0.33], np.float32)
# What a layer is cut by, family by family, and the source each menu entry reads.
FAMILIES = {"segmentation": {"prompt": "prompt", "region": "region"},
            "picture": {"light": "light", "shadow": "shadow", "edges": "edges", "flat": "flat",
                        "saturation": "saturation", "colour": "colour"},
            "field": {"field": "field"},
            "texture": {"light": "texture light", "dark": "texture dark"},
            "shape": {"gradient": "gradient", "spot": "spot", "growth": "growth"}}
MENUS = ("segmentation", "picture", "texture", "shape")  # the families with more than one entry have a menu
READERS = ["sam 2", "clipseg"]
PROMPTED = [("by", ["segmentation"]), ("segmentation", ["prompt"])]
SOURCES = [src for entries in FAMILIES.values() for src in entries.values()]
RANKED = [f for f in FAMILIES if f != "segmentation"]
COMBINE = ["and", "or", "but not"]
FADES = ["off", "across", "from a spot", "from the edges", "along the field"]
FADING = FADES[1:]
WORK = [0, 256, 384, 512, 768, 1024]
RESEND = 1.0  # seconds: an unchanged mask is sent again this often, so a viewer opened later sees it
RESAMPLE = {"area": Image.BOX, "bilinear": Image.BILINEAR, "bicubic": Image.BICUBIC}
POOL = ThreadPoolExecutor(max_workers=4, thread_name_prefix="adaptive-mask")
# Settings that glide under `transition`, and the ones among them read as angles (the short way round).
GLIDES = {"mask": ("coverage", "colour_at", "spot_at", "angle", "grow"),
          "edges": ("soft", "scale", "follow", "noise", "amount"),
          "fade": ("side", "spot", "reach", "length")}
TURNS = {("mask", "angle"), ("fade", "side")}
# Settings that crossfade under `transition`: a choice has no values in between.
SWITCHES = {"mask": ("by", "segmentation", "prompt", "model", "picture", "texture", "shape", "keep_edge", "invert",
                     "combine"), "edges": ("seed",), "fade": ("fade", "reverse"), "frame": ("work",)}


def source_of(q):
    """The source the mask group `q` picks: its family, and the entry of that family's menu."""
    if q.by not in FAMILIES:
        raise ValueError(f"mask/by must be one of {', '.join(FAMILIES)}")
    entries = FAMILIES[q.by]
    entry = getattr(q, q.by) if q.by in MENUS else next(iter(entries))
    if entry not in entries:
        raise ValueError(f"mask/{q.by} must be one of {', '.join(entries)}")
    return entries[entry]


def as_params(source):
    """The mask params that pick `source` (one of SOURCES), as {name: value}."""
    for family, entries in FAMILIES.items():
        for entry, src in entries.items():
            if src == source:
                return {"by": family, **({family: entry} if family in MENUS else {})}
    raise ValueError(f"no source {source!r}")


def together(*calls):
    """Independent calls run side by side. numpy, scipy and PIL let go of the GIL inside them."""
    return list(POOL.map(lambda c: c(), calls))


# ---- fields on the working frame: arrays [h, w] float32 ---------------------------------------

def resize(a, h, w, mode="bilinear"):
    """A field [h, w] or a picture [h, w, c] at another size, as PIL resamples (antialiased when
    shrinking)."""
    if a.shape[:2] == (h, w):
        return a
    if a.ndim == 3:
        return np.stack(together(*(lambda c=c: resize(a[..., c], h, w, mode) for c in range(a.shape[2]))), -1)
    im = Image.fromarray(np.ascontiguousarray(a, dtype=np.float32), "F")
    return np.asarray(im.resize((w, h), RESAMPLE[mode]), np.float32)


def blur(a, sigma):
    """A Gaussian for smooth fields, reflected at the frame's edge, with wide blurs run on a shrunk
    copy (a field that wide carries no fine detail)."""
    if sigma <= 0.3:
        return a
    h, w = a.shape[:2]
    if sigma > 6 and min(h, w) > 64:
        f = int(min(sigma // 3, 32))
        small = resize(a, max(8, h // f), max(8, w // f), "area")
        return resize(blur(small, sigma / f), h, w, "bicubic")
    s = (sigma, sigma) + ((0.0,) if a.ndim == 3 else ())
    return ndimage.gaussian_filter(np.asarray(a, np.float32), s, mode="mirror", truncate=3.0)


_PICKS = {}


def rank(a, sample=20_000, knots=257):
    """A field passed through its own distribution: 0.3 means above 30% of this frame. The
    distribution is read from a fixed sample of places and held at 257 quantiles, so a share lands
    within half a percent."""
    flat = a.reshape(-1)
    if flat.size > sample:
        if flat.size not in _PICKS:
            _PICKS[flat.size] = np.random.default_rng(0).choice(flat.size, sample, replace=False)
        flat = flat[_PICKS[flat.size]]
    ref = np.sort(flat)
    at = np.linspace(0.0, 1.0, min(knots, ref.size))
    return np.interp(a, ref[np.round(at * (ref.size - 1)).astype(np.int64)], at).astype(np.float32)


_GROWN = {}


def growth(h, w, scale, seed):
    """Fractal noise whose blobs are `scale` px. The grids are sized relative to the frame, so one
    seed grows one pattern at any size. Kept: it changes only with its settings."""
    key = (h, w, round(float(scale), 3), int(seed))
    if key in _GROWN:
        return _GROWN[key]
    rng = np.random.default_rng(int(seed))
    cell = max(float(scale), 1.0) * 4.0
    f = int(max(1, min(16, (cell / 16.0) // 2)))  # the finest octave keeps cells of 2 px or more
    sh, sw = max(8, h // f), max(8, w // f)
    out = np.zeros((sh, sw), np.float32)
    amp, total = 1.0, 0.0
    for _ in range(5):
        gh, gw = max(2, int(round(h / cell)) + 1), max(2, int(round(w / cell)) + 1)
        out += amp * resize(rng.standard_normal((gh, gw)).astype(np.float32), sh, sw, "bicubic")
        total += amp
        amp *= 0.5
        cell /= 2.0
    out = out / total
    out = out if (sh, sw) == (h, w) else resize(out, h, w, "bicubic")
    if len(_GROWN) > 16:
        _GROWN.clear()
    _GROWN[key] = out
    return out


def guided(p, guide, radius, eps=1e-3):
    """Edge-aware smoothing of `p` steered by `guide`, so the boundary moves onto the guide's edges.
    The fast guided filter (He & Sun): coefficients solved on a shrunk copy, applied to the full guide."""
    h, w = p.shape
    f = int(max(1, min(8, radius // 3)))
    if f > 1 and min(h, w) // f >= 16:
        ps, gs = resize(p, h // f, w // f, "area"), resize(guide, h // f, w // f, "area")
    else:
        f, ps, gs = 1, p, guide
    s = max(float(radius) / f, 1.0) / 2.0
    mi, mp, gp, gg = together(lambda: blur(gs, s), lambda: blur(ps, s), lambda: blur(gs * ps, s),
                              lambda: blur(gs * gs, s))
    a = (gp - mi * mp) / (gg - mi * mi + eps)
    b = mp - a * mi
    a, b = together(lambda: blur(a, s), lambda: blur(b, s))
    if f > 1:
        a, b = together(lambda: resize(a, h, w, "bilinear"), lambda: resize(b, h, w, "bilinear"))
    return a * guide + b


def smoothstep(x):
    x = np.clip(x, 0.0, 1.0)
    return x * x * (3.0 - 2.0 * x)


def lum(img):
    return img @ LUM


def cover(img, h, w):
    """A picture cropped about its centre to the frame's shape, then resized: it covers the frame."""
    ih, iw = img.shape[:2]
    if iw * h > ih * w:
        keep = max(1, round(ih * w / h))
        img = img[:, (iw - keep) // 2:(iw - keep) // 2 + keep]
    else:
        keep = max(1, round(iw * h / w))
        img = img[(ih - keep) // 2:(ih - keep) // 2 + keep]
    return resize(img, h, w, "bilinear")


def side(h, w, angle):
    """Higher toward the side `angle` names: 0 the top, a quarter turn the right."""
    y, x = np.mgrid[0:h, 0:w].astype(np.float32)
    return (x - w / 2) * math.sin(angle) - (y - h / 2) * math.cos(angle)


def opponent(c):
    """A rough opponent space (light, red-green, yellow-blue), light counting a little less, so a
    shade of a colour still counts as that colour."""
    return np.stack([0.6 * lum(c), c[..., 0] - c[..., 1], 0.5 * (c[..., 0] + c[..., 1]) - c[..., 2]], -1)


# ---- what comes down the wires ----------------------------------------------------------------

class Unwired(ValueError):
    """A source or a fade reads an input that has no wire. The layer then selects nothing."""


def goofi_home():
    return pathlib.Path(os.environ.get("GOOFI_HOME") or pathlib.Path.home()) / ".goofi"


def worker_script():
    """`prompt_mask.py` beside this file. goofi compiles a node it runs in-process from its source,
    with no folder in `__file__`, so the bundle is looked for where goofi writes it out: the newest
    `shipped/*/texture/` of this runtime, beside `python/`."""
    here = pathlib.Path(globals().get("__file__") or "")
    if here.is_absolute() and (here.parent / "prompt_mask.py").is_file():
        return here.parent / "prompt_mask.py"
    versions = [pathlib.Path(sys.prefix).parent.parent, pathlib.Path(sys.base_prefix).parent.parent]
    found = [f for v in versions for f in v.glob("shipped/*/texture/prompt_mask.py")]
    found = [f for f in found if f.is_file()]
    if not found:
        raise ValueError("A prompt needs prompt_mask.py from goofi's texture bundle, and it is not where goofi "
                         "writes its bundles out")
    return max(found, key=lambda f: f.stat().st_mtime)


def runtime_python():
    """goofi's GIL Python, the one that has torch, beside the free-threaded one this node runs in."""
    exe = pathlib.Path("Scripts", "python.exe") if os.name == "nt" else pathlib.Path("bin", "python")
    roots = [pathlib.Path(sys.prefix).parent, pathlib.Path(sys.base_prefix).parent,
             pathlib.Path(sys.executable).resolve().parent.parent.parent]
    found = sorted(pathlib.Path(os.environ.get("LOCALAPPDATA", "")).glob("goofi/*/python")) if os.name == "nt" else []
    for root in roots + found[::-1]:
        if (root / "gil" / exe).is_file():
            return root / "gil" / exe
    raise ValueError("A prompt needs goofi's GIL Python, which has torch, and it is not beside this one")


class Worker:
    """The one prompt worker of this goofi, shared by every AdaptiveMask: `prompt_mask.py --serve` in
    goofi's GIL Python, holding SAM 2 with Grounding DINO and CLIPSeg. The first prompt starts it, the
    model it needs loads by itself in the background, and until it is in the layer waits. It leaves
    when goofi closes, or after ten minutes without a prompt, and the next prompt starts it again."""

    def __init__(self):
        self.proc = None
        self.lock = threading.Lock()
        self.warm = set()  # the models the running worker has loaded
        self.job = None
        self.failed = None

    def alive(self):
        return self.proc is not None and self.proc.poll() is None

    def start(self):
        if self.alive():
            return
        self.warm = set()
        log_dir = goofi_home() / "plugin-data" / "texture"
        log_dir.mkdir(parents=True, exist_ok=True)
        env = dict(os.environ, OPENBLAS_NUM_THREADS="1", PYTHONUNBUFFERED="1")
        flags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
        with open(log_dir / "prompt-worker.log", "ab") as log:
            self.proc = subprocess.Popen([str(runtime_python()), str(worker_script()), "--serve"],
                                         stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log, env=env,
                                         creationflags=flags)

    def ask(self, message):
        with self.lock:
            self.start()
            data = pickle.dumps(message, protocol=5)
            try:
                self.proc.stdin.write(struct.pack("<Q", len(data)) + data)
                self.proc.stdin.flush()
                head = self.proc.stdout.read(8)
                answer = pickle.loads(self.proc.stdout.read(struct.unpack("<Q", head)[0])) if len(head) == 8 else None
            except OSError:
                answer = None
            if answer is None:
                self.proc = None
                raise ValueError("The prompt worker stopped. Its log is .goofi/plugin-data/texture/prompt-worker.log")
        if not answer["ok"]:
            raise ValueError(answer["error"])
        return answer

    def _load(self, model):
        try:
            self.ask({"op": "warm", "model": model})
            self.warm.add(model)
        except Exception as e:  # reported on the status, and asked again on the next frame
            self.failed = f"{model} could not load: {e}"

    def ready(self, model):
        """True once the worker has `model` loaded. The first call starts it loading in the background."""
        if not self.alive():
            self.warm = set()
        if model in self.warm:
            return True
        if self.failed:
            failed, self.failed, self.job = self.failed, None, None
            raise ValueError(failed)
        if self.job is None or not self.job.is_alive():
            self.job = threading.Thread(target=self._load, args=(model,), daemon=True, name="prompt-models")
            self.job.start()
        return False

    def __call__(self, key, img, words, model, sure, at, every):
        """The cut-out [h, w] in 0..1 with its edge at 0.5, and the detector's scores with sam 2."""
        answer = self.ask({"op": "cut", "key": key, "img": np.ascontiguousarray(img, dtype=np.float32),
                           "words": list(words), "model": model, "sure": sure, "at": at, "every": every})
        return answer["cut"], answer["found"]


def shared_worker():
    """The one Worker of this goofi. Every AdaptiveMask loads this file afresh, so the worker is kept
    where all of them find it."""
    holder = sys.modules.setdefault("goofi_texture_prompt_worker", types.ModuleType("goofi_texture_prompt_worker"))
    return holder.__dict__.setdefault("worker", Worker())


def words_of(text):
    return [w.strip() for w in str(text).split(",") if w.strip()]


def picture(value, name):
    """Whatever came down the wire, as float RGB in 0..1 [h, w, 3]: a stack [n, h, w, c] gives its
    first picture."""
    x = np.asarray(value)
    if x.ndim == 4:
        x = x[0]
    if x.ndim == 2:
        x = np.repeat(x[..., None], 3, axis=-1)
    if x.ndim != 3 or x.shape[2] < 3:
        raise ValueError(f"`{name}` takes a picture [height, width, 3]. This frame is {list(x.shape)}")
    x = x[..., :3]
    if not np.issubdtype(x.dtype, np.floating):
        return x.astype(np.float32) / 255.0
    return np.clip(np.nan_to_num(x.astype(np.float32, copy=False)), 0.0, 1.0)


def plane(value, name, scaled=True):
    """A map [h, w]: a picture's first channel if it has several, channels last [h, w, c] or first
    [c, h, w], the first of a stack. A mask (`scaled`) in 0..255 is brought to 0..1. A field keeps
    its own range, since only its order is read."""
    x = np.asarray(value)
    if x.ndim == 4:
        x = x[0]
    if x.ndim == 3:
        x = x[0] if x.shape[0] in (1, 3, 4) and x.shape[-1] not in (1, 3, 4) else x[..., 0]
    if x.ndim != 2:
        raise ValueError(f"`{name}` takes a map [height, width]. This frame is {list(x.shape)}")
    x = x.astype(np.float32, copy=False)
    return np.nan_to_num(x / 255.0 if scaled and x.max(initial=0.0) > 1.5 else x)


def fingerprint(x):
    """Enough of a frame to tell it from the one before, cheaply: its shape and a hash of a spread
    sample of its values."""
    if x is None:
        return None
    a = np.ascontiguousarray(x).reshape(-1)
    step = max(1, a.size // 65536)
    return x.shape, hashlib.blake2b(np.ascontiguousarray(a[::step]).tobytes(), digest_size=12).hexdigest()


def clamp(v, lo, hi):
    return min(max(float(v), lo), hi)


def numbers(v):
    """A number or a vector setting as a tuple of floats."""
    return tuple(float(x) for x in v) if isinstance(v, (list, tuple, np.ndarray)) else (float(v),)


def between(a, b, t, turn=False):
    """`a` moved toward `b` by the share `t`, an angle going the short way round."""
    out = []
    for x, y in zip(a, b):
        d = y - x
        if turn:
            d = (d + math.pi) % (2.0 * math.pi) - math.pi
        out.append(x + d * t)
    return tuple(out)


class Transition:
    """The settings the mask is cut with while a change travels. Numbers glide from where they stood
    to where they were set along a smoothstep in time, and a switched choice crossfades from the last
    mask. With `seconds` at 0 every change lands at once."""

    def __init__(self):
        self.to = None        # the numbers as set, {(group, name): tuple}
        self.start = {}       # where each number set off from, and when
        self.switched = None  # the choices as set
        self.old = None       # the mask a crossfade leaves
        self.since = 0.0      # when that crossfade began

    def numbers_at(self, key, now, seconds):
        x0, t0 = self.start[key]
        t = 1.0 if seconds <= 0 else float(smoothstep((now - t0) / seconds))
        return between(x0, self.to[key], t, key in TURNS)

    def view(self, p, now, seconds, last):
        """The params as the mask is cut at `now`, and the share of the new mask in the crossfade."""
        to = {(g, n): numbers(getattr(getattr(p, g), n)) for g, names in GLIDES.items() for n in names}
        if self.to is None:
            self.to, self.start = to, {k: (v, now - seconds) for k, v in to.items()}
        for k, v in to.items():
            if v != self.to[k]:
                here = self.numbers_at(k, now, seconds)
                self.start[k] = (here if len(here) == len(v) else v, now)
                self.to[k] = v
        switched = tuple(getattr(getattr(p, g), n) for g, names in SWITCHES.items() for n in names)
        if self.switched is not None and switched != self.switched and seconds > 0 and last is not None:
            self.old, self.since = last, now
        self.switched = switched

        groups = {g: types.SimpleNamespace(**vars(getattr(p, g))) for g in ("mask", "edges", "fade", "frame")}
        for g, n in self.to:
            x = self.numbers_at((g, n), now, seconds)
            setattr(groups[g], n, list(x) if len(x) > 1 else x[0])
        share = 1.0
        if self.old is not None:
            share = 1.0 if seconds <= 0 else float(smoothstep((now - self.since) / seconds))
            if share >= 1.0:
                self.old = None
        return types.SimpleNamespace(**groups), share

    def moving(self, now, seconds):
        if seconds <= 0:
            return False
        return self.old is not None or any(now - t0 < seconds for _, t0 in self.start.values())


def pair(v):
    v = list(v) if isinstance(v, (list, tuple)) else [v, v]
    return clamp(v[0], 0.0, 1.0), clamp(v[1], 0.0, 1.0)


class AdaptiveMask(goofi.Node):
    """Keep the share of the frame a source picks, snapped to the picture's contours: one mask layer."""

    TAGS = ["image", "analysis"]
    INPUTS = {
        "image": goofi.InputSlot(goofi.DataType.ARRAY, required=True),
        "texture": goofi.InputSlot(goofi.DataType.ARRAY, trigger=False),
        "field": goofi.InputSlot(goofi.DataType.ARRAY),
        "region": goofi.InputSlot(goofi.DataType.ARRAY),
        "base": goofi.InputSlot(goofi.DataType.ARRAY),
    }
    OUTPUTS = {"mask": goofi.DataType.ARRAY, "preview": goofi.DataType.ARRAY, "status": goofi.DataType.STRING}
    PARAMS = {
        "mask": [
            {
                "by": goofi.StringParam("picture", list(FAMILIES), doc="What the layer is cut by. segmentation takes "
                                        "a model's cut-out of a thing, named in `prompt` or wired into `region`, and "
                                        "only reshapes its edge. picture, field, texture and shape rank a map of numbers "
                                        "and keep the share `coverage` asks for. A family whose wire is missing "
                                        "selects nothing."),
                "segmentation": goofi.StringParam("region", list(FAMILIES["segmentation"]), show=("by", ["segmentation"]),
                                                  doc="prompt cuts out what the words in `prompt` name. region takes a "
                                                  "cut-out wired in, from PromptMask or Segment."),
                "prompt": goofi.StringParam("flower", show=PROMPTED, doc="What to cut out, in a few words. Commas "
                                            "separate things taken together, as in `moss, lichen`."),
                "model": goofi.StringParam("sam 2", READERS, show=PROMPTED, doc="Who reads the prompt. sam 2 cuts out "
                                           "each thing named with a sharp edge (Grounding DINO and SAM 2.1, 0.9 GB, "
                                           "fetched once). clipseg gives a soft map that also knows surfaces such as wet "
                                           "or rough (0.6 GB). The first prompt starts one worker process that every "
                                           "AdaptiveMask shares, about 5 GB once, and the model loads by itself."),
                "confidence": goofi.NumParam(0.3, 0.0, 1.0, show=PROMPTED, doc="How sure the model must be that a thing "
                                             "(with sam 2) or a place (with clipseg) is what the prompt names. Lower "
                                             "takes more. The status shows sam 2's score for each thing found."),
                "detect_at": goofi.NumParam(800, 384, 800, int=True, options=[384, 512, 800],
                                            show=PROMPTED + [("model", ["sam 2"])], doc="Pixels on the short side "
                                            "Grounding DINO reads the picture at. 800 is its own. 384 is faster and found "
                                            "the same things in tests."),
                "redetect": goofi.NumParam(0.5, 0.0, 5.0, show=PROMPTED + [("model", ["sam 2"])], doc="Seconds between "
                                           "Grounding DINO's looks on a live picture. In between, SAM 2 cuts each new "
                                           "frame from the last boxes. 0 looks on every frame."),
                "picture": goofi.StringParam("light", list(FAMILIES["picture"]), show=("by", ["picture"]),
                                             doc="Which statistic of the picture's pixels. light and shadow rank "
                                             "luminance, edges local contrast, flat the calm areas, saturation "
                                             "colourfulness, colour nearness to the colour under `colour_at`."),
                "texture": goofi.StringParam("light", list(FAMILIES["texture"]), show=("by", ["texture"]),
                                             doc="The light or the dark of the wired texture, laid over the frame."),
                "shape": goofi.StringParam("gradient", list(FAMILIES["shape"]), show=("by", ["shape"]),
                                           doc="gradient rises toward `angle`, spot falls away from `spot_at`, growth "
                                           "is fractal noise whose grain is `scale`."),
                "coverage": goofi.NumParam(0.5, 0.0, 1.0, doc="The share of the frame the layer keeps, as a quantile "
                                           "of this frame. At 0.3 it is the lightest (darkest, highest, closest) 30%.",
                                           show=("by", RANKED)),
                "colour_at": goofi.NumParam([0.5, 0.5], 0.0, 1.0, doc="Where the colour is read, x and y across the "
                                            "frame from its top left.", show=[("by", ["picture"]), ("picture", ["colour"])]),
                "spot_at": goofi.NumParam([0.5, 0.5], 0.0, 1.0, doc="The spot's centre, x and y across the frame from "
                                          "its top left.", show=[("by", ["shape"]), ("shape", ["spot"])]),
                "angle": goofi.NumParam(0.0, 0.0, 6.283, doc="The side the gradient rises toward, 0 the top, 1.57 the "
                                        "right, 3.14 the bottom.", show=[("by", ["shape"]), ("shape", ["gradient"])]),
                "keep_edge": goofi.BoolParam(False, doc="Take the cut-out's own edge as the model drew it, for a soft "
                                             "matte. Off, the edge is reshaped by softness, follow and noise.",
                                             show=("by", ["segmentation"])),
            },
            {
                "invert": goofi.BoolParam(False, doc="Take what the source leaves out."),
                "grow": goofi.NumParam(0.0, -24.0, 24.0, doc="Grow the layer outward (or shrink it inward), in pixels of "
                                       "a 384 px frame. Corners round as it goes."),
                "combine": goofi.StringParam("and", COMBINE, doc="How this layer meets a wired `base`. Both, either, or "
                                             "the base without this layer."),
            },
        ],
        "edges": {
            "soft": goofi.NumParam(0.2, 0.0, 1.0, doc="How gradual the edge is, in the source's quantiles."),
            "scale": goofi.NumParam(4.0, 0.0, 32.0, doc="The feature size in pixels of a 384 px frame. It sets how much "
                                    "the source is smoothed and the grain of noise and growth."),
            "follow": goofi.NumParam(0.6, 0.0, 1.0, doc="How far the edge snaps to the picture's own contours."),
            "noise": goofi.NumParam(0.0, 0.0, 1.0, doc="A ragged edge, fractal noise eating into the boundary like a "
                                    "front of lichen."),
            "seed": goofi.NumParam(0, 0, 9999, int=True, doc="Which growth and which ragged edge."),
            "amount": goofi.NumParam(1.0, 0.0, 1.0, doc="The strength of the whole mask."),
        },
        "fade": {
            "fade": goofi.StringParam("off", FADES, doc="A gradient over the selection, measured over its own extent, "
                                      "from a side, from a spot, from its edges inward, or along a wired field (a depth "
                                      "map fades from near to far)."),
            "side": goofi.NumParam(0.0, 0.0, 6.283, doc="The side the selection is full on, 0 the top, 1.57 the right.",
                                   show=("fade", ["across"])),
            "spot": goofi.NumParam([0.5, 0.5], 0.0, 1.0, doc="The spot the selection is full at.",
                                   show=("fade", ["from a spot"])),
            "reach": goofi.NumParam(0.5, 0.0, 1.0, doc="Where the fade is half way, as a share of the selection.",
                                    show=("fade", FADING)),
            "length": goofi.NumParam(1.0, 0.0, 1.0, doc="How long the fade is, as a share of the selection.",
                                     show=("fade", FADING)),
            "reverse": goofi.BoolParam(False, doc="Fade the other way.", show=("fade", FADING)),
        },
        "frame": {
            "work": goofi.NumParam(512, 0, 1024, int=True, options=WORK, doc="Pixels on the long side the mask is cut "
                                   "at, 0 for the picture's own. The mask comes out at the picture's size."),
            "hold": goofi.NumParam(0.0, 0.0, 0.98, doc="Carry this share of the last mask into the next, so a live "
                                   "picture's mask moves smoothly."),
            "transition": goofi.NumParam(0.0, 0.0, 10.0, doc="Seconds a change of setting takes to reach the mask. "
                                         "Numbers such as coverage, grow or spot_at glide, so the boundary travels. A "
                                         "change of source, seed, invert, combine or fade crossfades from the last "
                                         "mask. At 0 a change lands at once."),
        },
        "common": {"autotrigger": goofi.BoolParam(True), "max_frequency": goofi.NumParam(30.0, 1.0, 120.0)},
    }

    def setup(self):
        self.waiting = None
        self.key = None
        self.last = None
        self.settled = False
        self.result = None
        self.sent = 0.0
        self.travel = Transition()
        self.prompts = shared_worker()
        self.found = None
        self.loading = False

    def process(self, image, texture=None, field=None, region=None, base=None):
        if image is None:
            return None
        frames = {name: (None if d is None else np.asarray(d.data))
                  for name, d in (("image", image), ("texture", texture), ("field", field),
                                  ("region", region), ("base", base))}
        q = self.params
        key = (tuple(fingerprint(v) for v in frames.values()), repr(vars(q.mask)), repr(vars(q.edges)),
               repr(vars(q.fade)), repr(vars(q.frame)))
        now = time.monotonic()
        seconds = clamp(q.frame.transition, 0.0, 60.0)
        moving = self.travel.moving(now, seconds)
        if key == self.key and self.settled and not moving:
            if self.result is not None and time.monotonic() - self.sent >= RESEND:
                self.sent = time.monotonic()
                return self.result
            return None
        if key != self.key:
            self.settled = False
        self.key = key
        started = time.perf_counter()
        p, share = self.travel.view(q, now, seconds, self.last)
        src = picture(frames["image"], "image")
        H, W = src.shape[:2]
        long = int(p.frame.work) or max(H, W)
        k = min(1.0, long / max(H, W))
        h, w = max(8, round(H * k)), max(8, round(W * k))
        img = resize(src, h, w, "bilinear")
        self.loading = False
        m = self.cut(p, img, frames)
        if share < 1.0:
            old = self.travel.old
            old = old if old.shape == m.shape else resize(old, h, w, "bilinear")
            m = old + share * (m - old)
        hold = clamp(p.frame.hold, 0.0, 0.98)
        if hold > 0 and self.last is not None and self.last.shape == m.shape:
            moved = m - self.last
            m = self.last + (1.0 - hold) * moved
            self.settled = float(np.abs(moved).max()) < 1e-3
        else:
            self.settled = True
        moving = self.travel.moving(time.monotonic(), seconds)
        self.settled = self.settled and not moving and not self.loading  # a loading model is asked again
        self.last = m
        mask = np.ascontiguousarray(np.clip(resize(m, H, W, "bilinear"), 0.0, 1.0))
        grey = (lum(src) * 0.55)[..., None]
        preview = np.ascontiguousarray(np.clip(grey * (1.0 - mask[..., None]) + (grey * 0.45 + GOLD * 0.75)
                                               * mask[..., None], 0.0, 1.0).astype(np.float32))
        ms = (time.perf_counter() - started) * 1000.0
        covered = float((mask > 0.5).mean())
        meta = {"mask": {"source": source_of(p.mask), "covered": covered, "width": W, "height": H}}
        wait = f" | waiting: {self.waiting}" if self.waiting else ""
        glide = " | in transition" if moving else ""
        what = f"{p.mask.by} / {source_of(p.mask)}"
        if source_of(p.mask) == "prompt":
            what += f" ({', '.join(words_of(p.mask.prompt))}) by {p.mask.model}"
            if self.found is not None:
                what += f", {len(self.found)} found" + (f" (scores {', '.join(map(str, self.found))})" if self.found else "")
        self.result = {"mask": (mask, meta),
                       "preview": (preview, {"channels": {"dim2": ["r", "g", "b"]}}),
                       "status": f"{what} | {covered:.0%} covered{wait}{glide} | {w} x {h} | {ms:.0f} ms"}
        self.sent = time.monotonic()
        return self.result

    def cut(self, p, img, frames):
        """The mask on the working frame [h, w]."""
        h, w = img.shape[:2]
        k = max(h, w) / REF  # pixel knobs are set in a 384 px frame
        kind = source_of(p.mask)
        scale = clamp(p.edges.scale, 0.0, 64.0) * k
        noise, seed = clamp(p.edges.noise, 0.0, 1.0), int(p.edges.seed)

        self.waiting = None
        under = None
        if frames["base"] is not None:
            under = np.clip(resize(plane(frames["base"], "base"), h, w, "bilinear"), 0.0, 1.0)
        try:
            layer = self.layer(p, kind, img, frames, scale, noise, seed, k)
        except Unwired as e:
            # Nothing to read: the layer selects nothing, and a base goes through as it came.
            self.waiting = str(e)
            m = under if under is not None else np.zeros((h, w), np.float32)
            return np.clip(m * clamp(p.edges.amount, 0.0, 1.0), 0.0, 1.0)

        m = layer
        if under is not None:
            how = p.mask.combine
            m = under * layer if how == "and" else 1.0 - (1.0 - under) * (1.0 - layer) if how == "or" \
                else under * (1.0 - layer)
        try:
            m = m * self.fade(p, m, frames)
        except Unwired as e:
            self.waiting = str(e)
        return np.clip(m * clamp(p.edges.amount, 0.0, 1.0), 0.0, 1.0).astype(np.float32)

    def layer(self, p, kind, img, frames, scale, noise, seed, k):
        """This node's own layer [h, w], before it meets the base."""
        h, w = img.shape[:2]
        keep = False
        self.found = None
        if kind in ("region", "prompt"):
            if kind == "prompt":
                words = words_of(p.mask.prompt)
                if not words:
                    raise Unwired("a few words in `prompt`")
                if p.mask.model not in READERS:
                    raise ValueError(f"mask/model must be one of {', '.join(READERS)}")
                if not self.prompts.ready(p.mask.model):
                    self.loading = True
                    raise Unwired(f"{p.mask.model} loading by itself, about 20 to 60 s the first time")
                r, self.found = self.prompts(fingerprint(frames["image"]), img, words, p.mask.model,
                                             clamp(p.mask.confidence, 0.0, 1.0), int(p.mask.detect_at),
                                             clamp(p.mask.redetect, 0.0, 60.0))
            else:
                if frames["region"] is None:
                    raise Unwired("a mask wired into `region`")
                r = np.clip(resize(plane(frames["region"], "region"), h, w, "bilinear"), 0.0, 1.0)
            keep = bool(p.mask.keep_edge) and noise <= 0
            if keep:
                layer = r
            else:
                r = blur(r, max(scale, 1.0))  # a band for noise to eat into and the threshold to sit on
                if noise > 0:
                    r = np.clip(r + noise * (rank(growth(h, w, max(scale, 2.0) * 2.0, seed + 17)) - 0.5), 0.0, 1.0)
                layer = self.edge(p, r, 0.5)
        else:
            r = rank(self.field(kind, p, img, frames, scale, seed))
            if noise > 0 and kind != "growth":
                r = rank((1.0 - noise) * r + noise * rank(growth(h, w, max(scale, 2.0) * 2.0, seed + 17)))
            layer = self.edge(p, r, 1.0 - clamp(p.mask.coverage, 0.0, 1.0))

        if p.mask.invert:
            layer = 1.0 - layer
        px = clamp(p.mask.grow, -96.0, 96.0) * k
        if abs(px) >= 0.5:
            b = blur(layer, abs(px) / 2.0)
            t = 0.023 if px > 0 else 0.977  # a step blurred by s is 2.3% two s out, 97.7% two s in
            layer = smoothstep((b - t) / 0.04 + 0.5)
        follow = clamp(p.edges.follow, 0.0, 1.0)
        if follow > 0 and not keep:
            layer = (1.0 - follow) * layer + follow * guided(layer, lum(img), max(scale * 2.0, 4.0))
        return np.clip(layer, 0.0, 1.0)

    def edge(self, p, r, t):
        soft = max(clamp(p.edges.soft, 0.0, 1.0), 1e-4)
        return smoothstep((r - (t - soft / 2.0)) / soft)

    def wired_field(self, frames, h, w):
        """The wired map on the working frame, in its own range."""
        if frames["field"] is None:
            raise Unwired("a map wired into `field`")
        return resize(plane(frames["field"], "field", scaled=False), h, w, "bilinear")

    def field(self, kind, p, img, frames, sc, seed):
        """The raw field a ranked source reads, higher where the material should go."""
        h, w = img.shape[:2]
        if kind == "growth":
            return growth(h, w, max(sc, 1.0), seed)
        if kind == "gradient":
            return side(h, w, clamp(p.mask.angle, -100.0, 100.0))
        if kind == "spot":
            x, y = pair(p.mask.spot_at)
            yy, xx = np.mgrid[0:h, 0:w].astype(np.float32)
            return -np.hypot(xx - x * w, yy - y * h)
        light = lum(img)
        if kind == "light":
            f = light
        elif kind == "shadow":
            f = -light
        elif kind == "field":
            f = self.wired_field(frames, h, w)
        elif kind == "edges":
            gy, gx = np.gradient(blur(light, max(sc / 2.0, 0.8)))
            f = np.hypot(gx, gy)
        elif kind == "flat":
            r = max(sc, 1.5)
            mean = blur(light, r)
            f = -np.sqrt(np.clip(blur(light * light, r) - mean * mean, 0.0, None))
        elif kind == "saturation":
            f = img.max(-1) - img.min(-1)
        elif kind == "colour":
            x, y = pair(p.mask.colour_at)
            r = max(1, int(0.01 * max(h, w)))
            x0, y0 = min(int(x * w), w - 1), min(int(y * h), h - 1)
            ref = img[max(0, y0 - r):y0 + r + 1, max(0, x0 - r):x0 + r + 1].reshape(-1, 3).mean(0)
            f = -np.linalg.norm(opponent(img) - opponent(ref[None, None]), axis=-1)
        elif kind in ("texture light", "texture dark"):
            if frames["texture"] is None:
                raise Unwired("a picture wired into `texture`")
            t = lum(cover(picture(frames["texture"], "texture"), h, w))
            f = t if kind == "texture light" else -t
        else:
            raise ValueError(f"no mask source {kind!r}")
        f = np.asarray(f, np.float32)
        return blur(f, sc) if sc > 0 else f

    def fade(self, p, m, frames):
        """The add-on gradient over a built mask: 1 on the side the material comes from, fading to 0,
        measured over the mask's own extent, a stray speck far off left out."""
        kind = p.fade.fade
        inside = m > 0.02
        if kind == "off" or not inside.any():
            return np.ones_like(m)
        h, w = m.shape
        if kind == "from the edges":
            f = -ndimage.distance_transform_edt(m > 0.5).astype(np.float32)
        elif kind == "along the field":
            f = self.wired_field(frames, h, w)
        elif kind == "across":
            f = side(h, w, clamp(p.fade.side, -100.0, 100.0))
        else:
            x, y = pair(p.fade.spot)
            yy, xx = np.mgrid[0:h, 0:w].astype(np.float32)
            f = -np.hypot(xx - x * w, yy - y * h)
        lo, hi = np.percentile(f[inside], [1, 99])
        d = np.clip((hi - f) / max(float(hi - lo), 1e-6), 0.0, 1.0)
        if p.fade.reverse:
            d = 1.0 - d
        length = max(clamp(p.fade.length, 0.0, 1.0), 1e-3)
        return 1.0 - smoothstep((d - (clamp(p.fade.reach, 0.0, 1.0) - length / 2.0)) / length)
