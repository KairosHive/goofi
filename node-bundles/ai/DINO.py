"""DINO — what DINOv3 sees in a picture: one embedding, the patch features, and a segmentation.

The model runs on the GPU end to end: the frame is resized and normalized there, the ViT runs in
bf16 through SDPA attention (`compile` adds CUDA graphs and fuses the PCA), and the PCA behind
`segment` runs on the patch features where they lie, so the answers cross back in one copy. The weights come from the Hugging
Face hub: Meta's repos are gated, so accept the DINOv3 license on the repo page and `hf auth
login` once, or point `repo` at an ungated mirror in the transformers format.

`image` takes a texture straight off the graphics plane — a camera, a video, a renderer — read
back once per frame, which is the one picture path that costs no copy on the way in.

`segment` is the self-supervised procedure of the DINOv2 paper: a PCA of the patch features,
whose first component splits foreground from background, and a second PCA of the foreground
patches whose three components are drawn as red, green and blue, both drawn at the picture's own size.
`memory` keeps the PCA basis and the foreground side from one frame to the next, so neither
swaps as the picture moves.
"""

import numpy as np
import torch
import torch.nn.functional as F

import goofi

MODES = ["embed", "segment"]
SIZES = ["vits16", "vits16plus", "vitb16", "vitl16", "vith16plus", "vit7b16"]
PRECISIONS = ["bf16", "fp32"]
PATCH = 16
MEAN = (0.485, 0.456, 0.406)
STD = (0.229, 0.224, 0.225)


