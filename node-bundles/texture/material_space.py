"""MaterialSpace treats a set of photographs as a space to move through, placed by the IP-Adapter's eye.

latent-lab's `ip_space.py`. Every photograph is cut into crops and each crop is placed by the image
encoder the IP-Adapter was trained with (CLIP ViT-H), so the cloud of vectors is the space. Nothing
is trained. A lab space (`.npz`) loads as it is, or MaterialEncoder builds one from a folder and
keeps it in goofi's plugin data, where this node finds it by the same folder.

The space decides WHICH photographs and how much of each. What draws them is another node's business.
- tour. The most typical photograph of each of `stops` regions of the space, in the order of the
  shortest loop through them, as the lab routes its films. `at` picks the stop handed on alone.
- near. The `count` photographs nearest a place, with how near each is as `weights`. The place is
  a wired `place` vector (MaterialEncoder's place of a picture, another MaterialSpace's place), else
  the space's centre moved along its principal axes by `axes` (in units of the space's own spread
  on each). Drive `axes` with signals and the selection slides through the archive.
- map. The archive as an n-dimensional map. Every photograph has `dims` coordinates, one per axis of
  the `basis`, each scaled so 0 and 1 are the archive's two ends on that axis (its 1st and 99th
  percentiles). `axis1` to `axis8` are where you stand on the map, each a param of its own so a
  signal can drive it, and the `count` photographs nearest it are handed on, weighted by how near
  each is within `reach`. A principal basis takes the main axes of
  the photographs' own places, strongest first. A concepts basis takes the named directions in a
  concept book (latent-lab's `concepts.json`, such as humid, warm or organic), so each coordinate
  reads as how much of that concept. `transition` glides the axes over that many seconds. Every
  photograph on disk is weighted by its distance on the map, so `place` moves continuously with the
  position, and it sits at the position on every map axis before it is brought to a real crop's
  length. Only photographs found on disk are handed on, and the map draws the others dim.

Movement
- `pace` moves the walk by itself. In a tour it is stops per minute, `at` carried round the loop. On
  a map it is how many slow swings a minute the position makes, `wander` wide around the axes, each
  axis on its own curve, so the walk never retraces a line.
- `picture` crossfades. A tour blends each stop into the next as it passes, and a map or near blends
  the photographs handed on by their weights, which reach 0 before a photograph leaves the set.
  `blend` set to nearest hands on one photograph instead.
- `transition` covers every change of choice too. A change of mode, basis, dims, stops, seed, count
  or blend crossfades `picture` and slerps `place` from where they stood, so nothing jumps.

It runs on numpy and PIL alone, so goofi runs it inside its own process, with no Python process of
its own.

Outputs
- pictures. The photographs as one stack [n, h, w, 3], cropped to one frame, for a node that walks
  or mixes several, such as a morph.
- picture. One photograph [h, w, 3], the nearest or the tour's stop at `at`, for any node that takes
  a picture, such as TextureTransfer's texture.
- weights. One number per photograph in `pictures`, summing to 1.
- place. The vector at the length a real crop's vector has, for anything else that reads the space
  (an IP-Adapter, the lab's space GAN, another MaterialSpace's `place`).
- map. A picture of the map in map mode, one panel per pair of axes (1 and 2, 3 and 4, and on).
  Each photograph is a dot coloured by its category, the photographs handed on are ringed, and the
  position is the white cross. A map of odd `dims` pairs its last axis with the first.
"""

import hashlib
import json
import math
import os
import pathlib
import time

import numpy as np

import goofi

MODES = ["tour", "near", "map"]
BASES = ["principal", "concepts"]
MOST = 8  # the most axes a map has
PANEL = 240  # pixels on a side of one panel of the map picture
HUES = np.array([[0.95, 0.55, 0.35], [0.40, 0.75, 0.95], [0.60, 0.90, 0.45], [0.95, 0.80, 0.30],
                 [0.80, 0.50, 0.95], [0.40, 0.90, 0.80], [0.95, 0.45, 0.65], [0.75, 0.75, 0.75]], np.float32)
FRAMES = ["1:1", "3:2", "2:3", "4:3", "3:4", "16:9", "9:16"]


def data_dir():
    """goofi's persistent data for this bundle."""
    home = pathlib.Path(os.environ.get("GOOFI_HOME") or pathlib.Path.home()) / ".goofi"
    return home / "plugin-data" / "texture" / "spaces"


# The space this node loads by itself when none is named: the four-star archive's, with its photographs.
ARCHIVE = {"space": ("Kairos-Hive/latent-textures", "space/texture-ip-sd15.npz"),
           "photos": ("Kairos-Hive/latent-textures-sets", "training-set-1600")}


