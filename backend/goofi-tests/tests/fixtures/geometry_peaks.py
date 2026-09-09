import numpy as np
import goofi


class GeometryPeaks(goofi.Node):
    OUTPUTS = {k: goofi.DataType.ARRAY for k in ("peaks", "amps", "phases", "opposite")}
    PARAMS = {"source": {"state": goofi.StringParam("normal", ["normal", "empty", "invalid", "silence", "short", "wrap", "near"])},
              "common": {"autotrigger": goofi.BoolParam(True)}}

    def process(self):
        peaks = np.array([[30, np.nan, 10, 20], [40, np.nan, 10, 25]], dtype=np.float32)
        amps = np.array([[3, np.nan, 1, 2], [4, np.nan, 1, 2.5]], dtype=np.float32)
        phases = np.array([[0.3, np.nan, 0.1, 0.2], [0.4, np.nan, 0.1, 0.25]], dtype=np.float32)
        state = self.params.source.state
        if state == "empty":
            peaks[:] = np.nan
        elif state == "invalid":
            peaks[1, 2] = -10
        elif state == "silence":
            amps[:] = 0
        elif state == "short":
            peaks, amps, phases = peaks[:, 2:], amps[:, 2:], phases[:, 2:]
        elif state == "wrap":
            phases[:] = np.deg2rad(179.0)
        elif state == "near":
            peaks[:] = [20.00001, np.nan, 10, np.nan]
            phases[:] = np.deg2rad(179.0)
        return {"peaks": (peaks, {"sfreq": 256.0, "channels": {"dim0": ["Fz", "Cz"]}}),
                "amps": (amps, {}), "phases": (phases, {}), "opposite": (-phases, {})}
