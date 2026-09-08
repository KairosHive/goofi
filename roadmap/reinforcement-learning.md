# Reinforcement learning

`node-bundles/ml/reinforcement_learning.py` is Stream AC(λ) — built, and the decisions below are
taken. What is here is what is NOT built.

## Decisions already taken

**Stream AC(λ) rather than PPO or SAC.** A session around a person is about a thousand samples and
the optimum moves the whole time, so the algorithm has to learn from every sample and must never
converge. PPO collects a buffer and flushes it, which is fourteen updates in a session; a replay
buffer is a memory of a participant who has since changed. Stream AC updates once per frame with no
buffer, no target network and no batch.

**Pure numpy, no torch.** One sample and no batch make the gradients small enough to write out by
hand, which keeps the node on the free-threaded in-process tier and keeps torch out of the tree.

**Nothing is squashed inside the density.** The policy is a plain Normal and bounding happens after
the sample, so `log pi(a|s)` is always read on the value that was drawn. The Python node on `main`
computed the log-probability before its `tanh` and stored the action after it, and every importance
ratio it ever computed was against a density it had not sampled from.

**The continual layer is not optional, and it is measured.** Forgetting input statistics, pulling
weights back toward their initial values, and reseeding low-utility units are what stop the network
losing the ability to learn. Ablated on a five-epoch moving target over four seeds, the full agent
went 0.54 early to 0.92 late; without the layer it went 0.87 early to 0.16 late — it learns FASTER
at first and then cannot re-adapt at all. That trade is deliberate.

**Pink action noise.** A sum of independent normals is normal, so Voss-McCartney noise leaves the
marginal exactly standard and the density exactly right, while giving a slow system something it
can actually follow. White noise averages to its own mean before a participant responds to it.

**The diagnostics slot exists so stagnation is seen rather than guessed at.** `dormant` rising and
`effective_rank` falling is what plasticity loss looks like before the reward shows it.

## Open

- **No warm start.** An agent does not persist across sessions, so every session starts cold. For a
  participant seen more than once this is worth more than any remaining tuning. Where the weights
  live is the open question: the document would carry them into every save and copy, and a sidecar
  keyed by uid is the alternative.
- **No bandit mode.** At a thousand samples with a seconds-long response the problem is nearly a
  contextual bandit, and a windowed GP or linear bandit is the baseline the agent should have to
  beat. It belongs as a `mode` on this node rather than a node beside it.
- **Reward hacking through artifacts is unguarded.** Raising Lempel-Ziv complexity is cheapest by
  adding broadband noise, so a stimulus that makes a participant clench, blink or move will be
  found and taken. Rejecting artifacts before the reward feature is a patch-level answer today;
  whether the node should refuse a reward computed from an epoch flagged bad is open.
- **The `dormant` reading is uncalibrated.** Its direction tracks plasticity correctly in both arms
  of the ablation, but the 0.025 share threshold is inherited from a metric defined over ReLU
  activations and its absolute level means little here.
- **Slew and the density disagree.** Learning uses the sampled action, so a tight `slew` teaches the
  agent about actions it did not deliver. It defaults to off, and the honest fix is unresolved.