def hub_root():
    home = os.environ.get("HF_HOME") or str(pathlib.Path.home() / ".cache" / "huggingface")
    return pathlib.Path(os.environ.get("HF_HUB_CACHE") or pathlib.Path(home) / "hub"), pathlib.Path(home)


def hub_file(repo, path, dataset=False):
    """A file of a Hugging Face repo, from the Hub's own cache when it is there, else fetched once with
    the user's token into this bundle's data. The standard library does it, since goofi runs this node
    in its own Python, which has no huggingface_hub."""
    import shutil
    import urllib.request

    cache, home = hub_root()
    folder = cache / f"{'datasets' if dataset else 'models'}--{repo.replace('/', '--')}" / "snapshots"
    if folder.is_dir():
        for snap in sorted(folder.iterdir(), key=lambda s: s.stat().st_mtime, reverse=True):
            if (snap / path).is_file():
                return snap / path
    ours = data_dir().parent / "hub" / repo.replace("/", "--") / path
    if ours.is_file():
        return ours
    token = os.environ.get("HF_TOKEN") or os.environ.get("HUGGING_FACE_HUB_TOKEN")
    if not token and (home / "token").is_file():
        token = (home / "token").read_text(encoding="utf-8").strip()
    url = f"https://huggingface.co/{'datasets/' if dataset else ''}{repo}/resolve/main/{path}"
    ask = urllib.request.Request(url, headers={"Authorization": f"Bearer {token}"} if token else {})
    ours.parent.mkdir(parents=True, exist_ok=True)
    part = ours.with_name(f"{ours.name}.{os.getpid()}.part")
    try:
        with urllib.request.urlopen(ask, timeout=60) as answer, open(part, "wb") as out:
            shutil.copyfileobj(answer, out)
    except Exception as e:
        part.unlink(missing_ok=True)
        raise ValueError(f"Could not fetch {path} from {repo} ({e}). The archive is private: sign in with a "
                         "Hugging Face token that can read Kairos-Hive") from e
    part.replace(ours)
    return ours


def built_path(folder):
    """Where the space built from `folder` is kept."""
    folder = pathlib.Path(folder).expanduser().resolve()
    tag = hashlib.blake2b(str(folder).lower().encode(), digest_size=6).hexdigest()
    return data_dir() / f"{folder.name}-{tag}.npz"


def unit(v):
    return v / np.maximum(np.linalg.norm(v, axis=-1, keepdims=True), 1e-12)


def fading(e, next_one):
    """Weights for the photographs handed on, summing to 1, from their nearness `e`. The nearness of
    the next one outside the set is taken off every weight, so a photograph's weight reaches 0 just
    as it leaves the set and a crossfade by these weights never jumps."""
    w = np.clip(np.asarray(e, np.float64) - float(next_one), 0.0, None)
    if w.sum() <= 1e-12:
        w = np.zeros(len(e))
        w[0] = 1.0
    return (w / w.sum()).astype(np.float32)


def slerp(a, b, t):
    """Along the sphere from `a` to `b`, as the lab moves between places, the length eased between."""
    na, nb = float(np.linalg.norm(a)), float(np.linalg.norm(b))
    ua, ub = a / max(na, 1e-12), b / max(nb, 1e-12)
    angle = math.acos(min(max(float(ua @ ub), -1.0), 1.0))
    if angle < 1e-4:
        return (a + t * (b - a)).astype(np.float32)
    v = (math.sin((1 - t) * angle) * ua + math.sin(t * angle) * ub) / math.sin(angle)
    return (v * ((1 - t) * na + t * nb)).astype(np.float32)


def ease(t):
    t = min(max(float(t), 0.0), 1.0)
    return t * t * (3.0 - 2.0 * t)


def loop_order(stops):
    """Near enough the order of the shortest closed loop through the places: nearest neighbour first,
    then any two crossing legs untangled."""
    far = 1 - stops @ stops.T
    order = [0]
    while len(order) < len(stops):
        order.append(min((i for i in range(len(stops)) if i not in order), key=lambda i: far[order[-1], i]))
    better = True
    while better:
        better = False
        for i in range(1, len(order) - 1):
            for j in range(i + 1, len(order)):
                a, b, c, d = order[i - 1], order[i], order[j], order[(j + 1) % len(order)]
                if far[a, c] + far[b, d] < far[a, b] + far[c, d] - 1e-9:
                    order[i:j + 1] = order[i:j + 1][::-1]
                    better = True
    return order


