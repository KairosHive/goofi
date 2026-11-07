# Inference

Three nodes for building a Markov blanket between a person and a machine in a patch, from the
Bayesian filtering and active inference work the free energy principle draws on. All three run on
numpy alone, so goofi runs them inside its own process.

| node | reads | gives |
|---|---|---|
| `Belief` | a signal, [C] a sample or [C, N] a chunk | per channel mean, precision, prediction error, surprise, volatility |
| `ActiveAgent` | one observed signal, and optionally a live preference | an action, its level in 0..1, the policy probabilities, the belief, the expected free energy per action, the surprise |
| `Blanket` | an internal set and an external set of signals, and optionally a blanket set | partial correlations, leak, blanket membership, transfer entropy each way |

In biofeedback the person's signals are the machine's sensory states and the machine's outputs
(what it draws, plays or changes) are its active states. Belief is the machine's model of the
person, ActiveAgent chooses the active states, and Blanket measures what crosses between the two
sides and which way.

## Belief

A Kalman filter on a random walk (`kalman`), or the two-level Hierarchical Gaussian Filter of
Mathys et al. (2014) in the update order of Weber et al.'s eHGF (`hgf`), where a second level
learns how restless each channel is. `drift` is the log step variance, `noise` the log measurement
variance, `coupling` and `meta` the volatility level's strength and speed. `log` takes the log of
positive quantities such as band powers, and `normalize` z-scores each channel against its own
running mean and spread over `memory` seconds. A sample that is not finite, or not positive under
`log`, is missing, and that channel's belief waits for the next sample.

## ActiveAgent

A discrete POMDP agent. Its hidden state has `states` levels, read through the observed signal
binned into `bins` levels against its own recent quantiles, and moved by `actions` choices. It
learns how each action moves the state (Dirichlet counts at rate `learn`), prefers observations
near `target` with weight `strength`, and every `period` seconds scores every sequence of `depth`
actions by expected free energy (risk, ambiguity, and novelty weighted by `curiosity`). It samples
a policy at precision `gamma`, or takes the best.

## Blanket

Over the last `window` joint samples it fits a Gaussian graphical model, shrinking the correlation
matrix toward the identity by `shrink`. `leak` is the mean |partial correlation| between the
internal and external sets given every other signal, so it falls toward zero when the wired blanket
signals screen the two sets from each other. `membership` scores each signal by its strongest link
to the internal set. `influence` is Gaussian transfer entropy at `lag` samples, internal to external
and external to internal.

## Measured

`tools/benchmark_inference.py` runs these checks with goofi's GIL Python, on simulations with a known
answer and on the sample `eeg-rest-srm.edf` (4 minutes of eyes-closed resting EEG, OpenNeuro
ds003775). Times are on a Core Ultra 9 185H.

| node | check | result |
|---|---|---|
| Belief | a random walk switching between step sd 0.02 and 0.2 every 750 samples, seen through noise sd 0.1, tracking error | Kalman 0.161 (slow) and 0.089 (fast), HGF 0.077 |
| Belief | the HGF's volatility against the walk's true log step variance | r = 0.92 |
| Belief | time a call, 1 to 64 channels, one sample | 0.05 to 0.10 ms |
| Belief | 64 channels by 256 samples in one frame | 12 to 15 ms |
| Blanket | linear system where internal and external states meet only through sensory and active states, leak given those, and without them | 0.06 and 0.23 |
| Blanket | the same system with a direct external-to-internal path | 0.16 and 0.42 |
| Blanket | membership of the internal set's blanket, strongest first | the active state 0.73, the others 0.08 or less |
| Blanket | x drives y, transfer entropy x to y and y to x | 0.234 and 0.000 nats |
| Blanket | time a sample, window 2000, refit every sample | 1.3 ms |
| ActiveAgent | a simulated person whose level each action lowers, holds or raises, mean level over the last 200 of 400 decisions, target the top | always lower 0.02, random 0.54, always raise 0.97, the agent 0.94 to 0.96 (depth 1 to 3, 5 seeds) |
| ActiveAgent | the same agent with its target at the bottom | 0.05 |
| ActiveAgent | time a decision, 3 actions, depth 1, 2 and 3 | 0.2, 0.3 and 1.0 ms |

On the resting EEG, relative alpha power at 4 Hz through Belief cost 0.06 ms a frame for 12
channels. With frontal and parieto-occipital alpha as the two sets, Blanket gave a leak of 0.138,
0.115 given central and temporal alpha, and transfer entropy of 0.038 nats from back to front
against 0.011 from front to back.

## Examples

`examples/markov-blanket/` holds four patches, each built through goofi's own ops, run headless,
and loaded again from disk with no warnings.

1. `01-surprise-map.gfi`. Resting EEG through EegPreprocess, a 1 s buffer, Psd and EegPowerBands,
   relative alpha into Belief (hgf, log), its surprise and volatility drawn as topomaps. After 45 s
   the 64 channels' surprise ran from 0.99 to 2.35 nats.
2. `02-agent-loop.gfi`. A closed loop with a simulated person. A Jansen-Rit NeuralMass stands for
   the person, ActiveAgent's level is added to its input, and its alpha power through Belief is the
   agent's observation, with the target at the top. In the build run, after 100 decisions the agent
   saw the top bin and chose action 0 with probability 0.83 to 0.96.
3. `03-coupled-blankets.gfi`. Posterior alpha of resting EEG through Belief steers the coupling of
   16 Kuramoto oscillators. A second Belief models the oscillators' order, and Blanket reads what
   passes between the two beliefs.
4. `04-material-blanket.gfi`. The blanket as material. Occipital alpha's surprise frays an
   AdaptiveMask's edge, its belief moves MaterialSpace's first map axis (to 0.65 after 60 s), and
   the material is laid into the mask on the GPU through Composite's luminosity mode. It needs
   AdaptiveMask (the `texture` bundle) and MaterialSpace (the latent-texture plugin), and its
   ImageFile names a photograph on the machine it was built on, so point it at one of your own.

The EEG patches play goofi's sample `eeg-rest-srm.edf` through EegPlayback's own `sample` param, which
fetches it on first use. `tools/build_patches.py` rebuilds them against a running goofi named by
`GOOFI_SESSION`.

## Limitations

- In `03-coupled-blankets` the recorded EEG cannot respond to the machine, yet transfer entropy
  read 0.04 to 0.10 nats from the machine's belief to the person's, at or above the forward value.
  Both beliefs are smoothed signals, and Gaussian transfer entropy at a short lag on smoothed
  signals reports such reverse flows. Influence on filtered beliefs is a relative reading, and the
  simulation above is where its direction was validated.
- Blanket's leak and membership read conditional independence at one instant. In the simulation
  the sensory state, which acts on the internal states with a step's delay, scored 0.08 against
  0.73 for the active state.
- In `02-agent-loop`, reloaded from disk, the agent starts learning again, and after 102 decisions
  it had not yet settled (policy probabilities 0.34, 0.32, 0.34).
- ActiveAgent reads one observed signal and plans by enumerating every action sequence, which
  grows as actions to the power depth.
- Where a Markov blanket sits depends on the model written for the system, so these nodes measure
  the blanket of the signals wired into them.
