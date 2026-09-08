"""Decoder — a trained generator, wired to whatever is driving it.

Any `.onnx` that takes one float vector and answers one image: a GAN generator, a VAE decoder, a
latent diffusion decoder. `training/fastgan.py` writes one from a folder of images.

What the node does beyond running the file is the part a patch needs. A generator wants a latent of
a few hundred numbers and a patch has five band powers, so each wired number gets a DIRECTION drawn
from `seed` and moves the latent along it. Five signals then steer one image smoothly, every one of
them doing something of its own, and the latent stays on the sphere the model was trained on
however hard they push.

Where the model's own axes already mean something — the components of `training/pixel_pca.py`, or a
generator whose directions have been sorted — `axes` is set to `direct` instead, and then wired
number i is axis i and the value IS its weight.
"""

import numpy as np
import goofi

RANGES = ["signed", "unit"]
AXES = ["drawn", "direct"]
PROVIDERS = ["CUDAExecutionProvider", "DmlExecutionProvider", "CPUExecutionProvider"]


class Decoder(goofi.Node):
    """Run a trained generator, steering its latent from the patch."""

    TAGS = ["image", "generator", "ml"]
    INPUTS = {"drive": goofi.InputSlot(goofi.DataType.ARRAY)}
    OUTPUTS = {"out": goofi.DataType.ARRAY}
    PRODUCER = True
    PARAMS = {
        "decoder": {
            "file": goofi.StringParam("", doc="An `.onnx` generator: one float vector in, one image out."),
            "range": goofi.StringParam("signed", RANGES, doc="What the model's output spans: [-1, 1] or [0, 1]."),
            "axes": goofi.StringParam(
                "drawn",
                AXES,
                doc="`drawn`: each wired number takes a random direction from `seed`. `direct`: wired "
                "number i IS latent i, which is what a model whose axes already mean something wants.",
            ),
            "spread": goofi.FloatParam(1.0, 0.0, 4.0, doc="How far from the mean latent to sit. Below 1 is safer, and duller."),
            "reach": goofi.FloatParam(1.0, 0.0, 8.0, doc="How hard a wired number pushes its own axis."),
            "smooth": goofi.FloatParam(0.8, 0.0, 0.999, doc="How much of the last drive to keep, so a jumpy signal walks."),
            "seed": goofi.IntParam(0, 0, 1000000, doc="Which mean latent, and which direction each wired number takes."),
        },
        "common": {
            "max_frequency": goofi.FloatParam(30.0, 0.1, 240.0, doc="Rate cap: a generator is asked for no more frames than a viewer draws."),
        },
    }

    def setup(self):
        self.loaded = None
        self.session = None
        self.width = 0
        self.drawn = (None, None, None)
        self.held = None

    def process(self, drive):
        p = self.params.decoder
        if not p.file:
            return None
        if p.file != self.loaded:
            self.session, self.width = open_model(p.file)
            self.loaded, self.drawn, self.held = p.file, (None, None, None), None

        x = np.asarray(drive.data, dtype=np.float32).ravel() if drive is not None else np.zeros(0, np.float32)
        self.held = x if self.held is None or self.held.shape != x.shape else p.smooth * self.held + (1.0 - p.smooth) * x
        latent = self.steer(self.held, p)

        got = self.session.run(None, {self.session.get_inputs()[0].name: latent[None]})[0]
        return frame(got, p.range), {"provider": self.session.get_providers()[0], "latent": self.width}

    def steer(self, x, p):
        """The latent the drive asks for: one drawn direction per wired number, or one axis each."""
        if p.axes == "direct":
            # No sphere here. A model whose axes mean something is one where `2` on axis 0 must be
            # two of THAT, and pushing the vector back out to the shell would spend it on the rest.
            z = np.zeros(self.width, np.float32)
            take = min(x.size, self.width)
            z[:take] = x[:take] * p.reach
            return (z * p.spread).astype(np.float32)
        drawn_for, base, dirs = self.drawn
        if drawn_for != (p.seed, x.shape[0]):
            rng = np.random.default_rng(p.seed)
            base = rng.standard_normal(self.width).astype(np.float32)
            dirs = rng.standard_normal((x.shape[0], self.width)).astype(np.float32)
            self.drawn = ((p.seed, x.shape[0]), base, dirs)
        z = base + p.reach * (x @ dirs) if x.size else base
        scale = p.spread * np.sqrt(self.width) / (np.linalg.norm(z) + 1e-6)
        return (z * scale).astype(np.float32)


def open_model(path):
    """A session on the best provider this machine has, and the latent width the model wants."""
    import onnxruntime

    ready = onnxruntime.get_available_providers()
    session = onnxruntime.InferenceSession(path, providers=[q for q in PROVIDERS if q in ready])
    shape = session.get_inputs()[0].shape
    width = next((int(d) for d in reversed(shape) if isinstance(d, int) and d > 1), 0)
    if not width:
        raise ValueError(f"{path} takes {shape}, which names no latent width")
    return session, width


def frame(got, span):
    """One image out of whatever the model answered: `[H, W, C]`, rows down, values in [0, 1]."""
    out = np.asarray(got, dtype=np.float32)
    out = out[0] if out.ndim == 4 else out
    if out.ndim != 3:
        raise ValueError(f"the model answered {out.shape}, which is no image")
    if out.shape[0] <= 4:
        out = out.transpose(1, 2, 0)
    if span == "signed":
        out = out * 0.5 + 0.5
    return np.ascontiguousarray(np.clip(out, 0.0, 1.0))