class Space:
    """A lab space: crop vectors `emb`, the photograph each crop is from (`owner`, `files`), and the
    principal axes, spread, centre and typical length of the cloud."""

    def __init__(self, path):
        with np.load(path, allow_pickle=False) as z:
            self.data = {k: z[k] for k in z.files}
        d = self.data
        self.path = str(path)
        self.emb = d["emb"].astype(np.float32)
        self.owner = d["owner"].astype(np.int64)
        self.files = [str(f) for f in d["files"]]
        self.centre = d["centre"].astype(np.float32)
        self.axes = d["axes"].astype(np.float32)
        self.spread = d["spread"].astype(np.float32)
        self.norm = float(d["norm"])
        # A photograph's place: the mean of its crops' vectors.
        sums = np.zeros((len(self.files), self.emb.shape[1]), np.float64)
        np.add.at(sums, self.owner, self.emb)
        counts = np.bincount(self.owner, minlength=len(self.files))[:, None]
        self.has = counts[:, 0] > 0
        self.usable = self.has.copy()  # the photographs that can be handed on: placed, and found on disk
        self.places = unit((sums / np.maximum(counts, 1)).astype(np.float32))
        # A photograph's category: the tag its crops carry.
        self.kind = np.zeros(len(self.files), np.int64)
        if "tag" in d:
            self.kind[self.owner] = d["tag"].astype(np.int64)
        self.maps = {}

    def map_of(self, basis, book=None):
        """The axes [k, d] of a map, their names, and every photograph's coordinates [n, k] in 0..1
        between the archive's 1st and 99th percentiles on each axis. Kept per basis."""
        key = (basis, None if book is None else (str(book), book.stat().st_mtime_ns))
        if key in self.maps:
            return self.maps[key]
        live = self.places[self.has]
        if basis == "principal":
            # The space's own principal axes, strongest first, as the lab's builder fitted them.
            mid = live.mean(0)
            axes = unit(self.axes[:MOST].copy())
            # Each axis points toward its longer tail, so a seed of signs never flips a map.
            proj = (live - mid) @ axes.T
            axes *= np.where(np.abs(np.percentile(proj, 99, 0)) >= np.abs(np.percentile(proj, 1, 0)), 1, -1)[:, None]
            names = [f"axis {i + 1}" for i in range(len(axes))]
        elif basis == "concepts":
            if book is None:
                raise ValueError("A concepts map needs a concept book. Set walk/concepts to a concepts.json")
            with open(book, encoding="utf-8") as f:
                found = json.load(f)
            names = [n for n, c in found.items() if isinstance(c, dict) and len(c.get("direction", [])) == live.shape[1]]
            if not names:
                raise ValueError(f"{book.name} holds no direction of {live.shape[1]} numbers, as this space has")
            names = names[:MOST]
            axes = unit(np.array([found[n]["direction"] for n in names], np.float32))
        else:
            raise ValueError(f"walk/basis must be one of {', '.join(BASES)}")
        proj = self.places @ axes.T
        lo, hi = np.percentile(proj[self.has], 1, 0), np.percentile(proj[self.has], 99, 0)
        span = np.maximum(hi - lo, 1e-6)
        made = {"axes": axes, "names": names, "lo": lo, "span": span,
                "coords": ((proj - lo) / span).astype(np.float32)}
        self.maps[key] = made
        return made

    def on_map(self, m, position, count, reach):
        """The `count` photographs nearest `position` on the map `m`, their weights, and the place.

        Every photograph on disk is weighted by a Gaussian of its distance on the map, `reach` wide.
        The place is the weighted mean of all their places, so it moves continuously with the position,
        then moved so that on every map axis it sits at `position`. The pictures handed on are the
        `count` heaviest, their weights brought to sum to 1."""
        k = len(position)
        pos = np.asarray(position, np.float32)
        d = np.linalg.norm(m["coords"][:, :k] - pos, axis=1)
        d[~self.usable] = np.inf
        order = np.argsort(d)
        idx = order[:max(1, count)]
        z = -(d ** 2 - d[idx[0]] ** 2) / (2.0 * max(reach, 1e-4) ** 2)
        every = np.where(self.usable, np.exp(np.minimum(z, 0.0)), 0.0)
        w = fading(every[idx], every[order[len(idx)]] if len(order) > len(idx) else 0.0)
        v = (every[:, None] * self.places).sum(0) / every.sum()
        # Least squares on the map's axes: the smallest step that puts the place at the position.
        A = m["axes"][:k].astype(np.float64)
        want = m["lo"][:k] + pos * m["span"][:k]
        step = A.T @ np.linalg.solve(A @ A.T + 1e-9 * np.eye(k), want - A @ v)
        return [int(i) for i in idx], w, (v + step).astype(np.float32)

    def on_disk(self, idx):
        """The photographs `idx` with each one missing from disk replaced by the nearest that is
        there, each kept once, in order. Also how many were replaced."""
        out, swapped = [], 0
        for i in idx:
            if not self.usable[i]:
                sims = self.places @ self.places[i]
                sims[~self.usable] = -np.inf
                sims[out] = -np.inf
                if not np.isfinite(sims).any():
                    continue
                i, swapped = int(sims.argmax()), swapped + 1
            if i not in out:
                out.append(int(i))
        return out, swapped

    def typical(self, count, seed):
        """`count` photographs, each the most typical of one region of the space, in loop order."""
        live = np.flatnonzero(self.has)  # the tour picks from the whole archive, as the lab does
        count = max(1, min(count, len(live)))
        rng = np.random.default_rng(seed)
        places = self.places[live]
        middles = places[rng.choice(len(places), count, replace=False)]
        for _ in range(30):
            near = (places @ middles.T).argmax(1)
            middles = unit(np.stack([places[near == k].mean(0) if (near == k).any() else middles[k] for k in range(count)]))
        picked = list(dict.fromkeys(int(live[(places @ m).argmax()]) for m in middles))
        order = loop_order(self.places[picked]) if len(picked) > 2 else range(len(picked))
        return [picked[i] for i in order]

    def point(self, coords):
        """The centre moved along the principal axes, `coords` in units of the spread on each."""
        v = self.centre.astype(np.float64).copy()
        for i, c in enumerate(coords[: len(self.axes)]):
            v += float(c) * float(self.spread[i]) * self.axes[i]
        return v.astype(np.float32)

    def nearest(self, v, count, sharp):
        """The `count` photographs nearest the place `v`, and how near: softmax of the cosine over
        `sharp`, so a small `sharp` gives the nearest nearly all of it."""
        sims = self.places @ unit(v)
        sims[~self.usable] = -np.inf
        order = np.argsort(-sims)
        idx = order[:max(1, count)]
        z = (sims[order[:len(idx) + 1]] - sims[idx].max()) / max(sharp, 1e-4)
        e = np.exp(z)
        w = fading(e[:len(idx)], e[len(idx)] if len(e) > len(idx) else 0.0)
        return [int(i) for i in idx], w, sims[idx]


