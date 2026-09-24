# The simulation bundle

`node-bundles/simulation/`: models that MAKE their own dynamics instead of measuring somebody
else's. A biosignal is one half of a loop; the other half is a model the signal drives and that
drives the patch back. The bundle leaves this repo with the others (`library.md`); the decisions
below travel with it.

## Decisions taken

**A simulation advances by ELAPSED time, never by one step per frame.** `ctx.now` is the clock;
the node integrates the steps `output.sfreq` owes for the time that passed, counted from the start
so it cannot drift, and capped at one second's worth so a stalled frame is not a freeze. A frame
rate is a viewing rate.

**`output.mode` is `value` or `block`, and `LFO` is the precedent.** `block` emits every step since
the last frame at the model's own `sfreq` — the only way a model faster than the frame rate is heard
rather than aliased. A model whose state is a SNAPSHOT (`Swarm`, `Physarum`, `Hopfield`) has no
`mode`.

**`sim.dt` is the model's own time step and `output.sfreq` is the clock; their product is the
speed.** A node whose model is a MAP declares no `dt` at all.

**Seeding is `sim.seed` plus `sim.reset`.** An integer seed, negative for "fresh from the clock";
`reset` is a `Pulse`. Any edit that changes what was DRAWN re-draws.

**Size comes from the wire where there is one.** A model wired to an `[N, N]` matrix IS `N` wide;
the `size` param decides only when nothing is wired.

**A model is one node with a `model` param, not one node per equation.**

**No dependency.** Every model is plain Rust: `xorshift64*` for the RNG, a loop for a matrix-vector
product.

**Noise belongs in a model's INPUT, not in one of its state variables**, and the default is not
zero where the model's whole point is a noise-driven resonance.

**A graphics automaton reads state with `textureLoad`, never `textureSample`**, seeds at
`frame == 0u`, and its `shade` reads the state rather than recomputing it. `Lenia` is separate from
`Life`, not a mode of it: they share no param.

## Already covered — do not add

- **Advection and diffusion.** `Displace` in a `Feedback` loop IS semi-Lagrangian advection, and
  `Blur` in one is diffusion. A `Fluid` node earns its place only when pressure projection does,
  and projection is many Jacobi passes, which one tick cannot express. Iterating ACROSS ticks — a
  few sweeps per frame into a state buffer — is the shape that fits; see sub-stepping below.
  `fractal-textures.md`'s `Fluid.wgsl` waits on this.
- **Ornstein-Uhlenbeck, fractional Brownian motion, 1/f^beta.** Modes for the shipped `Noise.rs`
  in the `signal` bundle. A second noise node is drift, not a feature.
- **A generic ODE node.** A param source is already Python, so "integrate whatever the user
  writes" is `Function` plus a state.

## Considered and not built

- **An active-inference agent.** What its input and output ARE is a research question, not a
  manifest.
- **A self-organizing map.** It measures rather than simulates; it belongs with the analysis nodes.
- **A digital waveguide (audio).** Physical modelling is simulation and the audio bundle has none.
  The strongest argument for this bundle reaching a third engine, and a separate question.
- **Diffusion-limited aggregation, and L-systems.** Both want geometry goofi cannot draw yet.

## Open

- **Nothing draws points.** `Swarm` and every particle model answer `[N, D]` positions, and the
  graphics engine is a full-screen fragment chain with no way to splat them. `Physarum` rasterizes
  its own field into `graphics:SignalIn`, at a copy per frame. Whether the answer is a
  point-splatting graphics node or a compute stage is the graphics engine's question, worth asking
  before more particle models arrive.
- **The graphics tick has no per-stage budget** and no way to say "this stage may run at a lower
  rate"; eight automata at 512 square on one tick pushed `tick_max_us` past the 60 Hz budget, and
  it is the TICK that saturates, not any one node. A demand-driven engine that drops stages under
  load is worth a look on its own. Re-measure before designing for it.
- **Sub-stepping in the graphics plan.** One tick is one pass, so an automaton that wants ten steps
  a frame cannot have them. A fragment shader cannot loop over a texture it writes, so this is the
  plan's question and not a node's.
- **`Swarm` is O(N squared) and capped at 2000 particles.** A uniform grid would lift that by an
  order of magnitude, and nothing needs it yet.
- **Long-range delays in `NeuralMass`.** The ring buffer `Spiking` already has is the shape of the
  fix.
- **Flow-Lenia** is not built. Mass-conserving transport is a gather this shader shape can express
  but only correctly, and "correctly" is a numerical claim a frame cannot show. It wants a
  measurement of total mass across ticks before it ships.
- **Swift-Hohenberg** is not a `Reaction` mode. It needs a biharmonic and a step small enough not
  to blow up.
- `Sandpile` is Bak-Tang-Wiesenfeld alone. Manna needs each toppling cell to pick its neighbours at
  random, which a gather formulation can do only if both sides draw the same number.
- Whether this bundle reaches the audio engine at all.
