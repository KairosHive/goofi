# goofi: graphics
"""Depth — how far each pixel of a picture is from the camera, in metres, by Apple's Depth Pro 2.

A graphics node: the tick calls it at its place between the shaders, so the depth a shader reads
is of the picture that was drawn in the same frame. On Linux with an NVIDIA GPU the picture never
leaves the GPU: the input is the engine's own memory as a torch tensor, the model runs on it, and
the depth goes back the same way. The node shows its share of the frame; a model too large for the
frame rate holds every stage back, so pick `size` and `resolution` for the rate the patch needs.

`image` mode estimates each picture alone. `live` mode is the model's unbuffered stream: each
frame also attends the `window - 1` frames before it, which keeps a camera or a video steady in
time, and the focal length is the median of the estimates so far. A picture that equals the last
one is not estimated again, so a still picture costs no GPU time after its first frame.

The weights (`apple/DepthPro2-S` to `-H` on the Hugging Face hub, 97 MB to 3.5 GB, and the 94 MB
focal length estimator they share) download on first use, on a thread of its own: the node draws
nothing until its model is loaded, and the tick goes on. The weights are under the Apple Machine Learning Research Model License: research
use only.
"""

import math
import threading

import torch
import torch.nn.functional as F

import goofi

REPO = "apple/DepthPro2-{size}"
FOCAL = "apple/DepthPro2-focal"  # the focal length estimator every size shares
SIZES = ["S", "B", "L", "H"]
MODES = ["image", "live"]
MAPS = ["auto", "inverse", "linear", "metres"]
STEADY = 0.1  # how far one frame of a feed moves the range `auto` draws between
PRECISIONS = {"bf16": torch.bfloat16, "fp16": torch.float16, "fp32": torch.float32}
MEAN = (0.485, 0.456, 0.406)
STD = (0.229, 0.224, 0.225)
MULTIPLE = 32  # the model pads a picture to whole multiples of this
BRIGHTEST = 65504.0  # the largest number a texture holds: what an endless distance is drawn as


