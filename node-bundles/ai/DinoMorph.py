"""DinoMorph — a glide between pictures through DINO's token space, drawn back by a decoder.

The morph on the front of `Kairos-Hive/latent-textures`: every wired picture becomes a grid of
DINO tokens, `position` picks a point along the list of them (a straight mix of two neighbours,
or a closed Catmull-Rom curve through all of them, which passes through every picture and stops at
none), and the representation autoencoder's decoder draws that grid as a picture. Halfway between
two pictures DINOv2 draws one new surface; DINOv3 keeps the photographs, nearer a double exposure.

The encoders and decoders are the ones the lab names: DINOv2-B with registers under RAE's ViT-XL
decoder (Zheng, Ma, Tong and Xie, 2025, MIT), or DINOv3 ViT-L under RAEv2's (Singh, Zheng, Wu,
Zhang, Shechtman and Xie, 2026, CC BY-NC 4.0), whose token is the mean of 23 of its 24 blocks
(`k23`) or every other one from 11 (`k7`). The decoder network is written out here so only its
weights are read; DINOv3 is gated under Meta's licence, as `DINO` says. A picture is encoded once
and kept until its frame changes, so a run is one decode: 19 ms at 512 on an RTX 3090 for DINOv2.
"""

import math

import numpy as np
import torch
import torch.nn.functional as F
from torch import nn

import goofi

SPACES = ["dinov2", "dinov3-k23", "dinov3-k7"]
CURVES = ["linear", "spline"]
MEAN = (0.485, 0.456, 0.406)
STD = (0.229, 0.224, 0.225)
# What each space is made of: the encoder repo, the decoder's repo and file, pixels a token, and
# whether the decoder draws ImageNet-normalised pixels (RAE's) or 0..1 (RAEv2's).
MODELS = {
    "dinov2": ("facebook/dinov2-with-registers-base", "nyu-visionx/RAE-collections", "decoders/dinov2/wReg_base/ViTXL_n08_i512/model.pt", 14, True),
    "dinov3-k23": ("facebook/dinov3-vitl16-pretrain-lvd1689m", "nyu-visionx/RAEv2-models", "stage1/general/dinov3l-k23/decoder.pt", 16, False),
    "dinov3-k7": ("facebook/dinov3-vitl16-pretrain-lvd1689m", "nyu-visionx/RAEv2-models", "stage1/general/dinov3l-k7/decoder.pt", 16, False),
}
LAYERS = {"dinov3-k23": range(1, 24), "dinov3-k7": range(11, 24, 2)}
SIZES = {768: dict(heads=12, layers=12, inner=3072), 1152: dict(heads=16, layers=28, inner=4096)}  # ViT-B, ViT-XL


