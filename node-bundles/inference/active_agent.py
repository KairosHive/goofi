"""ActiveAgent acts on its world to bring its observations toward what it prefers, by active inference.

The agent's world is a hidden state with `states` levels, read through one observed signal binned
into `bins` levels, and moved by `actions` choices the agent makes (a palette, a speed, a sound).
The model is a discrete POMDP, as in pymdp.
- A. How each state shows in the observation, fixed: state s shows mostly as the bin at the same
  height.
- B. How each action moves the state, learned. Every decision adds the states it believes it went
  from and to under the action it took (Dirichlet counts at rate `learn`), so the agent learns how
  its world answers each action.
- C. Which observations it prefers, a bump at `target` (0 the lowest bin, 1 the highest) whose
  height is `strength`. A wired `preference` sets the target live.

Every `period` seconds the agent bins the mean of the observations since the last decision, updates
its belief about the state, and scores every sequence of `depth` actions by expected free energy.
- risk. How far the predicted observations fall from the preferred ones (KL divergence).
- ambiguity. How unclear the states it visits are to observe.
- novelty. How much the visits would teach it about B, weighted by `curiosity`.

Policies are chosen by a softmax of -G at precision `gamma`, by sampling or the best. So the agent
explores while it knows little and exploits once it knows how its world answers.

In biofeedback the agent's observation is the person (a band power through Belief) and its actions
are the machine's, so its sensory and active states are the two halves of the Markov blanket
between person and machine. The observation is binned against its own recent quantiles (`memory`
decisions), so the bins follow the person's range. Pure numpy, so goofi runs it inside its own
process.
"""

import itertools
import time
from collections import deque

import numpy as np

import goofi

CHOOSE = ["sample", "best"]


def softmax(x):
    e = np.exp(x - x.max())
    return e / e.sum()


def normalise(x, axis=0):
    return x / np.clip(x.sum(axis=axis, keepdims=True), 1e-16, None)


def likelihood(bins, states):
    """A [bins, states]: state s shows as the bin at its own height, spread over its neighbours."""
    cb = np.linspace(0.0, 1.0, bins)[:, None]
    cs = np.linspace(0.0, 1.0, states)[None, :]
    width = 0.6 / max(states - 1, 1)
    return normalise(np.exp(-0.5 * ((cb - cs) / width) ** 2) + 1e-3)


def preferred(bins, target, strength):
    """ln C over bins: a bump at `target`, `strength` nats above the far end."""
    pos = np.linspace(0.0, 1.0, bins)
    return np.log(softmax(-strength * ((pos - target) / 0.5) ** 2))


class Model:
    def __init__(self, states, actions, bins):
        self.S, self.K, self.O = states, actions, bins
        self.A = likelihood(bins, states)
        self.HA = -(self.A * np.log(self.A)).sum(0)  # the ambiguity of each state
        self.counts = np.stack([np.ones((states, states)) + 3.0 * np.eye(states) for _ in range(actions)])
        self.q = np.full(states, 1.0 / states)
        self.prev_action = None

    @property
    def B(self):
        return normalise(self.counts, axis=1)

    def infer(self, o, learn):
        """The belief after observing bin `o`, the surprise of it, and the learning of B."""
        prior = self.q if self.prev_action is None else self.B[self.prev_action] @ self.q
        joint = self.A[o] * prior
        evidence = float(joint.sum())
        q = joint / max(evidence, 1e-16)
        if self.prev_action is not None and learn > 0:
            self.counts[self.prev_action] += learn * np.outer(q, self.q)
        self.q = q
        return -np.log(max(evidence, 1e-16))

    def evaluate(self, lnC, depth, curiosity):
        """Expected free energy of every action sequence of length `depth`."""
        B = self.B
        sums = self.counts.sum(axis=1, keepdims=True)
        W = 0.5 * (1.0 / self.counts - 1.0 / sums)  # what a visit would teach about B
        policies = list(itertools.product(range(self.K), repeat=depth))
        G = np.empty(len(policies))
        for i, pol in enumerate(policies):
            qs, g = self.q, 0.0
            for a in pol:
                nxt = B[a] @ qs
                qo = self.A @ nxt
                risk = float(qo @ (np.log(np.clip(qo, 1e-16, None)) - lnC))
                ambiguity = float(nxt @ self.HA)
                novelty = float(nxt @ (W[a] @ qs))
                g += risk + ambiguity - curiosity * novelty
                qs = nxt
            G[i] = g
        return policies, G


