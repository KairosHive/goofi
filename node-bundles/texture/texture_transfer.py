"""TextureTransfer carries a picture into a texture where each output pixel depends on the whole frame.

latent-lab's texture mapping (`composite.py`), the three modes a single shader pass cannot do. Each
needs the whole picture at once (a Poisson solve, a guided filter's box sums, a stroke traced through
the frame), so they run here on the CPU, in goofi's own free-threaded Python. The modes that work
pixel by pixel run on the GPU with the graphics nodes (README, "From TextureMap").

- blend. A mixed-gradient Poisson solve. At each pixel the stronger edge wins, the picture's or the
  texture's, and the image is rebuilt from those edges. `wmix` leans the contest toward the picture.
- guided. The texture's tones poured into the picture's forms by a local linear fit, held at the
  picture's edges. `radius` sets the size of each fit, `eps` how strong an edge must be to hold
  the texture back, and `detail` how much of the texture's own grain comes back on top.
- flow. The texture combed along the picture's contours in `steps` moves over `length`, its grain
  kept. `smear` averages the stroke into a soft brush, and `reach` softens the picture first.

A wired `mask` (AdaptiveMask, Segment, any map in 0..1) says where the material goes, and outside it
the picture keeps its own pixels. `zoom` and `centre` move the material under the picture, and past
0..1 `centre` carries on in mirror, so a signal on it drifts the material through the frame. Pixel
knobs are set in a 384 px frame and scale with the frame they run in.
"""

import hashlib
import time
from concurrent.futures import ThreadPoolExecutor

import numpy as np
from PIL import Image
from scipy import fft as sfft
from scipy import ndimage

import goofi

REF = 384
LUM = np.array([0.299, 0.587, 0.114], np.float32)
MODES = ["blend", "guided", "flow"]
WORK = [0, 256, 384, 512, 768, 1024]
RESEND = 1.0  # seconds: an unchanged picture is sent again this often, so a viewer opened later sees it
RESAMPLE = {"area": Image.BOX, "bilinear": Image.BILINEAR, "bicubic": Image.BICUBIC}
POOL = ThreadPoolExecutor(max_workers=3, thread_name_prefix="texture-transfer")


def channels(fn, a):
    """`fn` on each channel of a picture [h, w, c] at once, stacked back: goofi runs this node in its
    free-threaded Python, where the threads run side by side."""
    return np.stack(list(POOL.map(lambda c: fn(a[..., c]), range(a.shape[2]))), -1)


def resize(a, h, w, mode="bilinear"):
    """A field [h, w] or a picture [h, w, c] at another size, as PIL resamples (antialiased when
    shrinking)."""
    if a.shape[:2] == (h, w):
        return a
    if a.ndim == 3:
        return channels(lambda x: resize(x, h, w, mode), a)
    im = Image.fromarray(np.ascontiguousarray(a, dtype=np.float32), "F")
    return np.asarray(im.resize((w, h), RESAMPLE[mode]), np.float32)


