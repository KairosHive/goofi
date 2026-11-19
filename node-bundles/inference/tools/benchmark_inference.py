"""Benchmarks for Belief, Blanket and ActiveAgent on simulations with a known answer and on recorded EEG.

    <goofi runtime>/python/gil/Scripts/python.exe node-bundles/inference/tools/benchmark_inference.py [EDF]

The EDF defaults to goofi's sample `eeg-rest-srm.edf` (eyes-closed resting EEG, OpenNeuro ds003775).
"""

import importlib.util
import json
import pathlib
import sys
import time
import types

import numpy as np

import goofi

HERE = pathlib.Path(__file__).resolve().parents[1]
EDF = pathlib.Path.home() / ".goofi" / "data" / "samples" / "eeg-rest-srm.edf"


class Frame:
    def __init__(self, data, meta=None):
        self.data, self.meta = data, meta or {}


def make(name, **over):
    """A node from this bundle with its declared defaults, `group__name=value` overriding them."""
    path = HERE / f"{name}.py"
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    info = json.loads(goofi.introspect(str(path)))
    groups = {}
    for p in info["params"]:
        v = p.get("default")
        if p["kind"] == "num":
            v = v if len(v) > 1 else v[0]
            v = int(round(v)) if p.get("int") else v
        elif p["kind"] == "pulse":
            v = None
        groups.setdefault(p["group"], {})[p["name"]] = v
    for k, v in over.items():
        g, n = k.split("__")
        assert n in groups[g], k
        groups[g][n] = v
    cls = next(v for v in vars(mod).values() if isinstance(v, type) and issubclass(v, goofi.Node) and v is not goofi.Node)
    node = cls()
    node.params = types.SimpleNamespace(**{g: types.SimpleNamespace(**d) for g, d in groups.items()})
    node.setup()
    return node, mod


def out(result, key):
    v = result[key]
    return v[0] if isinstance(v, tuple) else v


# ---- Belief --------------------------------------------------------------------------------------

