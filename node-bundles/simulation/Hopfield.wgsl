/* goofi
{ "doc": "an associative memory's state as the picture it is\nThe state vector folded back into a grid of cells, cool for -1 and warm for +1, with the overlap against each stored memory as a strip of bars underneath. Wire `state`, and `overlap` for the strip. `columns` is 0 for as square a grid as the width allows.",
  "tags": ["image", "simulation"],
  "inputs": [
    {"name": "state", "kind": "ARRAY"},
    {"name": "overlap", "kind": "ARRAY"} ],
  "params": [
    {"group": "pattern", "name": "columns", "kind": "int", "default": 0, "min": 0, "max": 256},
    {"group": "pattern", "name": "gap", "kind": "float", "default": 0.10, "min": 0.0, "max": 0.6},
    {"group": "pattern", "name": "bars", "kind": "float", "default": 0.16, "min": 0.0, "max": 0.6},
    {"group": "pattern", "name": "gain", "kind": "float", "default": 1.0, "min": 0.05, "max": 8.0} ] }
*/

const GROUND: vec3f = vec3f(0.05, 0.06, 0.09);

/// Cool below zero, warm above it, and the ground at nothing.
fn diverge(v: f32) -> vec3f {
    let cool = vec3f(0.30, 0.60, 1.00);
    let warm = vec3f(1.00, 0.62, 0.25);
    return mix(GROUND, select(cool, warm, v > 0.0), clamp(abs(v), 0.0, 1.0));
}

fn wired(dims: vec2i, alpha: f32) -> bool {
    return dims.x > 1 || dims.y > 1 || alpha >= 0.5;
}

/// How many units the frame holds: a `value` frame is one row of them, a `block` frame one ROW
/// each, and the newest step is its last column.
fn units(dims: vec2i) -> i32 {
    return select(dims.y, dims.x, dims.y == 1);
}

fn newest(tex: texture_2d<f32>, i: i32, dims: vec2i) -> f32 {
    return textureLoad(tex, select(vec2i(dims.x - 1, i), vec2i(i, 0), dims.y == 1), 0).r;
}

/// Whether a point sits inside a cell rather than in the gap around it.
fn within(inside: vec2f) -> bool {
    let edge = p.gap * 0.5;
    return inside.x > edge && inside.x < 1.0 - edge && inside.y > edge && inside.y < 1.0 - edge;
}

fn grid(uv: vec2f) -> vec3f {
    let dims = vec2i(textureDimensions(state));
    if !wired(dims, textureLoad(state, vec2i(0, 0), 0).a) {
        return GROUND;
    }
    let width = units(dims);
    let cols = select(i32(ceil(sqrt(f32(width)))), p.columns, p.columns > 0);
    let rows = (width + cols - 1) / cols;
    let over = uv * vec2f(f32(cols), f32(rows));
    let cell = vec2i(floor(over));
    let index = cell.y * cols + cell.x;
    if index >= width || !within(fract(over)) {
        return GROUND;
    }
    return diverge(newest(state, index, dims) * p.gain);
}

fn strip(uv: vec2f) -> vec3f {
    let dims = vec2i(textureDimensions(overlap));
    if !wired(dims, textureLoad(overlap, vec2i(0, 0), 0).a) {
        return GROUND;
    }
    let count = units(dims);
    let over = uv.x * f32(count);
    let index = i32(floor(over));
    // The baseline the bars grow out of: up for a memory the state matches, down for its
    // negative — which a Hopfield network settles into just as readily.
    if abs(uv.y - 0.5) < 0.006 {
        return vec3f(0.22, 0.24, 0.29);
    }
    if index >= count || fract(over) < p.gap * 0.5 || fract(over) > 1.0 - p.gap * 0.5 {
        return GROUND;
    }
    let v = newest(overlap, index, dims) * p.gain;
    let reach = clamp(abs(v), 0.0, 1.0) * 0.5;
    let up = v > 0.0 && uv.y <= 0.5 && uv.y >= 0.5 - reach;
    let down = v <= 0.0 && uv.y >= 0.5 && uv.y <= 0.5 + reach;
    return select(GROUND, diverge(v), up || down);
}

fn shade(uv: vec2f) -> vec4f {
    let split = 1.0 - p.bars;
    if p.bars <= 0.0 || uv.y < split {
        return vec4f(grid(vec2f(uv.x, uv.y / max(split, 1e-6))), 1.0);
    }
    return vec4f(strip(vec2f(uv.x, (uv.y - split) / max(p.bars, 1e-6))), 1.0);
}
