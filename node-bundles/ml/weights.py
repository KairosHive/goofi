"""Weights — a trained model's numbers, on a wire.

The door in from a training run. It reads the file once and repeats what it holds, because a node
wired up after the file was read would otherwise wait forever for a source that never speaks twice.
"""

import numpy as np
import goofi


class Weights(goofi.Node):
    """Hold a `.npy` or `.npz` array on the wire, reloading it when the path changes."""

    TAGS = ["input", "ml"]
    OUTPUTS = {"out": goofi.DataType.ARRAY}
    PRODUCER = True
    PARAMS = {
        "weights": {
            "file": goofi.StringParam("", doc="A `.npy` or `.npz` file a trainer wrote."),
            "key": goofi.StringParam("", doc="Which array in an `.npz`; the first one when empty."),
        },
        "common": {
            "max_frequency": goofi.FloatParam(2.0, 0.1, 100.0, doc="Rate cap: a static file needs no more."),
        },
    }

    def setup(self):
        self.read = None
        self.held = None

    def process(self):
        p = self.params.weights
        if not p.file:
            self.read, self.held = None, None
            return None
        if (p.file, p.key) != self.read:
            self.held = load(p.file, p.key)
            self.read = (p.file, p.key)
        return self.held


def load(path, key):
    """The array a path names, as float32."""
    got = np.load(path, allow_pickle=False)
    if isinstance(got, np.lib.npyio.NpzFile):
        names = list(got.files)
        if not names:
            raise ValueError(f"{path} holds no arrays")
        if key and key not in names:
            raise ValueError(f"{path} holds {names}, not `{key}`")
        got = got[key or names[0]]
    return np.ascontiguousarray(got, dtype=np.float32)
