"""Epoch — the average response to a repeated event, built as the events arrive.

Continuous `[C, T]` on `data`, a trigger on `trigger`, and out comes the running average across
every epoch so far. The time axis is labelled in SECONDS relative to the trigger, so a viewer
shows real latency and a negative number is before the event.

Two things make this an ERP rather than a windowed average. It keeps a rolling buffer, so an epoch
can start BEFORE the trigger that defined it — without pre-stimulus samples there is no baseline
to correct against, and an ERP without baseline correction is a drift measurement. And the trigger
fires on a rising EDGE, so a trigger channel that stays high does not re-fire on every frame.
"""

import numpy as np
import goofi


class Epoch(goofi.Node):
    """Averages the signal around each trigger, as they come.

    Outputs the running average, the `latest` epoch taken, and how many have gone into it.
    """

    TAGS = ["analysis", "eeg"]
    INPUTS = {
        "data": goofi.InputSlot(goofi.DataType.ARRAY, required=True),
        "trigger": goofi.InputSlot(goofi.DataType.ARRAY, required=False),
    }
    OUTPUTS = {
        "erp": goofi.DataType.ARRAY,
        "latest": goofi.DataType.ARRAY,
        "count": goofi.DataType.ARRAY,
    }
    PARAMS = {
        "epoch": {
            "before": goofi.FloatParam(0.2, 0.0, 5.0, doc="Seconds kept before the trigger. The baseline lives here."),
            "after": goofi.FloatParam(0.8, 0.05, 10.0, doc="Seconds kept after it."),
            "baseline": goofi.StringParam(
                "pre", options=["none", "pre", "whole"], doc="What to subtract: nothing, the pre-trigger mean, or the epoch's."
            ),
            "reject": goofi.FloatParam(
                0.0, 0.0, 1.0e6, doc="Drop an epoch whose peak-to-peak passes this. 0 keeps every one."
            ),
            "level": goofi.FloatParam(0.5, -1.0e6, 1.0e6, doc="The trigger fires when its value rises past this."),
            "reset": goofi.PulseParam(doc="Forget the average and start again."),
        }
    }

    def setup(self):
        self.buffer = None
        self.written = 0
        self.pending = []
        self.total = None
        self.count = 0
        self.last = None
        self.armed = True

    def process(self, data, trigger=None):
        p = self.params.epoch
        x = np.atleast_2d(np.asarray(data.data, dtype=np.float64))
        if x.ndim != 2:
            raise ValueError(f"needs channels by time, got {list(np.shape(data.data))}")
        sfreq = float(data.meta.get("sfreq") or 0.0)
        if sfreq <= 0:
            raise ValueError("an epoch is measured in seconds, and this frame carries no `sfreq`")
        channels, incoming = x.shape

        before = int(round(p.before * sfreq))
        after = max(1, int(round(p.after * sfreq)))
        width = before + after
        # Room for one whole epoch plus whatever a single frame can add, so a trigger at the very
        # start of a frame still has its pre-stimulus samples in hand. The buffer only ever GROWS:
        # a block producer emits 7 samples one frame and 8 the next, and a buffer sized to the
        # frame would be rebuilt on that alone — losing the history every epoch is measured from.
        need = width + max(incoming, 1) * 2
        if self.buffer is None or self.buffer.shape[0] != channels:
            self.buffer = np.zeros((channels, need))
            self.written = 0
            self.pending = []
        elif self.buffer.shape[1] < need:
            grown = np.zeros((channels, need))
            grown[:, -self.buffer.shape[1] :] = self.buffer
            self.buffer = grown
        span = self.buffer.shape[1]
        if incoming >= span:
            self.buffer[:] = x[:, -span:]
        else:
            self.buffer = np.roll(self.buffer, -incoming, axis=1)
            self.buffer[:, -incoming:] = x
        self.written += incoming

        if self.total is not None and self.total.shape != (channels, width):
            self.total, self.count, self.last = None, 0, None

        if trigger is not None:
            v = np.nan_to_num(np.asarray(trigger.data, dtype=np.float64).ravel(), nan=0.0)
            # Every sample of the trigger frame is looked at, not only its last: a trigger lands
            # wherever it lands inside a frame, and reading one sample missed all but the last.
            # A trigger frame the same length as the data is read sample for sample, so the epoch
            # starts where the event did; any other length is one event for the whole frame.
            base = self.written - incoming
            if v.size == incoming:
                for k, value in enumerate(v):
                    high = value >= p.level
                    if high and self.armed:
                        self.pending.append(base + k + after)
                    self.armed = not high
            elif v.size:
                high = bool(np.max(v) >= p.level)
                if high and self.armed:
                    self.pending.append(base + after)
                self.armed = not high

        ready = [end for end in self.pending if end <= self.written]
        self.pending = [end for end in self.pending if end > self.written]
        for end in ready:
            stop = span - (self.written - end)
            start = stop - width
            if start < 0:
                continue
            epoch = self.buffer[:, start:stop].copy()
            if p.baseline == "pre" and before > 0:
                epoch -= epoch[:, :before].mean(axis=1, keepdims=True)
            elif p.baseline == "whole":
                epoch -= epoch.mean(axis=1, keepdims=True)
            if p.reject > 0 and float(np.ptp(epoch, axis=1).max()) > p.reject:
                continue
            self.total = epoch if self.total is None else self.total + epoch
            self.count += 1
            self.last = epoch

        if self.total is None:
            # Nothing has been averaged yet, so the shape is right and the content is empty rather
            # than absent: a viewer opened on this slot draws a flat line instead of waiting.
            self.total = np.zeros((channels, width))
            self.last = np.zeros((channels, width))

        times = [float(v) for v in np.round(np.arange(width) / sfreq - before / sfreq, 6)]
        axes = {k: v for k, v in data.meta.get("channels", {}).items() if k != "dim1"}
        axes["dim1"] = times
        meta = {"channels": axes, "sfreq": float(sfreq)}
        average = self.total / max(self.count, 1)
        return {
            "erp": (average.astype(np.float32), meta),
            "latest": (self.last.astype(np.float32), meta),
            "count": (np.array([self.count], dtype=np.float32), {}),
        }

    def pulse_epoch_reset(self):
        self.total, self.count, self.last, self.pending = None, 0, None, []
