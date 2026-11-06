"""DINO — what DINOv3 sees in a picture: one embedding, the patch features, and a similarity map.

The model runs on the GPU end to end: the frame is resized and normalized there, the ViT runs in
bf16 through SDPA attention (`compile` adds CUDA graphs), and the map is drawn from the patch
features where they lie, so the answers cross back in one copy. The weights come from the Hugging
Face hub: Meta's repos are gated, so accept the DINOv3 license on the repo page and `hf auth
login` once, or point `repo` at an ungated mirror in the transformers format.

`image` takes a texture straight off the graphics plane — a camera, a video, a renderer — read
back once per frame, which is the one picture path that costs no copy on the way in.

`segment` is the picture on the front of the DINOv3 repo: the cosine similarity between the patch
under `point` and every other patch, a continuous field over the whole picture that follows the
thing the point sits on, drawn at the picture's own size; `mask` is that field above `threshold`.
Drive `point` from a pose or a cursor and the map tracks what is pointed at.
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
                    "embed", MODES, doc="`embed` answers the embedding and patch features; `segment` also draws the similarity map and mask."
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
                "point": goofi.NumParam(
                    (0.5, 0.5), 0.0, 1.0, show=("mode", ["segment"]), doc="Where the reference patch sits, as a share of the picture's width and height from its top left."
                ),
                "threshold": goofi.NumParam(
                    0.5, 0.0, 1.0, show=("mode", ["segment"]), doc="The similarity above which a patch is in `mask`."
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
                sim = similarity(patches, hp, wp, p.point)
                parts += [draw(sim, full).reshape(-1), draw((sim > p.threshold).float(), full).reshape(-1)]
            flat = self.land(torch.cat(parts))

        # One copy for every answer: the views are cut out of it on the host.
        meta = {"model": want[0]}
        at = 0
        out = {}
        for name, shape, extra in (
            ("embedding", (cls.numel(),), meta),
            ("patches", (hp, wp, patches.shape[1]), meta),
            ("map", full, {}),
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
        if self.device.type == "cuda":
            torch.cuda.empty_cache()


def similarity(patches, hp, wp, point):
    """The cosine similarity of every patch to the one under `point`, as a `[1, hp, wp]` field in 0..1."""
    x = F.normalize(patches, dim=1)
    px = min(wp - 1, int(point[0] * wp))
    py = min(hp - 1, int(point[1] * hp))
    return (x @ x[py * wp + px]).clamp(0.0, 1.0).view(1, hp, wp)


def draw(field, size):
    """A `[1, hp, wp]` patch grid as the `[H, W]` picture the graphics plane draws, bilinear."""
    return F.interpolate(field[None], size=size, mode="bilinear", align_corners=False)[0, 0].contiguous()
