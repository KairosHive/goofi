"""DINO — what DINOv3 sees in a picture: one embedding, the patch features, and a segmentation.

The model runs on the GPU end to end: the frame is resized and normalized there, the ViT runs in
bf16 through SDPA attention (`compile` adds CUDA graphs), and the PCA behind `segment` runs on the
patch features where they lie, so only the answers cross back. The weights come from the Hugging
Face hub: Meta's repos are gated, so accept the DINOv3 license on the repo page and `hf auth
login` once, or point `repo` at an ungated mirror in the transformers format.

`segment` is the self-supervised procedure of the DINOv2 paper: a PCA of the patch features,
whose first component splits foreground from background, and a second PCA of the foreground
patches whose three components are drawn as red, green and blue. `memory` keeps the PCA basis
from one frame to the next, so the colours do not swap as the picture moves.
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
    INPUTS = {"image": goofi.InputSlot(goofi.DataType.ARRAY)}
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
            x = self.prepare(image.data, p.resolution)
            tokens = self.model(pixel_values=x).last_hidden_state[0]
            cls = tokens[0].float()
            patches = tokens[self.prefix :].float()
            hp, wp = x.shape[2] // PATCH, x.shape[3] // PATCH
            out = {
                "embedding": (cls.cpu().numpy(), {"model": want[0]}),
                "patches": (patches.reshape(hp, wp, -1).cpu().numpy(), {"model": want[0]}),
            }
            if p.mode == "segment":
                if self.pca is None or self.pca.dim != patches.shape[1]:
                    self.pca = PCA(patches.shape[1], self.device)
                mask, colors = self.pca.step(patches, hp, wp, p.threshold, p.invert, p.memory)
                out["map"] = (draw(colors, mask, x.shape[2:]), {"channels": {"dim2": ["r", "g", "b"]}})
                out["mask"] = draw(mask[None], None, x.shape[2:])[..., 0]
        return out

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
        """The frame as the `[1, 3, H, W]` the model reads: resized to `resolution` on its short side, normalized, on the GPU."""
        x = np.asarray(data, dtype=np.float32)
        if x.ndim == 2:
            x = x[..., None]
        if x.ndim != 3 or x.shape[2] < 1:
            raise ValueError(f"DINO reads a picture; this frame is {x.shape}")
        x = x[..., :3] if x.shape[2] >= 3 else np.repeat(x[..., :1], 3, axis=2)
        t = torch.from_numpy(np.ascontiguousarray(x)).to(self.device, non_blocking=True).permute(2, 0, 1)[None]
        h, w = x.shape[0], x.shape[1]
        scale = resolution / min(h, w)
        size = (max(PATCH, round(h * scale / PATCH) * PATCH), max(PATCH, round(w * scale / PATCH) * PATCH))
        t = F.interpolate(t, size=size, mode="bilinear", antialias=True, align_corners=False)
        mean = torch.tensor(MEAN, device=self.device).view(1, 3, 1, 1)
        std = torch.tensor(STD, device=self.device).view(1, 3, 1, 1)
        return ((t.clamp(0.0, 1.0) - mean) / std).to(self.dtype)

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
    matmuls on fixed shapes, with no sync: the axes come from power iteration warm-started on the
    last frame's, which also keeps their signs from swapping.
    """

    def __init__(self, dim, device):
        self.dim = dim
        self.device = device
        self.all = Moments(dim, device)
        self.fore = Moments(dim, device)
        self.first = torch.randn(dim, 1, device=device)
        self.axes = torch.randn(dim, 3, device=device)
        self.span = None

    def step(self, x, hp, wp, threshold, invert, memory):
        """The foreground mask `[hp, wp]` and the colours `[3, hp, wp]` of one frame's patch features."""
        mean, cov = self.all.update(x, torch.ones(x.shape[0], 1, device=self.device), memory)
        self.first = top(cov, self.first)
        score = ((x - mean) @ self.first)[:, 0]
        border = torch.ones(hp, wp, dtype=torch.bool, device=self.device)
        border[1:-1, 1:-1] = False
        sign = torch.where(score[border.flatten()].mean() > score.mean(), -1.0, 1.0)
        score = score * (-sign if invert else sign)
        mask = (score > threshold * score.std()).float()

        weight = mask[:, None]
        mean, cov = self.fore.update(x, weight, memory)
        self.axes = top(cov, self.axes)
        proj = (x - mean) @ self.axes
        count = weight.sum().clamp_min(1.0)
        center = (proj * weight).sum(0) / count
        spread = (((proj - center) ** 2 * weight).sum(0) / count).sqrt()
        span = torch.stack([center, spread])
        self.span = span if self.span is None or memory <= 0.0 else memory * self.span + (1.0 - memory) * span
        colors = ((proj - self.span[0]) / (5.0 * self.span[1]).clamp_min(1e-6) + 0.5).clamp(0.0, 1.0) * weight
        return mask.view(hp, wp), colors.T.reshape(3, hp, wp)


class Moments:
    """A weighted mean and covariance, replaced each frame or kept as a running average by `memory`."""

    def __init__(self, dim, device):
        self.mean = torch.zeros(dim, device=device)
        self.cov = torch.eye(dim, device=device)
        self.eye = torch.eye(dim, device=device) * 1e-6
        self.seen = False

    def update(self, x, weight, memory):
        count = weight.sum().clamp_min(1.0)
        mean = (x * weight).sum(0) / count
        centered = x - mean
        cov = (centered * weight).T @ centered / count + self.eye
        if self.seen and memory > 0.0:
            self.mean = memory * self.mean + (1.0 - memory) * mean
            self.cov = memory * self.cov + (1.0 - memory) * cov
        else:
            self.mean, self.cov = mean, cov
        self.seen = True
        return self.mean, self.cov


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
    return np.ascontiguousarray(up[0].permute(1, 2, 0).clamp(0.0, 1.0).cpu().numpy())
