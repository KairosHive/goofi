# Fractal textures: porting mfractal into the graphics engine

`AntoineBellemare/fractal_visuals`, branch `mfractal-toolbox` (the branch is `mfractal-toolbox`,
not `mfractal`), is a numpy/scipy generator of
**multifractal** texture fields — 15 rock families, 8 cloud, 7 fluid — plus the wavelet-leaders
code that measures c1 and c2. This file is what of it belongs in `graphics:Noise`, what needs a
node of its own, and what cannot cross at all. **Steps 1 to 3 are built**; the rest is not.

## What the library actually computes

Almost everything is built on ONE primitive: `_fractional_field(n, beta)` — white noise, multiplied
by `k^(-beta/2)` in the Fourier plane, inverse transformed. A field with a power-law spectrum.
Around it:

- **MRW / lognormal cascade** (`multifractal_cloud`, `lognormal_mrm`, `mrw_field`): `G * exp(sigma
  * omega)`, where `G` is that fractional field and `omega` is a log-correlated Gaussian. The
  product is what makes the result genuinely multifractal — c2 goes negative with `sigma`.
- **mBm blend** (`multifractional`, `mbm_blend`): a bank of fractional fields at different `beta`,
  mixed by a smooth control map, so the local fractal dimension VARIES across the image.
- **Domain warping, ridging, thresholds**: the rock and cloud families are these three applied to
  the two above.
- **Navier-Stokes** (`eddies`, `vorticity`, `plume`): real iterative sims with FFT Poisson solves.

## Measured, on this machine, at 256 square

The point of the table is not the numbers themselves — it is that **numpy slowness does not
predict shader slowness**. A family is slow here because numpy walks octaves in Python, and that
is exactly the work a fragment shader does for nothing.

| family | domain | ms @256 | what it is |
|---|---|---|---|
| `warped_fbm` | cloud | 29 | domain-warped fBm |
| `billow_smoke`, `cellular_stone`, `gneiss`, `granite`, `schist`, `breccia` | — | 35–39 | fBm + fold/threshold |
| `cascade_lognormal` | cloud | 46 | multiplicative cascade |
| `marble`, `turbulent`, `flow_banded`, `agate` | rock | 60–72 | warped fBm + banding |
| `veined`, `convoluted` | rock | 98–119 | several warps |
| `rheoscopic`, `dye_diffusion`, `curl_weave` | fluid | 291–448 | curl-noise advection |
| `billow` | cloud | 725 | many octaves in Python |
| `plume`, `eddies`, `vorticity` | fluid | **20 900–24 100** | iterative Navier-Stokes |

And the flagships, which are the ones the research rests on:

| generator | ms @256 | ms @512 |
|---|---|---|
| `_fractional_field` (the fBm core) | 4.9 | 24.4 |
| `lognormal_mrm` | 5.7 | 25.7 |
| `universal_multifractal` (Levy) | 9.6 | 49.7 |
| `multifractal_cloud` (flagship 2) | 18.7 | 95.6 |
| `multifractional` (flagship 1) | 33.8 | 155.9 |

**The FFT is not the bottleneck it looks like.** At 256 square the two flagships run at 53 and 30
frames a second in plain numpy. That matters for the signal-side half of this, later.

## What `graphics:Noise` already has

`field()` is a textbook fBm octave sum: `harmonics` octaves, `rough` as persistence, `spread` as
lacunarity, normalized by the amplitude sum so roughness changes character and not brightness.
`kind` picks the BASIS — simplex, perlin, worley, random.

