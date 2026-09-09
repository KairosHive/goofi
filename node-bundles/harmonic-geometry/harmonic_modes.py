"""HarmonicModes: bounded Biotuner Chladni modes for shader synthesis.

input and target are harmonic TABLEs. Each is mapped independently by Biotuner.
Then mode coordinates, amplitudes, and phases interpolate by component rank.
Fractional intermediate modes draw a continuous visual field; they are not
eigenmodes of the original closed plate. Use fixed fields plus GeometryBlend
to retain integer endpoint fields instead.

modes is [4,32]: m, n, amplitude, phase. Unused columns have zero amplitude.
"""

import numpy as np
from biotuner.harmonic_geometry import ratios_to_modes
import goofi


class HarmonicModes(goofi.Node):
    """Map harmonic ratios to Chladni modes and interpolate two mode structures."""

    TAGS = ["transform"]
    INPUTS = {"input": goofi.InputSlot(goofi.DataType.TABLE, required=True),
              "target": goofi.InputSlot(goofi.DataType.TABLE, required=False),
              "mix": goofi.InputSlot(goofi.DataType.ARRAY, required=False)}
    OUTPUTS = {"modes": goofi.DataType.ARRAY}
    PARAMS = {"modes": {
        "strategy": goofi.StringParam("best_simple", ["stern_brocot", "continued_fraction", "rounded", "best_simple"], doc="Biotuner's ratio-to-mode mapping."),
        "max_mode": goofi.IntParam(12, 1, 24, doc="Largest mode index on either axis."),
        "mix": goofi.FloatParam(0.0, 0.0, 1.0, doc="Interpolation from input modes to target modes; wire mix or set here."),
    }}

    def _modes(self, data):
        table, p = data.table, self.params.modes
        try:
            r, a, ph = [np.asarray(table[k].data, dtype=np.float64) for k in ("ratios", "amplitudes", "phases")]
        except KeyError as e:
            raise ValueError("HarmonicModes needs HarmonicMorph.harmonic") from e
        if r.ndim != 1 or a.shape != r.shape or ph.shape != r.shape or len(r) > 32:
            raise ValueError("Mode input needs aligned vectors of at most 32 components")
        if not np.all(np.isfinite(np.r_[r, a, ph])) or np.any(r <= 0) or np.any(a < 0):
            raise ValueError("Mode components must be finite, with positive ratios and nonnegative amplitudes")
        modes = np.zeros((4, 32), dtype=np.float64)
        if len(r):
            modes[:2, :len(r)] = np.array(ratios_to_modes(r, strategy=p.strategy, max_mode=p.max_mode)).T
            modes[2, :len(r)] = a
            modes[3, :len(r)] = ph
        return modes, len(r)

    def process(self, input, target=None, mix=None):
        modes, na = self._modes(input)
        t = self.params.modes.mix
        if mix is not None:
            v = np.asarray(mix.data).ravel()
            if len(v) != 1 or not np.isfinite(v[0]):
                raise ValueError("HarmonicModes mix needs one finite value")
            t = float(np.clip(v[0], 0, 1))
        if target is not None:
            other, nb = self._modes(target)
            n = min(na, nb)
            modes[:2, :n] = (1-t)*modes[:2, :n]+t*other[:2, :n]
            modes[3, :n] += t*np.angle(np.exp(1j*(other[3, :n]-modes[3, :n])))
            modes[[0, 1, 3], na:nb] = other[[0, 1, 3], na:nb]
            modes[2] = (1-t)*modes[2]+t*other[2]
        return modes.astype(np.float32), {"channels": {"dim0": ["m", "n", "amplitude", "phase"]}}