class ActiveAgent(goofi.Node):
    """Choose actions that bring an observed signal toward a preferred level, learning how each acts."""

    TAGS = ["analysis", "transform"]
    INPUTS = {
        "observation": goofi.InputSlot(goofi.DataType.ARRAY, required=True),
        "preference": goofi.InputSlot(goofi.DataType.ARRAY),
    }
    OUTPUTS = {
        "action": goofi.DataType.ARRAY,
        "level": goofi.DataType.ARRAY,
        "choice": goofi.DataType.ARRAY,
        "belief": goofi.DataType.ARRAY,
        "energy": goofi.DataType.ARRAY,
        "surprise": goofi.DataType.ARRAY,
        "status": goofi.DataType.STRING,
    }
    PARAMS = {
        "model": {
            "states": goofi.NumParam(3, 2, 8, int=True, doc="Levels of the hidden state the agent believes in."),
            "actions": goofi.NumParam(3, 2, 8, int=True, doc="Choices it can make. `level` maps them to 0..1."),
            "bins": goofi.NumParam(5, 2, 9, int=True, doc="Levels the observation is binned into."),
            "memory": goofi.NumParam(120, 10, 2000, int=True, doc="Decisions of history the bins' quantiles follow."),
        },
        "prefer": {
            "target": goofi.NumParam(0.8, 0.0, 1.0, doc="The preferred observation, 0 its lowest bin and 1 its highest."),
            "strength": goofi.NumParam(3.0, 0.0, 12.0, doc="How much the agent cares about reaching the target."),
        },
        "plan": {
            "period": goofi.NumParam(1.0, 0.05, 30.0, doc="Seconds between decisions."),
            "depth": goofi.NumParam(2, 1, 4, int=True, doc="Actions planned ahead. Policies grow as actions ^ depth."),
            "curiosity": goofi.NumParam(1.0, 0.0, 4.0, doc="Weight on what an action would teach about B."),
            "gamma": goofi.NumParam(4.0, 0.25, 32.0, doc="Policy precision. Higher picks the best policy more surely."),
            "choose": goofi.StringParam("sample", CHOOSE, doc="sample draws a policy by its probability, best takes "
                                        "the most probable."),
            "learn": goofi.NumParam(0.5, 0.0, 4.0, doc="How fast B learns from each decision. 0 keeps it fixed."),
            "seed": goofi.NumParam(-1, -1, 9999, int=True, doc="Sampling seed, -1 for a fresh one."),
        },
        "reset": {"reset": goofi.PulseParam(doc="Forget what has been learned and start again.")},
        "common": {"autotrigger": goofi.BoolParam(False), "max_frequency": goofi.NumParam(60.0, 1.0, 1000.0)},
    }

    def setup(self):
        self.model = None
        self.history = None
        self.pending = []
        self.last = None
        self.decisions = 0
        self.rng = None
        self.shape = None
        self.target = None

    def pulse_reset_reset(self):
        self.model = None

    def bin_of(self, value, bins):
        h = np.asarray(self.history)
        if len(h) < 3:
            return bins // 2
        edges = np.quantile(h, np.linspace(0.0, 1.0, bins + 1)[1:-1])
        return int(np.searchsorted(edges, value))

    def decide(self, value, now=None):
        """One decision on an observed value, as a goofi output dict."""
        p = self.params
        S, K, O = int(p.model.states), int(p.model.actions), int(p.model.bins)
        if self.model is None or self.shape != (S, K, O):
            self.model, self.shape, self.decisions = Model(S, K, O), (S, K, O), 0
            self.history = deque(maxlen=int(p.model.memory))
        if self.rng is None or (int(p.plan.seed) >= 0 and self.decisions == 0):
            self.rng = np.random.default_rng(None if int(p.plan.seed) < 0 else int(p.plan.seed))
        if self.history.maxlen != int(p.model.memory):
            self.history = deque(self.history, maxlen=int(p.model.memory))
        started = time.perf_counter()
        self.history.append(value)
        o = self.bin_of(value, O)
        m = self.model
        surprise = m.infer(o, float(p.plan.learn))
        target = float(p.prefer.target) if self.target is None else self.target
        lnC = preferred(O, float(np.clip(target, 0.0, 1.0)), float(p.prefer.strength))
        policies, G = m.evaluate(lnC, int(p.plan.depth), float(p.plan.curiosity))
        qpi = softmax(-float(p.plan.gamma) * G)
        firsts = np.array([pol[0] for pol in policies])
        choice = np.array([qpi[firsts == a].sum() for a in range(K)])
        per_action = np.array([G[firsts == a].min() for a in range(K)])
        pick = int(self.rng.choice(len(policies), p=qpi)) if p.plan.choose == "sample" else int(np.argmax(qpi))
        action = int(policies[pick][0])
        m.prev_action = action
        self.decisions += 1
        ms = (time.perf_counter() - started) * 1000.0
        return {
            "action": (np.array([float(action)]), {}),
            "level": (np.array([action / max(K - 1, 1)]), {}),
            "choice": (choice, {}),
            "belief": (m.q.copy(), {}),
            "energy": (per_action, {}),
            "surprise": (np.array([surprise]), {}),
            "status": f"decision {self.decisions} | saw bin {o} of {O} | believes state {int(np.argmax(m.q))} "
                      f"({m.q.max():.2f}) | action {action} ({choice[action]:.2f}) | {len(policies)} policies | "
                      f"{ms:.2f} ms",
        }

    def process(self, observation, preference=None):
        if observation is None:
            return None
        p = self.params
        self.target = float(np.asarray(preference.data).reshape(-1)[0]) if preference is not None else None
        self.pending.append(float(np.nanmean(np.asarray(observation.data, dtype=np.float64))))
        now = time.monotonic()
        if self.last is not None and now - self.last < float(p.plan.period):
            return None
        self.last = now
        value = float(np.mean(self.pending))
        self.pending = []
        return self.decide(value)
