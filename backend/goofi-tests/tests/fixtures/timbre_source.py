import numpy as np
import goofi


class TimbreSource(goofi.Node):
    """Two tunings with padding, duplicates, and controllable voice gates."""

    PRODUCER = True
    OUTPUTS = {"tuning": goofi.DataType.ARRAY, "gates": goofi.DataType.ARRAY, "signal": goofi.DataType.ARRAY}
    PARAMS = {"source": {
        "mode": goofi.StringParam("normal", ["normal", "empty", "single", "reverse"]),
        "gate": goofi.FloatParam(1.0, 0.0, 1.0),
    }}

    def process(self):
        p = self.params.source
        tuning = np.array([[2, 1, 1.5, 1, 0, np.nan], [1.75, 1, 1.25, -1, np.nan, np.nan]], dtype=np.float32)
        if p.mode == "empty":
            tuning[:] = np.nan
        elif p.mode == "single":
            tuning[:] = np.nan
            tuning[:, 0] = 1
        elif p.mode == "reverse":
            tuning = tuning[:, ::-1].copy()
        t = np.arange(512) / 256
        return {
            "tuning": (tuning, {"channels": {"dim0": ["Fz", "Cz"]}}),
            "gates": np.array([[p.gate], [0], [p.gate]], dtype=np.float32),
            "signal": (np.sin(2 * np.pi * 8 * t).astype(np.float32), {"sfreq": 256.0}),
        }
