---
name: designing-emergent-shaders
description: Use when writing or reviewing a shader that should generate evolving visuals — a field, automaton, simulation or generative texture — or when such a shader looks flat, settles, loops, or reads as animated rather than alive.
---

# Designing Emergent Shaders

## Overview

**Evolution comes from state, not from the clock.** Anything written as `f(time)` is motion you
prescribed; anything written as `f(previous state)` is behaviour you discovered. A shader can be
beautiful and still be dead — the difference is measurable, and it is the whole subject here.

This is about what goes inside the rule that advances state. It is not about how to declare a
node, which you already know.

## Two kinds of shader, and choosing on purpose

Both are legitimate, and they are not the same craft:

| | reads `time` | holds state | what it is |
|---|---|---|---|
| **evolved** | never | yes | the picture is the state's own history |
| **choreographed** | throughout | maybe | a designed motion, richly rendered |

A choreographed shader can be spectacular. But its motion is a curve you chose, so it repeats on
your period and the pattern can never surprise you.

**This skill is about the first row.** There the rule is absolute: **if the shader holds state,
`time` in the body is a defect.** A clock-driven term runs on its own schedule regardless of what
the field is doing — it cannot be affected by the pattern, and the pattern cannot be affected by
it. The only clock reads in an evolved shader are the first-frame test that seeds it and,
optionally, the frame counter as a hash salt.

Decide which you are writing before you start. Wanting life and reaching for `time` is the single
most common way to end up with neither.

## The five moves

Each is independent; each buys something specific.

### 1. Close the loop

Ask: **does any quantity derived from the state feed back into that same state?** A buffer that
only *reads* another is a one-way pipeline — it decorates, it does not develop.

```
open   (baseline):  flux ← chemistry,  velocity = f(time)     -- chemistry never sees flux
closed (alive):     velocity = ∇memory → advects activity → memory follows activity
```

### 2. Compete scales — never sum them

Summed octaves give *texture*: every scale present, none in charge. To get structure at many sizes
that argues with itself, run **one rule at several radii and let a selector pick**:

```wgsl
var best = 10.0; var drive = 0.0;
for (var k = 0; k < scales; k++) {
    let r = i32(round(p.size * pow(p.spacing, f32(k))));             // geometric spacing
    let delta = ring(at, r) - ring(at, i32(f32(r) * p.inhibition));  // centre − surround
    if abs(delta) < best { best = abs(delta); drive = delta; }       // nearest balance rules
}
```

The selector is the non-obvious part. Taking the *strongest* scale lets one size dominate
everywhere; taking the one **nearest equilibrium** lets domains of different characteristic size
coexist and fight for territory. Centre-minus-surround at each radius is what gives a domain a
*characteristic size* at all rather than a blur.

### 3. Two timescales, coupled by fatigue

One fast channel and one slow one, with their *difference* fed back:

```wgsl
let fatigue = fast - slow;                   // departure from what is remembered
next_fast = fast + pace * (drive + p.persistence * fatigue + p.balance * (mid - fast));
next_slow = mix(next_fast, slow, p.memory);  // slow EMA
```

