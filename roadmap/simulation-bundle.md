# The simulation bundle

`node-bundles/simulation/` leaves this repo with the other bundles (`library.md`); these items go
with it.

## Not to be done

- **A `Fluid` node in this bundle.** `Displace` in a `Feedback` loop is advection and `Blur` in one
  is diffusion; pressure projection needs many passes per frame, which waits on graphics
  sub-stepping (`fractal-textures.md` owns `Fluid.wgsl`).
- **A second noise node.** Ornstein-Uhlenbeck, fractional Brownian motion and 1/f^beta go into
  the `signal` bundle's `Noise.rs` as modes; it has `uniform`, `normal` and `pink` today.
- **A generic ODE node.** A param source is already Python; `Function` plus a state does it.
- **An active-inference agent.** Its input and output are a research question, not a manifest.
- **A self-organizing map.** It measures rather than simulates; it belongs with the analysis nodes.
- **A digital waveguide.** Physical modelling is audio-engine work and a separate question.
- **Diffusion-limited aggregation and L-systems.** Both need geometry goofi cannot draw yet.

## Open

- **Nothing draws points.** `Swarm` and every particle model answer `[N, D]` positions; the
  graphics engine is a fragment chain with no way to splat them, and `Physarum` rasterizes its own
  field into `graphics:SignalIn` at a copy per frame. Decide between a point-splatting graphics
  node and a compute stage before more particle models arrive.
- **The graphics tick has no per-stage budget** and no way to run a stage at a lower rate; the
  tick saturates, not one node. Re-measure before designing for it.
- **Sub-stepping in the graphics plan.** One tick is one pass; an automaton that wants ten steps a
  frame cannot have them. A fragment shader cannot loop over a texture it writes, so this is the
  plan's question, not a node's.
- **`Swarm` is O(N squared) and capped at 2000 particles.** A uniform grid would lift that by an
  order of magnitude; nothing needs it yet.
- **Long-range delays in `NeuralMass`.** The ring buffer `Spiking` has is the shape of the fix.
- **Flow-Lenia.** Mass-conserving transport is a gather this shader shape can express; it needs a
  measurement of total mass across ticks before it ships.
- **Swift-Hohenberg** as a `Reaction` mode needs a biharmonic and a step small enough not to blow up.
- **Manna in `Sandpile`.** Each toppling cell picks its neighbours at random, which a gather
  formulation can do only if both sides draw the same number.
- Whether this bundle reaches the audio engine at all.
