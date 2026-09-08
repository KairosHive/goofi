# The goofi `.wgsl` graphics node contract (mechanical facts only)

The authority is the code: `backend/graphics/goofi-graphics/src/shader.rs` generates the prelude
and validates the file, and `AGENTS.md` states the decisions behind it. This page is a summary to
write against, and it goes stale — check it against `shader.rs` if something here does not hold.

A graphics node is ONE `.wgsl` file. Its type name is the file stem. The first comment block is
its manifest, and the engine appends a prelude AFTER the file's text.

## Header

```
/* goofi
{
  "doc": "First line is the nutshell, 80 chars or less.\nMore lines may follow.",
  "tags": ["generator"],
  "state": ["field"],
  "inputs": [{"name": "input", "kind": "TEXTURE"}],
  "params": [
    {"group": "g", "name": "speed", "kind": "float", "default": 1.0, "min": 0.0, "max": 4.0,
     "doc": "What this does, in a sentence."}
  ]
}
*/
```

- `state` declares named buffers. `inputs` is optional; `outputs` must NOT be listed — every
  graphics node has the one output `out`.
- **`tags` is a CLOSED vocabulary** and an unknown one greys the node with no WGSL error to find,
  because the header is refused before naga ever runs. It is exactly:
  `input, output, generator, transform, analysis, control, image, text, midi, eeg, cardio, motion,
  music, ml, connectivity, simulation`.
  An evolved field is `["generator", "simulation"]`. Do not invent one — `field`, `noise`, `fluid`
  are all refusals.
- Param `kind` is `float`, `int`, or `bool`.
- A node with NO texture input is a producer and takes the patch's default size.

## What the prelude gives you

```wgsl
@group(0) @binding(0) var<uniform> time: f32;        // patch seconds
@group(0) @binding(1) var<uniform> resolution: vec2f;
@group(0) @binding(2) var samp: sampler;
struct Params { speed: f32, /* one scalar per declared param, in order */ }
@group(0) @binding(3) var<uniform> p: Params;
@group(0) @binding(4) var<uniform> frame: u32;       // renders since the buffers were made
// one texture_2d<f32> per input, by its own name
// one texture_2d<f32> per state buffer, by its own name
```

## What you write

- `fn shade(uv: vec2f) -> vec4f` — REQUIRED. The output texel.
- `fn next_<buffer>(uv: vec2f) -> vec4f` — one per declared state buffer.

A state buffer NAME reads what the LAST tick left (via `textureLoad`); `next_<name>` writes what
this tick leaves. They are never the same texture. All targets of one pass share one size, so a
buffer is the node's own size.

## Rules

- `uv` is (0,0) at the TOP-left — WGSL's own texture space. Every row order in the engine matches.
- Every texture is `Rgba16Float`. Values are NOT clamped to [0,1]; HDR survives a chain.
- Reserved names you may not take: `time`, `frame`, `resolution`, `samp`, `p`, `Params`, `shade`.
- A pass cannot read the texture it writes. `textureLoad(tex, vec2i, 0)` for exact texels,
  `textureSample(tex, samp, uv)` for filtered.
- No push constants. Storage/uniform pointers may not be function parameters.
