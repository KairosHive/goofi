# The simulation bundle

A fifth first-party bundle, `node-bundles/simulation/`: models that MAKE their own dynamics
instead of measuring somebody else's. **BUILT** — fourteen Rust signal nodes in that bundle, seven
`.wgsl` automata beside `Life` in `node-bundles/graphics/`, and one scenario in
`goofi-tests/tests/simulation.rs`. `Tag::Simulation` existed in `goofi-node/src/tags.rs` and
nothing wore it; this bundle is its first user.

**A bundle is a DOMAIN and the graphics automata still live in the graphics bundle.** The owner put
`Life.wgsl` there, and `graphics.rs` asserts that every shipped `.wgsl` under `node-bundles/graphics`
is a type "and nothing else is" — so a `.wgsl` in `node-bundles/simulation/` would fail that test.
The `simulation` TAG is what unites the family across the two bundles, which is what a tag is for.
Whether the test should scan every bundle instead is the owner's call.

## What it is for

A biosignal is one half of a loop. The other half is a model the signal drives and that drives the
patch back — a synchronizing network, a growing pattern, a swarm a heart rate steers. goofi 2 had
three such nodes and they were the ones people played with.

## Decisions taken

**A simulation advances by ELAPSED time, never by one step per frame.** `ctx.now` is the clock;
the node integrates the steps `output.sfreq` owes for the time that passed, counted from the start
so it cannot drift, and capped at one second's worth so a stalled frame is not a freeze. A frame
rate is a viewing rate. This is what goofi 2's Kuramoto got wrong: it re-integrated a thousand
steps from zero on every call, so the model had no history and the coupling never had time to act.

**`output.mode` is `value` or `block`, and `LFO` is the precedent.** `value` emits the state now,
which a param reference reads as a number. `block` emits every step since the last frame at the
model's own `sfreq`, which is a signal — the only way a model faster than the frame rate is heard
rather than aliased. A model whose state is a SNAPSHOT rather than a signal has no `mode`:
`Swarm`, `Physarum` and `Hopfield` are positions, a field and a settled state, not time series.

**`sim.dt` is the model's own time step and `output.sfreq` is the clock; their product is the
speed.** This settles what the first draft left open. Neither derives the other, and a node whose
model is a MAP — the logistic map, a Boolean network, an avalanche — declares no `dt` at all.

**Seeding is `sim.seed` plus `sim.reset`.** An integer seed, negative for "fresh from the clock",
is `Noise.rs`'s rule. `reset` is a `Pulse`, so an expression re-runs the model from a threshold
crossing. Any edit that changes what was DRAWN — a size, a wiring, a density — re-draws.

**Size comes from the wire where there is one.** A model wired to an `[N, N]` matrix IS `N` wide;
the `size` param decides only when nothing is wired. Two owners of N is the defect principle 8
names, and the scenario pins the precedence.

**A model is one node with a `model` param, not one node per equation.** `Filter` is one node with
four modes where goofi 2 had four; the same holds here. Fourteen nodes carry forty-odd named
models between them.

**No dependency.** Every model is plain Rust: `xorshift64*` for the RNG, a loop for a
matrix-vector product. goofi 2's spiking node needed torch and a GPU; this one needs neither.

**Noise belongs in a model's INPUT, not in one of its state variables**, and the default is not
zero where the model's whole point is a noise-driven resonance. `NeuralMass` was written the other
way first and the scenario caught it: Jansen-Rit on a constant input falls onto a slow cycle of
its own and peaked at 3.9 Hz. Driven the way Jansen and Rit drive it, it peaks in the alpha band,
and the scenario now measures that through a `Buffer` and a `Psd` — the same chain a user would.

## What is built

The three from goofi 2, changed rather than ported:

- **`Kuramoto`** — N phase oscillators. State persists; natural frequencies arrive as an ARRAY
  rather than a comma-separated string; the coupling is a matrix off the graph with the scalar as
  its gain. Outputs `phases`, `waveforms`, `order`.