class DinoMorph(goofi.Node):
    """Glide between pictures through DINO's token space, and draw the point reached."""

    TAGS = ["image", "ml", "generator"]
    INPUTS = {"images": goofi.InputSlot(goofi.DataType.TEXTURE, multi=True, trigger=False)}
    OUTPUTS = {"out": goofi.DataType.ARRAY}
    PARAMS = {
        "morph": [
            {
                "position": goofi.NumParam(
                    0.0, 0.0, 64.0, doc="Where along the pictures to be, in pictures: 1.5 is halfway from the second to the third, and the list loops."
                ),
                "curve": goofi.StringParam(
                    "linear", CURVES, doc="`linear` mixes the two nearest pictures; `spline` runs one smooth loop through all of them."
                ),
                "space": goofi.StringParam(
                    "dinov2", SPACES, doc="Whose tokens: DINOv2 redraws a new surface between pictures; DINOv3 keeps them (`k23` closest, `k7` more semantic)."
                ),
                "resolution": goofi.NumParam(
                    512, 128, 1536, int=True, doc="Pixels on the long side of the drawing; the pictures take the first one's proportions."
                ),
            }
        ],
        "common": {
            "autotrigger": goofi.BoolParam(True, doc="Run on the node's own clock, so a moving `position` draws between the pictures' frames."),
            "max_frequency": goofi.NumParam(30.0, 0.1, 240.0, doc="Rate cap: drawings a second at most."),
        },
    }

    def setup(self):
        self.device = torch.device("cuda" if torch.cuda.is_available() else "cpu")
        self.auto = None
        self.built = None
        self.grids = {}  # by sender: (frame key, grid of tokens)
        self.drawn = None

    def process(self, images):
        if not images:
            return None
        p = self.params.morph
        if p.space != self.built:
            self.stop()  # the old pair's memory goes before the new one asks for its own
            self.auto = Autoencoder(p.space, self.device)
            self.built = p.space

        first = np.asarray(images[0][1].data)
        rows, cols = self.auto.grid(first.shape[1], first.shape[0], max(1, p.resolution // 16))
        senders = []
        for src, frame in images:
            senders.append(src)
            key = (rows, cols, frame.meta.get("index"), frame.meta.get("procend"))
            if src not in self.grids or self.grids[src][0] != key:
                self.grids[src] = (key, self.auto.encode(picture(frame.data), (rows, cols)))
        for gone in [s for s in self.grids if s not in senders]:
            del self.grids[gone]

        grids = [self.grids[s][1] for s in senders]
        want = (tuple(self.grids[s][0] for s in senders), round(p.position, 4), p.curve)
        if want == self.drawn:
            return None
        self.drawn = want
        z = along(grids, p.position) if p.curve == "spline" else between(grids, p.position)
        out = self.auto.decode(self.auto.shell(z)[None], (rows, cols))[0]
        return out, {"channels": {"dim2": ["r", "g", "b"]}, "space": p.space, "pictures": len(grids)}

    def stop(self):
        self.auto = None
        self.built = None
        self.grids, self.drawn = {}, None
        if self.device.type == "cuda":
            torch.cuda.empty_cache()


def between(grids, position):
    """The straight mix of the two pictures either side of `position`, the list looping."""
    n = len(grids)
    x = float(position) % n
    k, s = int(x), x - int(x)
    return (1.0 - s) * grids[k].float() + s * grids[(k + 1) % n].float()


def along(grids, position):
    """The closed Catmull-Rom curve through the loop of pictures at `position`: through every one, stopping at none."""
    n = len(grids)
    x = float(position) % n
    k, s = int(x), x - int(x)
    p0, p1, p2, p3 = (grids[(k + d) % n].float() for d in (-1, 0, 1, 2))
    return 0.5 * (2 * p1 + (p2 - p0) * s + (2 * p0 - 5 * p1 + 4 * p2 - p3) * s * s + (3 * p1 - p0 - 3 * p2 + p3) * s**3)


def picture(data):
    """Whatever came down the wire, as `[H, W, 3]` floats in 0..1."""
    x = np.asarray(data, dtype=np.float32)
    if x.ndim == 2:
        x = x[..., None]
    if x.ndim != 3:
        raise ValueError(f"DinoMorph reads a picture; this frame is {x.shape}")
    x = x[..., :3] if x.shape[2] >= 3 else np.repeat(x[..., :1], 3, axis=2)
    if x.max() > 1.0:
        x = x / 255.0
    return np.clip(x, 0.0, 1.0)


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

    def __init__(self, patches, width, tokens, patch=16):
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
        """`[b, h*w, tokens]` -> `[b, 3, h*16, w*16]` pixels, for a grid no larger than the trained one."""
        h, w = grid
        x = self.decoder_embed(z)
        x = torch.cat([self.trainable_cls_token.expand(len(x), -1, -1), x], 1) + self.positions(h, w)
        for layer in self.decoder_layers:
            x = layer(x)
        x = self.decoder_pred(self.decoder_norm(x))[:, 1:]
        p = self.patch
        return torch.einsum("nhwpqc->nchpwq", x.reshape(len(x), h, w, p, p, 3)).reshape(len(x), 3, h * p, w * p)


class Autoencoder:
    """One space: its frozen encoder and the decoder trained on its tokens, both on the GPU."""

    def __init__(self, space, device):
        from huggingface_hub import hf_hub_download

        encoder, repo, file, self.pixels, self.normalised = MODELS[space]
        self.space, self.device = space, device
        self.half = torch.float16 if device.type == "cuda" else torch.float32
        self.mean = torch.tensor(MEAN, device=device).view(1, 3, 1, 1)
        self.std = torch.tensor(STD, device=device).view(1, 3, 1, 1)
        state = torch.load(hf_hub_download(repo, file), map_location="cpu", weights_only=True)
        for wrap in ("model", "ema", "state_dict"):
            if isinstance(state, dict) and wrap in state and isinstance(state[wrap], dict):
                state = state[wrap]
        state = {k.removeprefix("decoder."): v for k, v in state.items()}
        patches, width = state["decoder_pos_embed"].shape[1] - 1, state["decoder_pos_embed"].shape[2]
        self.decoder = Decoder(patches, width, state["decoder_embed.weight"].shape[1])
        missing, unexpected = self.decoder.load_state_dict(state, strict=False)
        if missing or unexpected:
            raise ValueError(f"{file} is not an RAE decoder: missing {missing[:4]}, unexpected {unexpected[:4]}")
        self.side = self.decoder.side
        self.decoder = self.decoder.to(device, self.half).eval().requires_grad_(False)
        self.encoder = self.load_encoder(encoder).to(device, torch.bfloat16 if space != "dinov2" and device.type == "cuda" else self.half)
        self.encoder = self.encoder.eval().requires_grad_(False)

    def load_encoder(self, repo):
        """The frozen encoder: DINOv2's last layer norm without its learned scale and shift, or DINOv3 whole."""
        try:
            if self.space == "dinov2":
                from transformers import Dinov2WithRegistersModel

                encoder = Dinov2WithRegistersModel.from_pretrained(repo, attn_implementation="sdpa")
                encoder.layernorm.elementwise_affine = False
                encoder.layernorm.weight = None
                encoder.layernorm.bias = None
                return encoder
            from transformers import DINOv3ViTModel

            return DINOv3ViTModel.from_pretrained(repo, attn_implementation="sdpa")
        except Exception as e:
            if "gated" in str(e).lower() or "403" in str(e):
                raise ValueError(f"{repo} is gated: accept the DINOv3 license at https://huggingface.co/{repo} and run `hf auth login`") from e
            raise

    def grid(self, width, height, long):
        """The token grid (rows, columns) for a frame of this shape, `long` tokens on its long side."""
        if width >= height:
            return max(1, round(long * height / width)), long
        return long, max(1, round(long * width / height))

    @torch.inference_mode()
    def encode(self, image, grid):
        """`[H, W, 3]` in 0..1 -> `[rows*cols, dim]` tokens, the picture fitted to the grid."""
        rows, cols = grid
        x = torch.from_numpy(np.ascontiguousarray(image)).to(self.device).permute(2, 0, 1)[None]
        x = F.interpolate(x, size=(rows * self.pixels, cols * self.pixels), mode="bicubic", align_corners=False)
        x = ((x - self.mean) / self.std).to(self.encoder.dtype)
        if self.space == "dinov2":
            return self.encoder(pixel_values=x).last_hidden_state[0, 5:].to(self.half)  # 1 class + 4 registers first
        out = self.encoder(pixel_values=x, output_hidden_states=True).hidden_states
        first = len(out) - self.encoder.config.num_hidden_layers
        skip = 1 + self.encoder.config.num_register_tokens
        layers = [F.layer_norm(out[first + k][0, skip:].float(), (out[0].shape[-1],), eps=1e-5) for k in LAYERS[self.space]]
        return (torch.stack(layers).mean(0) + layers[-1].mean(0, keepdim=True)).to(self.half)

    def shell(self, z):
        """A mix of tokens put back where real tokens lie: DINOv2's each zero-mean and unit spread, DINOv3's as they are."""
        if self.space == "dinov2":
            return F.layer_norm(z.float(), (z.shape[-1],))
        return z

    def spans(self, n, overlap=8):
        """Where the decoder's windows start along one side: one if the side fits its table, else as few as cover it sharing `overlap`."""
        if n <= self.side:
            return [0]
        k = math.ceil((n - self.side) / (self.side - overlap)) + 1
        return [round(i * (n - self.side) / (k - 1)) for i in range(k)]

    @torch.inference_mode()
    def decode(self, tokens, grid):
        """`[b, rows*cols, dim]` -> `[b, rows*16, cols*16, 3]` in 0..1, larger grids drawn in cross-faded windows."""
        rows, cols = grid
        z = tokens.to(self.device, self.half)
        ys, xs = self.spans(rows), self.spans(cols)
        hh, ww = min(rows, self.side), min(cols, self.side)
        g = z.view(len(z), rows, cols, -1)
        pieces = torch.cat([g[:, y : y + hh, x : x + ww].reshape(len(z), hh * ww, -1) for y in ys for x in xs])
        x = self.decoder(pieces, (hh, ww)).float()
        if self.normalised:
            x = x * self.std + self.mean
        x = x.clamp(0.0, 1.0)
        if len(ys) * len(xs) == 1:
            return x.permute(0, 2, 3, 1).contiguous().cpu().numpy()
        # Each window weighs most in its middle and fades over the overlap, so neighbours cross-fade.
        ramp = lambda n: torch.clamp(torch.minimum(torch.arange(n) + 0.5, n - torch.arange(n) - 0.5) / 128, 0.01, 1)  # noqa: E731
        weight = (ramp(hh * 16)[:, None] * ramp(ww * 16)[None]).to(x.device)
        out = torch.zeros(len(z), 3, rows * 16, cols * 16, device=x.device)
        total = torch.zeros(1, 1, rows * 16, cols * 16, device=x.device)
        x = x.view(len(ys) * len(xs), len(z), 3, hh * 16, ww * 16)
        for k, (y, xx) in enumerate((y, xx) for y in ys for xx in xs):
            out[:, :, y * 16 : (y + hh) * 16, xx * 16 : (xx + ww) * 16] += x[k] * weight
            total[:, :, y * 16 : (y + hh) * 16, xx * 16 : (xx + ww) * 16] += weight
        return (out / total).permute(0, 2, 3, 1).contiguous().cpu().numpy()
