import numpy as np
from scipy import signal
import goofi

NORMALIZE = ["none", "zscore", "robust", "minmax"]


class StreamingFilter:
    """Causal SOS cascade; each new sample is processed exactly once."""

    def __init__(self, sfreq, low, high, order, notches, width, channels):
        if not 0 < low < high < sfreq / 2:
            raise ValueError("Require 0 < low < high < sample rate / 2")
        sections = []
        for f in notches:
            if not 0 < f < sfreq / 2:
                raise ValueError(f"Notch frequency {f:g} Hz must be below Nyquist")
            b, a = signal.iirnotch(f, f / width, fs=sfreq)
            sections.append(signal.tf2sos(b, a))
        sections.append(signal.butter(order, [low, high], btype="bandpass", fs=sfreq, output="sos"))
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


class Quantile:
    """The P² estimate of one quantile of every value pushed, without keeping the values."""

    def __init__(self, p):
        self.n = 0
        self.q = []
        self.pos = [1.0, 2.0, 3.0, 4.0, 5.0]
        self.want = [1.0, 1.0 + 2.0 * p, 1.0 + 4.0 * p, 3.0 + 2.0 * p, 5.0]
        self.step = [0.0, p / 2.0, p, (1.0 + p) / 2.0, 1.0]
        self.p = p

    def push(self, x):
        self.n += 1
        q, pos = self.q, self.pos
        if self.n <= 5:
            q.append(x)
            if self.n == 5:
                q.sort()
            return
        if x < q[0]:
            q[0], k = x, 0
        elif x < q[1]:
            k = 0
        elif x < q[2]:
            k = 1
        elif x < q[3]:
            k = 2
        elif x <= q[4]:
            k = 3
        else:
            q[4], k = x, 3
        for i in range(k + 1, 5):
            pos[i] += 1.0
        for i in range(5):
            self.want[i] += self.step[i]
        for i in (1, 2, 3):
            d = self.want[i] - pos[i]
            if (d >= 1.0 and pos[i + 1] - pos[i] > 1.0) or (d <= -1.0 and pos[i - 1] - pos[i] < -1.0):
                d = 1.0 if d > 0 else -1.0
                below, above = pos[i] - pos[i - 1], pos[i + 1] - pos[i]
                parabolic = q[i] + d / (pos[i + 1] - pos[i - 1]) * (
                    (below + d) * (q[i + 1] - q[i]) / above + (above - d) * (q[i] - q[i - 1]) / below
                )
                if q[i - 1] < parabolic < q[i + 1]:
                    q[i] = parabolic
                else:
                    j = i + int(d)
                    q[i] = q[i] + d * (q[j] - q[i]) / (pos[j] - pos[i])
                pos[i] += d

    def value(self):
        return self.q[2] if self.n >= 5 else float(np.quantile(self.q, self.p)) if self.q else 0.0


class RunningScale:
    """One scale for every channel, from every value seen so far: each sample of each channel
    counts once, so a chunk `[C, T]` adds `C * T` observations."""

    def __init__(self, mode):
        self.mode = mode
        self.n = 0
        self.mean = 0.0
        self.m2 = 0.0
        self.low = np.inf
        self.high = -np.inf
        self.quantiles = [Quantile(0.25), Quantile(0.5), Quantile(0.75)] if mode == "robust" else []

    def apply(self, y):
        v = y.ravel()
        if self.mode == "zscore":
            # Welford's update, merged a chunk at a time.
            m, mean = v.size, float(v.mean())
            delta = mean - self.mean
            total = self.n + m
            self.mean += delta * m / total
            self.m2 += float(((v - mean) ** 2).sum()) + delta * delta * self.n * m / total
            self.n = total
            centre, half = self.mean, np.sqrt(self.m2 / self.n)
        elif self.mode == "minmax":
            self.low, self.high = min(self.low, float(v.min())), max(self.high, float(v.max()))
            centre, half = (self.low + self.high) / 2, (self.high - self.low) / 2
        else:
            lower, median, upper = self.quantiles
            for x in v.tolist():
                lower.push(x)
                median.push(x)
                upper.push(x)
            centre, half = median.value(), (upper.value() - lower.value()) / 2
        return (y - centre) / half if half > 0 else np.zeros_like(y)


def notch_list(text):
    """The notch frequencies a param spells, separated by commas or spaces."""
    try:
        return tuple(float(t) for t in text.replace(",", " ").split())
    except ValueError:
        raise ValueError(f"Notch frequencies must be numbers: {text!r}") from None


class EegPreprocess(goofi.Node):
    """Filter fresh EEG chunks continuously before the rolling PSD buffer.

    Preserves channel labels, units and sample rate. Uses a causal Butterworth
    bandpass of the given order and one notch per listed frequency, each of the
    given -3 dB width, retaining SOS state across chunks.
    Startup is initialized from the first sample and withheld for two seconds.
    Wire directly from LslIn, never from an overlapping rolling buffer.
    `normalize` then puts every channel on one running scale; no clipping or
    automatic rereferencing is applied.
    """

    INPUTS = {"input": goofi.InputSlot(goofi.DataType.ARRAY, required=True)}
    OUTPUTS = {"out": goofi.DataType.ARRAY}
    PARAMS = {"eeg": [
        {
            "low": goofi.NumParam(1.0, 0.1, 100.0, doc="Bandpass low cut in Hz."),
            "high": goofi.NumParam(40.0, 0.1, 100.0, doc="Bandpass high cut in Hz."),
            "order": goofi.NumParam(4, 1, 8, int=True, doc="Butterworth bandpass order."),
        },
        {
            "notch": goofi.StringParam("60", doc="Notch frequencies in Hz, comma or space separated; empty for none."),
            "width": goofi.NumParam(2.0, 0.1, 20.0, doc="The -3 dB width of each notch, in Hz."),
        },
        {
            "normalize": goofi.StringParam(
                "none", options=NORMALIZE,
                doc="One running scale over every sample of every channel seen so far. `zscore` is standard "
                "deviations from the mean (Welford), `robust` puts the middle half between -1 and 1 around the "
                "median, `minmax` puts the extremes at -1 and 1.",
            ),
        },
    ]}

    def setup(self):
        self.config = None
        self.filter = None
        self.scale = None

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
        config = (float(fs), float(p.low), float(p.high), int(p.order), notch_list(p.notch), float(p.width), x.shape[0])
        if config != self.config:
            self.filter = StreamingFilter(*config)
            self.config = config
            self.scale = None
        if p.normalize == "none":
            self.scale = None
        elif self.scale is None or self.scale.mode != p.normalize:
            self.scale = RunningScale(p.normalize)
        before = self.filter.seen
        y = self.filter.process(x)
        skip = max(0, int(2 * fs) - before)
        if skip >= y.shape[-1]:
            return None
        y = y[:, skip:]
        if self.scale is not None:
            y = self.scale.apply(y)
        return {"out": (y.astype(np.float32), input.meta)}
