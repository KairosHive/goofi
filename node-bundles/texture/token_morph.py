"""TokenMorph morphs pictures in DINOv2's token grid and draws them back with the RAE decoder.

latent-lab's `rae.py`. The frozen DINOv2-B with registers turns a picture into a grid of 768-number
tokens (32 x 32 at 512 px). A ViT decoder trained by Zheng, Ma, Tong and Xie ("Diffusion
Transformers with Representation Autoencoders", 2025, MIT) draws the picture back from them. Two
materials mixed in the grid stay one surface at every step, where a crossfade double-exposes. Each
mix is put back on the encoder's shell (every token zero-mean and unit spread), because a mix of two
tokens is shorter than either.

Wire any number of pictures into `pictures` (each wire a picture, or a stack [n, h, w, 3]). They are
encoded once and kept, and only the drawing runs per frame.
- loop: a closed Catmull-Rom curve through the pictures in wire order; `at` from 0 to 1 goes round
  once, passing through every picture and stopping at none.
- mix: a weighted mean of the pictures' grids, by `weights` or by a wired `weights` vector (a
  MaterialSpace's nearness, a band power per picture).
With a wired `image` and `mask`, the morph goes only where the mask is, in the picture's own grid:
the picture's tokens fill the rest and the decoder draws the seam.

A grid larger than the decoder's 32 x 32 is drawn as 32 x 32 windows that overlap and cross-fade.
On a CPU a 512 px frame takes seconds, and on CUDA tens of milliseconds.
"""

import hashlib
import math
import pathlib
import time

import numpy as np
import torch
import torch.nn.functional as F
from torch import nn

import goofi

ENCODER = "facebook/dinov2-with-registers-base"
DECODER_REPO = "nyu-visionx/RAE-collections"
DECODER_FILE = "decoders/dinov2/wReg_base/ViTXL_n08_i512/model.pt"
MEAN = (0.485, 0.456, 0.406)
STD = (0.229, 0.224, 0.225)
SIZES = {768: dict(heads=12, layers=12, inner=3072), 1152: dict(heads=16, layers=28, inner=4096)}  # ViT-B, ViT-XL
MODES = ["loop", "mix"]
LONG = [8, 12, 16, 24, 32, 48, 64]
RESEND = 1.0  # seconds: an unchanged drawing is sent again this often, so a viewer opened later sees it


class Holder(nn.Module):
    """A named slot, so the attribute paths match the checkpoint's keys."""

    def __init__(self, **parts):
        super().__init__()
        for name, part in parts.items():
            setattr(self, name, part)