class DINO(goofi.Node):
    """DINOv3 embeddings and patch features of a picture, and a self-supervised segmentation of it."""

    TAGS = ["analysis", "image", "ml"]
    INPUTS = {"image": goofi.InputSlot(goofi.DataType.TEXTURE)}
    OUTPUTS = {
        "embedding": goofi.DataType.ARRAY,
        "patches": goofi.DataType.ARRAY,
        "map": goofi.DataType.ARRAY,
        "mask": goofi.DataType.ARRAY,
    }
    PARAMS = {
        "dino": [
            {
                "mode": goofi.StringParam(
                    "embed", MODES, doc="`embed` answers the embedding and patch features; `segment` also draws the PCA map and mask."
                ),
                "size": goofi.StringParam("vits16", SIZES, doc="Which DINOv3 ViT: `vits16` runs in a millisecond, `vit7b16` needs 14 GB."),
                "repo": goofi.StringParam(
                    "", doc="A Hugging Face repo in the transformers format instead of Meta's gated `facebook/dinov3-<size>-pretrain-lvd1689m`."
                ),
                "resolution": goofi.NumParam(
                    224, 112, 1024, int=True, doc="The short side the picture is resized to, in pixels; one patch is 16, so the cost grows with its square."
                ),
                "precision": goofi.StringParam("bf16", PRECISIONS, doc="bf16 is the model's own precision and twice as fast; the CPU always runs fp32."),
                "compile": goofi.BoolParam(
                    False, doc="Compile the model with CUDA graphs: a minute of warm-up, then the fastest the GPU can run it."
                ),
            },
            {
                "threshold": goofi.NumParam(
                    0.0, -3.0, 3.0, show=("mode", ["segment"]), doc="Where the first PCA component splits foreground from background, in standard deviations."
                ),
                "invert": goofi.BoolParam(
                    False, show=("mode", ["segment"]), doc="Swap foreground and background, when the border of the picture is the subject."
                ),
                "memory": goofi.NumParam(
                    0.9, 0.0, 0.99, show=("mode", ["segment"]), doc="How much of the last frame's PCA to keep, so the colours stay put; 0 fits every frame alone."
                ),
            },
        ],
        "common": {
            "max_frequency": goofi.NumParam(30.0, 0.1, 240.0, doc="Rate cap: the model is asked for no more answers than this a second."),
        },
    }

    def setup(self):
        self.model = None
        self.built = None
        self.device = torch.device("cuda" if torch.cuda.is_available() else "cpu")
        self.pca = None
        self.landing = [None, None]
        self.turn = 0
        self.mean = torch.tensor(MEAN, device=self.device).view(1, 3, 1, 1)
        self.std = torch.tensor(STD, device=self.device).view(1, 3, 1, 1)
        torch.backends.cuda.matmul.allow_tf32 = True
        torch.backends.cudnn.allow_tf32 = True

    def process(self, image):
        if image is None:
            return None
        p = self.params.dino
        want = (p.repo or f"facebook/dinov3-{p.size}-pretrain-lvd1689m", p.precision, p.compile)
        if want != self.built:
            self.build(want)
        self.clear_input("image")

        with torch.inference_mode():
            x, full = self.prepare(image.data, p.resolution)
            tokens = self.model(pixel_values=x).last_hidden_state[0]
            hp, wp = x.shape[2] // PATCH, x.shape[3] // PATCH
            cls = tokens[0].float()
            patches = tokens[self.prefix :].float()
            parts = [cls, patches.reshape(-1)]
            if p.mode == "segment":
                if self.pca is None or self.pca.dim != patches.shape[1]:
                    self.pca = PCA(patches.shape[1], self.device, p.compile)
                mask, colors = self.pca.step(patches, hp, wp, p.threshold, p.invert, p.memory)
                parts += [draw(colors, mask, full).reshape(-1), draw(mask[None], None, full).reshape(-1)]
            flat = self.land(torch.cat(parts))

        # One copy for every answer: the views are cut out of it on the host.
        meta = {"model": want[0]}
        at = 0
        out = {}
        for name, shape, extra in (
            ("embedding", (cls.numel(),), meta),
            ("patches", (hp, wp, patches.shape[1]), meta),
            ("map", (*full, 3), {"channels": {"dim2": ["r", "g", "b"]}}),
            ("mask", full, {}),
        ):
            n = int(np.prod(shape))
            if at + n > flat.numel():
                break
            out[name] = (flat[at : at + n].numpy().reshape(shape), extra)
            at += n
        return out

    def land(self, flat):
        """`flat` on the host, in pinned memory: two buffers in turn, so a frame handed off is whole while the next one lands."""
        self.turn ^= 1
        if self.landing[self.turn] is None or self.landing[self.turn].numel() != flat.numel():
            self.landing[self.turn] = torch.empty(flat.numel(), pin_memory=self.device.type == "cuda")
        self.landing[self.turn].copy_(flat)
        return self.landing[self.turn]

    def build(self, want):
        """The model `want` names, on the GPU in the precision asked, compiled when asked."""
        from transformers import DINOv3ViTModel

        repo, precision, compiled = want
        self.stop()
        dtype = torch.bfloat16 if precision == "bf16" and self.device.type == "cuda" else torch.float32
        try:
            model = DINOv3ViTModel.from_pretrained(repo, dtype=dtype, attn_implementation="sdpa")
        except Exception as e:
            if "gated" in str(e).lower() or "403" in str(e):
                raise ValueError(
                    f"{repo} is gated: accept the DINOv3 license at https://huggingface.co/{repo} and run `hf auth login`, or set `repo` to an ungated mirror"
                ) from e
            raise
        model = model.eval().requires_grad_(False).to(self.device)
        self.prefix = 1 + model.config.num_register_tokens
        self.model = torch.compile(model, mode="reduce-overhead", dynamic=False) if compiled and self.device.type == "cuda" else model
        self.dtype = dtype
        self.built = want
        self.pca = None

    def prepare(self, data, resolution):
        """The frame as the `[1, 3, H, W]` the model reads, resized to `resolution` on its short side and normalized on the GPU, and the frame's own `(H, W)`."""
        x = np.asarray(data, dtype=np.float32)
        if x.ndim == 2:
            x = x[..., None]
        if x.ndim != 3 or x.shape[2] < 1:
            raise ValueError(f"DINO reads a picture; this frame is {x.shape}")
        x = x[..., :3] if x.shape[2] >= 3 else np.repeat(x[..., :1], 3, axis=2)
        t = torch.from_numpy(np.ascontiguousarray(x)).to(self.device).permute(2, 0, 1)[None]
        h, w = x.shape[0], x.shape[1]
        scale = resolution / min(h, w)
        size = (max(PATCH, round(h * scale / PATCH) * PATCH), max(PATCH, round(w * scale / PATCH) * PATCH))
        t = F.interpolate(t, size=size, mode="bilinear", antialias=True, align_corners=False)
        # A picture is 0..1 or 0..255; one that is 8-bit in a float array is told by its peak.
        t = torch.where(t.amax() > 1.0, t / 255.0, t)
        return ((t.clamp(0.0, 1.0) - self.mean) / self.std).to(self.dtype), (h, w)

    def stop(self):
        self.model = None
        self.built = None
        self.pca = None
        if self.device.type == "cuda":
            torch.cuda.empty_cache()