def blur(a, sigma):
    """A Gaussian on the even extension, a field [h, w] or a picture [h, w, c]. A wide one runs on a
    shrunk copy and comes back up bicubic, so a slope read from it has no steps."""
    if sigma <= 0:
        return np.array(a, np.float32, copy=True)
    h, w = a.shape[:2]
    if a.ndim == 3:
        return channels(lambda x: blur(x, sigma), a)
    if sigma > 6 and min(h, w) > 64:
        f = int(min(sigma // 3, 16))
        small = resize(a, max(8, h // f), max(8, w // f), "area")
        return resize(blur(small, sigma / f), h, w, "bicubic")
    return ndimage.gaussian_filter(np.asarray(a, np.float32), sigma, mode="reflect", truncate=4.0)


def lum(img):
    return img @ LUM


def sample(tex, X, Y):
    """The texture read at fractional pixel coordinates, bilinear, held at its edge."""
    h, w = tex.shape[:2]
    at = (np.clip(Y, 0.0, h - 1.001), np.clip(X, 0.0, w - 1.001))
    return channels(lambda x: ndimage.map_coordinates(x, at, order=1, mode="nearest"), tex)


def frame_of(tex, h, w, zoom, centre):
    """The material under a frame [h, w]: covering it at zoom 1, magnified above, repeated in mirror
    below, its middle at `centre` (0..1 across the photograph)."""
    th, tw = tex.shape[:2]
    if tw * h > th * w:  # the piece of the photograph the frame covers at zoom 1, as a share of each side
        sx, sy = (th * w / h) / tw, 1.0
    else:
        sx, sy = 1.0, (tw * h / w) / th
    sx, sy = sx / zoom, sy / zoom
    if sx * tw > w * 1.5 and sy * th > h * 1.5:  # read from a copy near the frame's own scale, not aliased
        k = min(sx * tw / w, sy * th / h)
        tex = resize(tex, max(2, round(th / k)), max(2, round(tw / k)), "area")
        th, tw = tex.shape[:2]
    u = (np.arange(w, dtype=np.float32) + 0.5) / w - 0.5
    v = (np.arange(h, dtype=np.float32) + 0.5) / h - 0.5
    xs = (centre[0] + u * sx) * tw - 0.5
    ys = (centre[1] + v * sy) * th - 0.5
    Y, X = np.meshgrid(ys, xs, indexing="ij")
    return channels(lambda x: ndimage.map_coordinates(x, (Y, X), order=1, mode="reflect"), tex).astype(np.float32)


# ---- the modes: content c and texture t, [h, w, 3] in 0..1 (latent-lab's composite.py) ----------

def solve(gx, gy):
    """An image whose gradients are these, by FFT Poisson on the even extension."""
    h, w = gx.shape
    div = (gx - np.roll(gx, 1, axis=1)) + (gy - np.roll(gy, 1, axis=0))
    big = np.block([[div, div[:, ::-1]], [div[::-1, :], div[::-1, ::-1]]])
    H, W = big.shape
    ky = 2.0 * np.cos(2.0 * np.pi * np.arange(H) / H) - 2.0
    kx = 2.0 * np.cos(2.0 * np.pi * np.arange(W // 2 + 1) / W) - 2.0  # a real input needs half the spectrum
    denom = ky[:, None] + kx[None, :]
    denom[0, 0] = 1.0
    hat = sfft.rfft2(big, workers=-1) / denom
    hat[0, 0] = 0.0
    return sfft.irfft2(hat, s=(H, W), workers=-1)[:h, :w].astype(np.float32)


def diff(a):
    return np.diff(a, axis=1, append=a[:, -1:]), np.diff(a, axis=0, append=a[-1:, :])


def blend(c, t, wmix):
    gcx, gcy = diff(lum(c))
    gtx, gty = diff(lum(t))
    pick = (wmix * np.hypot(gcx, gcy)) >= ((1.0 - wmix) * np.hypot(gtx, gty))
    out = []
    for ch in range(3):
        cx, cy = diff(c[..., ch])
        tx, ty = diff(t[..., ch])
        u = solve(np.where(pick, cx, tx), np.where(pick, cy, ty))
        out.append(u + wmix * c[..., ch].mean() + (1 - wmix) * t[..., ch].mean())
    return np.stack(out, -1)


def flow(c, t, length, reach, smear, steps):
    h, w = c.shape[:2]
    gy, gx = np.gradient(blur(lum(c), reach))
    mag = np.hypot(gx, gy) + 1e-8
    tx, ty = -gy / mag, gx / mag
    Y, X = np.mgrid[0:h, 0:w].astype(np.float32)
    step = length * max(h, w) / max(steps, 1)
    px, py = X.copy(), Y.copy()
    acc = t.copy()
    for _ in range(steps):
        ix = np.clip(px, 0, w - 1).astype(np.int32)
        iy = np.clip(py, 0, h - 1).astype(np.int32)
        px += tx[iy, ix] * step
        py += ty[iy, ix] * step
        acc += sample(t, px, py)
    return (1.0 - smear) * sample(t, px, py) + smear * (acc / (steps + 1))


def guided(c, t, radius, eps, detail):
    guide = lum(c)
    s = max(float(radius), 1.0) / 2.0
    m_i = blur(guide, s)
    v_i = blur(guide * guide, s) - m_i * m_i
    m_p, gp = blur(t, s), blur(guide[..., None] * t, s)
    a = (gp - m_i[..., None] * m_p) / (v_i[..., None] + eps)
    b = m_p - a * m_i[..., None]
    return blur(a, s) * guide[..., None] + blur(b, s) + detail * (t - m_p)


# ---- what comes down the wires ----------------------------------------------------------------

def plane(value, name):
    """A mask [h, w] in 0..1: a picture's first channel if it has several, channels last [h, w, c] or
    first [c, h, w], the first of a stack, a mask in 0..255 brought to 0..1."""
    x = np.asarray(value)
    if x.ndim == 4:
        x = x[0]
    if x.ndim == 3:
        x = x[0] if x.shape[0] in (1, 3, 4) and x.shape[-1] not in (1, 3, 4) else x[..., 0]
    if x.ndim != 2:
        raise ValueError(f"`{name}` takes a map [height, width]. This frame is {list(x.shape)}")
    x = x.astype(np.float32, copy=False)
    return np.nan_to_num(x / 255.0 if x.max(initial=0.0) > 1.5 else x)


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


def fingerprint(x):
    if x is None:
        return None
    a = np.ascontiguousarray(x).reshape(-1)
    step = max(1, a.size // 65536)
    return x.shape, hashlib.blake2b(np.ascontiguousarray(a[::step]).tobytes(), digest_size=12).hexdigest()


def clamp(v, lo, hi):
    return min(max(float(v), lo), hi)


class TextureTransfer(goofi.Node):
    """Rebuild the picture from the material's pixels, where the mask says, by a whole-frame method."""

    TAGS = ["image", "transform"]
    INPUTS = {
        "image": goofi.InputSlot(goofi.DataType.ARRAY, required=True),
        "texture": goofi.InputSlot(goofi.DataType.ARRAY, required=True),
        "mask": goofi.InputSlot(goofi.DataType.ARRAY),
    }
    OUTPUTS = {"image": goofi.DataType.ARRAY, "status": goofi.DataType.STRING}
    PARAMS = {
        "transfer": [
            {
                "mode": goofi.StringParam("guided", MODES, doc="blend rebuilds the image from whichever edge is "
                                          "stronger at each pixel. guided pours the texture's tones into the "
                                          "picture's forms. flow combs the texture along the picture's contours."),
            },
            {
                "wmix": goofi.NumParam(0.65, 0.0, 1.0, doc="Which edges win. At 0 the texture's win everywhere, at 1 "
                                       "the picture's.", show=("mode", ["blend"])),
                "radius": goofi.NumParam(14.0, 2.0, 60.0, doc="How big an area each local fit covers, in pixels of a "
                                         "384 px frame. Larger lets the texture's tones spread further.",
                                         show=("mode", ["guided"])),
                "eps": goofi.NumParam(0.015, 0.002, 0.08, doc="How strong an edge of the picture must be to hold the "
                                      "texture back. Higher lets the texture run over weaker edges.",
                                      show=("mode", ["guided"])),
                "detail": goofi.NumParam(0.35, 0.0, 1.0, doc="How much of the texture's own grain comes back on top.",
                                         show=("mode", ["guided"])),
                "length": goofi.NumParam(0.06, 0.0, 0.12, doc="How far the texture is combed along the picture's "
                                         "contours, as a share of the frame.", show=("mode", ["flow"])),
                "smear": goofi.NumParam(0.25, 0.0, 1.0, doc="0 combs sharply and keeps the grain, 1 smears softly.",
                                        show=("mode", ["flow"])),
                "steps": goofi.NumParam(8, 3, 16, int=True, doc="How many moves a stroke is made of. More curves "
                                        "more smoothly and costs more.", show=("mode", ["flow"])),
                "reach": goofi.NumParam(3.0, 1.0, 10.0, doc="How much the picture is softened before its contours "
                                        "are read, in pixels of a 384 px frame.", show=("mode", ["flow"])),
            },
        ],
        "material": {
            "zoom": goofi.NumParam(1.0, 0.25, 8.0, doc="At 1 the photograph covers the frame. Above 1 a piece of it is "
                                   "magnified, and below 1 it repeats, mirrored."),
            "centre": goofi.NumParam([0.5, 0.5], -4.0, 4.0, doc="The point of the photograph at the frame's middle. Past "
                                     "0..1 it carries on in mirror, so a signal can drift it without end."),
        },
        "mix": {
            "amount": goofi.NumParam(1.0, 0.0, 1.0, doc="How much of the transfer replaces the picture, inside the mask."),
        },
        "frame": {
            "work": goofi.NumParam(512, 0, 1024, int=True, options=WORK, doc="Pixels on the long side the picture is "
                                   "worked at and comes out at, 0 for its own size."),
        },
        "common": {"autotrigger": goofi.BoolParam(True), "max_frequency": goofi.NumParam(30.0, 1.0, 120.0)},
    }

    def setup(self):
        self.key = None
        self.result = None
        self.sent = 0.0
        self.laid = (None, None)  # the texture as laid over the frame, kept while it and the frame hold

    def process(self, image, texture, mask=None):
        if image is None or texture is None:
            return None
        p = self.params
        frames = [np.asarray(d.data) if d is not None else None for d in (image, texture, mask)]
        key = (tuple(fingerprint(v) for v in frames), repr(vars(p.transfer)), repr(vars(p.material)),
               repr(vars(p.mix)), repr(vars(p.frame)))
        if key == self.key:
            if self.result is not None and time.monotonic() - self.sent >= RESEND:
                self.sent = time.monotonic()
                return self.result
            return None
        self.key = key
        started = time.perf_counter()
        src = picture(frames[0], "image")
        H, W = src.shape[:2]
        long = int(p.frame.work) or max(H, W)
        k = long / max(H, W)
        h, w = max(8, round(H * k)), max(8, round(W * k))
        c = resize(src, h, w, "bilinear")
        centre = p.material.centre if isinstance(p.material.centre, (list, tuple)) else [p.material.centre] * 2
        where = (key[0][1], h, w, clamp(p.material.zoom, 0.05, 64.0), float(centre[0]), float(centre[1]))
        if self.laid[0] != where:
            self.laid = (where, frame_of(picture(frames[1], "texture"), h, w, where[3], where[4:]))
        t = self.laid[1]
        out = self.transferred(p.transfer, c, t, max(h, w) / REF)
        m = np.full((h, w), clamp(p.mix.amount, 0.0, 1.0), np.float32)
        if frames[2] is not None:
            m = m * np.clip(resize(plane(frames[2], "mask"), h, w, "bilinear"), 0.0, 1.0)
        result = np.ascontiguousarray(np.clip(c * (1.0 - m[..., None]) + out * m[..., None], 0.0, 1.0).astype(np.float32))
        ms = (time.perf_counter() - started) * 1000.0
        self.result = {"image": (result, {"channels": {"dim2": ["r", "g", "b"]}}),
                       "status": f"{p.transfer.mode} | {w} x {h} | {ms:.0f} ms"}
        self.sent = time.monotonic()
        return self.result

    @staticmethod
    def transferred(q, c, t, k):
        """The mode picked, on the working frame. `k` carries pixel knobs from the 384 px frame."""
        if q.mode == "blend":
            return blend(c, t, clamp(q.wmix, 0.0, 1.0))
        if q.mode == "guided":
            return guided(c, t, clamp(q.radius, 1.0, 240.0) * k, clamp(q.eps, 1e-4, 1.0), clamp(q.detail, 0.0, 2.0))
        if q.mode == "flow":
            return flow(c, t, clamp(q.length, 0.0, 0.5), clamp(q.reach, 0.5, 40.0) * k, clamp(q.smear, 0.0, 1.0),
                        int(clamp(q.steps, 1, 32)))
        raise ValueError(f"transfer/mode must be one of {', '.join(MODES)}")
