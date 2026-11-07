"""Belief tracks each channel of a signal with a Bayesian filter, the inside of a Markov blanket.

The filter holds a belief about the hidden cause of each channel, a mean and a precision, and
updates it sample by sample from the prediction error weighted by precision. Its outputs are the
quantities active inference is built from.

- mean. The belief about the hidden value of each channel.
- precision. How sure the belief is, 1 / variance.
- error. The prediction error before the update, in units of the predicted spread (a z-score).
- surprise. -log p(sample) under the prediction, per channel, the term free energy bounds.
- volatility. How fast the hidden value is believed to move, as the log of its step variance.

By `kalman` each channel is a random walk with a fixed step variance (`drift`), the optimal linear
filter. By `hgf` it is the two-level Hierarchical Gaussian Filter of Mathys et al. (2014), updated in the eHGF order
of Weber et al. (2023): a second
level learns the walk's own volatility, so the belief speeds up when the signal turns restless and
settles when it calms. `coupling` sets how strongly the second level scales the first, and `meta`
how fast the volatility itself may move.

`log` takes the log of each sample first, for band powers. `normalize` z-scores each channel
against its own running mean and spread (`memory` seconds, learned from the first samples on) before
it reaches the filter, so the same settings serve band powers, heart rate or a shader's statistic.
A frame [C] is one sample per channel, and a frame [C, N] is N samples in time order, filtered one
by one. A sample that is not finite, or not positive under `log`, is missing, and that channel's
belief waits for the next one. Pure numpy, so goofi runs it inside its own process.
"""

import time

import numpy as np

import goofi

METHODS = ["hgf", "kalman"]
LOG2PI = float(np.log(2.0 * np.pi))


def samples_of(x, name):
    """A frame as [C, N]: one sample per channel [C], N samples [C, N], or a scalar."""
    a = np.asarray(x, dtype=np.float64)
    if a.ndim == 0:
        return a.reshape(1, 1)
    if a.ndim == 1:
        return a[:, None]
    if a.ndim == 2:
        return a
    raise ValueError(f"`{name}` takes [channels] or [channels, samples]. This frame is {list(a.shape)}")


class Filter:
    """The per-channel state of a two-level HGF. With `coupling` 0 it is a Kalman filter on a random
    walk whose step variance is exp(`drift`)."""

    def __init__(self, n):
        self.mu1 = np.zeros(n)
        self.pi1 = np.ones(n)
        self.mu2 = np.zeros(n)
        self.pi2 = np.ones(n)

    def step(self, u, pi_u, kappa, omega, theta):
        """One sample `u` [C] with observation precision `pi_u`. Returns error, surprise."""
        step_var = np.exp(np.clip(kappa * self.mu2 + omega, -30.0, 30.0))
        pi1_hat = 1.0 / (1.0 / self.pi1 + step_var)
        mu1_hat = self.mu1
        spread = 1.0 / pi1_hat + 1.0 / pi_u  # the predicted variance of the sample itself
        delta = u - mu1_hat
        surprise = 0.5 * (LOG2PI + np.log(spread) + delta * delta / spread)
        pi1 = pi1_hat + pi_u
        mu1 = mu1_hat + (pi_u / pi1) * delta
        if kappa != 0.0:
            # The eHGF order (Weber et al. 2023): the volatility's mean moves first, by its predicted
            # precision, then its precision is updated, which keeps a sudden error from throwing it.
            w = step_var * pi1_hat  # the share of the predicted variance the volatility explains
            vape = (1.0 / pi1 + (mu1 - mu1_hat) ** 2) * pi1_hat - 1.0
            pi2_hat = 1.0 / (1.0 / self.pi2 + theta)
            self.mu2 = self.mu2 + 0.5 * kappa * w / pi2_hat * vape
            pi2 = pi2_hat + 0.5 * kappa * kappa * w * (w + (2.0 * w - 1.0) * vape)
            self.pi2 = np.maximum(pi2, pi2_hat)  # a precision never falls below its prediction
        self.mu1, self.pi1 = mu1, pi1
        return delta / np.sqrt(spread), surprise