- **`Reservoir`** — an echo state network. goofi 2's had no input drive at all, so it decayed to
  nothing, and it scaled its weights by a mean and a standard deviation, which does not control
  the echo-state property. This one has an input with its own weights, a `leak` rate, and
  `spectral_radius` — rescaled by power iteration — as the one edge-of-chaos knob.
- **`Spiking`** — LIF, Izhikevich and adaptive exponential, sparse fixed in-degree, delays by
  distance, a spike scattered forward through a ring rather than every neuron polling. goofi 2
  built an `N x N` distance matrix, 400 MB at its own maximum. `potentials` is reported the same
  way for all three models: 0 at rest, 1 at threshold.

The ten classic:

| Node | Engine | Models | State |
|---|---|---|---|
| `Attractor` | signal | lorenz, rossler, chua, thomas, aizawa, halvorsen, henon, ikeda, logistic, standard | built |
| `Oscillator` | signal | vanderpol, duffing, fitzhugh, stuartlandau, pendulum | built |
| `NeuralMass` | signal | jansenrit, wilsoncowan, wongwang — connectome-coupled | built |
| `Swarm` | signal | boids, vicsek, gravity (+ the two creative modes) | built |
| `Population` | signal | lotkavolterra, rosenzweig, sir, seir, ricker | built |
| `Reaction` | graphics | grayscott, brusselator | built |
| `Ising` | graphics | Metropolis on a checkerboard, temperature as the knob | built |
| `Sandpile` | graphics | Bak-Tang-Wiesenfeld, with `threshold` as its class | built |
| `Elementary` | graphics | Wolfram 1-D, rules 0 to 255, scrolling | built |
| `Wave` | graphics | the 2-D wave equation, on two frames of history | built |

The ten creative. Six are new signal nodes, two are modes of `Swarm`, one is a shader, and one is open:

| Node | Engine | What it is | State |
|---|---|---|---|
| `Physarum` | signal | slime-mould agents growing a transport network | built |
| `Hopfield` | signal | classic and dense associative memory | built |
| `Readout` | signal | recursive least squares over a reservoir, trained live | built |
| `Boolean` | signal | Kauffman's random Boolean network | built |
| `Branching` | signal | the neuronal-avalanche model | built |
| `Quantum` | signal | an exact statevector of up to twelve qubits | built |
| `Swarm` `swarmalators` | signal | position coupled to phase | built |
| `Swarm` `particlelife` | signal | asymmetric attraction between colours | built |
| `NeuralCA` | graphics | a learned automaton, weights on an ARRAY input | built |
| `Lenia` flow mode | graphics | mass-conserving, multi-species | open |

`Attractor` normalizes by each system's own size, so Lorenz and the logistic map both modulate a
param directly. `Oscillator`'s `stuartlandau` puts the Hopf bifurcation on one slider, and the
scenario walks across it in both directions.

## How a graphics automaton is written

The engine gained named state buffers and a frame counter (`f2e598db`), and every automaton here
is built on them. `Life.wgsl` shipped with that change and is the reference; these seven follow it.

- **`"state": ["name"]` declares a buffer.** The name reads what the LAST tick wrote, through
  `textureLoad`; the body writes it by defining `next_name(uv) -> vec4f`. Two buffers is what
  `Wave` needs, because the wave equation cannot be advanced from one frame of history alone —
  that is the case a boolean `state` flag could not have expressed, and the list can.
- **`frame == 0u` is where a grid is seeded**, so `density` and `seed` are ordinary params rather
  than a `Noise -> Threshold` chain wired in from outside.
- **`frame` is what a checkerboard update needs.** `Ising` updates one sublattice per tick, and
  `time` is a float that cannot say which turn it is.
- **`textureLoad`, never `textureSample`.** The shared sampler filters linearly, so a sampled cell
  is a smeared one. Every automaton here reads exact texels and wraps its own coordinates.
- **`shade` reads the state; it does not recompute it.** The engine calls `shade` and every
  `next_*` on the same pixel, so a body that computes the step in both pays for it twice —
  `Lenia`'s kernel is 441 samples at the default radius. The picture is one tick behind, which at
  60 Hz nobody sees, and it lets `Reaction` show a chemical rather than the raw buffer.