Positive `persistence` sustains departures (excitable); negative pulls back (relaxing). This is
habituation, and **it is what stops the field settling into a fixed point**. The restoring term
toward mid-level is a weak force so the field neither saturates nor dies.

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
let grad = vec2f(mem(at + dx) - mem(at - dx), mem(at + dy) - mem(at - dy));
let vel  = (vec2f(-grad.y, grad.x) * cos(a) + grad * sin(a)) * p.curl;  // a: curl ↔ divergence
let old  = bilinear(vec2f(at) - vel * p.flow);   // semi-Lagrangian, sub-texel
```

Rotating between curl (rotational) and gradient (convergent/divergent) with one angle param is a
large behavioural range for one knob. Sample **bilinearly** — nearest-neighbour quantizes the flow
into blocky jitter.

## Craft that carries the fidelity

- **Wrap toroidally**: `((at % size) + size) % size`. No edges, so structure can leave and return.
- **Be isotropic.** Sample rings on 8 neighbours (axials + diagonals), or a 9-point Laplacian. A
  4-neighbour stencil makes patterns line up with the pixel grid.
- **Seed on the first frame** from a hashed `seed` param — deterministic and reproducible. Add
  continuous injection as an opt-in defaulting to **off**.
- **Spend a spare channel on provenance.** Slowly follow *which* scale or rule won at each texel.
  It costs nothing and hands the colourist a semantic handle no post-process could recover.
- **Don't colour the simulation.** Emit near-scalar state; let a second node map it. The look then
  changes without touching the dynamics, and the palette works on other fields.

## Colouring, downstream

One scalar field yields several independent visual channels — this is where detail comes from:

| Source | Reads as |
|---|---|
| value → ramp between 2–3 anchored colours | the body |
| \|∇value\| | edges, filigree |
| \|fast − slow\| (departure from memory) | warmth where it is *changing* |
| provenance channel | glow keyed to structure size |
| `pow(0.5 + 0.5 * cos(value * count * TAU), sharpness)` | etched topographic contours |

Finish with a filmic curve then gamma — `1 - exp(-col * exposure)`, then `pow(·, 1/gamma)`. Values
exceed 1 in a float target, so tone-map rather than clamp.

## Cost model

Count texture loads per texel per tick, then multiply by pixels and by frame rate before you wire
it up. Scale count and ring taps are the multipliers, and they multiply: several scales × two
rings × eight taps, plus gradient and bilinear samples, is easily a hundred loads per texel —
hundreds of millions per tick at full HD. Raising the scale count is a proportional cost, so it is
a decision, not a default.

## Params

Name for the **percept**, not the maths: `pace`, `flow`, `memory`, `persistence`, `balance`,
`warmth` — not `dt`, `alpha`, `k1`. Document every one.

Expose the **dynamics' own constants**, not just surface knobs. A shader that exposes only speed,
size and hue can be made faster or bigger but cannot be made to *behave* differently. Exposing the
terms that set the regime — scale spacing, inhibition strength, saturation sensitivity,
persistence, restoring balance — lets one file reach genuinely different regimes. That is where
diversity comes from.

Group params by what the user is doing (structure / motion / tone), and put defaults in the
interesting region with both ends still valid pictures.

## Common mistakes

| Mistake | Why it disappoints |
|---|---|
| **3D noise sampled at `(x, y, time)`** | the most convincing fake there is — it boils and drifts and looks alive, but every frame was computable before the first one ran. Nothing accumulates. |
| `sin(time * k)` anywhere in a stateful body | prescribed loop; visibly cycles |
| Analytic flow field driven by the clock | the field cannot bend the flow; motion is imposed |
| Summed octaves for "multi-scale" | texture, not competing structure |
| One buffer feeding another, never back | decoration, not development |
| `clamp` on the drive term | flat derivative → frozen regions |
| Colour and simulation in one node | can't re-light without touching the dynamics |
| Nearest-neighbour advection | blocky jitter instead of flow |
| Only surface params exposed | one look; no regimes to explore |

## Verify it

**Check the structure, not the statistics.** Pixel measures — spatial variance, mean frame-to-frame
difference — catch a shader that is *dead* (flat, frozen, NaN) and nothing more. They cannot tell
development from churn: a clock-driven noise field scores higher on both "detail" and "motion"
than a genuinely evolving one, because drift and boil move a great many pixels without anything
happening. That is measured, not asserted.

Two checks are decisive and take seconds: **count the reads of `time` in the body — it must be
zero** where the shader holds state, and **look for summed octaves**, which are texture rather
than competing structure.

Then read the rule that advances state and ask: **is any quantity derived from this state used to
change this state?** If not, the loop is open and it will not develop.

**Budget a tuning pass regardless.** Saturation sensitivity, feedback strength and the restoring
balance decide whether the field lives in its interesting band or pins to one end, and no amount
of correct architecture prevents needing to find that band by hand.

Then judge the picture, which no number does for you:

- Does it still change after a minute? (state-driven, not settled)
- Does it look different after five? (not a short loop)
- Is there structure at more than one size *at once* — and do those sizes interact?
- Do two `seed` values give genuinely different pictures, or the same picture shifted?
- Does moving a structural param change the *behaviour*, not just the scale?
