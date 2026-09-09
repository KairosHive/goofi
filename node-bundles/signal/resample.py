"""Resample each complete window independently with an anti-alias filter.

The output length rounds up to a whole sample. No history is kept between windows.
"""

from fractions import Fraction

import numpy as np
from scipy.signal import resample_poly
import goofi


class Resample(goofi.Node):
    """Change the sample rate of a complete window; use Buffer to set its length."""

    TAGS = ["transform"]
    INPUTS = {"input": goofi.InputSlot(goofi.DataType.ARRAY, required=True)}
    OUTPUTS = {"out": goofi.DataType.ARRAY}
    PARAMS = {
        "resample": {
            "sfreq": goofi.FloatParam(250.0, 1.0, 100000.0, doc="Target rate in Hz. The output carries the actual rate."),
            "axis": goofi.IntParam(-1, -8, 7, options=[-2, -1, 0, 1, 2], doc="Which axis holds the samples. -1 is time."),
        }
    }

    def process(self, input):
        p = self.params.resample
        source = input.meta.get("sfreq")
        if source is None or not np.isfinite(source) or source <= 0:
            raise ValueError("this node needs a positive finite sample rate")
        if not np.isfinite(p.sfreq) or p.sfreq <= 0:
            raise ValueError("the target sample rate must be positive and finite")
        if not -input.data.ndim <= p.axis < input.data.ndim:
            raise ValueError(f"axis {p.axis} is outside this {input.data.ndim}-dimensional window")
        axis = p.axis if p.axis >= 0 else p.axis + input.data.ndim
        ratio = Fraction(p.sfreq / source).limit_denominator(100000)
        up, down = ratio.numerator, ratio.denominator
        if up == 0 or max(up, down) > 100000:
            raise ValueError("rate conversion is too large; choose a target closer to the input rate")
        out = resample_poly(np.asarray(input.data, dtype=np.float64), up, down, axis=axis)
        axes = {k: v for k, v in input.meta.get("channels", {}).items() if k != f"dim{axis}"}
        return out.astype(np.float32), {**input.meta, "channels": axes, "sfreq": source * up / down}
