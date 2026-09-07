"""WaveletLeaders — c1 and c2, the log-cumulants of the multifractal spectrum.

c1 is where the spectrum peaks — the dominant Holder exponent, so how rough the thing is. c2 is
INTERMITTENCY: 0 says monofractal, and the more negative it gets the more the activity concentrates
into rare bursts. c2 is the one that separates a multifractal signal from a merely rough one, and
it is the robust, monotonic readout.

`signal` reads the last axis as time and answers one pair per channel; `image` reads the last two
axes as a field and answers one pair for it. The estimator follows the wavelet-leaders convention
so its numbers line up with the same measurement made offline.
"""

import numpy as np
import goofi


def leaders_1d(x, wavelet, levels):
    """Wavelet leaders per scale: the running maximum of the detail coefficients, coarsening up."""
    import pywt

    coeffs = pywt.wavedec(x, wavelet, level=levels, mode="periodization")
    detail = [np.abs(c) for c in coeffs[1:]][::-1]  # finest first
    out, prev = [], None
    for d in detail:
        # The leader at a point is the largest coefficient in its own neighbourhood AND in every
        # finer scale below it — that nesting is what makes a leader, not just a coefficient.
        ell = np.maximum.reduce([np.roll(d, 1), d, np.roll(d, -1)])
        if prev is not None:
            half = (prev.shape[0] // 2) * 2
            down = np.maximum(prev[0:half:2], prev[1:half:2])
            k = min(ell.shape[0], down.shape[0])
            ell[:k] = np.maximum(ell[:k], down[:k])
        out.append(ell)
        prev = ell
    return out


def leaders_2d(field, wavelet, levels):
    import pywt
    from scipy.ndimage import maximum_filter

    coeffs = pywt.wavedec2(field, wavelet, level=levels, mode="periodization")
    detail = [np.maximum.reduce([np.abs(h), np.abs(v), np.abs(d)]) for h, v, d in coeffs[1:]][::-1]
    out, prev = [], None
    for d in detail:
        ell = maximum_filter(d, size=3, mode="nearest")
        if prev is not None:
            ph, pw = (prev.shape[0] // 2) * 2, (prev.shape[1] // 2) * 2
            down = np.maximum.reduce(
                [prev[0:ph:2, 0:pw:2], prev[1:ph:2, 0:pw:2], prev[0:ph:2, 1:pw:2], prev[1:ph:2, 1:pw:2]]
            )
            h = min(ell.shape[0], down.shape[0])
            w = min(ell.shape[1], down.shape[1])
            ell[:h, :w] = np.maximum(ell[:h, :w], down[:h, :w])
        out.append(ell)
        prev = ell
    return out


def cumulants(leaders, first, last):
    """The slopes of the first two log-cumulants against scale, which are c1 and c2."""
    scales = np.arange(1, len(leaders) + 1)
    logs = [np.log(e[e > 0] + 1e-12) for e in leaders]
    c1 = np.array([v.mean() if v.size else 0.0 for v in logs])
    c2 = np.array([v.var() if v.size else 0.0 for v in logs])
    hi = min(last, len(leaders))
    lo = max(1, min(first, hi - 1))
    fit = slice(lo - 1, hi)
    if fit.stop - fit.start < 2:
        return 0.0, 0.0
    return (
        float(np.polyfit(scales[fit], c1[fit], 1)[0] / np.log(2)),
        float(np.polyfit(scales[fit], c2[fit], 1)[0] / np.log(2)),
    )


class WaveletLeaders(goofi.Node):
    """c1 and c2: how rough a thing is, and how much its roughness varies.

    c2 near 0 is monofractal; more negative is more multifractal.
    """

    TAGS = ["analysis"]
    INPUTS = {"data": goofi.InputSlot(goofi.DataType.ARRAY, required=True)}
    OUTPUTS = {"c1": goofi.DataType.ARRAY, "c2": goofi.DataType.ARRAY}
    PARAMS = {
        "leaders": {
            "mode": goofi.StringParam(
                "signal", options=["signal", "image"], doc="Read the last axis as time, or the last two as a field."
            ),
            "wavelet": goofi.StringParam("db3", options=["db2", "db3", "db4", "sym4"], doc="The analysing wavelet."),
            "levels": goofi.IntParam(0, 0, 12, doc="Scales to decompose over. 0 takes as many as the length allows."),
            "first": goofi.IntParam(2, 1, 10, doc="First scale in the straight-line fit; the finest are the noisiest."),
            "last": goofi.IntParam(8, 2, 14, doc="Last scale in it; the coarsest have the fewest coefficients."),
        }
    }

    def process(self, data):
        p = self.params.leaders
        x = np.asarray(data.data, dtype=np.float64)
        image = p.mode == "image"
        if image and x.ndim < 2:
            raise ValueError(f"`image` needs two axes to read as a field, got {list(x.shape)}")
        if x.ndim == 0:
            raise ValueError("nothing to measure")

        span = min(x.shape[-2:]) if image else x.shape[-1]
        levels = int(p.levels) or max(3, int(np.log2(max(span, 8))) - 3)
        if 2 ** (levels + 1) > span:
            raise ValueError(f"{span} samples is too short for {levels} scales")

        if image:
            flat = x.reshape(-1, *x.shape[-2:]) if x.ndim > 2 else x[None]
            pairs = [cumulants(leaders_2d(f, p.wavelet, levels), p.first, p.last) for f in flat]
            shape = x.shape[:-2]
        else:
            flat = x.reshape(-1, x.shape[-1])
            pairs = [cumulants(leaders_1d(f, p.wavelet, levels), p.first, p.last) for f in flat]
            shape = x.shape[:-1]

        c1 = np.array([a for a, _ in pairs], dtype=np.float32).reshape(shape or (1,))
        c2 = np.array([b for _, b in pairs], dtype=np.float32).reshape(shape or (1,))
        axes = {k: v for k, v in data.meta.get("channels", {}).items() if k != f"dim{x.ndim - 1}"}
        meta = {"channels": axes} if shape else {}
        return {"c1": (c1, meta), "c2": (c2, meta)}
