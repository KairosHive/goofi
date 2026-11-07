"""Blanket measures the boundary between two sets of signals: what passes it, and which way.

Two sets meet here, `internal` (such as a person's EEG features) and `external` (such as the
machine's own state, the parameters it draws with). An optional `blanket` set holds the candidates
for the boundary between them (what the screen shows, what the body does). Over the last `window`
joint samples the node fits a Gaussian graphical model and reads off three things.

- partial. The partial correlation of every pair of signals given all the others [V, V], from the
  precision matrix, shrunk toward the identity by `shrink`. Zero means conditionally independent.
- leak. The mean |partial correlation| between internal and external signals given everything
  else. Near zero, the wired signals screen the two sides from each other, a Markov blanket. A
  direct path between the sides that bypasses them shows as leak.
- membership. For each signal, its strongest |partial correlation| with an internal one, so the
  signals that belong to the internal set's blanket stand out [V]. Internal signals read 0.
- influence. Gaussian transfer entropy in nats at `lag` samples, [internal to external, external
  to internal]. It says which side's past predicts the other's future beyond that side's own past,
  so who leads the coupling.

Each process call adds one joint sample, the latest value of every wired input (a frame [C, N]
gives its last column). Pure numpy, so goofi runs it inside its own process.
"""

import time
from collections import deque

import numpy as np

import goofi


def latest(x, name):
    a = np.asarray(x, dtype=np.float64)
    if a.ndim == 0:
        return a.reshape(1)
    if a.ndim == 1:
        return a
    if a.ndim == 2:
        return a[:, -1]
    raise ValueError(f"`{name}` takes [channels] or [channels, samples]. This frame is {list(a.shape)}")


def partial_correlation(z, shrink):
    """Partial correlations of standardised samples z [T, V], from the shrunk precision matrix."""
    s = np.corrcoef(z, rowvar=False) if z.shape[1] > 1 else np.ones((1, 1))
    s = np.nan_to_num(s)
    s = (1.0 - shrink) * s + shrink * np.eye(len(s))
    prec = np.linalg.pinv(s)
    d = np.sqrt(np.clip(np.diag(prec), 1e-12, None))
    pc = -prec / np.outer(d, d)
    np.fill_diagonal(pc, 1.0)
    return pc


def residual_logdet(y, x, ridge):
    """log det of the covariance of y's residual after a ridge regression on x (with a constant)."""
    x1 = np.hstack([x, np.ones((len(x), 1))])
    beta = np.linalg.solve(x1.T @ x1 + ridge * np.eye(x1.shape[1]), x1.T @ y)
    r = y - x1 @ beta
    cov = np.atleast_2d(np.cov(r, rowvar=False)) + 1e-9 * np.eye(y.shape[1])
    return np.linalg.slogdet(cov)[1]


def transfer_entropy(src, dst, lag, ridge=1e-3):
    """Gaussian transfer entropy src -> dst in nats: how much src's past adds to dst's own past."""
    if len(src) <= lag + 3:
        return 0.0
    future, past_d, past_s = dst[lag:], dst[:-lag], src[:-lag]
    own = residual_logdet(future, past_d, ridge)
    both = residual_logdet(future, np.hstack([past_d, past_s]), ridge)
    return max(0.0, 0.5 * (own - both))


class Blanket(goofi.Node):
    """Measure what passes between two sets of signals, and whether a third screens them."""

    TAGS = ["analysis"]
    INPUTS = {
        "internal": goofi.InputSlot(goofi.DataType.ARRAY, required=True),
        "external": goofi.InputSlot(goofi.DataType.ARRAY, required=True),
        "blanket": goofi.InputSlot(goofi.DataType.ARRAY),
    }
    OUTPUTS = {
        "partial": goofi.DataType.ARRAY,
        "leak": goofi.DataType.ARRAY,
        "membership": goofi.DataType.ARRAY,
        "influence": goofi.DataType.ARRAY,
        "status": goofi.DataType.STRING,
    }
    PARAMS = {
        "blanket": {
            "window": goofi.NumParam(400, 50, 5000, int=True, doc="Joint samples the graph is fitted on. Longer is "
                                     "steadier and slower to follow a change."),
            "shrink": goofi.NumParam(0.1, 0.0, 0.9, doc="How far the correlation matrix is pulled toward the identity "
                                     "before it is inverted. More is steadier with many signals and few samples."),
            "lag": goofi.NumParam(1, 1, 50, int=True, doc="Samples between a cause's past and the effect, for "
                                  "influence."),
            "every": goofi.NumParam(5, 1, 100, int=True, doc="Refit after this many new samples."),
        },
        "reset": {"reset": goofi.PulseParam(doc="Forget the window and start again.")},
        "common": {"autotrigger": goofi.BoolParam(False), "max_frequency": goofi.NumParam(60.0, 1.0, 1000.0)},
    }

    def setup(self):
        self.rows = deque()
        self.sizes = None
        self.count = 0
        self.result = None

    def pulse_reset_reset(self):
        self.rows.clear()
        self.result = None

    def process(self, internal, external, blanket=None):
        if internal is None or external is None:
            return None
        p = self.params.blanket
        parts = [latest(internal.data, "internal"), latest(external.data, "external")]
        if blanket is not None:
            parts.append(latest(blanket.data, "blanket"))
        sizes = tuple(len(x) for x in parts)
        if sizes != self.sizes:
            self.rows.clear()
            self.sizes = sizes
        self.rows.append(np.concatenate(parts))
        window = int(p.window)
        while len(self.rows) > window:
            self.rows.popleft()
        self.count += 1
        need = sum(sizes) + int(p.lag) + 10
        if len(self.rows) < need:
            return {"status": f"filling the window, {len(self.rows)} of at least {need} samples"}
        if self.result is not None and self.count % max(int(p.every), 1):
            return None
        started = time.perf_counter()
        data = np.asarray(self.rows)
        z = (data - data.mean(0)) / (data.std(0) + 1e-9)
        ni, ne = sizes[0], sizes[1]
        inside, outside = slice(0, ni), slice(ni, ni + ne)
        pc = partial_correlation(z, float(np.clip(p.shrink, 0.0, 0.99)))
        leak = float(np.abs(pc[inside, outside]).mean())
        member = np.abs(pc[:, inside]).max(axis=1)
        member[inside] = 0.0
        lag = int(p.lag)
        flow = np.array([transfer_entropy(z[:, inside], z[:, outside], lag),
                         transfer_entropy(z[:, outside], z[:, inside], lag)])
        ms = (time.perf_counter() - started) * 1000.0
        lead = "internal leads" if flow[0] > flow[1] else "external leads"
        self.result = {
            "partial": (pc, {}),
            "leak": (np.array([leak]), {}),
            "membership": (member, {}),
            "influence": (flow, {"channels": {"dim0": ["internal to external", "external to internal"]}}),
            "status": f"{len(self.rows)} samples of {sum(sizes)} signals | leak {leak:.3f} | "
                      f"TE in->ex {flow[0]:.3f}, ex->in {flow[1]:.3f} nats, {lead} | {ms:.1f} ms",
        }
        return self.result
