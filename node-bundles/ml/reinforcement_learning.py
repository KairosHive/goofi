"""ReinforcementLearning — Stream AC(λ), an agent that learns from every sample and never settles.

Observations in, continuous actions out, a reward on its own slot. One update per frame, on that
frame alone: no replay buffer, no target network, no batch. That is what makes it fit a closed loop
around a person, where a session is a thousand samples and the thing being learned keeps moving.

Four decisions carry the design, and each is here because a converging agent is a FAILED agent when
the environment is a participant who habituates:

- The step is bounded, not tuned. ObGD scales each update from the trace and the error, so one
  noisy sample can move the weights by at most `1/kappa` in L1 and a step size of 1.0 is safe.
- The entropy bonus is multiplied by `sign(delta)`, which cancels against the `delta` the update
  carries. Exploration is therefore pushed UP in every state, whatever the sign of the error.
- The reward is centred against its own running average, so the agent optimises the EFFECT of an
  action rather than the drift the effect sits on.
- The input statistics forget, the weights are pulled back toward their initial values, and low
  utility units are recycled. Together these are what stop the network losing the ability to learn.

The policy is a plain Normal. Nothing is squashed inside the density, so `log pi(a|s)` is always
evaluated on the exact value that was drawn: bounding happens after the sample, on the way out.
"""

import numpy as np
import goofi

_LEAK = 0.01
_PINK_OCTAVES = 7
_RANK_UNITS = 64


def _sparse(rng, fan_out, fan_in, sparsity):
    """LeCun-uniform weights with `sparsity` of each unit's inputs held at zero.

    Every unit keeps at least one live input. A short observation would otherwise round the share
    up to the whole fan and hand the layer nothing but zeros to normalise.
    """
    w = rng.uniform(-1.0, 1.0, size=(fan_out, fan_in)) * np.sqrt(1.0 / fan_in)
    zeros = min(int(np.ceil(sparsity * fan_in)), fan_in - 1)
    for row in range(fan_out):
        w[row, rng.permutation(fan_in)[:zeros]] = 0.0
    return w


class _Norm:
    """Running mean and variance that FORGET, so a drifting signal stays inside its own scale."""

    def __init__(self, shape, forget):
        self.mean = np.zeros(shape)
        self.var = np.ones(shape)
        self.forget = forget
        self.n = 0

    def observe(self, x):
        self.n += 1
        rate = max(self.forget, 1.0 / self.n)
        diff = x - self.mean
        self.mean = self.mean + rate * diff
        self.var = (1.0 - rate) * (self.var + rate * diff * diff)

    @property
    def std(self):
        return np.sqrt(self.var + 1e-8)

    def whiten(self, x):
        return (x - self.mean) / self.std


class _Pink:
    """Voss-McCartney 1/f noise whose marginal is exactly standard normal.

    A sum of independent normals is normal, so `log pi` stays exact while the action sequence gains
    the correlation a participant can actually respond to — white noise averages to its own mean.
    """

    def __init__(self, rng, width):
        self.rng = rng
        self.rows = rng.standard_normal((_PINK_OCTAVES, width))
        self.t = 0

    def __call__(self):
        self.t += 1
        octave = min(int(self.t & -self.t).bit_length() - 1, _PINK_OCTAVES - 1)
        self.rows[octave] = self.rng.standard_normal(self.rows.shape[1])
        white = self.rng.standard_normal(self.rows.shape[1])
        return (self.rows.sum(axis=0) + white) / np.sqrt(_PINK_OCTAVES + 1)


class _Rank:
    """Effective rank of what a bounded sample of hidden units spans.

    The sample is what keeps this affordable: a wide layer would otherwise cost a covariance and an
    eigendecomposition that grow with its square, to answer a question a projection answers as well.
    """

    def __init__(self, rng, width, decay=0.99):
        self.pick = rng.permutation(width)[: min(_RANK_UNITS, width)]
        self.mean = np.zeros(self.pick.size)
        self.cov = np.zeros((self.pick.size, self.pick.size))
        self.decay = decay
        self.value = 0.0

    def observe(self, hidden):
        h = hidden[self.pick]
        self.mean *= self.decay
        self.mean += (1.0 - self.decay) * h
        # Centred, or the mean direction alone is the leading eigenvalue and every rank reads as 1.
        off = h - self.mean
        self.cov *= self.decay
        self.cov += (1.0 - self.decay) * np.outer(off, off)

    def compute(self):
        values = np.clip(np.linalg.eigvalsh(self.cov), 0.0, None)
        total = values.sum()
        share = values[values > 0.0] / total if total > 0.0 else np.empty(0)
        self.value = float(np.exp(-np.sum(share * np.log(share)))) if share.size else 0.0
        return self.value