def picture(value, name):
    """A picture [h, w, 3] in 0..1 from whatever came down the wire: grey, RGBA, 0..255, or the first
    of a stack."""
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


def pairs_of(k):
    """The pairs of axes a k-axis map is drawn as: 1 and 2, 3 and 4, an odd last one with the first."""
    pairs = [(a, a + 1) for a in range(0, k - 1, 2)] + ([(k - 1, 0)] if k % 2 and k > 1 else [])
    return pairs or [(0, 0)]


def panel_at(n, cols, c):
    """Pixel x, y of map coordinates `c` (a pair in 0..1) on panel n."""
    inner = PANEL - 24
    ox, oy = (n % cols) * PANEL, (n // cols) * PANEL
    return (ox + 12 + float(np.clip(c[0], -0.05, 1.05)) * inner,
            oy + 12 + (1.0 - float(np.clip(c[1], -0.05, 1.05))) * inner)


_BACKS = {}


def map_back(sp, m, k):
    """The still part of the map picture: frames, names, and every photograph as a dot in its
    category's colour, dim when only placed and bright when it is on disk. Kept per map."""
    from PIL import Image, ImageDraw

    key = (id(sp), id(m), k, int(sp.usable.sum()))
    if key in _BACKS:
        return _BACKS[key]
    pairs = pairs_of(k)
    cols = min(len(pairs), 2)
    rows = math.ceil(len(pairs) / cols)
    im = Image.new("RGB", (cols * PANEL, rows * PANEL), (14, 14, 16))
    pen = ImageDraw.Draw(im)
    inner = PANEL - 24
    placed = np.flatnonzero(sp.has & ~sp.usable)
    found = np.flatnonzero(sp.usable)
    for n, (a, b) in enumerate(pairs):
        ox, oy = (n % cols) * PANEL, (n // cols) * PANEL
        pen.rectangle([ox + 12, oy + 12, ox + 12 + inner, oy + 12 + inner], outline=(48, 48, 54))
        for i in placed:
            pen.point(panel_at(n, cols, m["coords"][i, [a, b]]),
                      fill=tuple(int(c) for c in HUES[sp.kind[i] % len(HUES)] * 130))
        for i in found:
            x, y = panel_at(n, cols, m["coords"][i, [a, b]])
            pen.ellipse([x - 2, y - 2, x + 2, y + 2], fill=tuple(int(c) for c in HUES[sp.kind[i] % len(HUES)] * 240))
        pen.text((ox + 14, oy + PANEL - 12), f"{m['names'][a]} >", fill=(170, 170, 180))
        pen.text((ox + 2, oy + 1), f"^ {m['names'][b]}", fill=(170, 170, 180))
    if len(_BACKS) > 8:
        _BACKS.clear()
    _BACKS[key] = im
    return im


def draw_map(sp, m, position, idx):
    """The map as panels, one per pair of axes: the still dots, the photographs handed on ringed, and
    the position a white cross. RGB [h, w, 3] in 0..1."""
    from PIL import ImageDraw

    k = len(position)
    im = map_back(sp, m, k).copy()
    pen = ImageDraw.Draw(im)
    pairs = pairs_of(k)
    cols = min(len(pairs), 2)
    for n, (a, b) in enumerate(pairs):
        for i in idx:
            x, y = panel_at(n, cols, m["coords"][i, [a, b]])
            pen.ellipse([x - 5, y - 5, x + 5, y + 5], outline=(255, 210, 90))
        x, y = panel_at(n, cols, (position[a], position[b]))
        pen.line([x - 7, y, x + 7, y], fill=(255, 255, 255), width=2)
        pen.line([x, y - 7, x, y + 7], fill=(255, 255, 255), width=2)
    return np.asarray(im, np.float32) / 255.0


def frame_size(frame, long):
    a, b = (int(x) for x in frame.split(":"))
    return (long, max(16, round(long * b / a))) if a >= b else (max(16, round(long * a / b)), long)


def photograph(path, w, h):
    """A photograph cropped about its centre to w x h and resized, RGB in 0..1."""
    from PIL import Image

    with Image.open(path) as im:
        im.draft("RGB", (w * 2, h * 2))
        im = im.convert("RGB")
    iw, ih = im.size
    if iw * h > ih * w:
        keep = round(ih * w / h)
        im = im.crop(((iw - keep) // 2, 0, (iw - keep) // 2 + keep, ih))
    else:
        keep = round(iw * h / w)
        im = im.crop((0, (ih - keep) // 2, iw, (ih - keep) // 2 + keep))
    return np.asarray(im.resize((w, h), Image.LANCZOS), np.float32) / 255.0


class MaterialSpace(goofi.Node):
    """Move through a set of photographs as the IP-Adapter's eye places them, and hand on the photographs."""

    TAGS = ["image", "ml", "generator"]
    INPUTS = {"place": goofi.InputSlot(goofi.DataType.ARRAY)}
    OUTPUTS = {"pictures": goofi.DataType.ARRAY, "picture": goofi.DataType.ARRAY, "weights": goofi.DataType.ARRAY,
               "place": goofi.DataType.ARRAY, "map": goofi.DataType.ARRAY,
               "status": goofi.DataType.STRING}
    PARAMS = {
        "space": {
            "space": goofi.StringParam("", doc="A space file (.npz) from the lab's ip_space.py or from MaterialEncoder. "
                                       "Left empty, the node uses the space MaterialEncoder built from `folder`, and "
                                       "with no folder either the four-star archive's own, loaded by itself from "
                                       "Hugging Face with its photographs."),
            "folder": goofi.StringParam("", doc="The folder of photographs a MaterialEncoder built a space from."),
            "photos": goofi.StringParam("", doc="Where the photographs are now, when they have moved since the space "
                                        "was built. Files are looked up here by name."),
        },
        "walk": {
            "mode": goofi.StringParam("tour", MODES, doc="Tour hands on the space's most typical photographs in loop "
                                      "order. Near hands on the photographs nearest a place, with their weights. Map "
                                      "lays the archive out on `dims` axes and hands on the photographs nearest "
                                      "`position`."),
            "basis": goofi.StringParam("principal", BASES, doc="The axes of the map. principal takes the main axes of "
                                       "the photographs' places, strongest first. concepts takes the named directions "
                                       "of a concept book, one axis each.", show=("mode", ["map"])),
            "dims": goofi.NumParam(4, 1, MOST, int=True, doc="How many axes the map has. axis1 to axis`dims` are "
                                   "read.", show=("mode", ["map"])),
            **{f"axis{i + 1}": goofi.NumParam(0.5, 0.0, 1.0, doc=f"Where you stand on the map's axis {i + 1}. 0 and 1 "
                                              "are the archive's two ends on it, 0.5 its middle, and values past "
                                              "them carry on. The map output names each axis.",
                                              show=("mode", ["map"])) for i in range(MOST)},
            "wander": goofi.NumParam(0.0, 0.0, 0.5, doc="How far the position drifts by itself around the axes, in "
                                     "map units. Each axis swings on its own slow curve, `pace` swings a minute.",
                                     show=("mode", ["map"])),
            "pace": goofi.NumParam(0.0, 0.0, 30.0, doc="How fast the walk moves by itself. In a tour, stops a minute. "
                                   "On a map, swings a minute of the wander. 0 holds still.",
                                   show=("mode", ["tour", "map"])),
            "reach": goofi.NumParam(0.08, 0.005, 0.5, doc="How far around the position the weight spreads, in map "
                                    "units. Small gives the nearest photograph nearly all of it.", show=("mode", ["map"])),
            "transition": goofi.NumParam(0.0, 0.0, 20.0, doc="Seconds a change takes. The map's axes glide along an "
                                         "eased curve, and a change of mode, basis, dims, stops, seed, count or blend "
                                         "crossfades the picture and slerps the place. At 0 a change lands at once."),
            "blend": goofi.StringParam("blend", ["blend", "nearest"], doc="What `picture` is. blend crossfades the "
                                       "photographs handed on by their weights, and a tour's stops into each other. "
                                       "nearest hands on one photograph."),
            "concepts": goofi.StringParam("", doc="A concept book (concepts.json) for a concepts map. Left empty, the "
                                          "node reads concepts.json beside the space file, then latent-lab's own.",
                                          show=("mode", ["map"])),
            "stops": goofi.NumParam(6, 2, 32, int=True, doc="How many regions the tour visits, one photograph each.",
                                    show=("mode", ["tour"])),
            "seed": goofi.NumParam(0, 0, 9999, int=True, doc="Another seed, another division into regions.",
                                   show=("mode", ["tour"])),
            "at": goofi.NumParam(0.0, 0.0, 1.0, doc="Where on the tour `picture` and `place` stand, as a share of the "
                                 "loop. Between two stops they blend from one to the next. Values past 1 go round "
                                 "again, and `pace` carries it on by itself.", show=("mode", ["tour"])),
            "axes": goofi.NumParam([0.0, 0.0, 0.0, 0.0], -3.0, 3.0, doc="The place, as the space's centre moved along "
                                   "its four main axes in units of the spread on each. A wired `place` "
                                   "takes over.", show=("mode", ["near"])),
            "count": goofi.NumParam(4, 1, 8, int=True, doc="How many of the nearest photographs are handed on.",
                                    show=("mode", ["near", "map"])),
            "sharp": goofi.NumParam(0.02, 0.002, 0.2, doc="How slowly a photograph's weight falls with distance. Small "
                                    "gives the nearest nearly all of it.", show=("mode", ["near"])),
        },
        "frame": {
            "frame": goofi.StringParam("3:2", FRAMES, doc="The shape every photograph is cropped to."),
            "long": goofi.NumParam(512, 128, 1024, int=True, doc="Pixels on the long side of every photograph handed on."),
        },
        "common": {"autotrigger": goofi.BoolParam(True), "max_frequency": goofi.NumParam(10.0, 0.5, 60.0)},
    }

    def setup(self):
        self.space = None
        self.archive = False
        self.loaded = None
        self.photos = {}
        self.key = None
        self.result = None
        self.sent = 0.0
        self.glide = None  # the position: where it set off, where it goes, and when it left
        self.landed = True
        self.indexed = None  # the `photos` folder the photographs on disk were last looked up in
        self.where = {}
        self.clock = None  # the last tick, and how far `pace` has carried the tour, in stops
        self.travelled = 0.0
        self.choice = None  # the choices as last set, and the crossfade a change of them started
        self.fade = None
        self.shown = None  # the picture and place last handed on

    def book(self, path):
        """The concept book: walk/concepts, else concepts.json beside the space, else latent-lab's."""
        given = self.params.walk.concepts.strip()
        lab = os.environ.get("LATENT_LAB", pathlib.Path(__file__).resolve().parents[3] / "latent-lab")
        here = pathlib.Path(lab).expanduser() / "concepts.json"
        for c in ([pathlib.Path(given).expanduser()] if given else [path.parent / "concepts.json", here]):
            if c.is_file():
                return c
        return None

    def position(self, now):
        """The position as it stands at `now`, gliding toward walk/position over walk/transition."""
        p = self.params.walk
        k = max(1, min(int(p.dims), MOST))
        to = tuple(float(getattr(p, f"axis{i + 1}")) for i in range(k))
        seconds = min(max(float(p.transition), 0.0), 60.0)
        if self.glide is None or len(self.glide[1]) != k or seconds <= 0:
            self.glide = (to, to, now - seconds)
        elif to != self.glide[1]:
            self.glide = (self.standing(now, seconds), to, now)
        moving = seconds > 0 and now - self.glide[2] < seconds
        return self.standing(now, seconds), moving

    def wandered(self, position, now):
        """The position moved by `wander`, each axis on two slow sines of its own, `pace` swings a
        minute, phases set by the seed."""
        p = self.params.walk
        amount, pace = float(p.wander), float(p.pace)
        if amount <= 0 or pace <= 0:
            return position, False
        rng = np.random.default_rng(int(p.seed) + 7)
        phase = rng.uniform(0, 2 * math.pi, (MOST, 2))
        f = pace / 60.0 * 2 * math.pi
        out = []
        for j, x in enumerate(position):
            a, b = 1.0 + 0.37 * j, 1.618 * (1.0 + 0.21 * j)
            out.append(x + amount * (0.6 * math.sin(f * a * now + phase[j, 0]) + 0.4 * math.sin(f * b * now + phase[j, 1])))
        return tuple(out), True

    def standing(self, now, seconds):
        a, b, t0 = self.glide
        t = 1.0 if seconds <= 0 else min(max((now - t0) / seconds, 0.0), 1.0)
        t = t * t * (3.0 - 2.0 * t)
        return tuple(x + (y - x) * t for x, y in zip(a, b))

    def source(self):
        """The space file: the one named, the one built from `folder`, else the four-star archive's,
        loaded by itself from Hugging Face."""
        self.archive = False
        given = self.params.space.space.strip()
        if given:
            return pathlib.Path(given).expanduser()
        folder = self.params.space.folder.strip()
        if folder:
            return built_path(folder)
        self.archive = True
        return hub_file(*ARCHIVE["space"])

    def find_all(self, path):
        """Every photograph of the space that is on disk, where the space says or in `photos` by its
        name, or by a name that ends with its own (a copy numbered in front, `03_00259_DSC09814.jpg`).
        Only these are handed on. Read once per space and per `photos`."""
        sp = self.space
        given = self.params.space.photos.strip()
        by_name = {}
        if given and pathlib.Path(given).is_dir():
            for f in pathlib.Path(given).rglob("*"):
                if f.is_file():
                    parts = f.name.split("_")
                    for k in range(len(parts)):  # the name, and the name with each numbered prefix taken off
                        by_name.setdefault("_".join(parts[k:]).lower(), f)
        self.where = {}
        if self.archive and not given:
            # The archive's photographs are all to hand: each is fetched the first time it is handed on.
            sp.usable = sp.has.copy()
            return
        for i, name in enumerate(sp.files):
            path = pathlib.Path(name)
            for c in (path, pathlib.Path(sp.path).parent / path):
                if c.is_file():
                    self.where[i] = c
                    break
            else:
                if path.name.lower() in by_name:
                    self.where[i] = by_name[path.name.lower()]
        sp.usable = sp.has & np.isin(np.arange(len(sp.files)), list(self.where))

    def file_of(self, i):
        if i not in self.where and self.archive and not self.params.space.photos.strip():
            repo, folder = ARCHIVE["photos"]
            self.where[i] = hub_file(repo, f"{folder}/{pathlib.Path(self.space.files[i]).name}", dataset=True)
        if i not in self.where:
            raise ValueError(f"No photograph at {self.space.files[i]}. Set space/photos to where the set is now")
        return self.where[i]

    def photo(self, i, w, h):
        key = (i, w, h, self.params.space.photos)
        if key not in self.photos:
            if len(self.photos) > 96:
                self.photos.pop(next(iter(self.photos)))
            self.photos[key] = photograph(self.file_of(i), w, h)
        return self.photos[key]

    def process(self, place=None):
        path = self.source()
        if path is None:
            return {"status": "Set space/space to a space file, or space/folder to a folder of photographs"}
        if not path.is_file():
            return {"status": f"no space at {path.name}. Build it with a MaterialEncoder on the same folder"}
        stamp = (str(path), path.stat().st_mtime_ns)
        if self.loaded != stamp:
            self.space = Space(path)
            self.loaded = stamp
            self.photos = {}
            self.key = None
            self.indexed = None
        if self.indexed != self.params.space.photos:
            self.find_all(path)
            self.indexed = self.params.space.photos
            self.key = None
        if not self.space.usable.any():
            return {"status": f"none of the {len(self.space.files)} photographs of this space is on disk. Set "
                              "space/photos to the folder that holds them"}
        p = self.params
        v = None if place is None else np.asarray(place.data, np.float32).reshape(-1)
        if v is not None and v.size != self.space.emb.shape[1]:
            raise ValueError(f"`place` takes a vector of {self.space.emb.shape[1]} numbers, as this space holds. "
                             f"This one has {v.size}")
        key = (stamp, repr(vars(p.walk)), repr(vars(p.frame)), p.space.photos,
               None if v is None else hashlib.blake2b(v.tobytes()).hexdigest())
        now = time.monotonic()
        dt = 0.0 if self.clock is None else min(max(now - self.clock, 0.0), 1.0)
        self.clock = now
        seconds = min(max(float(p.walk.transition), 0.0), 60.0)
        moving = False
        if p.walk.mode == "map":
            position, moving = self.position(now)
            position, drifting = self.wandered(position, now)
            moving = moving or drifting
        elif p.walk.mode == "tour" and float(p.walk.pace) > 0:
            self.travelled += dt * float(p.walk.pace) / 60.0
            moving = True
        choice = (p.walk.mode, p.walk.basis, int(p.walk.dims), int(p.walk.stops), int(p.walk.seed),
                  int(p.walk.count), p.walk.blend, p.frame.frame, int(p.frame.long), str(path), p.space.photos)
        if self.choice is not None and choice != self.choice and seconds > 0 and self.shown is not None:
            self.fade = (self.shown[0], self.shown[1], now)
        self.choice = choice
        if self.fade is not None and now - self.fade[2] >= seconds:
            self.fade = None
        moving = moving or self.fade is not None
        if key == self.key and self.landed and not moving:
            if self.result is not None and time.monotonic() - self.sent >= 1.0:
                self.sent = time.monotonic()
                return self.result
            return None
        self.key = key
        self.landed = not moving
        w, h = frame_size(p.frame.frame, int(p.frame.long))
        sp = self.space
        drawn = None
        if p.walk.mode == "map":
            m = sp.map_of(p.walk.basis, self.book(path) if p.walk.basis == "concepts" else None)
            position = position[:len(m["axes"])]
            idx, weights, where = sp.on_map(m, position, int(p.walk.count), float(p.walk.reach))
            mix = weights
            drawn = np.ascontiguousarray(draw_map(sp, m, position, idx))
            how = (f"map of {len(position)} ({p.walk.basis}) at "
                   + " ".join(f"{n} {x:.2f}" for n, x in zip(m["names"], position))
                   + (" | wandering" if drifting else "") + " | " + " ".join(f"{wi:.2f}" for wi in weights))
        elif p.walk.mode == "tour":
            idx, swapped = sp.on_disk(sp.typical(int(p.walk.stops), int(p.walk.seed)))
            weights = np.full(len(idx), 1.0 / len(idx), np.float32)
            n = len(idx)
            at = (float(p.walk.at) + self.travelled / n) % 1.0 * n
            a, t = int(at) % n, ease(at - int(at))
            b = (a + 1) % n
            where = slerp(sp.places[idx[a]], sp.places[idx[b]], t)
            mix = np.zeros(n, np.float32)
            mix[a] += 1.0 - t
            mix[b] += t
            how = f"tour of {n}, at {at / n:.3f}, stop {a + 1} to {b + 1}" + (
                f", {swapped} stops off disk taken by their nearest" if swapped else "")
        elif p.walk.mode == "near":
            if v is not None:
                where = v
                how = "near the wired place"
            else:
                coords = p.walk.axes if isinstance(p.walk.axes, (list, tuple)) else [p.walk.axes]
                where = sp.point([float(c) for c in coords])
                how = "near " + " ".join(f"{float(c):+.2f}" for c in coords)
            idx, weights, sims = sp.nearest(where, int(p.walk.count), float(p.walk.sharp))
            mix = weights
            how += " | " + " ".join(f"{wi:.2f}" for wi in weights)
        else:
            raise ValueError(f"walk/mode must be one of {', '.join(MODES)}")
        stack = np.ascontiguousarray(np.stack([self.photo(i, w, h) for i in idx]))
        if drawn is None:  # tour and near: where their place falls on the principal map
            m = sp.map_of("principal")
            k = max(1, min(int(p.walk.dims), MOST))
            at = (m["axes"][:k] @ unit(np.asarray(where, np.float32)) - m["lo"][:k]) / m["span"][:k]
            drawn = np.ascontiguousarray(draw_map(sp, m, tuple(float(x) for x in at), idx))
        where = (unit(np.asarray(where, np.float32)) * sp.norm).astype(np.float32)
        if p.walk.blend == "nearest":
            one = int(np.argmax(mix))
            shown = stack[one].copy()
        else:
            one = int(np.argmax(mix))
            shown = np.tensordot(np.asarray(mix, np.float32), stack, axes=1).astype(np.float32)
        if self.fade is not None and self.fade[0].shape == shown.shape:
            t = ease((now - self.fade[2]) / seconds)
            shown = self.fade[0] + t * (shown - self.fade[0])
            where = (unit(slerp(self.fade[1], where, t)) * sp.norm).astype(np.float32)
            how += " | in transition"
        self.landed = not moving
        self.shown = (shown, where)
        names = [pathlib.Path(sp.files[i]).stem for i in idx]
        self.result = {
            "pictures": (stack, {"space": {"files": [sp.files[i] for i in idx]}}),
            "picture": (np.ascontiguousarray(np.clip(shown, 0.0, 1.0)), {"channels": {"dim2": ["r", "g", "b"]},
                                                                         "space": {"file": sp.files[idx[one]]}}),
            "weights": (np.ascontiguousarray(weights, dtype=np.float32), {"space": {"names": names}}),
            "place": (where, {"space": {"path": sp.path}}),
            "status": f"{how} | {', '.join(names)} | {int(sp.usable.sum())} of {int(sp.has.sum())} on disk",
        }
        self.result["map"] = (drawn, {"channels": {"dim2": ["r", "g", "b"]}})
        self.sent = time.monotonic()
        return self.result
