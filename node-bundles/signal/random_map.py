"""Map any number of inputs to `outputs` values through a seeded random matrix.

All inputs are flattened and joined, then multiplied by a random (outputs x n) matrix whose
largest singular value is 1, so |Wx| <= |x|. The matrix is rebuilt whenever the seed, the output
count or the total input size changes.
"""

import numpy as np
import goofi


class RandomMap(goofi.Node):
    """Seeded random linear map from flattened inputs to m values."""

    TAGS = ['transform']
    INPUTS = {"input": goofi.InputSlot(goofi.DataType.ARRAY, required=True, multi=True)}
    OUTPUTS = {"out": goofi.DataType.ARRAY}
    PARAMS = {
        "map": {
            "outputs": goofi.NumParam(32, 1, 256, int=True, doc="Number of output values (m)."),
            "seed": goofi.NumParam(0, 0, 100000, int=True, doc="Seed of the random matrix."),
        }
    }

    def setup(self):
        self.key = None
        self.W = None

    def process(self, input):
        items = input.items() if isinstance(input, dict) else input if isinstance(input, (list, tuple)) else [input]
        frames = [i[-1] if isinstance(i, tuple) else i for i in items]
        x = np.concatenate([np.asarray(getattr(f, "data", f), dtype=np.float64).ravel() for f in frames if f is not None])
        p = self.params.map
        m, n = int(p.outputs), x.size
        key = (int(p.seed), m, n)
        if key != self.key:
            W = np.random.default_rng(int(p.seed)).standard_normal((m, n))
            W /= np.linalg.svd(W, compute_uv=False)[0]
            self.W, self.key = W, key
        return {"out": ((self.W @ x).astype(np.float32), {})}