That already spans mfractal's `beta`. The two spellings are one relation: `H = -ln(rough) /
ln(spread)` and `beta = 2H + 2`, so the default `rough` 0.5 / `spread` 2 is `beta` 4, and `rough`
0.7 is `beta` 3 — mfractal's own default granularity. **No new node is needed to reach the
monofractal families.** What is missing is everything above the power law.

## Decisions

**`kind` is the basis; a second param is the COMBINATION rule.** Ridged, billow, warped and
multifractal are not other noises — they are other ways to fold the same octaves together. Putting
them in `kind` would cross two axes in one param and make `worley + ridged` unreachable. So:
`fractal` = `fbm | ridged | billow | warped | multifractal | multifractional`.

**Multifractality is six lines and it is the highest-value thing here.** The cascade is a product,
so in a shader it is a second octave sum used as an exponent:

```
value = tanh(field(v) * exp(sigma * volatility(v + offset) - 0.25 * sigma * sigma))
```

`volatility` is the same loop with the amplitude held CONSTANT across octaves — the `H -> 0` limit,
which is what a log-correlated field is. What the correction term should be is NOT obvious and the
measurements below settled it: the textbook `-sigma^2/2` holds the exponent's mean and kills the
picture, no correction at all saturates the `tanh`, and half of it is what works.

**It is an APPROXIMATION.** The shader's `omega` is a few octaves, not a true log-correlated field,
so its c2 does not equal `multifractal_cloud`'s. The claim that survived measurement is the useful
one: c2 is negative, monotone in `amount`, and of the same order as the library's.

**Nothing in the rock and cloud sets earns a node of its own.** `marble`, `agate`, `veined`,
`turbulent` and the rest are warped fBm plus banding plus a threshold — which is `Noise` into
`Displace`, `Lookup` and `Threshold`, nodes that already exist. Shipping fifteen rock nodes would
be exactly the per-need node explosion the library rule exists to prevent.

**The fluids are the one real new node, and the sims are not portable.** `eddies`, `vorticity` and
`plume` take twenty seconds because they iterate; a shader with state buffers would run them at
frame rate, but pressure projection is many Jacobi passes and the plan cannot express multi-pass
within a tick. Iterating ACROSS ticks — a few Jacobi sweeps per frame into a state buffer — is the
shape that fits, and it is the same open question `simulation-bundle.md` already records.

## Built: steps 1 to 3

`graphics:Noise` now carries `fractal` — `fbm | ridged | billow | warped | multifractal |
multifractional` — and one `amount` float whose meaning each mode names. `fbm` is the default and
is byte-for-byte what the node did before, so no patch changes.

**The cascade was measured, not asserted.** Frames were rendered at 256 square by the running
engine, pulled through `node snapshot --raw`, and put through mfractal's own
`wavelet_leaders_2d`:

| `amount` | mean | spread | kurtosis | c1 | c2 |
|---|---|---|---|---|---|
| 0.0 | 0.494 | 0.075 | -0.71 | 2.13 | **-0.013** |
| 0.5 | 0.494 | 0.079 | 0.20 | 1.62 | -0.099 |
| 1.0 | 0.495 | 0.085 | 2.86 | 1.38 | -0.185 |
| 1.5 | 0.496 | 0.089 | 6.08 | 1.27 | -0.250 |
| 2.0 | 0.496 | 0.089 | 9.39 | 1.21 | -0.321 |
| 2.5 | 0.497 | 0.084 | 13.44 | 1.18 | **-0.416** |

c2 is monotone and the brightness and contrast hold. For scale, `multifractal_cloud` at sigma 1.2
measures c2 -0.266 and this node at `amount` 1.5 measures -0.250, so `VOL_GAIN` of 2 is what makes
the knob mean roughly the sigma the literature means. At gain 1 it meant about half of it.

**Two wrong answers came first, and both are in the file as comments.** Holding the exponent's MEAN
at one — the textbook `exp(-sigma^2/2)` — drives its MEDIAN to `exp(-sigma^2/2)`, so nearly every
pixel is squashed towards grey and a few clipped spikes carry the picture: the spread fell from
0.077 to 0.007 by `amount` 2.5. Holding the median exactly and letting `tanh` take the tails fixed
the contrast but saturated, and c2 stopped at -0.17 and then came back towards zero. Half the mean
correction, with `tanh` on the tails, is what gives the table above.

## Order of the rest

4. **A `curl` output mode** — a divergence-free flow field as rgb, so `Displace` and `Feedback`
   advect with it. That is `curl_weave`, `rheoscopic` and `dye_diffusion` by composition rather
   than three nodes.
5. **`Fluid.wgsl`**, with state buffers and projection iterated across ticks. Only after the
   graphics engine's sub-stepping question is settled.

## Open

- **The estimator is out of band.** c2 needs pywavelets, which goofi does not depend on, so the
  committed test pins KURTOSIS — the half of the signature that moved with c2 across the whole
  sweep. Re-measuring c2 after a change to the cascade is a manual step until the analysis node
  below exists, and that is the argument for building it.
- **`amount` is one knob for four meanings.** The cascade's sigma, the warp's distance and the
  spread of local dimension are not the same quantity, and `fbm`, `ridged` and `billow` ignore it
  entirely. `Oscillator`'s `nonlinearity` is the precedent, and it is a compromise there too.
- **`ridged` and `billow` cannot combine with `multifractal`.** They are all one param, so the fold
  and the cascade are exclusive. Splitting them into two params would fix it and cost a third knob.
- **The signal half.** `multifractal_cloud` at 256 square is 53 fps in numpy, so the EXACT
  generators are viable as a Python node emitting `[H, W]` into `graphics:SignalIn` — approximate in the
  shader for motion, exact on the signal side for stimuli. Which one a patch wants is the user's
  call, and both should exist. `graphics:Noise` and `signal:Noise` already share a name.
- **c1/c2 as an analysis node.** `wavelet_leaders_2d` measures any field, not only these. It
  belongs beside Lempel-Ziv and DFA in the `complexity` bundle, and it is what closes the loop:
  biosignal drives the texture, the texture's multifractality is measured, the measurement drives
  the patch back.
- Whether `universal_multifractal`'s Levy-stable cascade has a shader form at all. Its variates are
  not something a hash gives you cheaply.
