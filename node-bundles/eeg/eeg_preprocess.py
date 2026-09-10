import numpy as np
from scipy import signal
import goofi


class StreamingFilter:
    """Causal SOS cascade; each new sample is processed exactly once."""

    def __init__(self, sfreq, low, high, mains, channels):
        if not 0 < low < high < sfreq / 2:
            raise ValueError("Require 0 < low < high < sample rate / 2")
        sections = []
        if mains:
            if not 0 < mains < sfreq / 2:
                raise ValueError("Notch frequency must be below Nyquist")
            b, a = signal.iirnotch(mains, 30, fs=sfreq)
            sections.append(signal.tf2sos(b, a))
        sections.append(signal.butter(4, [low, high], btype="bandpass", fs=sfreq, output="sos"))
        self.sos = np.concatenate(sections)
        self.zi = None
        self.channels = channels
        self.seen = 0

    def process(self, x):
        if self.zi is None:
            self.zi = signal.sosfilt_zi(self.sos)[:, None, :] * x[None, :, :1]
        y, self.zi = signal.sosfilt(self.sos, x, axis=-1, zi=self.zi)
        self.seen += x.shape[-1]
        return y


class EegPreprocess(goofi.Node):
    """Filter fresh EEG chunks continuously before the rolling PSD buffer.

    Preserves channel labels, units and sample rate. Uses a causal fourth-order
    Butterworth bandpass and Q=30 mains notch, retaining SOS state across chunks.
    Startup is initialized from the first sample and withheld for two seconds.
    Wire directly from LslIn, never from an overlapping rolling buffer.
    No clipping, normalization or automatic rereferencing is applied.
    """

    INPUTS = {"input": goofi.InputSlot(goofi.DataType.ARRAY, required=True)}
    OUTPUTS = {"out": goofi.DataType.ARRAY}
    PARAMS = {"eeg": {
        "low": goofi.FloatParam(1.0, 0.1, 10.0),
        "high": goofi.FloatParam(40.0, 10.0, 100.0),
        "mains": goofi.FloatParam(60.0, 0.0, 100.0),
    }}

    def setup(self):
        self.config = None
        self.filter = None

    def process(self, input):
        x = np.asarray(input.data, dtype=np.float64)
        if x.ndim != 2 or not x.shape[0]:
            raise ValueError("Expected EEG [channels, new samples]")
        if not x.shape[1]:
            return None
        if not np.isfinite(x).all():
            self.config = None
            raise ValueError("Nonfinite EEG samples: filter reset; check acquisition")
        fs = input.meta.get("sfreq")
        if fs is None or fs <= 0:
            raise ValueError("EEG input needs its actual sample rate")
        p = self.params.eeg
        config = (float(fs), float(p.low), float(p.high), float(p.mains), x.shape[0])
        if config != self.config:
            self.filter = StreamingFilter(*config)
            self.config = config
        before = self.filter.seen
        y = self.filter.process(x)
        skip = max(0, int(2 * fs) - before)
        if skip >= y.shape[-1]:
            return None
        return {"out": (y[:, skip:].astype(np.float32), input.meta)}
