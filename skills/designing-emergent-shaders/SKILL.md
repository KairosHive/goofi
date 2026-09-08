---
name: designing-emergent-shaders
description: Use when writing or reviewing a goofi `.wgsl` graphics node that should generate evolving visuals — a field, automaton, simulation or generative texture — or when such a shader looks flat, settles, loops, or reads as animated rather than alive.
---

# Designing Emergent Shaders

## Overview

**Evolution comes from state, not from the clock.** Anything written as `f(time)` is motion you
prescribed; anything written as `f(previous state)` is behaviour you discovered. A shader can be
beautiful and still be dead — the difference is measurable, and it is the whole subject here.

Mechanics of the file format are in `AGENTS.md` and `reference.md` beside this skill. This is
about what to put inside `next_<buffer>`.

The moves below were distilled from `ThoughtField`, a private node that is not in this repo —
every part of it you need is quoted here, so nothing is missing, and there is no file to go
looking for.

## Two kinds of shader, and choosing on purpose

Both are legitimate. They are not the same craft, and the corpus splits cleanly on one measure —
how often the body reads `time`:

| | `time` | state | what it is |
|---|---|---|---|
| Lenia, Reaction, NeuralCA, ThoughtField | **0** | 1 | **evolved** — the picture is the state's history |
| FluidPigments | 3 | 1 | **choreographed** — a designed motion, richly rendered |
| RibbonSpace | 34 | 1 | choreographed |
| FractalJewelry | 3 | **0** | choreographed; no state at all, so it cannot evolve |

A choreographed shader can be spectacular — the `inception` bundle is exactly that, and it is
doing what it means to do. But its motion is a curve you chose, so it repeats on your period and
the pattern can never surprise you.

**This skill is about the first column.** There, the rule is absolute: **if your node declares
state, `time` in the body is a defect.** A clock-driven term runs on its own schedule regardless
of what the field is doing — it cannot be affected by the pattern, and the pattern cannot be
affected by it. The only clock reads in an evolved shader are `frame == 0u` to seed and,
optionally, `frame` as a hash salt.

Decide which you are writing before you start. Wanting life and reaching for `time` is the single
most common way to end up with neither.

## The five moves

Each is independent; each buys something specific.

### 1. Close the loop

Ask: **does any quantity derived from the state feed back into that same state?** A buffer that
only *reads* another is a one-way pipeline — it decorates, it does not develop.

```
open   (baseline):  flux ← chem,  velocity = f(time)     -- chem never sees flux
closed (alive):     velocity = ∇memory → advects activity → memory follows activity
```

### 2. Compete scales — never sum them

fBm sums octaves and gets *texture*: every scale present, none in charge. To get structure at
many sizes that argues with itself, run **one rule at several radii and let a selector pick**:

```wgsl
var best = 10.0; var drive = 0.0;
for (var k = 0; k < scales; k++) {
    let r = i32(round(p.size * pow(p.spacing, f32(k))));       // geometric spacing
    let delta = ring(at, r) - ring(at, i32(f32(r)*p.inhibition));   // centre − surround
    if abs(delta) < best { best = abs(delta); drive = delta; }  // the scale nearest balance rules
}
```

The selector is the non-obvious part. Taking the *strongest* scale lets one size dominate
everywhere; taking the one **nearest equilibrium** lets domains of different characteristic size
coexist and fight for territory. Centre-minus-surround at each radius is what gives a domain a
*characteristic size* at all rather than a blur.

### 3. Two timescales, coupled by fatigue

One fast channel and one slow one, with their *difference* fed back:

```wgsl
let fatigue = old.r - old.g;                 // departure from what is remembered
next = old.r + pace * (... + p.persistence * fatigue + p.balance * (0.5 - old.r));
memory = mix(next, old.g, 0.975);            // slow EMA
```

Positive `persistence` sustains departures (excitable); negative pulls back (relaxing). This is
habituation, and **it is what stops the field settling into a fixed point**. `balance` is a weak
restoring force toward mid-level so it neither saturates nor dies.

### 4. Saturate, never clamp

```wgsl
let drive = raw / (p.sensitivity + abs(raw));   // amplifies weak, asymptotes for strong
```

A hard `clamp` on the drive creates flat regions where the derivative is zero — the field freezes
there. A ratio is bounded everywhere and still has gain near zero, so small differences grow.
Clamp the final *value*; never the force.

### 5. Advect through your own gradient

Structure that only pulses in place reads as boiling. Move it, using a velocity taken from the
field's own slow channel:

```wgsl
let grad = vec2f(mem(at+dx) - mem(at-dx), mem(at+dy) - mem(at-dy));
let vel  = (vec2f(-grad.y, grad.x) * cos(a) + grad * sin(a)) * p.curl;  // a: curl ↔ divergence
let old  = bilinear(vec2f(at) - vel * p.flow);   // semi-Lagrangian, sub-texel
```

Rotating between curl (rotational) and gradient (convergent/divergent) with one angle param is a
large behavioural range for one knob. Sample **bilinearly** — nearest-neighbour quantizes the flow
into blocky jitter.

## Craft that carries the fidelity

- **Wrap toroidally**: `((at % size) + size) % size`. No edges, so structure can leave and return.
- **Be isotropic.** Sample rings on 8 neighbours (axials + diagonals), or a 9-point Laplacian.
  A 4-neighbour stencil makes patterns line up with the pixel grid.
- **Seed at `frame == 0u`** from a hashed `seed` param — deterministic and reproducible. Add
  continuous `novelty` as an opt-in defaulting to **0**.
