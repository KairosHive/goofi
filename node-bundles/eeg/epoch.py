"""Average complete input windows captured by triggers."""

import numpy as np
import goofi


class Epoch(goofi.Node):
    """Capture each window once; use Buffer to set the window length."""

    TAGS = ["analysis", "eeg"]
    INPUTS = {
        "data": goofi.InputSlot(goofi.DataType.ARRAY, required=False, trigger=False),
        "trigger": goofi.InputSlot(goofi.DataType.ARRAY, required=False),
    }
    OUTPUTS = {
        "erp": goofi.DataType.ARRAY,
        "latest": goofi.DataType.ARRAY,
        "count": goofi.DataType.ARRAY,
    }
    PARAMS = {
        "epoch": {
            "baseline": goofi.StringParam(
                "none", options=["none", "whole"], doc="Subtract each channel's window mean."
            ),
            "reject": goofi.FloatParam(
                0.0, 0.0, 1.0e6, doc="Drop a window whose peak-to-peak passes this. 0 keeps every window."
            ),
            "level": goofi.FloatParam(0.5, -1.0e6, 1.0e6, doc="Capture if any trigger sample reaches this level."),
            "reset": goofi.PulseParam(doc="Forget the average and start again."),
        }
    }

    def setup(self):
        self.total = None
        self.count = 0

    def process(self, data=None, trigger=None):
        if trigger is None:
            return
        self.clear_input("trigger")
        p = self.params.epoch
        values = np.asarray(trigger.data, dtype=np.float64)
        if data is None or not np.any(values >= p.level):
            return
        window = np.asarray(data.data, dtype=np.float64)
        if window.ndim not in (1, 2) or not window.size:
            raise ValueError(f"needs a nonempty time or channels by time window, got {list(np.shape(data.data))}")
        self.clear_input("data")
        if p.reject > 0 and float(np.ptp(window, axis=-1).max()) > p.reject:
            return
        if self.total is None or self.total.shape != window.shape:
            self.total = window.copy()
            self.count = 1
        else:
            self.total += window
            self.count += 1
        average = self.total / self.count
        if p.baseline == "whole":
            window = window - window.mean(axis=-1, keepdims=True)
            average -= average.mean(axis=-1, keepdims=True)
        return {
            "erp": (average.astype(np.float32), data.meta),
            "latest": (window.astype(np.float32), data.meta),
            "count": (np.array([self.count], dtype=np.float32), {}),
        }

    def pulse_epoch_reset(self):
        self.total, self.count = None, 0
