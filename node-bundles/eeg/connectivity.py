"""Connectivity — how every pair of channels relates, as a square matrix.

Takes `[C, T]` with time on the last axis and answers `[C, C]`, labelled with the channel names on
both axes so the matrix reads the same way round whichever way you slice it.

The phase measures are read off the analytic signal, which is only meaningful inside a band: a
phase difference taken across the whole spectrum is the difference of two numbers that each mean
nothing. Set `band` before trusting `plv`, `pli` or `wpli`.
"""

import numpy as np
import goofi

METHODS = [
    "wpli",
    "pli",
    "plv",
    "coherence",
    "imag_coherence",
    "aec",
    "aec_orth",
    "pearson",
    "covariance",
    "mutual_info",
]


def bandpass(x, sfreq, lo, hi):
    """Zero out the spectrum outside `[lo, hi]`. Nothing to design and nothing to go unstable."""
    if hi <= lo:
        return x
    spectrum = np.fft.rfft(x, axis=-1)
    freqs = np.fft.rfftfreq(x.shape[-1], 1.0 / sfreq)
    spectrum[..., (freqs < lo) | (freqs > hi)] = 0.0
    return np.fft.irfft(spectrum, n=x.shape[-1], axis=-1)


def analytic(x):
    """The analytic signal, without scipy: the one-sided spectrum, doubled."""
    n = x.shape[-1]
    spectrum = np.fft.fft(x, axis=-1)
    weight = np.zeros(n)
    weight[0] = 1.0
    if n % 2 == 0:
        weight[n // 2] = 1.0
        weight[1 : n // 2] = 2.0
    else:
        weight[1 : (n + 1) // 2] = 2.0
    return np.fft.ifft(spectrum * weight, axis=-1)


def mutual_info(x, bins):
    """Pairwise mutual information from a 2-D histogram, in nats."""
    c = x.shape[0]
    ranked = np.argsort(np.argsort(x, axis=-1), axis=-1)
    binned = (ranked * bins // x.shape[-1]).astype(np.int64)
    out = np.zeros((c, c))
    for i in range(c):
        for j in range(i, c):
            joint = np.bincount(binned[i] * bins + binned[j], minlength=bins * bins).reshape(bins, bins)
            joint = joint / joint.sum()
            px, py = joint.sum(1, keepdims=True), joint.sum(0, keepdims=True)
            nz = joint > 0
            out[i, j] = out[j, i] = float(np.sum(joint[nz] * np.log(joint[nz] / (px @ py)[nz])))
    return out


class Connectivity(goofi.Node):
    """A connectivity matrix over channels.

    Phase, amplitude and plain statistical couplings, all as `[C, C]`.
    """

    TAGS = ["analysis", "eeg", "connectivity"]
    INPUTS = {"data": goofi.InputSlot(goofi.DataType.ARRAY, required=True)}
    OUTPUTS = {"matrix": goofi.DataType.ARRAY}
    PARAMS = {
        "connectivity": {
            "method": goofi.StringParam("wpli", options=METHODS, doc="What counts as a relation between two channels."),
            "low": goofi.FloatParam(8.0, 0.0, 200.0, doc="Band low edge in Hz. The phase measures need one."),
            "high": goofi.FloatParam(13.0, 0.0, 200.0, doc="Band high edge in Hz; at or below `low` the band is off."),
            "bins": goofi.IntParam(16, 4, 64, doc="Histogram bins for `mutual_info`; the others ignore it."),
        },
        "adjacency": {
            "binarize": goofi.BoolParam(False, doc="Keep only the edges above `threshold`, as 1 and 0."),
            "threshold": goofi.FloatParam(0.5, 0.0, 1.0, doc="Where that cut falls, on the absolute value."),
            "absolute": goofi.BoolParam(False, doc="Report the magnitude, so an anticorrelation is a strong edge."),
        },
    }

    def process(self, data):
        p = self.params.connectivity
        x = np.squeeze(np.asarray(data.data, dtype=np.float64))
        if x.ndim != 2:
            raise ValueError(f"needs channels by time, got {list(np.shape(data.data))}")
        channels, samples = x.shape
        if samples < 8:
            raise ValueError(f"{samples} samples is too short to relate anything")
        sfreq = float(data.meta.get("sfreq") or 0.0)
        method = p.method

        if p.high > p.low:
            if sfreq <= 0:
                raise ValueError("a band needs `sfreq` on the frame, and this one carries none")
            x = bandpass(x, sfreq, p.low, p.high)
        x = x - x.mean(axis=-1, keepdims=True)

        if method == "covariance":
            matrix = np.cov(x)
        elif method == "pearson":
            matrix = np.corrcoef(x)
        elif method == "mutual_info":
            matrix = mutual_info(x, int(p.bins))
        else:
            z = analytic(x)
            if method == "plv":
                # The mean phasor over time, for every pair at once: one matrix product, where the
                # loop it replaces called `hilbert` once per PAIR.
                unit = z / (np.abs(z) + 1e-12)
                matrix = np.abs(unit @ unit.conj().T) / samples
            elif method in ("coherence", "imag_coherence"):
                cross = (z @ z.conj().T) / samples
                power = np.real(np.diag(cross))
                norm = np.sqrt(np.outer(power, power)) + 1e-24
                coherency = cross / norm
                matrix = np.abs(coherency) if method == "coherence" else np.abs(np.imag(coherency))
            elif method in ("aec", "aec_orth"):
                envelope = np.abs(z)
                if method == "aec":
                    matrix = np.corrcoef(envelope)
                else:
                    matrix = np.eye(channels)
                    for i in range(channels):
                        # Leakage is instantaneous and real, so what survives orthogonalizing y
                        # against x is the part that cannot be a copy of it.
                        orth = np.imag(z * np.conj(z[i]) / (np.abs(z[i]) + 1e-12))
                        both = np.vstack([envelope[i], np.abs(orth)])
                        row = np.corrcoef(both)[0, 1:]
                        matrix[i, :] = row
                    matrix = (matrix + matrix.T) / 2
            else:
                matrix = np.eye(channels)
                for i in range(channels):
                    imag = np.imag(z[i] * np.conj(z))
                    if method == "wpli":
                        row = np.abs(imag.sum(-1)) / (np.abs(imag).sum(-1) + 1e-24)
                    else:
                        # PLI is a MAGNITUDE: the sign of the lag is a convention, not a strength.
                        row = np.abs(np.sign(imag).mean(-1))
                    matrix[i, :] = row
                np.fill_diagonal(matrix, 1.0)

        matrix = np.nan_to_num(matrix, nan=0.0, posinf=0.0, neginf=0.0)
        a = self.params.adjacency
        if a.absolute:
            matrix = np.abs(matrix)
        if a.binarize:
            matrix = (np.abs(matrix) >= a.threshold).astype(np.float64)

        names = data.meta.get("channels", {}).get("dim0")
        axes = {"dim0": names, "dim1": names} if names else {}
        return matrix.astype(np.float32), {"channels": axes}
