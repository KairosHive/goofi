/* goofi
{ "doc": "a cellular automaton whose rule is a small network\nTwelve channels per cell. Each one sees itself, the slope around it and the curvature, and a learned two-layer rule says what to become. Train one against a folder of images with `training/style_ca.py` and wire the `[1, N, 4]` weights it writes into `weights`; the hidden width is read from the array's own length. With nothing wired it draws its own rule from `seed`, which is a texture nobody trained.",
  "tags": ["image", "generator", "simulation", "ml"],
  "state": ["cells0", "cells1", "cells2"],
  "inputs": [{"name": "weights", "kind": "ARRAY"}],
  "params": [
    {"group": "neuralca", "name": "rate", "kind": "float", "default": 1.0, "min": 0.0, "max": 2.0,
     "doc": "How much of the step a cell takes. 1.0 is what a model was trained at."},
    {"group": "neuralca", "name": "fire", "kind": "float", "default": 0.5, "min": 0.05, "max": 1.0,
     "doc": "The chance a cell updates on a tick. 0.5 is what a model was trained at."},
    {"group": "neuralca", "name": "reach", "kind": "int", "default": 1, "min": 1, "max": 12,
     "doc": "How many texels away a cell looks. Above 1 coarsens a trained texture."},
    {"group": "neuralca", "name": "alive", "kind": "bool", "default": false,
     "doc": "Growth: a cell with no living neighbour holds at zero, so a pattern spreads from its seed. Off for a texture."},
    {"group": "neuralca", "name": "start", "kind": "str", "default": "noise", "options": ["noise", "dot", "zero"]},
    {"group": "neuralca", "name": "density", "kind": "float", "default": 0.3, "min": 0.0, "max": 1.0},
    {"group": "neuralca", "name": "scale", "kind": "float", "default": 4.0, "min": 1.0, "max": 32.0},
    {"group": "neuralca", "name": "seed", "kind": "float", "default": 0.0, "min": 0.0, "max": 1000.0} ] }
*/
const CH: i32 = 12;
const GROUPS: i32 = CH / 4;
const ROW: i32 = CH;
// One hidden unit is one contiguous block: its input row, its bias, then its column of the output
// layer. So the array's length names the hidden width, and a unit is read in sequence.
const BLOCK: i32 = ROW + 1 + GROUPS;
const DRAWN: i32 = 96;
const BOUND: f32 = 8.0;

fn hash1(i: f32) -> f32 {
    var h = u32(i32(i) + 65536) * 1597334673u;
    h = (h ^ (h >> 16u)) * 2246822519u;
    h = (h ^ (h >> 13u)) * 3266489917u;
    return f32(h ^ (h >> 16u)) * (1.0 / 4294967296.0);
}

fn hash3(v: vec3f) -> f32 {
    let q = vec3u(vec3i(v) + 65536);
    var h = q.x * 1597334673u ^ q.y * 3812015801u ^ q.z * 2654435761u;
    h = (h ^ (h >> 16u)) * 2246822519u;
    h = (h ^ (h >> 13u)) * 3266489917u;
    return f32(h ^ (h >> 16u)) * (1.0 / 4294967296.0);
}

/// The hidden units the wired array holds, or 0 where nothing usable is wired.
fn units() -> i32 {
    let n = i32(textureDimensions(weights).x);
    if n >= BLOCK + GROUPS && (n - GROUPS) % BLOCK == 0 {
        return (n - GROUPS) / BLOCK;
    }
    return 0;
}

fn hidden() -> i32 {
    let n = units();
    return select(DRAWN, n, n > 0);
}

// The rule the node draws for itself, scaled by each layer's fan-in so the first step neither
// saturates nor stands still.
fn drawn(i: i32) -> vec4f {
    if i >= BLOCK * DRAWN {
        return vec4f(0.0);
    }
    var v = vec4f(0.0);
    for (var k = 0; k < 4; k++) {
        v[k] = hash1(f32(i * 4 + k) + p.seed) * 2.0 - 1.0;
    }
    if i % BLOCK < ROW {
        return v * 0.125;
    }
    return v * 0.1;
}

/// Weight texel `i`: four numbers, in the order the trainer packed them.
fn weight(i: i32) -> vec4f {
    if units() > 0 {
        return textureLoad(weights, vec2i(i, 0), 0);
    }
    return drawn(i);
}

fn cells(g: i32, at: vec2i) -> vec4f {
    let size = vec2i(resolution);
    let q = ((at % size) + size) % size;
    switch g {
        case 0: { return textureLoad(cells0, q, 0); }
        case 1: { return textureLoad(cells1, q, 0); }
        default: { return textureLoad(cells2, q, 0); }
    }
}