class Layer(nn.Module):
    """ViT-MAE's block: pre-norm attention, pre-norm GELU MLP, both residual."""

    def __init__(self, width, heads, inner):
        super().__init__()
        self.heads = heads
        self.attention = Holder(attention=Holder(query=nn.Linear(width, width), key=nn.Linear(width, width),
                                                 value=nn.Linear(width, width)),
                                output=Holder(dense=nn.Linear(width, width)))
        self.intermediate = Holder(dense=nn.Linear(width, inner))
        self.output = Holder(dense=nn.Linear(inner, width))
        self.layernorm_before = nn.LayerNorm(width, eps=1e-12)
        self.layernorm_after = nn.LayerNorm(width, eps=1e-12)

    def forward(self, x):
        b, n, c = x.shape
        att = self.attention.attention
        h = self.layernorm_before(x)
        q, k, v = (proj(h).view(b, n, self.heads, c // self.heads).transpose(1, 2) for proj in (att.query, att.key, att.value))
        y = F.scaled_dot_product_attention(q, k, v).transpose(1, 2).reshape(b, n, c)
        x = x + self.attention.output.dense(y)
        return x + self.output.dense(F.gelu(self.intermediate.dense(self.layernorm_after(x))))


class Decoder(nn.Module):
    def __init__(self, patches, width, tokens=768, patch=16):
        super().__init__()
        cfg = SIZES[width]
        self.patch, self.side = patch, int(math.isqrt(patches))
        self.decoder_embed = nn.Linear(tokens, width)
        self.decoder_pos_embed = nn.Parameter(torch.zeros(1, patches + 1, width), requires_grad=False)
        self.trainable_cls_token = nn.Parameter(torch.zeros(1, 1, width))
        self.decoder_layers = nn.ModuleList(Layer(width, cfg["heads"], cfg["inner"]) for _ in range(cfg["layers"]))
        self.decoder_norm = nn.LayerNorm(width, eps=1e-12)
        self.decoder_pred = nn.Linear(width, patch * patch * 3)

    def positions(self, h, w):
        """The position table for an h x w grid no larger than the trained square: its top-left corner."""
        if (h, w) == (self.side, self.side):
            return self.decoder_pos_embed
        pe = self.decoder_pos_embed
        corner = pe[:, 1:].reshape(1, self.side, self.side, -1)[:, :h, :w].reshape(1, h * w, -1)
        return torch.cat([pe[:, :1], corner], 1)

    def forward(self, z, grid):
        h, w = grid
        x = self.decoder_embed(z)
        x = torch.cat([self.trainable_cls_token.expand(len(x), -1, -1), x], 1) + self.positions(h, w)
        for layer in self.decoder_layers:
            x = layer(x)
        x = self.decoder_pred(self.decoder_norm(x))[:, 1:]
        p = self.patch
        return torch.einsum("nhwpqc->nchpwq", x.reshape(len(x), h, w, p, p, 3)).reshape(len(x), 3, h * p, w * p)


def decoder_path(given):
    """The decoder file: the given path, or RAE's 512 decoder from the Hugging Face cache (fetched once)."""
    if given.strip():
        path = pathlib.Path(given.strip()).expanduser()
        if not path.is_file():
            raise ValueError(f"model/decoder names no file at {path}")
        return path
    from huggingface_hub import hf_hub_download

    try:
        return pathlib.Path(hf_hub_download(DECODER_REPO, DECODER_FILE, local_files_only=True))
    except Exception:
        return pathlib.Path(hf_hub_download(DECODER_REPO, DECODER_FILE))


class RAE:
    """DINOv2-B with registers as the encoder, its last layer norm without its scale and shift, and
    RAE's ViT decoder, which draws ImageNet-normalised pixels."""

    def __init__(self, decoder, dev):
        state = torch.load(decoder, map_location="cpu", weights_only=True)
        for wrap in ("model", "ema", "state_dict"):
            if isinstance(state, dict) and wrap in state and isinstance(state[wrap], dict):
                state = state[wrap]
        state = {k.removeprefix("decoder."): v for k, v in state.items()}
        patches, width = state["decoder_pos_embed"].shape[1] - 1, state["decoder_pos_embed"].shape[2]
        self.decoder = Decoder(patches, width, state["decoder_embed.weight"].shape[1])
        missing, unexpected = self.decoder.load_state_dict(state, strict=False)
        if missing or unexpected:
            raise ValueError(f"{decoder} lacks an RAE decoder's weights (missing {missing[:4]}, unexpected {unexpected[:4]})")
        self.side = self.decoder.side
        self.dev = dev
        self.dtype = torch.float16 if dev == "cuda" else torch.float32
        from transformers import Dinov2WithRegistersModel

        try:
            enc = Dinov2WithRegistersModel.from_pretrained(ENCODER, local_files_only=True, attn_implementation="sdpa")
        except OSError:
            enc = Dinov2WithRegistersModel.from_pretrained(ENCODER, attn_implementation="sdpa")
        enc.layernorm.elementwise_affine = False
        enc.layernorm.weight = None
        enc.layernorm.bias = None
        self.encoder = enc.to(dev, self.dtype).eval()
        self.decoder.to(dev, self.dtype).eval()
        self.mean = torch.tensor(MEAN, device=dev).view(1, 3, 1, 1)
        self.std = torch.tensor(STD, device=dev).view(1, 3, 1, 1)

    @torch.inference_mode()
    def encode(self, image, grid):
        """[h, w, 3] in 0..1 -> [rows * cols, 768] tokens, the picture fitted to the grid."""
        rows, cols = grid
        x = torch.as_tensor(image, dtype=torch.float32, device=self.dev).permute(2, 0, 1)[None]
        x = F.interpolate(x, size=(rows * 14, cols * 14), mode="bicubic", align_corners=False, antialias=True)
        x = ((x - self.mean) / self.std).to(self.dtype)
        return self.encoder(pixel_values=x).last_hidden_state[0, 5:].float()  # 1 class + 4 register tokens first

    def spans(self, n, overlap=8):
        """Where the windows start along one side: one if it fits the trained table, else as few as
        cover it with at least `overlap` tokens shared between neighbours."""
        if n <= self.side:
            return [0]
        k = math.ceil((n - self.side) / (self.side - overlap)) + 1
        return [round(i * (n - self.side) / (k - 1)) for i in range(k)]

    @torch.inference_mode()
    def decode(self, tokens, grid):
        """[rows * cols, 768] -> [rows * 16, cols * 16, 3] in 0..1."""
        rows, cols = grid
        z = tokens.to(self.dev, self.dtype)[None]
        ys, xs = self.spans(rows), self.spans(cols)
        hh, ww = min(rows, self.side), min(cols, self.side)
        g = z.view(1, rows, cols, -1)
        pieces = torch.cat([g[:, y:y + hh, x:x + ww].reshape(1, hh * ww, -1) for y in ys for x in xs])
        x = (self.decoder(pieces, (hh, ww)).float() * self.std + self.mean).clamp(0, 1)
        if len(ys) * len(xs) == 1:
            return x[0].permute(1, 2, 0)
        # Each window weighs most in its middle and fades to almost nothing at its edges, over the
        # overlap, so neighbours cross-fade; at the frame's own edge one window is all there is.
        def ramp(n):
            i = torch.arange(n, device=x.device, dtype=torch.float32)
            return torch.clamp(torch.minimum(i + 0.5, n - i - 0.5) / 128, 0.01, 1)

        weight = ramp(hh * 16)[:, None] * ramp(ww * 16)[None]
        out = torch.zeros(3, rows * 16, cols * 16, device=x.device)
        total = torch.zeros(1, rows * 16, cols * 16, device=x.device)
        for k, (y, xx) in enumerate((y, xx) for y in ys for xx in xs):
            out[:, y * 16:(y + hh) * 16, xx * 16:(xx + ww) * 16] += x[k] * weight
            total[:, y * 16:(y + hh) * 16, xx * 16:(xx + ww) * 16] += weight
        return (out / total).permute(1, 2, 0)


def shell(z):
    """Tokens back on the encoder's shell: each zero-mean and unit spread over its 768 numbers."""
    return F.layer_norm(z.float(), (z.shape[-1],))


def along(grids, u):
    """The closed Catmull-Rom curve through a loop of grids at u (one turn per unit)."""
    n = len(grids)
    x = float(u) % 1.0 * n
    k, s = int(x), x - int(x)
    p0, p1, p2, p3 = (grids[(k + d) % n] for d in (-1, 0, 1, 2))
    return 0.5 * (2 * p1 + (p2 - p0) * s + (2 * p0 - 5 * p1 + 4 * p2 - p3) * s * s + (3 * p1 - p0 - 3 * p2 + p3) * s ** 3)


def picture(value, name):
    """A picture [h, w, 3] in 0..1 from whatever came down the wire."""
    x = np.asarray(value)
    if x.ndim == 2:
        x = np.repeat(x[..., None], 3, axis=-1)
    if x.ndim != 3 or x.shape[2] < 3:
        raise ValueError(f"`{name}` takes pictures [height, width, 3] or stacks of them. This frame is {list(x.shape)}")
    x = x[..., :3]
    if not np.issubdtype(x.dtype, np.floating):
        return x.astype(np.float32) / 255.0
    return np.clip(np.nan_to_num(x.astype(np.float32, copy=False)), 0.0, 1.0)


def stops_of(frames):
    """Every picture on the multi input, in wire order, a stack opened into its pictures."""
    out = []
    for _, d in frames or []:
        x = np.asarray(d.data)
        if x.ndim == 4:
            out += [picture(p, "pictures") for p in x]
        else:
            out.append(picture(x, "pictures"))
    return out


def piece(img, rows, cols, zoom):
    """The middle of a picture, 1 / zoom of it across, in the grid's shape."""
    h, w = img.shape[:2]
    if w * rows > h * cols:
        ch, cw = h, h * cols / rows
    else:
        ch, cw = w * rows / cols, w
    ch, cw = max(1, round(ch / zoom)), max(1, round(cw / zoom))
    top, left = (h - ch) // 2, (w - cw) // 2
    return img[top:top + ch, left:left + cw]


def fingerprint(x):
    a = np.ascontiguousarray(x).reshape(-1)
    step = max(1, a.size // 65536)
    return x.shape, hashlib.blake2b(np.ascontiguousarray(a[::step]).tobytes(), digest_size=12).hexdigest()


def clamp(v, lo, hi):
    return min(max(float(v), lo), hi)


def device():
    if torch.cuda.is_available():
        return "cuda"
    if torch.get_num_threads() > 8:
        torch.set_num_threads(8)
    return "cpu"


class TokenMorph(goofi.Node):
    """Walk or mix pictures in DINOv2's token grid and draw the result back with the RAE decoder."""

    TAGS = ["image", "ml", "generator"]
    INPUTS = {
        # Not required: the node waits quietly for its first picture (process returns None), where a
        # required slot reports an error on every tick before the first frame arrives.
        "pictures": goofi.InputSlot(goofi.DataType.ARRAY, multi=True),
        "weights": goofi.InputSlot(goofi.DataType.ARRAY),
        "image": goofi.InputSlot(goofi.DataType.ARRAY),
        "mask": goofi.InputSlot(goofi.DataType.ARRAY),
    }
    OUTPUTS = {"image": goofi.DataType.ARRAY, "status": goofi.DataType.STRING}
    PARAMS = {
        "morph": {
            "mode": goofi.StringParam("loop", MODES, doc="Loop goes round the pictures in wire order by `at`. Mix takes "
                                      "a weighted mean of them, by `weights` or a wired weights vector."),
            "at": goofi.NumParam(0.0, 0.0, 1.0, doc="Where on the loop. 0 is the first picture, and each picture "
                                 "takes an equal share of one turn. Values past 1 go round again.",
                                 show=("mode", ["loop"])),
            "even": goofi.BoolParam(True, doc="Pace the loop by how far the picture moves, so `at` changes it "
                                    "evenly. A straight mix of two grids changes most in its middle. Costs a few small "
                                    "draws whenever the pictures change.", show=("mode", ["loop"])),
            "weights": goofi.NumParam([1.0, 0.0, 0.0, 0.0], -1.0, 2.0, doc="Each picture's share in the mix, in wire "
                                      "order (a wired `weights` vector overrides these and takes any number).",
                                      show=("mode", ["mix"])),
            "hold": goofi.NumParam(0.0, 0.0, 1.0, doc="Mix this much of the wired `image`'s own tokens back in, "
                                   "inside the mask too."),
            "keep_outside": goofi.BoolParam(True, doc="Outside the mask, the wired `image`'s own pixels. The decoder "
                                            "draws every token with all the others, so off, the whole frame shifts."),
        },
        "frame": {
            "long": goofi.NumParam(32, 8, 64, int=True, options=LONG, doc="Tokens on the long side, each 16 px, so 32 "
                                   "draws 512 px. Above 32 the decoder draws in overlapping windows."),
            "zoom": goofi.NumParam(1.0, 1.0, 8.0, doc="How close into each picture. At 2 it takes the middle half "
                                   "across, so the material is seen larger."),
        },
        "model": {
            "decoder": goofi.StringParam("", doc="An RAE decoder for DINOv2-B with registers (model.pt). Left empty, "
                                         "RAE's 512 decoder comes from the Hugging Face cache, fetched once."),
        },
        "common": {"autotrigger": goofi.BoolParam(True), "max_frequency": goofi.NumParam(30.0, 1.0, 120.0)},
    }

    def setup(self):
        self.rae = None
        self.built = None
        self.grids = {}
        self.paced = (None,)
        self.key = None
        self.result = None
        self.sent = 0.0

    def model(self, dev):
        path = str(decoder_path(self.params.model.decoder))
        if self.rae is None or self.built != (path, dev):
            self.rae = None
            self.grids = {}
            self.rae = RAE(path, dev)
            self.built = (path, dev)
        return self.rae

    def grid_of(self, rae, img, grid, zoom):
        """A picture's token grid, kept while the picture and the grid stay the same."""
        key = (fingerprint(img), grid, round(zoom, 4))
        if key not in self.grids:
            if len(self.grids) > 64:
                self.grids.pop(next(iter(self.grids)))
            self.grids[key] = rae.encode(piece(img, *grid, zoom), grid)
        return self.grids[key]

    def pace(self, rae, stops, grid, zoom, per=8):
        """Where on the curve each share of one turn falls when the turn is spaced by how far the
        picture moves: drawn on a half-size grid at `per` places a leg, averaged 4 x 4, as the lab's
        films are timed. Kept while the pictures, the grid and the zoom stay the same."""
        key = (tuple(fingerprint(s) for s in stops), grid, round(zoom, 4))
        if getattr(self, "paced", (None,))[0] != key:
            small = (max(1, grid[0] // 2), max(1, grid[1] // 2))
            grids = [self.grid_of(rae, s, small, zoom) for s in stops]
            us = np.arange(len(stops) * per) / (len(stops) * per)
            looks = [F.avg_pool2d(rae.decode(shell(along(grids, u)), small).permute(2, 0, 1)[None], 4) for u in us]
            moved = [float((looks[(i + 1) % len(looks)] - looks[i]).abs().mean()) for i in range(len(looks))]
            gone = np.concatenate([[0.0], np.cumsum(moved)])
            self.paced = (key, gone / max(gone[-1], 1e-9), np.concatenate([us, [1.0]]))
        return self.paced[1], self.paced[2]

    def process(self, pictures, weights=None, image=None, mask=None):
        stops = stops_of(pictures)
        if not stops:
            return None
        p = self.params
        wired = [None if d is None else np.asarray(d.data) for d in (weights, image, mask)]
        key = (tuple(fingerprint(s) for s in stops), tuple(None if v is None else fingerprint(v) for v in wired),
               repr(vars(p.morph)), repr(vars(p.frame)), p.model.decoder)
        if key == self.key:
            if self.result is not None and time.monotonic() - self.sent >= RESEND:
                self.sent = time.monotonic()
                return self.result
            return None
        self.key = key
        started = time.perf_counter()
        dev = device()
        rae = self.model(dev)
        frame = picture(wired[1], "image") if wired[1] is not None else stops[0]
        long = int(clamp(p.frame.long, 1, 128))
        fh, fw = frame.shape[:2]
        if fw >= fh:
            grid = (max(1, round(long * fh / fw)), long)
        else:
            grid = (long, max(1, round(long * fw / fh)))
        zoom = clamp(p.frame.zoom, 1.0, 64.0)
        with torch.inference_mode():
            grids = [self.grid_of(rae, s, grid, zoom) for s in stops]
            if p.morph.mode == "loop":
                at = float(p.morph.at) % 1.0
                u = at
                if p.morph.even and len(grids) > 1:
                    gone, us = self.pace(rae, stops, grid, zoom)
                    u = float(np.interp(at, gone, us))
                z = along(grids, u) if len(grids) > 1 else grids[0]
                how = f"loop at {at:.3f}" + (f" (curve {u:.3f})" if u != at else "")
            elif p.morph.mode == "mix":
                w = np.asarray(wired[0], np.float64).reshape(-1) if wired[0] is not None else \
                    np.asarray(p.morph.weights if isinstance(p.morph.weights, (list, tuple)) else [p.morph.weights], np.float64)
                w = np.resize(np.pad(w, (0, max(0, len(grids) - len(w)))), len(grids))
                if not np.isfinite(w).all() or abs(w.sum()) < 1e-9:
                    raise ValueError("The mix weights sum to zero. Give at least one picture a share")
                w = w / w.sum()
                z = sum(float(wi) * g for wi, g in zip(w, grids))
                how = "mix " + " ".join(f"{wi:.2f}" for wi in w)
            else:
                raise ValueError(f"morph/mode must be one of {', '.join(MODES)}")
            mask_px = None
            if wired[1] is not None:
                own = self.grid_of(rae, frame, grid, 1.0)
                m = torch.ones(grid, device=dev)
                if wired[2] is not None:
                    field = wired[2] if wired[2].ndim == 2 else wired[2][..., 0]
                    f = torch.as_tensor(np.asarray(field, np.float32), device=dev)[None, None].clamp(0, 1)
                    m = F.interpolate(f, size=grid, mode="area")[0, 0]
                    mask_px = F.interpolate(f, size=(grid[0] * 16, grid[1] * 16), mode="bilinear",
                                            align_corners=False)[0, 0, ..., None]
                m = (m * (1.0 - clamp(p.morph.hold, 0.0, 1.0))).reshape(-1, 1)
                z = m * z + (1.0 - m) * own
            out = rae.decode(shell(z), grid)
            if mask_px is not None and p.morph.keep_outside:
                # The decoder draws every token with all the others, so tokens kept outside the mask
                # still come back shifted: the picture's own pixels go there instead, as the lab exports.
                pic = torch.as_tensor(piece(frame, *grid, 1.0), device=dev).permute(2, 0, 1)[None]
                pic = F.interpolate(pic, size=out.shape[:2], mode="bilinear", align_corners=False, antialias=True)
                out = out * mask_px + pic[0].permute(1, 2, 0) * (1.0 - mask_px)
            result = np.ascontiguousarray(out.float().cpu().numpy())
        ms = (time.perf_counter() - started) * 1000.0
        self.result = {"image": (result, {"channels": {"dim2": ["r", "g", "b"]}}),
                       "status": f"{len(stops)} pictures | {how} | grid {grid[0]} x {grid[1]} | {ms:.0f} ms on {dev}"}
        self.sent = time.monotonic()
        return self.result
