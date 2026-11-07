"""DinoDecode — a grid of DINO tokens drawn back into a picture by the decoder trained on it.

The back half of `Kairos-Hive/latent-textures`: `tokens` is the `[rows, columns, dim]` grid a
`DINO` node encodes in `tokens` mode, or anything made from such grids — a `Morph` between
pictures, an `Operation` adding two, a `Math` scaling one — and the drawing is `rows * 16` by
`columns * 16` pixels. The meta's `space` names the decoder: RAE's ViT-XL for DINOv2-B with
registers (Zheng, Ma, Tong and Xie, 2025, MIT), or RAEv2's for DINOv3 ViT-L `k23` and `k7`
(Singh, Zheng, Wu, Zhang, Shechtman and Xie, 2026, CC BY-NC 4.0). The decoder network is written
out here so only its weights are read, 1.7 GB from the hub on first use.

A grid is drawn in one pass whatever its size: the decoder's position table, trained on a square,
is interpolated to the grid, which draws a 768 x 768 picture without the seams of tiling. A grid
that equals the last one drawn is answered with the last drawing, so a still picture costs no GPU
time, and a run on a changed grid is one decode: 62 ms for 48 x 48 tokens on an RTX 4090 Laptop
GPU at 1500 MHz, 29 ms for 32 x 32.
"""

import math

import numpy as np
import torch
import torch.nn.functional as F
from torch import nn

import goofi

MEAN = (0.485, 0.456, 0.406)
STD = (0.229, 0.224, 0.225)
PATCH = 16
# Each space's decoder: its repo and file, and whether it draws ImageNet-normalised pixels
# (RAE's) or 0..1 (RAEv2's).
DECODERS = {
    "dinov2": ("nyu-visionx/RAE-collections", "decoders/dinov2/wReg_base/ViTXL_n08_i512/model.pt", True),
    "dinov3-k23": ("nyu-visionx/RAEv2-models", "stage1/general/dinov3l-k23/decoder.pt", False),
    "dinov3-k7": ("nyu-visionx/RAEv2-models", "stage1/general/dinov3l-k7/decoder.pt", False),
}
SIZES = {768: dict(heads=12, layers=12, inner=3072), 1152: dict(heads=16, layers=28, inner=4096)}  # ViT-B, ViT-XL