// Channel 3 is aliveness. A cell with no living neighbour holds at zero, which is what makes a
// pattern GROW from its seed instead of the bias filling the whole grid with one colour.
fn awake(at: vec2i) -> bool {
    if p.alive == 0u {
        return true;
    }
    var most = 0.0;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            most = max(most, cells(0, at + vec2i(dx, dy) * p.reach).w);
        }
    }
    return most > 0.1;
}

// The engine calls one writer per state buffer, so the rule runs once and the three share it.
var<private> ruled: bool = false;
var<private> delta: array<vec4f, 3>;

fn rule(at: vec2i) {
    if ruled {
        return;
    }
    ruled = true;
    var seen: array<vec4f, 12>;
    for (var g = 0; g < GROUPS; g++) {
        let nw = cells(g, at + vec2i(-1, -1) * p.reach);
        let nn = cells(g, at + vec2i(0, -1) * p.reach);
        let ne = cells(g, at + vec2i(1, -1) * p.reach);
        let ww = cells(g, at + vec2i(-1, 0) * p.reach);
        let here = cells(g, at);
        let ee = cells(g, at + vec2i(1, 0) * p.reach);
        let sw = cells(g, at + vec2i(-1, 1) * p.reach);
        let ss = cells(g, at + vec2i(0, 1) * p.reach);
        let se = cells(g, at + vec2i(1, 1) * p.reach);
        seen[g] = here;
        seen[GROUPS + g] = (ne + 2.0 * ee + se - nw - 2.0 * ww - sw) / 8.0;
        seen[2 * GROUPS + g] = (sw + 2.0 * ss + se - nw - 2.0 * nn - ne) / 8.0;
        seen[3 * GROUPS + g] =
            (nw + 2.0 * nn + ne + 2.0 * ww - 12.0 * here + 2.0 * ee + sw + 2.0 * ss + se) / 16.0;
    }
    let h = hidden();
    for (var o = 0; o < GROUPS; o++) {
        delta[o] = weight(BLOCK * h + o);
    }
    for (var u = 0; u < h; u++) {
        let block = u * BLOCK;
        var sum = weight(block + ROW).x;
        for (var k = 0; k < ROW; k++) {
            sum = sum + dot(weight(block + k), seen[k]);
        }
        if sum > 0.0 {
            for (var o = 0; o < GROUPS; o++) {
                delta[o] = delta[o] + weight(block + ROW + 1 + o) * sum;
            }
        }
    }
    for (var o = 0; o < GROUPS; o++) {
        delta[o] = tanh(delta[o]);
    }
}

fn seeded(at: vec2i, g: i32) -> vec4f {
    if p.start == 2u {
        return vec4f(0.0);
    }
    if p.start == 1u {
        let on = step(length(vec2f(at) - resolution * 0.5), 2.0);
        return select(vec4f(0.0), vec4f(on), g == 0);
    }
    // Blocks rather than pixels: a single live pixel has nothing around it to read.
    let block = floor(vec2f(at) / max(p.scale, 1.0));
    let on = step(1.0 - p.density, hash3(vec3f(block, p.seed)));
    var v = vec4f(0.0);
    for (var k = 0; k < 4; k++) {
        v[k] = (hash3(vec3f(block, p.seed + f32(g * 4 + k) + 1.0)) - 0.5) * on;
    }
    if p.alive != 0u && g == 0 {
        v.w = on;
    }
    return v;
}

fn advanced(uv: vec2f, g: i32) -> vec4f {
    let at = vec2i(floor(uv * resolution));
    if frame == 0u {
        return seeded(at, g);
    }
    if !awake(at) {
        return vec4f(0.0);
    }
    rule(at);
    // Cells step at random rather than in lockstep, which is what keeps the grid from moving as
    // one block.
    let fires = step(1.0 - p.fire, hash3(vec3f(vec2f(at), f32(frame) + p.seed)));
    return clamp(cells(g, at) + p.rate * fires * delta[g], vec4f(-BOUND), vec4f(BOUND));
}

fn next_cells0(uv: vec2f) -> vec4f { return advanced(uv, 0); }
fn next_cells1(uv: vec2f) -> vec4f { return advanced(uv, 1); }
fn next_cells2(uv: vec2f) -> vec4f { return advanced(uv, 2); }

fn shade(uv: vec2f) -> vec4f {
    let s = cells(0, vec2i(floor(uv * resolution)));
    return vec4f(clamp(s.rgb * 0.5 + 0.5, vec3f(0.0), vec3f(1.0)), 1.0);
}