class Depth(goofi.Node):
    """Metric depth of a picture or a live feed, by Depth Pro 2 (S, B, L or H), drawn as a texture."""

    TAGS = ["analysis", "image", "ml"]
    INPUTS = {"image": goofi.InputSlot(goofi.DataType.TEXTURE)}
    OUTPUTS = {"out": goofi.DataType.TEXTURE}
    PARAMS = {
        "depth": [
            {
                "size": goofi.StringParam(
                    "S", SIZES, doc="Which Depth Pro 2: `S` (97 MB) is the fastest, `H` (3.5 GB) the sharpest. The weights download on first use."
                ),
                "mode": goofi.StringParam(
                    "live", MODES, doc="`image` estimates each picture alone; `live` also reads the frames before it, which keeps a feed steady in time."
                ),
                "window": goofi.NumParam(
                    16, 2, 64, int=True, show=("mode", ["live"]), doc="How many frames a `live` estimate reads, the new one included."
                ),
                "resolution": goofi.NumParam(
                    384,
                    0,
                    2048,
                    int=True,
                    doc="The short side the picture is resized to for the model, in pixels; 0 keeps the picture's own size. The cost grows with its square.",
                ),
                "precision": goofi.StringParam(
                    "bf16", list(PRECISIONS), doc="`bf16` is the precision the model was trained in and the fastest; the CPU always runs `fp32`."
                ),
                "fov": goofi.NumParam(
                    0.0, 0.0, 170.0, doc="The camera's horizontal field of view in degrees, which sets the scale in metres; 0 lets the model estimate it."
                ),
            },
            {
                "map": goofi.StringParam(
                    "auto",
                    MAPS,
                    doc="`auto` draws near bright, from the nearest to the farthest thing in view; `inverse` does so from `near` to `far`; `linear` draws near dark, even in metres; `metres` is the distance itself as the texture's value.",
                ),
                "near": goofi.NumParam(
                    0.5, 0.01, 100.0, show=("map", ["inverse", "linear"]), doc="The distance in metres at the bright end of `inverse` and the dark end of `linear`."
                ),
                "far": goofi.NumParam(
                    10.0, 0.1, 1000.0, show=("map", ["inverse", "linear"]), doc="The distance in metres at the dark end of `inverse` and the bright end of `linear`."
                ),
            },
        ],
    }

    def setup(self):
        self.device = torch.device("cuda" if torch.cuda.is_available() else "cpu")
        self.model = None
        self.built = None  # (size, precision) of the model that is loaded
        self.loading = None  # the thread that loads one
        self.failed = None  # (size, precision) of a load that failed, and why
        self.stream = None
        self.streamed = None  # what the open stream was opened for
        self.seen = None  # the picture and the settings the held depth was estimated from
        self.depth = None
        self.span = None  # the nearest and farthest 1/distance in view, which `auto` draws between
        self.drawn = None  # what the last picture out was drawn from
        self.mean = torch.tensor(MEAN, device=self.device).view(3, 1, 1)
        self.std = torch.tensor(STD, device=self.device).view(3, 1, 1)

    def process(self, image):
        if image is None:
            return None
        p = self.params.depth
        if not self.ready((p.size, p.precision)):
            return None
        # Taken: a frame off another engine's wire is not sent again until a new one arrives.
        self.clear_input("image")
        frame = image.data.torch().to(self.device)
        seen = (fingerprint(frame), self.built, p.mode, p.window, p.resolution, p.fov)
        if seen != self.seen:
            with torch.inference_mode():
                depth = self.estimate(frame, p)
            if depth is None:
                return None
            self.depth, self.seen = depth, seen
            # A feed's range moves slowly, so the picture does not pump; a picture alone has its own.
            self.span = span(depth, self.span if p.mode == "live" else None)
        drawn = (seen, p.map, p.near, p.far)
        if drawn == self.drawn:
            return None
        self.drawn = drawn
        return goofi.Texture(draw(self.depth, p.map, p.near, p.far, self.span))

    def ready(self, want):
        """Whether the model `want` names is loaded. One that is not loads on its own thread."""
        if self.built == want:
            return True
        if self.failed is not None and self.failed[0] == want:
            raise RuntimeError(self.failed[1])
        if self.loading is None:
            self.loading = threading.Thread(target=self.load, args=(want,), daemon=True)
            self.loading.start()
        return False

    def load(self, want):
        """Fetch and build the model `want` names; the last one is freed first."""
        size, precision = want
        self.model = self.stream = self.built = self.seen = self.depth = self.span = None
        try:
            from depth_pro_2 import DepthPro2
            from depth_pro_2.network.fov import FOVEstimator
            from huggingface_hub import hf_hub_download
            from safetensors.torch import load_file

            if self.device.type == "cuda":
                torch.cuda.empty_cache()
            dtype = PRECISIONS[precision] if self.device.type == "cuda" else torch.float32
            model = DepthPro2.from_pretrained(REPO.format(size=size), precision=dtype)
            model.fov = FOVEstimator(dtype)
            model.fov.load_state_dict(load_file(hf_hub_download(FOCAL, "model.safetensors")))
            self.model = model.to(self.device).eval().requires_grad_(False)
            self.failed, self.built = None, want
        except Exception as e:
            self.failed = (want, f"Depth Pro 2 {size} did not load: {e}")
        self.loading = None

    def estimate(self, frame, p):
        """The depth of `frame` in metres, `[H, W]` at the size the model ran at."""
        x = self.prepare(frame, p.resolution)
        f_px = 0.5 * x.shape[-1] / math.tan(math.radians(p.fov) / 2) if p.fov > 0 else None
        if p.mode == "image":
            self.stream = None
            return self.model.infer(x, f_px=f_px)["depth"]
        # A stream is one feed at one size: another size, window or focal length opens a new one.
        opened = (tuple(x.shape), p.window, p.fov)
        if self.stream is None or self.streamed != opened:
            self.stream, self.streamed = self.model.stream(p.window, f_px=f_px, mode="unbuffered"), opened
        done = self.stream.step(x)
        return done[-1]["depth"] if done else None

    def prepare(self, frame, resolution):
        """A `[H, W, C]` picture as the normalized `[3, H, W]` the model reads, `resolution` on its short side."""
        if frame.dim() != 3 or frame.shape[2] < 1:
            raise ValueError(f"Depth reads a picture; this frame is {tuple(frame.shape)}")
        x = frame.permute(2, 0, 1).float()
        x = x / 255.0 if frame.dtype == torch.uint8 else x
        x = x[:3] if x.shape[0] >= 3 else x[:1].expand(3, -1, -1)
        h, w = x.shape[1:]
        if resolution and min(h, w) != resolution:
            scale = resolution / min(h, w)
            size = (max(MULTIPLE, round(h * scale)), max(MULTIPLE, round(w * scale)))
            x = F.interpolate(x[None], size=size, mode="bilinear", antialias=True, align_corners=False)[0]
        return (x.clamp(0.0, 1.0) - self.mean) / self.std


def fingerprint(frame):
    """What a picture is, cheaply: a hash of every 32nd texel, which a new frame of a feed moves."""
    return hash(frame[::32, ::32].contiguous().cpu().numpy().tobytes())


def span(depth, held):
    """The 1/distance of the farthest and the nearest thing in `depth`, moved a step from `held`."""
    inverse = (1.0 / depth.float())[::4, ::4].reshape(-1)
    found = torch.quantile(torch.nan_to_num(inverse, nan=0.0), torch.tensor([0.02, 0.98], device=inverse.device))
    return found if held is None else held + (found - held) * STEADY


def draw(depth, how, near, far, seen):
    """Depth in metres as the `[H, W]` values of a grey picture. An endless distance is the far end."""
    depth = depth.float()
    if how == "metres":
        return torch.nan_to_num(depth, nan=0.0, posinf=BRIGHTEST).clamp(0.0, BRIGHTEST)
    far = max(far, near + 1e-6)
    if how == "linear":
        return torch.nan_to_num((depth - near) / (far - near), nan=0.0).clamp(0.0, 1.0)
    low, high = (float(seen[0]), float(seen[1])) if how == "auto" else (1.0 / far, 1.0 / near)
    return torch.nan_to_num((1.0 / depth - low) / max(high - low, 1e-9), nan=0.0).clamp(0.0, 1.0)
