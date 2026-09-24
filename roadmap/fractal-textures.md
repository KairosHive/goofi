# Fractal textures: porting mfractal into the graphics engine

`AntoineBellemare/fractal_visuals`, branch `mfractal-toolbox`, is a numpy/scipy generator of
multifractal texture fields plus the wavelet-leaders code that measures c1 and c2. `graphics:Noise`
carries `fractal` (`fbm | ridged | billow | warped | multifractal | multifractional`) and one
`amount` float; `complexity:WaveletLeaders` measures c1/c2. This file is what remains and the
decisions that bind it.

## Decisions

**`kind` is the basis; `fractal` is the COMBINATION rule.** Ridged, billow, warped and
multifractal are other ways to fold the same octaves together, not other noises. Putting them in
`kind` would cross two axes in one param and make `worley + ridged` unreachable.

**The cascade is an APPROXIMATION and its correction term is half the textbook's.** The shader's
`omega` is a few octaves, not a true log-correlated field. The textbook `-sigma^2/2` holds the
exponent's mean and kills the picture; no correction saturates the `tanh`; half of it is what
works. The claim that survived measurement: c2 is negative, monotone in `amount`, and of the same
order as the library's (`VOL_GAIN` 2 makes the knob mean roughly the literature's sigma). The
committed test pins KURTOSIS, the half of the signature that moved with c2 across the sweep.

**Nothing in the rock and cloud sets earns a node of its own.** `marble`, `agate`, `veined` and the
rest are warped fBm plus banding plus a threshold — `Noise` into `Displace`, `Lookup` and
`Threshold`. Fifteen rock nodes would be the per-need node explosion the library rule exists to
prevent.

**The fluids are the one real new node, and the sims are not portable as written.** `eddies`,
`vorticity` and `plume` iterate; pressure projection is many Jacobi passes. `simulation-bundle.md`
owns the multi-pass and sub-stepping question this waits on.

## Order of the rest

4. **A `curl` output mode** — a divergence-free flow field as rgb, so `Displace` and `Feedback`
   advect with it. That is `curl_weave`, `rheoscopic` and `dye_diffusion` by composition rather
   than three nodes.
5. **`Fluid.wgsl`**, with state buffers and projection iterated across ticks. Only after the
   graphics engine's sub-stepping question is settled.

## Open

- **Re-measuring c2 after a change to the cascade** runs `WaveletLeaders` on `node snapshot --raw`
  frames by hand; the committed test sees kurtosis only.
- **`amount` is one knob for four meanings.** The cascade's sigma, the warp's distance and the
  spread of local dimension are not the same quantity, and `fbm`, `ridged` and `billow` ignore it.
- **`ridged` and `billow` cannot combine with `multifractal`.** They are all one param. Splitting
  them into two would fix it and cost a third knob.
- **The signal half.** `multifractal_cloud` at 256 square runs at 53 fps in numpy, so the EXACT
  generators are viable as a Python node emitting `[H, W]` into `graphics:SignalIn` — approximate in
  the shader for motion, exact on the signal side for stimuli. Both should exist.
- Whether `universal_multifractal`'s Levy-stable cascade has a shader form at all. Its variates are
  not something a hash gives you cheaply.