- **Spend a spare channel on provenance.** ThoughtField slowly follows *which scale won*
  (`mix(old.b, scale, 0.035)`). It costs nothing and hands the colourist a semantic handle no
  post-process could recover.
- **Don't colour the simulation.** Emit near-scalar state; let a second node map it. The look then
  changes without touching the dynamics, and the palette works on other fields.

## Colouring, in the second node

One scalar field yields several independent visual channels — this is where detail comes from:

| Source | Reads as |
|---|---|
| value → ramp between 2-3 anchored colours | the body |
| \|∇value\| | edges, filigree |
| \|fast − slow\| (departure from memory) | warmth where it is *changing* |
| provenance channel | glow keyed to structure size |
| `pow(0.5+0.5*cos(value*count*TAU), sharpness)` | etched topographic contours |

Finish with a filmic curve then gamma — `1 - exp(-col*exposure)`, then `pow(·, 1/gamma)`. Every
texture is `Rgba16Float` and values exceed 1, so tone-map rather than clamp.

## Cost model

Count texture loads per texel per tick, then multiply.

> ThoughtField: 6 scales × 2 rings × 8 taps + 4 gradient + 4 bilinear = **104 loads/texel**.
> At 1920×1080 that is 216 M loads/tick — **6.5 G loads/s at 30 fps**, which holds on an RTX 4090.

Know this number *before* you wire it up. Scale count and ring taps are the multipliers; a
`scales` param that reaches 8 costs a third more than 6.

## Params

Name for the **percept**, not the maths: `pace`, `flow`, `memory`, `persistence`, `balance`,
`novelty`, `warmth` — not `dt`, `alpha`, `k1`. Give every one a `doc`.

Expose the **dynamics' own constants**, not just surface knobs. The baseline exposed speed, size,
flow, hue — you could make it faster or bigger but not make it *behave* differently. ThoughtField
exposes `spacing`, `inhibition`, `sensitivity`, `persistence`, `balance`, so the same file reaches
genuinely different regimes. That is where diversity comes from.

Group by what the user is doing (`structure` / `motion` / `tone`), and put defaults in the
interesting region with both ends still valid pictures.

## Common mistakes

| Mistake | Why it disappoints |
|---|---|
| **3D noise sampled at `(x, y, time)`** | the most convincing fake there is — it boils and drifts and looks alive, but every frame was computable before the first one ran. Nothing accumulates. Both agents this skill was tested against started here; the one that stopped when it felt finished shipped it. |
| `sin(time * k)` anywhere in a stateful body | prescribed loop; visibly cycles |
| Analytic flow field driven by the clock | the field cannot bend the flow; motion is imposed |
| fBm octaves summed for "multi-scale" | texture, not competing structure |
| One buffer feeding another, never back | decoration, not development |
| `clamp` on the drive term | flat derivative → frozen regions |
| Colour and simulation in one node | can't re-light without touching the dynamics |
| Nearest-neighbour advection | blocky jitter instead of flow |
| Only surface params exposed | one look; no regimes to explore |

## Verify it

Iterate live — a running node hot-reloads:

```bash
cp Mine.wgsl "$GOOFI_HOME/.goofi/custom/"
goofi library refresh                     # a broken file greys, carrying naga's error
goofi library get --type graphics:Mine    # at YOUR file's own line number
```

**Check the structure, not the statistics.** Pixel measures — spatial variance, mean frame-to-frame
difference — catch a shader that is *dead* (flat, frozen, NaN) and nothing more. They cannot tell
development from churn: a clock-driven fBm field scored higher on both "detail" and "motion" than
the reference field, because drift and boil move a great many pixels without anything happening.
Measured, on the shaders written for this skill's own test.

So grep yourself first — these three are decisive and take seconds:

```bash
grep -c '\btime\b' Mine.wgsl        # must be 0 in a stateful body
grep -n 'fbm\|octave' Mine.wgsl     # summed octaves → texture, not competing structure
```

…and read `next_<buffer>` asking: **is any quantity derived from this buffer used to change this
buffer?** If not, the loop is open and it will not develop.

**What this skill is worth, measured honestly.** Three shaders were written for the same brief —
"genuine emergent complexity" — two without this skill and one with it. All three opened the same
way: a clock-driven field sampling noise along *t*. What separated them was iteration, not talent.

| | `time` | `fbm` | params / groups |
|---|---|---|---|
| stopped when it felt finished | 8 | 5 | 8 / 2 |
| kept going, no skill, ~40 min more | 0 | 0 | 23 / 5 |
| with this skill | 0 | 0 | 27 / 5 |

So the honest claim is **not** "agents fail without this". An agent that iterates long enough
rediscovers every move here unaided — which is the best evidence they are real and not one
person's taste. What the skill buys is having them at the start instead of after forty minutes,
and a verification habit: the agent that had it checked its own field for periodicity, found two
defects — a collapsed channel and a washed-out render — and fixed both before calling it done.

**Budget the tuning pass regardless.** `sensitivity`, `reinforcement` and `balance` decide whether
the field lives in its interesting band or pins to one end. Both good runs above needed a
correction there that no amount of correct architecture prevented.

Then judge the picture, which no number does for you:

- Does it still change after a minute? (state-driven, not settled)
- Does it look different after five? (not a short loop)
- Is there structure at more than one size *at once* — and do those sizes interact?
- Do two `seed` values give genuinely different pictures, or the same picture shifted?
- Does moving a `structure` param change the *behaviour*, not just the scale?
