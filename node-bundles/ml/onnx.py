"""Onnx — run any `.onnx` model, wired to whatever is driving it.

The `Decoder` node runs one shape of model: a latent in, an image out, with the steering a
generator wants. This one makes no such assumption. It reads what the file declares — how many
inputs, their shapes, their types — and fits whatever the patch sends into them.

    a classifier          one array in, one score vector out
    an embedding model    one array in, one feature vector out
    a generator           one latent in, one image out, with `layout` set to `image`

Wire as many senders into `input` as the model has inputs. A sender whose node name matches an
input name feeds that input; the rest fill the remaining inputs in the order they arrive. A
dimension the model leaves free is taken from the data, and an array that is too short is padded,
so a model still runs while a patch is half-built.
"""

import numpy as np
import goofi

LAYOUTS = ["raw", "image"]
RANGES = ["signed", "unit"]
PROVIDERS = ["CUDAExecutionProvider", "DmlExecutionProvider", "CPUExecutionProvider"]

# What onnxruntime calls a type, and what numpy calls it.
KINDS = {
    "tensor(float)": np.float32,
    "tensor(double)": np.float64,
    "tensor(float16)": np.float16,
    "tensor(int64)": np.int64,
    "tensor(int32)": np.int32,
    "tensor(int16)": np.int16,
    "tensor(int8)": np.int8,
    "tensor(uint8)": np.uint8,
    "tensor(bool)": np.bool_,
}


class Onnx(goofi.Node):
    """Run any ONNX model on the arrays wired into it."""

    TAGS = ["ml", "transform"]
    INPUTS = {"input": goofi.InputSlot(goofi.DataType.ARRAY, multi=True)}
    OUTPUTS = {"out": goofi.DataType.ARRAY}
    PARAMS = {
        "onnx": {
            "file": goofi.StringParam("", doc="Any `.onnx` file."),
            "output": goofi.StringParam(
                "",
                doc="Which of the model's outputs to emit, by name or by number. Empty takes the first.",
            ),
            "layout": goofi.StringParam(
                "raw",
                LAYOUTS,
                doc="`raw` emits what the model answered. `image` drops the batch, puts the channels "
                "last, and clamps — what a graphics node can draw.",
            ),
            "range": goofi.StringParam(
                "signed", RANGES, doc="For `image`, what the model's output spans: [-1, 1] or [0, 1]."
            ),
        },
        "common": {
            "max_frequency": goofi.FloatParam(
                30.0, 0.1, 240.0, doc="Rate cap: a model is asked for no more answers than a viewer draws."
            ),
        },
    }

    def setup(self):
        self.loaded = None
        self.session = None
        self.wants = []
        self.gives = []

    def process(self, input):
        p = self.params.onnx
        if not p.file:
            return None
        if p.file != self.loaded:
            self.session = open_model(p.file)
            self.wants = [(q.name, q.shape, KINDS.get(q.type, np.float32)) for q in self.session.get_inputs()]
            self.gives = [q.name for q in self.session.get_outputs()]
            self.loaded = p.file

        sent = [(src, np.asarray(d.data)) for src, d in input] if input else []
        feed = {name: fit(x, shape, kind) for (name, shape, kind), x in bind(sent, self.wants)}
        if len(feed) < len(self.wants):
            missing = [n for n, _, _ in self.wants if n not in feed]
            raise ValueError(f"{pretty(self.wants)} — nothing is wired to {', '.join(missing)}")

        got = self.session.run(self.gives, feed)
        out = np.asarray(got[self.chosen(p.output)])
        if p.layout == "image":
            out = image(out, p.range)
        return out, {
            "provider": self.session.get_providers()[0],
            "inputs": pretty(self.wants),
            "outputs": ", ".join(f"{n}{list(np.shape(g))}" for n, g in zip(self.gives, got)),
        }

    def chosen(self, which):
        """Which output the patch asked for, by name or by number."""
        if not which:
            return 0
        if which in self.gives:
            return self.gives.index(which)
        if which.strip("-").isdigit() and -len(self.gives) <= int(which) < len(self.gives):
            return int(which)
        raise ValueError(f"the model has no output {which!r}: it answers {', '.join(self.gives)}")


def open_model(path):
    """A session on the best provider this machine has."""
    import onnxruntime

    ready = onnxruntime.get_available_providers()
    return onnxruntime.InferenceSession(path, providers=[q for q in PROVIDERS if q in ready])


def sender(src):
    """The node a multi slot entry came from: it names each one `node.slot`."""
    return src.rsplit(".", 1)[0]


def bind(sent, wants):
    """Which array feeds which input: by matching name first, then by the order they arrived.

    Name first, because a patch that grows a second sender must not silently re-route the first.
    Rename a node to an input's name and it feeds that input wherever it sits in the wiring.
    """
    left = list(sent)
    pairs = []
    for want in wants:
        hit = next((q for q in left if sender(q[0]) == want[0] or q[0] == want[0]), None)
        if hit is None:
            hit = left[0] if left else None
        if hit is None:
            break
        left.remove(hit)
        pairs.append((want, hit[1]))
    return pairs


def fit(x, shape, kind):
    """One array shaped the way an input declares it, free dimensions taken from the data.

    A patch is wired one cable at a time, so an array that does not fill the shape is padded rather
    than refused: the model answers something while the patch is still half-built.
    """
    want = [d if isinstance(d, int) and d > 0 else -1 for d in shape]
    flat = np.asarray(x, dtype=kind).ravel()
    if want.count(-1) == 1:
        fixed = int(np.prod([d for d in want if d > 0])) if len(want) > 1 else 1
        want[want.index(-1)] = max(1, flat.size // max(fixed, 1))
    else:
        # More than one free dimension names no single answer; one row of everything else is the
        # only reading that cannot be wrong about which of them the data meant.
        want = [1 if d < 0 else d for d in want]
    need = int(np.prod(want)) if want else 1
    if flat.size < need:
        flat = np.concatenate([flat, np.zeros(need - flat.size, dtype=kind)])
    return flat[:need].reshape(want)


def image(out, span):
    """What a graphics node can draw: `[H, W, C]`, rows down, values in [0, 1]."""
    out = np.asarray(out, dtype=np.float32)
    while out.ndim > 3 and out.shape[0] == 1:
        out = out[0]
    if out.ndim == 2:
        out = out[:, :, None]
    if out.ndim != 3:
        raise ValueError(f"the model answered {out.shape}, which is no image")
    if out.shape[0] <= 4:
        out = out.transpose(1, 2, 0)
    if span == "signed":
        out = out * 0.5 + 0.5
    return np.ascontiguousarray(np.clip(out, 0.0, 1.0))


def pretty(wants):
    """The inputs a model declares, as one line a patch can read."""
    return ", ".join(f"{n}{[d if isinstance(d, int) else '?' for d in s]}" for n, s, _ in wants)