class PCA:
    """The two PCAs of the segmentation, with the running statistics `memory` keeps between frames.

    The first PCA is of every patch: its leading component, signed so the picture's border reads as
    background, is the foreground score. The second is of the foreground patches alone: its three
    leading components, each scaled by its spread, are the colours. Every step is a handful of
    matmuls on fixed shapes with no sync and no branch, so it compiles into a few kernels: the axes
    come from power iteration warm-started on the last frame's, which also keeps their signs from
    swapping, and the foreground side is read off a running average, so it does not flip either.
    """

    def __init__(self, dim, device, compiled):
        self.dim = dim
        self.device = device
        self.state = (
            torch.zeros(dim, device=device),  # mean of every patch
            torch.eye(dim, device=device),  # covariance of every patch
            torch.randn(dim, 1, device=device),  # the foreground axis
            torch.zeros(dim, device=device),  # mean of the foreground patches
            torch.eye(dim, device=device),  # covariance of the foreground patches
            torch.randn(dim, 3, device=device),  # the colour axes
            torch.zeros(2, 3, device=device),  # centre and spread of the colours
            torch.zeros((), device=device),  # how far the border scores above the rest
            torch.zeros((), device=device),  # 1 once a frame was seen, so `memory` has a past to keep
        )
        self.eye = torch.eye(dim, device=device) * 1e-6
        self.border = None
        self.fn = torch.compile(segment, dynamic=False) if compiled and device.type == "cuda" else segment

    def step(self, x, hp, wp, threshold, invert, memory):
        """The foreground mask `[hp, wp]` and the colours `[3, hp, wp]` of one frame's patch features."""
        if self.border is None or self.border.shape != (hp, wp):
            self.border = torch.ones(hp, wp, dtype=torch.bool, device=self.device)
            self.border[1:-1, 1:-1] = False
        knobs = torch.tensor([threshold, -1.0 if invert else 1.0, memory], device=self.device)
        mask, colors, *state = self.fn(x, self.border.flatten(), self.eye, knobs, *self.state)
        self.state = tuple(state)
        return mask.view(hp, wp), colors.T.reshape(3, hp, wp)


def segment(x, border, eye, knobs, mean, cov, first, mean_fg, cov_fg, axes, span, gap, seen):
    """One frame of the PCA segmentation, as a pure function of the patch features and the running state."""
    threshold, flip, memory = knobs[0], knobs[1], knobs[2]
    blend = memory * seen
    ones = torch.ones(x.shape[0], 1, device=x.device)
    mean, cov = moments(x, ones, mean, cov, blend, eye)
    first = top(cov, first)
    score = ((x - mean) @ first)[:, 0]
    gap = blend * gap + (1.0 - blend) * (score[border].mean() - score.mean())
    score = score * torch.where(gap > 0.0, -1.0, 1.0) * flip
    mask = (score > threshold * score.std()).float()

    weight = mask[:, None]
    mean_fg, cov_fg = moments(x, weight, mean_fg, cov_fg, blend, eye)
    axes = top(cov_fg, axes)
    proj = (x - mean_fg) @ axes
    count = weight.sum().clamp_min(1.0)
    center = (proj * weight).sum(0) / count
    spread = (((proj - center) ** 2 * weight).sum(0) / count).sqrt()
    span = blend * span + (1.0 - blend) * torch.stack([center, spread])
    colors = ((proj - span[0]) / (5.0 * span[1]).clamp_min(1e-6) + 0.5).clamp(0.0, 1.0) * weight
    return mask, colors, mean, cov, first, mean_fg, cov_fg, axes, span, gap, torch.ones_like(seen)


def moments(x, weight, mean_old, cov_old, blend, eye):
    """The weighted mean and covariance of `x`, blended into the running ones by `blend`."""
    count = weight.sum().clamp_min(1.0)
    mean = (x * weight).sum(0) / count
    centered = x - mean
    cov = (centered * weight).T @ centered / count + eye
    return blend * mean_old + (1.0 - blend) * mean, blend * cov_old + (1.0 - blend) * cov


def top(cov, warm, steps=8):
    """The leading eigenvectors of `cov`, as many as `warm` has columns, by power iteration from `warm`."""
    q = warm
    for _ in range(steps):
        q = orthonormal(cov @ q)
    return q * torch.sign((q * warm).sum(0, keepdim=True) + 1e-12)


def orthonormal(z):
    """Gram-Schmidt over the few columns of `z`: cheaper than a QR launch for three of them."""
    cols = []
    for i in range(z.shape[1]):
        v = z[:, i]
        for u in cols:
            v = v - (u @ v) * u
        cols.append(v / v.norm().clamp_min(1e-12))
    return torch.stack(cols, 1)


def draw(planes, mask, size):
    """A `[C, hp, wp]` grid as the `[H, W, C]` picture the graphics plane draws, bilinear, masked when given."""
    up = F.interpolate(planes[None], size=size, mode="bilinear", align_corners=False)
    if mask is not None:
        up = up * F.interpolate(mask[None, None], size=size, mode="bilinear", align_corners=False)
    return up[0].permute(1, 2, 0).clamp(0.0, 1.0).contiguous()
