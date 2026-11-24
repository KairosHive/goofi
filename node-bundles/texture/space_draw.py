"""SpaceDraw draws a texture from a place in the material space, the photographs' hybrid.

latent-lab's `ip_space.py` drawing, live. A place in the space is a CLIP ViT-H image vector, the one
the IP-Adapter reads. Stable Diffusion 1.5 with the SD 1.5 IP-Adapter draws what that vector means,
the LCM-LoRA brings the drawing down to a few steps, and TAESD decodes it in a few milliseconds. A
place between photographs (MaterialSpace's `place` on a map or a tour) draws a material that none of
them is, a hybrid of their surfaces.

The drawing starts from the same noise every frame, so a place that moves smoothly draws a texture
that changes smoothly. `drift` moves the noise itself along a slow great circle through the seeds,
so the texture's layout flows even where the place stands still. `smooth` carries part of the last
frame into the next, for the small flicker a diffusion model keeps between nearby places.

On CUDA a 512 px frame takes tens of milliseconds a step. On a CPU it takes many seconds, and the
status names the device. The models come from the Hugging Face cache, fetched once.
"""

import hashlib
import math
import time

import numpy as np
import torch

import goofi

BASE = "stable-diffusion-v1-5/stable-diffusion-v1-5"
ADAPTER = "h94/IP-Adapter"
LCM = "latent-consistency/lcm-lora-sdv1-5"
TAESD = "madebyollin/taesd"
FRAMES = ["1:1", "3:2", "2:3", "4:3", "3:4", "16:9", "9:16"]
LONG = [256, 384, 512, 640, 768]
RESEND = 1.0  # seconds: an unchanged drawing is sent again this often, so a viewer opened later sees it


def device():
    if torch.cuda.is_available():
        return "cuda"
    if torch.get_num_threads() > 8:
        torch.set_num_threads(8)
    return "cpu"


def frame_size(frame, long):
    """Width and height for the frame's shape, both multiples of 64 as the UNet needs."""
    a, b = (int(x) for x in frame.split(":"))
    w, h = (long, long * b / a) if a >= b else (long * a / b, long)
    return max(64, int(round(w / 64)) * 64), max(64, int(round(h / 64)) * 64)


def slerp(a, b, t):
    """Along the sphere between two noises, so a mix keeps the spread noise has."""
    fa, fb = a.flatten().double(), b.flatten().double()
    omega = torch.acos((fa @ fb / (fa.norm() * fb.norm())).clamp(-1, 1))
    if float(omega) < 1e-4:
        return a + t * (b - a)
    return ((torch.sin((1 - t) * omega) * a.double() + torch.sin(t * omega) * b.double()) / torch.sin(omega)).to(a.dtype)


def clamp(v, lo, hi):
    return min(max(float(v), lo), hi)


