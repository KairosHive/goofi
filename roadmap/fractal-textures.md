# Fractal textures: porting mfractal into the graphics engine

Source: `AntoineBellemare/fractal_visuals`, branch `mfractal-toolbox`.

## Not to be done

- **A node per rock or cloud texture.** `marble`, `agate`, `veined` and the rest are `Noise` into
  `Displace`, `Lookup` and `Threshold`.
- **Porting `eddies`, `vorticity` and `plume` as written.** They iterate; pressure projection is
  many Jacobi passes, which waits on the sub-stepping question in `simulation-bundle.md`.

## Remaining steps

1. **A `curl` output mode on `graphics:Noise`** — a divergence-free flow field as rgb, so
   `Displace` and `Feedback` advect with it. That is `curl_weave`, `rheoscopic` and
   `dye_diffusion` by composition rather than three nodes.
2. **`Fluid.wgsl`**, with state buffers and projection iterated across ticks. Only after the
   graphics engine's sub-stepping question is settled.

## Open

- **Re-measuring c2 after a change to the cascade** runs `WaveletLeaders` on `node snapshot --raw`
  frames by hand; the committed test (`textures.rs`) sees kurtosis only.
- **`amount` is one knob for four meanings.** The cascade's sigma, the warp's distance and the
  spread of local dimension are not the same quantity, and `fbm`, `ridged` and `billow` ignore it.
- **`ridged` and `billow` cannot combine with `multifractal`.** They share the `fractal` param.
  Splitting them into two would fix it and cost a third knob; `kind` stays the basis and `fractal`
  the combination rule.
- **The signal half.** The exact numpy generators are fast enough as a Python node emitting
  `[H, W]` into `graphics:SignalIn` — approximate in the shader for motion, exact on the signal
  side for stimuli. Both should exist.
- Whether `universal_multifractal`'s Levy-stable cascade has a shader form at all. Its variates are
  not something a hash gives you cheaply.