def volatile_walk(n=6000, seed=0):
    """A random walk whose step size switches between calm (0.02) and restless (0.2) every 750 samples,
    seen through measurement noise of sd 0.1. Returns the samples, the hidden walk and its log step var."""
    rng = np.random.default_rng(seed)
    sd = np.where((np.arange(n) // 750) % 2 == 0, 0.02, 0.2)
    x = np.cumsum(rng.normal(0, sd))
    return x + rng.normal(0, 0.1, n), x, np.log(sd ** 2)


def bench_belief():
    print("Belief")
    u, x, logvar = volatile_walk()
    n = len(u)
    rows = []
    for method, over in (("kalman, slow drift", dict(belief__method="kalman", belief__drift=-7.0)),
                         ("kalman, fast drift", dict(belief__method="kalman", belief__drift=-3.0)),
                         ("hgf", dict(belief__method="hgf", belief__drift=-5.0, belief__meta=-4.0))):
        node, _ = make("belief", input__normalize=False, belief__noise=float(np.log(0.1 ** 2)), **over)
        r = node.process(Frame(u[None, :], {"sfreq": 100.0}))
        mean, vol = out(r, "mean")[0], None
        rmse = float(np.sqrt(np.mean((mean - x) ** 2)))
        # the volatility the filter ends each sample with, re-run sample by sample for its time course
        node2, _ = make("belief", input__normalize=False, belief__noise=float(np.log(0.1 ** 2)), **over)
        vols = np.empty(n)
        for i in range(n):
            vols[i] = out(node2.process(Frame(u[i:i + 1], {"sfreq": 100.0})), "volatility")[0]
        corr = float(np.corrcoef(vols, logvar)[0, 1]) if vols.std() > 0 else float("nan")
        surprise = float(out(r, "surprise")[0].mean())
        rows.append((method, rmse, corr, surprise))
        print(f"  {method:<20} tracking RMSE {rmse:.3f}, volatility vs true log step variance r = {corr:.2f}, "
              f"mean surprise {surprise:.2f} nats")
    for c in (1, 8, 64):
        node, _ = make("belief")
        frame = np.random.default_rng(1).normal(size=(c, 1))
        node.process(Frame(frame))
        t = time.perf_counter()
        for _ in range(500):
            node.process(Frame(frame))
        print(f"  {c:>3} channels, one sample a call: {(time.perf_counter() - t) / 500 * 1000:.3f} ms a call")
    node, _ = make("belief")
    chunk = np.random.default_rng(2).normal(size=(64, 256))
    t = time.perf_counter()
    node.process(Frame(chunk, {"sfreq": 256.0}))
    print(f"   64 channels x 256 samples in one frame: {(time.perf_counter() - t) * 1000:.1f} ms")
    return rows


# ---- Blanket -------------------------------------------------------------------------------------

def blanket_system(n=4000, leak=0.0, seed=0):
    """Internal mu (2), sensory s (1), active a (1), external eta (2), slow linear dynamics in which mu
    and eta meet only through s and a, plus a direct eta -> mu path of strength `leak`."""
    rng = np.random.default_rng(seed)
    mu, s, a, eta = np.zeros(2), 0.0, 0.0, np.zeros(2)
    rows = []
    for _ in range(n):
        eta = 0.7 * eta + 0.15 * a + rng.normal(0, 0.5, 2)
        s = 0.5 * eta.sum() + rng.normal(0, 0.3)
        mu = 0.7 * mu + 0.15 * s + leak * eta[::-1] + rng.normal(0, 0.5, 2)
        a = 0.5 * mu.sum() + rng.normal(0, 0.3)
        rows.append(np.concatenate([mu, eta, [s, a]]))
    return np.asarray(rows)


def bench_blanket():
    print("Blanket")
    results = {}
    for leak in (0.0, 0.3):
        data = blanket_system(leak=leak)
        node, _ = make("blanket", blanket__window=2000, blanket__every=1, blanket__shrink=0.02)
        t = time.perf_counter()
        r = None
        for row in data:
            r = node.process(Frame(row[:2]), Frame(row[2:4]), Frame(row[4:6])) or r
        ms = (time.perf_counter() - t) / len(data) * 1000
        with_b = float(out(r, "leak")[0])
        member = out(r, "membership")
        node2, _ = make("blanket", blanket__window=2000, blanket__every=1, blanket__shrink=0.02)
        r2 = None
        for row in data:
            r2 = node2.process(Frame(row[:2]), Frame(row[2:4])) or r2
        without = float(out(r2, "leak")[0])
        results[leak] = (with_b, without)
        names = ["mu1", "mu2", "eta1", "eta2", "s", "a"]
        ranked = ", ".join(f"{names[i]} {member[i]:.2f}" for i in np.argsort(-member)[:4])
        print(f"  direct path {leak:.1f}: leak {with_b:.3f} given s and a, {without:.3f} without them | "
              f"membership {ranked} | {ms:.2f} ms a sample")
    rng = np.random.default_rng(3)
    x = np.zeros(3000)
    y = np.zeros(3000)
    for t in range(1, 3000):
        x[t] = 0.8 * x[t - 1] + rng.normal()
        y[t] = 0.5 * y[t - 1] + 0.6 * x[t - 1] + rng.normal()
    node, _ = make("blanket", blanket__window=3000, blanket__every=3000)
    r = None
    for t in range(3000):
        r = node.process(Frame([x[t]]), Frame([y[t]])) or r
    te = out(r, "influence")
    print(f"  x drives y: TE x->y {te[0]:.3f} nats, y->x {te[1]:.3f} nats")
    return results, te


# ---- ActiveAgent ---------------------------------------------------------------------------------

class Person:
    """A level in 0..1 that relaxes to 0.5, pushed by the machine's action: `effects[a]` a step."""

    def __init__(self, effects, seed):
        self.level, self.effects = 0.5, effects
        self.rng = np.random.default_rng(seed)

    def step(self, action):
        self.level += 0.15 * (0.5 - self.level) + self.effects[action] + self.rng.normal(0, 0.03)
        self.level = float(np.clip(self.level, 0.0, 1.0))
        return self.level + self.rng.normal(0, 0.05)


def run_agent(policy, effects, n=400, seed=0, **over):
    person = Person(effects, seed)
    obs = person.step(1)
    node, mod = make("active_agent", plan__seed=seed, **over) if policy == "agent" else (None, None)
    rng = np.random.default_rng(seed + 100)
    levels, times = [], []
    for _ in range(n):
        if policy == "agent":
            t = time.perf_counter()
            r = node.decide(obs)
            times.append(time.perf_counter() - t)
            action = int(out(r, "action")[0])
        elif policy == "random":
            action = int(rng.integers(len(effects)))
        else:
            action = policy
        obs = person.step(action)
        levels.append(person.level)
    late = np.asarray(levels[n // 2:])
    return late.mean(), (np.mean(times) * 1000 if times else 0.0), node


def bench_agent():
    print("ActiveAgent")
    effects = [-0.08, 0.0, 0.08]  # action 0 lowers the person's level, 2 raises it
    print("  the person's level in the second half of 400 decisions, the agent preferring the top (target 0.9):")
    for name, policy in (("always lower", 0), ("random", "random"), ("always raise (the best)", 2)):
        m, _, _ = run_agent(policy, effects)
        print(f"    {name:<24} {m:.3f}")
    for depth in (1, 2, 3):
        ms_all, means = [], []
        for seed in range(5):
            m, ms, node = run_agent("agent", effects, seed=seed, prefer__target=0.9, plan__depth=depth)
            means.append(m)
            ms_all.append(ms)
        B = node.model.B
        print(f"    agent, depth {depth}: {np.mean(means):.3f} (5 seeds, {min(means):.3f} to {max(means):.3f}), "
              f"{np.mean(ms_all):.2f} ms a decision")
    print("    what it learned (depth 2, last seed): for each action, the expected next state from the middle state")
    for a in range(3):
        print(f"      action {a}: {np.round(B[a][:, 1], 2)}")
    m_low, _, _ = run_agent("agent", effects, seed=0, prefer__target=0.1, plan__depth=2)
    print(f"    agent preferring the bottom (target 0.1): {m_low:.3f}")
    return effects


# ---- Belief on recorded EEG ----------------------------------------------------------------------

def bench_eeg(path):
    print(f"Belief and Blanket on recorded EEG ({path.name})")
    if not path.is_file():
        print("  not found, skipped")
        return
    import mne

    raw = mne.io.read_raw_edf(str(path), preload=True, verbose="error")
    wanted = ["Fp1", "Fp2", "F3", "F4", "C3", "C4", "T7", "T8", "P3", "P4", "O1", "O2"]
    raw.pick([c for c in wanted if c in raw.ch_names])
    sf = raw.info["sfreq"]
    data = raw.get_data()
    win, hop = int(sf), int(sf / 4)
    freqs = np.fft.rfftfreq(win, 1 / sf)
    band = (freqs >= 8) & (freqs <= 12)
    taper = np.hanning(win)
    powers = []
    for start in range(0, data.shape[1] - win, hop):
        spec = np.abs(np.fft.rfft(data[:, start:start + win] * taper, axis=1)) ** 2
        powers.append(np.log(spec[:, band].mean(1) + 1e-30))
    alpha = np.asarray(powers).T  # [channels, frames at 4 Hz]
    print(f"  {alpha.shape[0]} channels, {alpha.shape[1]} alpha-power frames at 4 Hz ({alpha.shape[1] / 4 / 60:.1f} min)")
    node, _ = make("belief", input__rate=4.0)
    surprise, vol, t = [], [], time.perf_counter()
    for i in range(alpha.shape[1]):
        r = node.process(Frame(alpha[:, i]))
        surprise.append(out(r, "surprise").mean())
        vol.append(out(r, "volatility").mean())
    ms = (time.perf_counter() - t) / alpha.shape[1] * 1000
    surprise = np.asarray(surprise)
    print(f"  hgf: {ms:.3f} ms a frame for {alpha.shape[0]} channels | surprise median {np.median(surprise):.2f}, "
          f"95th percentile {np.quantile(surprise, 0.95):.2f} nats | volatility {np.min(vol):.2f} to {np.max(vol):.2f}")
    # posterior alpha against occipital: do occipital channels form the blanket of frontal ones?
    names = raw.ch_names
    front = [i for i, n in enumerate(names) if n.startswith("F")]
    back = [i for i, n in enumerate(names) if n.startswith(("O", "P"))]
    mid = [i for i, n in enumerate(names) if n.startswith(("C", "T"))]
    if front and back and mid:
        node, _ = make("blanket", blanket__window=alpha.shape[1], blanket__every=alpha.shape[1])
        r = None
        for i in range(alpha.shape[1]):
            r = node.process(Frame(alpha[front, i]), Frame(alpha[back, i]), Frame(alpha[mid, i])) or r
        node2, _ = make("blanket", blanket__window=alpha.shape[1], blanket__every=alpha.shape[1])
        r2 = None
        for i in range(alpha.shape[1]):
            r2 = node2.process(Frame(alpha[front, i]), Frame(alpha[back, i])) or r2
        te = out(r, "influence")
        print(f"  alpha, frontal {[names[i] for i in front]} and parieto-occipital {[names[i] for i in back]}: "
              f"leak {out(r2, 'leak')[0]:.3f} alone, {out(r, 'leak')[0]:.3f} given central and temporal "
              f"{[names[i] for i in mid]} | TE front->back {te[0]:.3f}, back->front {te[1]:.3f} nats")


if __name__ == "__main__":
    bench_belief()
    bench_blanket()
    bench_agent()
    bench_eeg(pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else EDF)
