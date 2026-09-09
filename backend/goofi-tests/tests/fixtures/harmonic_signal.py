import numpy as np
import goofi


class HarmonicSignal(goofi.Node):
    """Two fixed chords, with controls for incomplete and invalid frames."""

    PRODUCER = True
    OUTPUTS = {"out": goofi.DataType.ARRAY}
    PARAMS = {
        "signal": {
            "samples": goofi.IntParam(512, 1, 1024),
            "first": goofi.StringParam("chord", ["chord", "flat", "nan"]),
            "sfreq": goofi.BoolParam(True),
            "vector": goofi.BoolParam(False),
        }
    }

    def process(self):
        p = self.params.signal
        t = np.arange(p.samples) / 256.0
        rows = np.array([
            sum(np.sin(2 * np.pi * f * t) for f in (8, 12, 16)),
            sum(np.sin(2 * np.pi * f * t) for f in (7, 11, 17)),
        ], dtype=np.float32)
        if p.first == "flat":
            rows[0] = 0
        elif p.first == "nan":
            rows[0, 0] = np.nan
        meta = {"channels": {"dim0": ["Fz", "Cz"], "dim1": t.tolist()}}
        if p.vector:
            rows = rows[0]
            meta = {"channels": {"dim0": t.tolist()}}
        if p.sfreq:
            meta["sfreq"] = 256.0
        return rows, meta