class _Net:
    """A LayerNorm/LeakyReLU MLP with several linear heads, and the traces its update needs.

    Actor and critic are the same class: the actor asks for two heads (mean and pre-std), the
    critic for one. LayerNorm carries no scale or bias, so there is nothing in it that can drift.
    """

    def __init__(self, rng, n_in, width, depth, heads, sparsity=0.9):
        self.rng = rng
        self.width = width
        self.depth = depth
        dims = [n_in] + [width] * depth
        self.w = [_sparse(rng, dims[i + 1], dims[i], sparsity) for i in range(depth)]
        self.b = [np.zeros(dims[i + 1]) for i in range(depth)]
        self.hw = [_sparse(rng, h, width, sparsity) for h in heads]
        self.hb = [np.zeros(h) for h in heads]
        self.sparsity = sparsity
        self.params = self.w + self.b + self.hw + self.hb
        self.init = [p.copy() for p in self.params]
        self.trace = [np.zeros_like(p) for p in self.params]
        self.grad = [np.zeros_like(p) for p in self.params]
        self.age = [np.zeros(width) for _ in range(depth)]
        self.util = [np.zeros(width) for _ in range(depth)]
        self.budget = 0.0

    def forward(self, x):
        self.cache = []
        self.acts = []
        h = x
        for w, b in zip(self.w, self.b):
            a = w @ h + b
            inv = 1.0 / np.sqrt(a.var() + 1e-5)
            hat = (a - a.mean()) * inv
            self.cache.append((h, hat, inv))
            h = np.where(hat > 0.0, hat, _LEAK * hat)
            self.acts.append(h)
        self.hidden = h
        return [hw @ h + hb for hw, hb in zip(self.hw, self.hb)]

    def backward(self, heads):
        n_heads = len(self.hw)
        for i, g in enumerate(heads):
            np.outer(g, self.hidden, out=self.grad[2 * self.depth + i])
            self.grad[2 * self.depth + n_heads + i][:] = g
        dh = sum(hw.T @ g for hw, g in zip(self.hw, heads))
        for layer in reversed(range(self.depth)):
            h_in, hat, inv = self.cache[layer]
            g = dh * np.where(hat > 0.0, 1.0, _LEAK)
            da = inv * (g - g.mean() - hat * np.mean(g * hat))
            np.outer(da, h_in, out=self.grad[layer])
            self.grad[self.depth + layer][:] = da
            dh = self.w[layer].T @ da

    def step(self, delta, lr, kappa, decay):
        """ObGD: the update cannot move the weights more than `1/kappa` in L1, whatever delta is."""
        total = 0.0
        for trace, grad in zip(self.trace, self.grad):
            trace *= decay
            trace += grad
            total += np.abs(trace).sum()
        bound = max(abs(delta), 1.0) * total * lr * kappa
        size = lr / bound if bound > 1.0 else lr
        for param, trace in zip(self.params, self.trace):
            param += size * delta * trace
        return size

    def regenerate(self, pull):
        for param, start in zip(self.params, self.init):
            param += pull * (start - param)

    def track(self, decay=0.99):
        for layer in range(self.depth):
            if layer + 1 < self.depth:
                outgoing = np.abs(self.w[layer + 1]).sum(axis=0)
            else:
                outgoing = sum(np.abs(hw).sum(axis=0) for hw in self.hw)
            use = np.abs(self.acts[layer]) * outgoing
            self.util[layer] *= decay
            self.util[layer] += (1.0 - decay) * use
            self.age[layer] += 1.0

    def recycle(self, rate, maturity):
        """Continual backprop: reseed the least useful mature unit, so capacity comes back."""
        self.budget += rate * self.width * self.depth
        while self.budget >= 1.0:
            self.budget -= 1.0
            best = None
            for layer in range(self.depth):
                mature = np.flatnonzero(self.age[layer] > maturity)
                if mature.size == 0:
                    continue
                unit = mature[int(np.argmin(self.util[layer][mature]))]
                if best is None or self.util[layer][unit] < best[2]:
                    best = (layer, int(unit), self.util[layer][unit])
            if best is None:
                break
            layer, unit, _ = best
            fan_in = self.w[layer].shape[1]
            self.w[layer][unit] = _sparse(self.rng, 1, fan_in, self.sparsity)[0]
            self.b[layer][unit] = 0.0
            self.init[layer][unit] = self.w[layer][unit]
            self.init[self.depth + layer][unit] = 0.0
            self.trace[layer][unit] = 0.0
            self.trace[self.depth + layer][unit] = 0.0
            # The new unit joins muted, so regaining capacity never disturbs the current policy.
            if layer + 1 < self.depth:
                self.w[layer + 1][:, unit] = 0.0
                self.init[layer + 1][:, unit] = 0.0
                self.trace[layer + 1][:, unit] = 0.0
            else:
                for i in range(len(self.hw)):
                    self.hw[i][:, unit] = 0.0
                    self.init[2 * self.depth + i][:, unit] = 0.0
                    self.trace[2 * self.depth + i][:, unit] = 0.0
            self.age[layer][unit] = 0.0
            self.util[layer][unit] = 0.0

    def norm(self):
        return float(np.sqrt(sum(float(np.sum(p * p)) for p in self.params)))