class SpaceDraw(goofi.Node):
    """Draw a hybrid texture from a place in the material space, by SD 1.5 and the IP-Adapter."""

    TAGS = ["image", "ml", "generator"]
    INPUTS = {
        # Not required: the node waits quietly for its first place, where a required slot reports an
        # error on every tick before the first frame arrives.
        "place": goofi.InputSlot(goofi.DataType.ARRAY),
    }
    OUTPUTS = {"image": goofi.DataType.ARRAY, "status": goofi.DataType.STRING}
    PARAMS = {
        "draw": {
            "scale": goofi.NumParam(1.0, 0.0, 1.5, doc="How strongly the place steers the drawing. At 1 the material "
                                    "speaks alone, below it the prompt and the model's own habits come through."),
            "prompt": goofi.StringParam("", doc="Words beside the place, such as `macro photograph`. Empty lets the "
                                        "space speak alone."),
            "steps": goofi.NumParam(4, 1, 8, int=True, doc="LCM steps. 4 is the lab's default, 2 is faster and "
                                    "softer."),
            "seed": goofi.NumParam(0, 0, 9999, int=True, doc="The noise the drawing starts from, which sets the "
                                   "texture's layout."),
            "drift": goofi.NumParam(0.0, 0.0, 10.0, doc="How fast the noise travels on by itself, in seeds a minute. "
                                    "The layout then flows while the material follows the place. 0 holds the layout."),
            "smooth": goofi.NumParam(0.0, 0.0, 0.95, doc="Carry this share of the last frame into the next, against "
                                     "the flicker between nearby places."),
        },
        "frame": {
            "frame": goofi.StringParam("1:1", FRAMES, doc="The shape of the drawing."),
            "long": goofi.NumParam(512, 256, 768, int=True, options=LONG, doc="Pixels on the long side. SD 1.5 draws "
                                   "best at 512."),
        },
        "model": {
            "base": goofi.StringParam(BASE, doc="The SD 1.5 checkpoint, a Hugging Face name or a folder."),
            "fast_decode": goofi.BoolParam(True, doc="Decode with TAESD, a few milliseconds a frame. Off, the full VAE "
                                           "decodes, sharper and several times slower."),
        },
        "common": {"autotrigger": goofi.BoolParam(True), "max_frequency": goofi.NumParam(30.0, 1.0, 60.0)},
    }

    def setup(self):
        self.pipe = None
        self.built = None
        self.noises = {}
        self.key = None
        self.result = None
        self.sent = 0.0
        self.last = None
        self.travelled = 0.0
        self.clock = None

    def model(self, dev):
        q = self.params.model
        want = (q.base, bool(q.fast_decode), dev)
        if self.pipe is not None and self.built == want:
            return self.pipe
        from diffusers import AutoencoderKL, AutoencoderTiny, AutoPipelineForText2Image, LCMScheduler

        dtype = torch.float16 if dev == "cuda" else torch.float32

        def load(fn, *args, **kw):
            try:
                return fn(*args, local_files_only=True, **kw)
            except Exception:  # not in the cache yet: fetch it once
                return fn(*args, **kw)

        self.pipe = None
        pipe = load(AutoPipelineForText2Image.from_pretrained, q.base, torch_dtype=dtype, safety_checker=None,
                    requires_safety_checker=False)
        pipe.load_ip_adapter(ADAPTER, subfolder="models", weight_name="ip-adapter_sd15.safetensors",
                             image_encoder_folder=None)  # places arrive as vectors, no encoder is needed
        pipe.load_lora_weights(LCM, adapter_name="fast")
        pipe.set_adapters(["fast"], [1.0])
        pipe.scheduler = LCMScheduler.from_config(pipe.scheduler.config)
        if q.fast_decode:
            pipe.vae = load(AutoencoderTiny.from_pretrained, TAESD, torch_dtype=dtype)
        elif not isinstance(pipe.vae, AutoencoderKL):
            pipe.vae = load(AutoencoderKL.from_pretrained, q.base, subfolder="vae", torch_dtype=dtype)
        pipe = pipe.to(dev)
        pipe.set_progress_bar_config(disable=True)
        self.pipe, self.built, self.dtype = pipe, want, dtype
        self.noises = {}
        return pipe

    def noise(self, seed, w, h, dev):
        """The starting noise of one seed, kept."""
        key = (seed, w, h, dev)
        if key not in self.noises:
            if len(self.noises) > 16:
                self.noises.pop(next(iter(self.noises)))
            g = torch.Generator("cpu").manual_seed(int(seed))
            self.noises[key] = torch.randn((1, 4, h // 8, w // 8), generator=g).to(dev, self.dtype)
        return self.noises[key]

    def process(self, place=None):
        if place is None:
            return None
        p = self.params
        v = np.asarray(place.data, np.float32).reshape(-1)
        now = time.monotonic()
        dt = 0.0 if self.clock is None else min(max(now - self.clock, 0.0), 1.0)
        self.clock = now
        drift = clamp(p.draw.drift, 0.0, 60.0)
        self.travelled += dt * drift / 60.0
        key = (hashlib.blake2b(v.tobytes(), digest_size=12).hexdigest(), repr(vars(p.draw)), repr(vars(p.frame)),
               repr(vars(p.model)))
        settling = self.last is not None and clamp(p.draw.smooth, 0.0, 0.95) > 0 and getattr(self, "moved", 1.0) > 2e-3
        if key == self.key and drift <= 0 and not settling:
            if self.result is not None and now - self.sent >= RESEND:
                self.sent = now
                return self.result
            return None
        self.key = key
        started = time.perf_counter()
        dev = device()
        pipe = self.model(dev)
        w, h = frame_size(p.frame.frame, int(p.frame.long))
        seed = int(p.draw.seed)
        here = seed + self.travelled
        a = int(math.floor(here))
        t = here - a
        t = t * t * (3.0 - 2.0 * t)
        latents = slerp(self.noise(a, w, h, dev), self.noise(a + 1, w, h, dev), t) if drift > 0 or t > 0 \
            else self.noise(a, w, h, dev)
        if v.size != 1024:
            raise ValueError(f"`place` takes the IP-Adapter's 1024-number image vector. This one has {v.size}")
        emb = torch.as_tensor(v, device=dev, dtype=self.dtype).view(1, 1, -1)
        pipe.set_ip_adapter_scale(clamp(p.draw.scale, 0.0, 3.0))
        with torch.inference_mode():
            out = pipe(prompt=p.draw.prompt, ip_adapter_image_embeds=[emb], num_inference_steps=int(p.draw.steps),
                       guidance_scale=1.0, height=h, width=w, latents=latents, output_type="np").images[0]
        img = np.clip(np.asarray(out, np.float32), 0.0, 1.0)
        keep = clamp(p.draw.smooth, 0.0, 0.95)
        if keep > 0 and self.last is not None and self.last.shape == img.shape:
            new = keep * self.last + (1.0 - keep) * img
            self.moved = float(np.abs(new - self.last).mean())
            img = new
        else:
            self.moved = 1.0
        self.last = img
        ms = (time.perf_counter() - started) * 1000.0
        drifting = f" | noise at seed {here:.2f}" if drift > 0 else ""
        self.result = {"image": (np.ascontiguousarray(img), {"channels": {"dim2": ["r", "g", "b"]}}),
                       "status": f"{w} x {h} | {int(p.draw.steps)} steps{drifting} | {ms:.0f} ms on {dev}"}
        self.sent = now
        return self.result