class Belief(goofi.Node):
    """Track each channel with a Kalman filter or a Hierarchical Gaussian Filter."""

    TAGS = ["analysis", "transform"]
    INPUTS = {"signal": goofi.InputSlot(goofi.DataType.ARRAY, required=True)}
    OUTPUTS = {
        "mean": goofi.DataType.ARRAY,
        "precision": goofi.DataType.ARRAY,
        "error": goofi.DataType.ARRAY,
        "surprise": goofi.DataType.ARRAY,
        "volatility": goofi.DataType.ARRAY,
        "status": goofi.DataType.STRING,
    }
    PARAMS = {
        "belief": {
            "method": goofi.StringParam("hgf", METHODS, doc="hgf learns how restless each channel is and adapts its "
                                        "speed. kalman holds one speed, set by drift."),
            "drift": goofi.NumParam(-4.0, -12.0, 2.0, doc="The log step variance of the hidden value per sample. "
                                    "Higher follows the signal faster. With hgf it is where the volatility starts."),
            "noise": goofi.NumParam(0.0, -6.0, 6.0, doc="The log variance of the measurement around the hidden "
                                    "value. Higher trusts each sample less."),
            "coupling": goofi.NumParam(1.0, 0.0, 4.0, doc="How strongly the learned volatility scales the step "
                                       "(hgf). 0 is the Kalman filter.", show=("method", ["hgf"])),
            "meta": goofi.NumParam(-6.0, -12.0, 0.0, doc="The log step variance of the volatility itself (hgf). "
                                   "Higher lets the speed change faster.", show=("method", ["hgf"])),
        },
        "input": {
            "log": goofi.BoolParam(False, doc="Take the log of each sample first, for powers and other positive "
                                   "quantities whose changes are multiplicative."),
            "normalize": goofi.BoolParam(True, doc="z-score each channel against its own running mean and spread."),
            "memory": goofi.NumParam(30.0, 1.0, 600.0, doc="Seconds of history the running mean and spread "
                                     "remember.", show=("normalize", ["true"])),
            "rate": goofi.NumParam(0.0, 0.0, 1000.0, doc="Samples a second, for `memory`. 0 reads the frame's "
                                   "sfreq, or counts frames as they arrive."),
        },
        "reset": {"reset": goofi.PulseParam(doc="Forget every belief and start again.")},
        "common": {"autotrigger": goofi.BoolParam(False), "max_frequency": goofi.NumParam(60.0, 1.0, 1000.0)},
    }

    def setup(self):
        self.filter = None
        self.stats = None
        self.seen = 0
        self.last = None

    def pulse_reset_reset(self):
        self.filter = None

    def process(self, signal):
        p = self.params
        if signal is None:
            return None
        x = samples_of(signal.data, "signal")
        c, n = x.shape
        if self.filter is None or len(self.filter.mu1) != c:
            self.filter, self.stats, self.seen = Filter(c), None, 0
        started = time.perf_counter()
        rate = float(p.input.rate) or float((signal.meta or {}).get("sfreq", 0.0) or 0.0)
        if rate <= 0.0:
            now = time.monotonic()
            rate = 1.0 / max(now - self.last, 1e-3) if self.last is not None else 10.0
            self.last = now
            rate *= n
        hgf = p.belief.method == "hgf"
        kappa = float(p.belief.coupling) if hgf else 0.0
        omega = float(p.belief.drift)
        theta = float(np.exp(p.belief.meta))
        pi_u = float(np.exp(-p.belief.noise))
        keep = float(np.exp(-1.0 / max(float(p.input.memory) * rate, 1.0)))
        errors, surprises = np.empty((c, n)), np.empty((c, n))
        means = np.empty((c, n))
        if p.input.log:  # a power of 0 (a buffer still filling) is missing, never log 0
            x = np.where(x > 0, np.log(np.where(x > 0, x, 1.0)), np.nan)
        f = self.filter
        for i in range(n):
            u = x[:, i]
            ok = np.isfinite(u)  # a channel with no sample this time keeps its belief as it was
            if p.input.normalize:
                # A running mean and variance that average a channel's first samples equally, then
                # forget at `memory`, so the scale is the data's own from the start.
                if self.stats is None:
                    self.stats = [np.where(ok, u, 0.0), np.zeros(c), np.zeros(c)]
                m, v, k = self.stats
                k = k + ok
                step = np.where(ok, np.maximum(1.0 / np.maximum(k, 1.0), 1.0 - keep), 0.0)
                d = np.where(ok, u - m, 0.0)
                m = m + step * d
                v = (1.0 - step) * (v + step * d * d)
                self.stats = [m, v, k]
                u = np.where(ok & (v > 0), (np.where(ok, u, 0.0) - m) / np.sqrt(np.maximum(v, 1e-30)), 0.0)
            before = (f.mu1.copy(), f.pi1.copy(), f.mu2.copy(), f.pi2.copy())
            e, s = f.step(np.where(ok, u, f.mu1), pi_u, kappa, omega, theta)
            if not ok.all():
                f.mu1, f.pi1, f.mu2, f.pi2 = (np.where(ok, new, old) for new, old in
                                              zip((f.mu1, f.pi1, f.mu2, f.pi2), before))
            errors[:, i], surprises[:, i] = np.where(ok, e, 0.0), np.where(ok, s, 0.0)
            means[:, i] = f.mu1
            self.seen += 1
        f = self.filter
        vol = kappa * f.mu2 + omega if hgf else np.full(c, omega)
        flat = n == 1
        out = lambda a: a[:, 0] if flat else a  # noqa: E731
        ms = (time.perf_counter() - started) * 1000.0
        names = ((signal.meta or {}).get("channels") or {}).get("dim0")
        per = {"channels": {"dim0": list(names)}} if names is not None and len(names) == c else {}
        meta = dict(per, sfreq=rate) if not flat else per
        return {
            "mean": (out(means), meta),
            "precision": (f.pi1.copy(), per),
            "error": (out(errors), meta),
            "surprise": (out(surprises), meta),
            "volatility": (vol, per),
            "status": f"{p.belief.method} | {c} channels | {self.seen} samples | surprise {surprises[:, -1].mean():.2f} "
                      f"| {ms:.2f} ms",
        }
