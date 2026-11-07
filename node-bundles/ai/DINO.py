"""DINO — what DINO sees in a picture: one embedding, the patch features, a similarity map, and
the token grid a `DinoDecode` draws back into a picture.

The model runs on the GPU end to end: the frame is resized and normalized there, the ViT runs in
bf16 through SDPA attention (`compile` adds CUDA graphs), and the map is drawn from the patch
features where they lie, so the answers cross back in one copy. A picture is encoded only when
its texels moved — a texture's stamp is the tick that drew it, which moves whether or not the
picture did — so a still picture costs no GPU time after its first frame. The weights come from
the Hugging Face hub: Meta's DINOv3 repos are gated, so accept the DINOv3 license on the repo page
and `hf auth login` once, or point `repo` at an ungated mirror in the transformers format.

`image` takes a texture straight off the graphics plane — a camera, a video, a renderer — read
back once per frame, which is the one picture path that costs no copy on the way in.

`segment` is the picture on the front of the DINOv3 repo: the cosine similarity between the patch
under `point` and every other patch, a continuous field over the whole picture that follows the
thing the point sits on, drawn at the picture's own size; `mask` is that field above `threshold`.
Drive `point` from a pose or a cursor and the map tracks what is pointed at.

`tokens` is the front of `Kairos-Hive/latent-textures`: the grid of tokens a representation
autoencoder's decoder was trained on (`DinoDecode`), one token for each 16 x 16 pixels of the
drawing. `space` picks the pair: DINOv2-B with registers (its last layer, trained under RAE's
decoder: a new surface between two pictures), or DINOv3 ViT-L whose token is the mean of 23 of
its 24 blocks (`k23`) or every other one from 11 (`k7`), under RAEv2's (the photographs kept).
Anything between the two nodes — a `Morph`, an `Operation`, a `Math` — moves in that space.
"""

import numpy as np
import torch
import torch.nn.functional as F

import goofi

MODES = ["embed", "segment", "tokens"]
SIZES = ["vits16", "vits16plus", "vitb16", "vitl16", "vith16plus", "vit7b16"]
SPACES = ["dinov2", "dinov3-k23", "dinov3-k7"]
PRECISIONS = ["bf16", "fp32"]
MEAN = (0.485, 0.456, 0.406)
STD = (0.229, 0.224, 0.225)
# Each token space's encoder, its pixels a patch, and the blocks its token averages (DINOv2's is
# its last layer alone, with the final norm's scale and shift left off).
ENCODERS = {
    "dinov2": ("facebook/dinov2-with-registers-base", 14, None),
    "dinov3-k23": ("facebook/dinov3-vitl16-pretrain-lvd1689m", 16, range(1, 24)),
    "dinov3-k7": ("facebook/dinov3-vitl16-pretrain-lvd1689m", 16, range(11, 24, 2)),
}
DRAWN = 16  # pixels a token draws, so the grid of a drawing's short side is `resolution / DRAWN`