class DinoDecode(goofi.Node):
    """Draw a grid of DINO tokens as a picture, with the decoder trained on that token space."""

    TAGS = ["image", "ml", "generator"]
    INPUTS = {"tokens": goofi.InputSlot(goofi.DataType.ARRAY)}
    OUTPUTS = {"out": goofi.DataType.ARRAY}
    PARAMS = {
        "common": {
            "max_frequency": goofi.NumParam(30.0, 0.1, 240.0, doc="Rate cap: drawings a second at most."),
        },
    }

    def setup(self):
        self.device = torch.device("cuda" if torch.cuda.is_available() else "cpu")
        self.half = torch.float16 if self.device.type == "cuda" else torch.float32
        self.decoder = None
        self.built = None
        self.drawn = None  # the grid on the GPU the landed drawing was made from
        self.grid = None
        self.landing = [None, None]
        self.turn = 0
        self.mean = torch.tensor(MEAN, device=self.device).view(1, 3, 1, 1)
        self.std = torch.tensor(STD, device=self.device).view(1, 3, 1, 1)

    def process(self, tokens):
        if tokens is None:
            return None
        self.clear_input("tokens")
        space = tokens.meta.get("space")
        if space not in DECODERS:
            raise ValueError("`tokens` names no token space: wire a grid from a `DINO` node in `tokens` mode")
        x = np.asarray(tokens.data)
        if x.ndim != 3:
            raise ValueError(f"`tokens` is a `[rows, columns, dim]` grid; this frame is {x.shape}")
        if space != self.built:
            self.build(space)
        with torch.inference_mode():
            z = torch.from_numpy(np.array(x, dtype=np.float32)).to(self.device, self.half)
            if self.drawn is None or self.drawn.shape != z.shape or not torch.equal(self.drawn, z):
                picture = self.draw(z)
                self.flat, self.shape = self.land(picture.reshape(-1)), tuple(picture.shape)
                self.drawn = z
        return self.flat.numpy().reshape(self.shape), {"channels": {"dim2": ["r", "g", "b"]}, "space": space}

    def draw(self, z):
        """A `[rows, cols, dim]` grid on the GPU -> `[rows*16, cols*16, 3]` pixels in 0..1, in one pass."""
        rows, cols, dim = z.shape
        x = z.view(1, rows * cols, dim)
        if self.normalised:
            # RAE's tokens are each zero-mean and unit spread: a mix is put back where real ones lie.
            x = F.layer_norm(x.float(), (dim,)).to(self.half)
        x = self.decoder(x, self.positions(rows, cols), (rows, cols)).float()
        if self.normalised:
            x = x * self.std + self.mean
        return x.clamp(0.0, 1.0)[0].permute(1, 2, 0).contiguous()

    def positions(self, rows, cols):
        """The decoder's position table at this grid: trained on a square, interpolated to the grid."""
        want = (rows, cols)
        if want != self.grid:
            pe = self.decoder.decoder_pos_embed
            side = self.decoder.side
            table = pe[:, 1:].reshape(1, side, side, -1).permute(0, 3, 1, 2).float()
            table = F.interpolate(table, size=want, mode="bicubic", align_corners=False)
            self.table = torch.cat([pe[:, :1], table.permute(0, 2, 3, 1).reshape(1, rows * cols, -1).to(self.half)], 1)
            self.grid = want
        return self.table

    def land(self, flat):
        """`flat` on the host, in pinned memory: two buffers in turn, so a frame handed off is whole while the next one lands."""
        self.turn ^= 1
        if self.landing[self.turn] is None or self.landing[self.turn].numel() != flat.numel():
            self.landing[self.turn] = torch.empty(flat.numel(), pin_memory=self.device.type == "cuda")
        self.landing[self.turn].copy_(flat)
        return self.landing[self.turn]

    def build(self, space):
        """The decoder of `space` on the GPU, the old one released first: the GPU may hold little else."""
        from huggingface_hub import hf_hub_download

        self.stop()
        repo, file, self.normalised = DECODERS[space]
        state = torch.load(hf_hub_download(repo, file), map_location="cpu", weights_only=True)
        for wrap in ("model", "ema", "state_dict"):
            if isinstance(state, dict) and wrap in state and isinstance(state[wrap], dict):
                state = state[wrap]
        state = {k.removeprefix("decoder."): v for k, v in state.items()}
        patches, width = state["decoder_pos_embed"].shape[1] - 1, state["decoder_pos_embed"].shape[2]
        decoder = Decoder(patches, width, state["decoder_embed.weight"].shape[1])
        missing, unexpected = decoder.load_state_dict(state, strict=False)
        if missing or unexpected:
            raise ValueError(f"{file} is not an RAE decoder: missing {missing[:4]}, unexpected {unexpected[:4]}")
        self.decoder = decoder.to(self.device, self.half).eval().requires_grad_(False)
        self.built = space

    def stop(self):
        self.decoder = None
        self.built = self.drawn = self.grid = None
        if self.device.type == "cuda":
            torch.cuda.empty_cache()


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
        self.attention = Holder(
            attention=Holder(query=nn.Linear(width, width), key=nn.Linear(width, width), value=nn.Linear(width, width)),
            output=Holder(dense=nn.Linear(width, width)),
        )
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
    """RAE's decoder: a plain ViT from a grid of tokens to 16 x 16 pixel patches."""

    def __init__(self, patches, width, tokens):
        super().__init__()
        cfg = SIZES[width]
        self.side = int(math.isqrt(patches))
        self.decoder_embed = nn.Linear(tokens, width)
        self.decoder_pos_embed = nn.Parameter(torch.zeros(1, patches + 1, width), requires_grad=False)
        self.trainable_cls_token = nn.Parameter(torch.zeros(1, 1, width))
        self.decoder_layers = nn.ModuleList(Layer(width, cfg["heads"], cfg["inner"]) for _ in range(cfg["layers"]))
        self.decoder_norm = nn.LayerNorm(width, eps=1e-12)
        self.decoder_pred = nn.Linear(width, PATCH * PATCH * 3)

    def forward(self, z, positions, grid):
        """`[b, h*w, tokens]` with its `[1, h*w+1, width]` position table -> `[b, 3, h*16, w*16]` pixels."""
        h, w = grid
        x = self.decoder_embed(z)
        x = torch.cat([self.trainable_cls_token.expand(len(x), -1, -1), x], 1) + positions
        for layer in self.decoder_layers:
            x = layer(x)
        x = self.decoder_pred(self.decoder_norm(x))[:, 1:]
        return torch.einsum("nhwpqc->nchpwq", x.reshape(len(x), h, w, PATCH, PATCH, 3)).reshape(len(x), 3, h * PATCH, w * PATCH)