**`Lenia` is separate from `Life`, not a mode of it.** They share no param: `Life` has no radius
and no growth, `Lenia` has no birth and no survive. It also needs its soup seeded in BLOCKS —
`scale` — because a field seeded per pixel is below the kernel's reach and dissolves before
anything forms.

## Already covered — do not add

- **Advection and diffusion.** `Displace` in a `Feedback` loop IS semi-Lagrangian advection, and
  `Blur` in one is diffusion. A `Fluid` node earns its place only when pressure projection does,
  and projection is many Jacobi passes, which the plan cannot express.
- **Ornstein-Uhlenbeck, fractional Brownian motion, 1/f^beta.** Modes for the shipped `Noise.rs`
  in the `signal` bundle. A second noise node is drift, not a feature.
- **A generic ODE node.** A param source is already Python, so "integrate whatever the user
  writes" is `Function` plus a state.

## Considered and not built

- **An active-inference agent.** On theme, and the hardest to give honest slots: what its input
  and output ARE is a research question, not a manifest.
- **A self-organizing map.** It measures rather than simulates; it belongs with the analysis nodes.
- **A digital waveguide (audio).** Physical modelling is simulation and the audio bundle has none.
  The strongest argument for this bundle reaching a third engine, and a separate question.
- **Diffusion-limited aggregation, and L-systems.** Both want geometry goofi cannot draw yet.

## Open

- **Nothing draws points.** `Swarm` and every particle model answer `[N, D]` positions, and the
  graphics engine is a full-screen fragment chain with no way to splat them. `Physarum` works
  around it by rasterizing its own field into `ArrayIn`, at a copy per frame. Whether the answer
  is a point-splatting graphics node or a compute stage is the graphics engine's question, and it
  is worth asking before more particle models arrive.
- **`Lenia` is expensive enough to starve its neighbours.** Measured on a live server, 2026-09-07,
  on an RTX 4090: eight automata at 512 square on one tick pushed `tick_max_us` to 24 ms, past the
  60 Hz budget, and four of the eight stopped producing frames — `Life` among them, which is the
  cheapest of the set, so it is the TICK that saturates and not any one node. Each renders fine
  alone. Lenia at radius 10 is 441 samples a pixel, which is 115 M samples a frame at 512 square.
  The engine has no per-stage budget and no way to say "this stage may run at a lower rate"; a
  demand-driven engine that silently drops stages under load is worth a look on its own.
  **Worth re-measuring before it is designed for**: the second half of that reading — stages that
  stopped producing — has a candidate cause that is now fixed, the tap pacing in
  `graphics-engine.md`, which held every viewer in the app at a third of the clock whatever the
  load. The 24 ms tick is real either way.
- **Sub-stepping in the graphics plan.** One tick is one pass, so an automaton that wants ten steps
  a frame cannot have them. A fragment shader cannot loop over a texture it writes, so this is the
  plan's question and not a node's.
- **`Swarm` is O(N squared) and capped at 2000 particles.** A uniform grid would lift that by an
  order of magnitude, and nothing needs it yet.
- **Long-range delays in `NeuralMass`.** A whole-brain model's delays are the point of the
  connectome, and this one couples instantaneously. The ring buffer `Spiking` already has is the
  shape of the fix.
- **Flow-Lenia** (2023) is not built. Mass-conserving transport is a velocity field derived from
  the growth and then mass moved along it, which is a gather this shader shape can express but only
  correctly — and "correctly" here is a numerical claim the scenario cannot check by looking at a
  frame. It wants a measurement of total mass across ticks before it ships.
- **Swift-Hohenberg** is not a `Reaction` mode. It needs a biharmonic, so a thirteen-point stencil
  and a step small enough not to blow up; the two that shipped are well behaved at the defaults and
  it is not.
- `Sandpile` is Bak-Tang-Wiesenfeld alone. Manna needs each toppling cell to pick its neighbours at
  random, which a gather formulation can do only if both sides draw the same number — worth doing,
  and worth doing carefully.
- Whether `graphics.rs`'s sweep should scan every bundle rather than `node-bundles/graphics` alone,
  which is what decides where a `.wgsl` in a domain bundle may live.
- Whether this bundle reaches the audio engine at all.