class ReinforcementLearning(goofi.Node):
    """Learns to drive a continuous action so a reward keeps rising, and never settles.

    Wire a feature of the signal to `observations`, the feature you want raised to `reward`, and
    `actions` to whatever sets the stimulus. `diagnostics` is how stagnation is SEEN before it costs
    a session: the average reward, how surprised the agent is, and three plasticity readings.
    """

    TAGS = ["ml", "control"]
    INPUTS = {
        "observations": goofi.InputSlot(goofi.DataType.ARRAY, required=True),
        "reward": goofi.InputSlot(goofi.DataType.ARRAY, required=False, trigger=False),
    }
    OUTPUTS = {
        "actions": goofi.DataType.ARRAY,
        "value": goofi.DataType.ARRAY,
        "diagnostics": goofi.DataType.ARRAY,
    }
    PARAMS = {
        "agent": {
            "width": goofi.IntParam(128, 4, 2048, doc="Units in each hidden layer of both networks."),
            "depth": goofi.IntParam(2, 1, 8, doc="Hidden layers in each network."),
            "actions": goofi.IntParam(2, 1, 64, doc="How many continuous actions to drive."),
            "reset": goofi.PulseParam(doc="Forget everything learned and start again."),
        },
        "learning": {
            "enabled": goofi.BoolParam(True, doc="Learn from what arrives. Off keeps acting on what is known."),
            "step_size": goofi.FloatParam(1.0, 0.0, 10.0, doc="ObGD bounds its own step, so 1.0 is the working default."),
            "gamma": goofi.FloatParam(0.9, 0.0, 0.999, doc="How far ahead to look. Seconds-long effects want less than 0.99."),
            "trace_decay": goofi.FloatParam(0.8, 0.0, 0.99, doc="Lambda: how long an action stays credited for what follows."),
            "entropy": goofi.FloatParam(0.01, 0.0, 1.0, doc="Pressure to keep exploring. It only ever pushes exploration up."),
            "kappa_policy": goofi.FloatParam(3.0, 0.1, 20.0, doc="Bounds how far one sample can move the policy."),
            "kappa_value": goofi.FloatParam(2.0, 0.1, 20.0, doc="Bounds how far one sample can move the value estimate."),
        },
        "continual": {
            "centering": goofi.FloatParam(0.01, 0.0, 1.0, doc="How fast the average reward is tracked. 0 stops centering."),
            "forget": goofi.FloatParam(0.001, 0.0, 0.5, doc="How fast input statistics forget. 0 freezes them after a while."),
            "regenerate": goofi.FloatParam(1e-4, 0.0, 0.01, doc="Pull back toward the initial weights, which bounds their growth."),
            "recycle": goofi.FloatParam(1e-4, 0.0, 0.01, doc="Share of units reseeded per step. This is what returns lost capacity."),
            "maturity": goofi.IntParam(200, 1, 100000, doc="Steps a unit is protected for before it can be reseeded."),
        },
        "action": {
            "noise": goofi.StringParam("pink", options=["pink", "white"], doc="Pink is correlated in time, so a slow system can follow it."),
            "low": goofi.FloatParam(-1.0, -1e6, 1e6, doc="What an action of -1 comes out as."),
            "high": goofi.FloatParam(1.0, -1e6, 1e6, doc="What an action of +1 comes out as."),
            "slew": goofi.FloatParam(0.0, 0.0, 1e6, doc="Largest change per step, in output units. 0 does not limit."),
        },
    }

    def setup(self):
        self.rng = np.random.default_rng()
        self.agent = None
        self.n_obs = 0
        self.prev = None
        self.average = 0.0
        self.trace = 0.0
        self.delivered = None
        self.seen = 0.0
        self.steps = 0
        self.delta = 0.0
        self.size = 0.0

    def _build(self, n_obs, n_act):
        p = self.params
        self.n_obs = n_obs
        self.n_act = n_act
        self.actor = _Net(self.rng, n_obs, p.agent.width, p.agent.depth, [n_act, n_act])
        self.critic = _Net(self.rng, n_obs, p.agent.width, p.agent.depth, [1])
        self.obs_norm = _Norm(n_obs, p.continual.forget)
        self.reward_norm = _Norm((), p.continual.forget)
        self.pink = _Pink(self.rng, n_act)
        self.rank = _Rank(self.rng, p.agent.width)
        self.agent = True
        self.prev = None
        self.delivered = None
        self.average = 0.0
        self.seen = 0.0
        self.trace = 0.0
        self.steps = 0

    def process(self, observations, reward=None):
        p = self.params
        obs = np.nan_to_num(np.asarray(observations.data, dtype=np.float64).ravel())
        if obs.size == 0:
            raise ValueError("an observation with no values says nothing about the state")
        if self.agent is None or obs.size != self.n_obs or p.agent.actions != self.n_act \
                or p.agent.width != self.actor.width or p.agent.depth != self.actor.depth:
            self._build(obs.size, p.agent.actions)
        self.obs_norm.forget = p.continual.forget
        self.reward_norm.forget = p.continual.forget

        self.obs_norm.observe(obs)
        state = self.obs_norm.whiten(obs)
        value = float(self.critic.forward(state)[0][0])

        if self.prev is not None:
            raw = 0.0
            if reward is not None:
                values = np.asarray(reward.data, dtype=np.float64).ravel()
                if values.size:
                    raw = float(np.nan_to_num(np.mean(values)))
            self.trace = p.learning.gamma * self.trace + raw
            self.reward_norm.observe(self.trace)
            self.seen += max(p.continual.forget, 1.0 / self.reward_norm.n) * (raw - self.seen)
            scaled = raw / float(self.reward_norm.std)
            was = float(self.critic.forward(self.prev["state"])[0][0])
            self.delta = scaled - self.average + p.learning.gamma * value - was
            self.average += p.continual.centering * self.delta

            if p.learning.enabled:
                self._learn(self.prev, self.delta)

        mu, pre_std = self.actor.forward(state)
        std = np.logaddexp(0.0, pre_std)
        noise = self.pink() if p.action.noise == "pink" else self.rng.standard_normal(self.n_act)
        action = mu + std * noise

        self.rank.observe(self.actor.hidden)
        self.steps += 1
        if self.steps % 16 == 1:
            self.rank.compute()

        # Learning uses the SAMPLE, never what was delivered: the density has to match what was
        # drawn, and clipping or slewing the output would silently break that.
        self.prev = {"state": state, "action": action}

        span = 0.5 * (p.action.high - p.action.low)
        mid = 0.5 * (p.action.high + p.action.low)
        out = mid + span * np.clip(action, -1.0, 1.0)
        if p.action.slew > 0.0 and self.delivered is not None:
            out = np.clip(out, self.delivered - p.action.slew, self.delivered + p.action.slew)
        self.delivered = out

        entropy = float(np.mean(np.log(std) + 0.5 * np.log(2.0 * np.pi * np.e)))
        # Off the tracked utility, not the instant activation: LeakyReLU holds half its units just
        # under any activation threshold, which would read as dormancy in a perfectly healthy net.
        util = self.actor.util[-1]
        share = util / max(float(util.mean()), 1e-12)
        stats = np.array(
            [self.seen, self.delta, entropy, self.rank.value, float(np.mean(share <= 0.025)), self.actor.norm(), self.size],
            dtype=np.float32,
        )
        names = ["avg_reward", "td_error", "entropy", "effective_rank", "dormant", "weight_norm", "step_size"]
        return {
            "actions": (out.astype(np.float32), {"channels": {"dim0": [f"action{i}" for i in range(self.n_act)]}}),
            "value": (np.array([value], dtype=np.float32), {}),
            "diagnostics": (stats, {"channels": {"dim0": names}}),
        }

    def _learn(self, prev, delta):
        p = self.params
        decay = p.learning.gamma * p.learning.trace_decay

        self.critic.forward(prev["state"])
        self.critic.backward([np.ones(1)])
        self.critic.step(delta, p.learning.step_size, p.learning.kappa_value, decay)

        mu, pre_std = self.actor.forward(prev["state"])
        std = np.logaddexp(0.0, pre_std)
        off = prev["action"] - mu
        d_mu = off / (std * std)
        # sign(delta) cancels against the delta the step carries, so entropy is only ever pushed up.
        d_std = (off * off - std * std) / (std ** 3) + p.learning.entropy * np.sign(delta) / std
        self.actor.backward([d_mu, d_std * (1.0 / (1.0 + np.exp(-pre_std)))])
        self.size = self.actor.step(delta, p.learning.step_size, p.learning.kappa_policy, decay)

        for net in (self.actor, self.critic):
            net.regenerate(p.continual.regenerate)
            net.track()
            net.recycle(p.continual.recycle, p.continual.maturity)

    def pulse_agent_reset(self):
        self.agent = None
        self.delivered = None