class DINO(goofi.Node):
    """DINO embeddings and patch features of a picture, a self-supervised segmentation of it, and its token grid for `DinoDecode`."""

    TAGS = ["analysis", "image", "ml"]
    INPUTS = {"image": goofi.InputSlot(goofi.DataType.TEXTURE)}
    OUTPUTS = {
        "embedding": goofi.DataType.ARRAY,
        "patches": goofi.DataType.ARRAY,
        "map": goofi.DataType.ARRAY,
        "mask": goofi.DataType.ARRAY,
        "tokens": goofi.DataType.ARRAY,
    }
    PARAMS = {
        "dino": [
            {
                "mode": goofi.StringParam(
                    "embed",
                    MODES,
                    doc="`embed` answers the embedding and patch features; `segment` also draws the similarity map and mask; `tokens` also encodes the grid `DinoDecode` draws.",
                ),
                "size": goofi.StringParam(
                    "vits16", SIZES, show=("mode", ["embed", "segment"]), doc="Which DINOv3 ViT: `vits16` runs in a millisecond, `vit7b16` needs 14 GB."
                ),
                "space": goofi.StringParam(
                    "dinov2",
                    SPACES,
                    show=("mode", ["tokens"]),
                    doc="Whose tokens `DinoDecode` draws: DINOv2 redraws a new surface between pictures; DINOv3 keeps them (`k23` closest, `k7` more semantic).",
                ),
                "repo": goofi.StringParam(
                    "", doc="A Hugging Face repo in the transformers format instead of Meta's gated `facebook/dinov3-<size>-pretrain-lvd1689m`."
                ),
                "resolution": goofi.NumParam(
                    224,
                    112,
                    1536,
                    int=True,
                    doc="The short side the picture is resized to, in pixels, in whole patches of 16; in `tokens` mode the drawing's short side. The cost grows with its square.",
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
        self.seen = None  # (fingerprint, resolution) of the picture the GPU holds encoded
        self.answered = None  # what the landed answers were drawn from
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
        space = p.space if p.mode == "tokens" else None
        want = (p.repo or (ENCODERS[space][0] if space else f"facebook/dinov3-{p.size}-pretrain-lvd1689m"), space, p.precision, p.compile)
        if want != self.built:
            self.build(want)
        self.clear_input("image")

        x = np.asarray(image.data)
        seen = (fingerprint(x), p.resolution)
        if seen != self.seen:
            with torch.inference_mode():
                self.encode(x, p.resolution)
            self.seen = seen
        asked = (seen, p.mode, tuple(p.point), p.threshold)
        if asked != self.answered:
            with torch.inference_mode():
                self.answer(p, {"model": want[0]})
            self.answered = asked

        # One copy for every answer: the views are cut out of it on the host.
        out, at = {}, 0
        for name, shape, meta in self.views:
            n = int(np.prod(shape))
            out[name] = (self.flat[at : at + n].numpy().reshape(shape), meta)
            at += n
        return out

    def answer(self, p, meta):
        """Land every answer the mode asks for in one copy, and keep where each one lies in it."""
        views = [("embedding", (self.cls.numel(),), meta), ("patches", (self.hp, self.wp, self.patches.shape[1]), meta)]
        parts = [self.cls, self.patches.reshape(-1)]
        if p.mode == "segment":
            sim = similarity(self.patches, self.hp, self.wp, p.point)
            parts += [draw(sim, self.full).reshape(-1), draw((sim > p.threshold).float(), self.full).reshape(-1)]
            views += [("map", self.full, {}), ("mask", self.full, {})]
        if p.mode == "tokens":
            parts.append(self.tokens.reshape(-1))
            views.append(("tokens", tuple(self.tokens.shape), {"space": p.space}))
        self.flat, self.views = self.land(torch.cat(parts)), views

    def encode(self, x, resolution):
        """Run the picture through the model: its class token, patch grid and token grid stay on the GPU."""
        x, self.full = self.prepare(x, resolution)
        self.hp, self.wp = x.shape[2] // self.patch, x.shape[3] // self.patch
        result = self.model(pixel_values=x, output_hidden_states=self.blocks is not None)
        last = result.last_hidden_state[0]
        self.cls, self.patches = last[0].float(), last[self.prefix :].float()
        if self.blocks is None:
            self.tokens = self.patches.view(self.hp, self.wp, -1)
            return
        # RAEv2's token: the mean of the layer-normed blocks, plus the mean token of the last one.
        states = result.hidden_states
        first = len(states) - self.layers
        layers = [F.layer_norm(states[first + k][0, self.prefix :].float(), (last.shape[-1],), eps=1e-5) for k in self.blocks]
        self.tokens = (torch.stack(layers).mean(0) + layers[-1].mean(0, keepdim=True)).view(self.hp, self.wp, -1)

    def land(self, flat):
        """`flat` on the host, in pinned memory: two buffers in turn, so a frame handed off is whole while the next one lands."""
        self.turn ^= 1
        if self.landing[self.turn] is None or self.landing[self.turn].numel() != flat.numel():
            self.landing[self.turn] = torch.empty(flat.numel(), pin_memory=self.device.type == "cuda")
        self.landing[self.turn].copy_(flat)
        return self.landing[self.turn]

    def build(self, want):
        """The model `want` names, on the GPU in the precision asked, compiled when asked."""
        repo, space, precision, compiled = want
        self.stop()
        dtype = torch.bfloat16 if precision == "bf16" and self.device.type == "cuda" else torch.float32
        try:
            if space == "dinov2":
                from transformers import Dinov2WithRegistersModel

                model = Dinov2WithRegistersModel.from_pretrained(repo, dtype=dtype, attn_implementation="sdpa")
                # RAE's decoder was trained on the last layer before the norm's learned scale and shift.
                model.layernorm.elementwise_affine = False
                model.layernorm.weight = None
                model.layernorm.bias = None
            else:
                from transformers import DINOv3ViTModel

                model = DINOv3ViTModel.from_pretrained(repo, dtype=dtype, attn_implementation="sdpa")
        except Exception as e:
            if "gated" in str(e).lower() or "403" in str(e):
                raise ValueError(
                    f"{repo} is gated: accept the DINOv3 license at https://huggingface.co/{repo} and run `hf auth login`, or set `repo` to an ungated mirror"
                ) from e
            raise
        model = model.eval().requires_grad_(False).to(self.device)
        self.prefix = 1 + model.config.num_register_tokens
        self.layers = model.config.num_hidden_layers
        self.patch, self.blocks = (ENCODERS[space][1], ENCODERS[space][2]) if space else (model.config.patch_size, None)
        self.model = torch.compile(model, mode="reduce-overhead", dynamic=False) if compiled and self.device.type == "cuda" else model
        self.dtype = dtype
        self.built = want
        self.seen = None

    def prepare(self, x, resolution):
        """The frame as the `[1, 3, H, W]` the model reads, `resolution / 16` patches on its short side and normalized on the GPU, and the frame's own `(H, W)`."""
        if x.ndim == 2:
            x = x[..., None]
        if x.ndim != 3 or x.shape[2] < 1:
            raise ValueError(f"DINO reads a picture; this frame is {x.shape}")
        t = torch.from_numpy(np.array(x, dtype=np.float32)).to(self.device).permute(2, 0, 1)[None]
        t = t[:, :3] if t.shape[1] >= 3 else t[:, :1].expand(-1, 3, -1, -1)
        h, w = x.shape[0], x.shape[1]
        short = max(1, round(resolution / DRAWN))
        grid = (short, max(1, round(short * w / h))) if h <= w else (max(1, round(short * h / w)), short)
        t = F.interpolate(t, size=(grid[0] * self.patch, grid[1] * self.patch), mode="bilinear", antialias=True, align_corners=False)
        # A picture is 0..1 or 0..255; one that is 8-bit in a float array is told by its peak.
        t = torch.where(t.amax() > 1.0, t / 255.0, t)
        return ((t.clamp(0.0, 1.0) - self.mean) / self.std).to(self.dtype), (h, w)

    def stop(self):
        self.model = None
        self.built = None
        self.seen = self.answered = None
        if self.device.type == "cuda":
            torch.cuda.empty_cache()


def fingerprint(x):
    """What a picture is, cheaply: a hash of every 32nd texel, which a new frame of a video moves."""
    return hash(np.ascontiguousarray(x[::32, ::32]).tobytes())


def similarity(patches, hp, wp, point):
    """The cosine similarity of every patch to the one under `point`, as a `[1, hp, wp]` field in 0..1."""
    x = F.normalize(patches, dim=1)
    px = min(wp - 1, int(point[0] * wp))
    py = min(hp - 1, int(point[1] * hp))
    return (x @ x[py * wp + px]).clamp(0.0, 1.0).view(1, hp, wp)


def draw(field, size):
    """A `[1, hp, wp]` patch grid as the `[H, W]` picture the graphics plane draws, bilinear."""
    return F.interpolate(field[None], size=size, mode="bilinear", align_corners=False)[0, 0].contiguous()
