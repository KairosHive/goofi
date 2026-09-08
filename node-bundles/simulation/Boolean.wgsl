/* goofi
{ "doc": "the space-time diagram a random Boolean network is judged by\nOne row per unit and one column per tick, oldest at the left. A unit that is on but never changes is grey — the frozen core — and one that keeps flipping burns amber, which is the whole difference between the ordered side of the Kauffman transition and the chaotic one. More units than rows fold into a band each.",
  "tags": ["image", "simulation"],
  "inputs": [{"name": "state", "kind": "ARRAY"}],
  "state": ["history"],
  "params": [
    {"group": "diagram", "name": "speed", "kind": "int", "default": 1, "min": 1, "max": 16},
    {"group": "diagram", "name": "changes", "kind": "bool", "default": true} ] }
*/

// A band may cover four thousand units, so it walks at most this many of them.
const MAX_FOLD: i32 = 32;

const GROUND: vec3f = vec3f(0.05, 0.06, 0.09);

fn wired(dims: vec2i, alpha: f32) -> bool {
    return dims.x > 1 || dims.y > 1 || alpha >= 0.5;
}

/// How many units the frame holds: a `value` frame is one row of them, a `block` frame one ROW
/// each, and the newest step is its last column.
fn units(dims: vec2i) -> i32 {
    return select(dims.y, dims.x, dims.y == 1);
}

/// The share of the band this raster row covers that is on, at the newest step.
fn density(row: i32, dims: vec2i) -> f32 {
    let n = units(dims);
    let rows = i32(resolution.y);
    let lo = row * n / rows;
    let hi = max(lo + 1, (row + 1) * n / rows);
    let stride = max(1, (hi - lo + MAX_FOLD - 1) / MAX_FOLD);
    var total = 0.0;
    var walked = 0.0;
    for (var i = lo; i < min(hi, n); i = i + stride) {
        total = total + textureLoad(state, select(vec2i(dims.x - 1, i), vec2i(i, 0), dims.y == 1), 0).r;
        walked = walked + 1.0;
    }
    return total / max(walked, 1.0);
}

fn next_history(uv: vec2f) -> vec4f {
    let at = vec2i(floor(uv * resolution));
    if frame == 0u {
        return vec4f(0.0, 0.0, 0.0, 1.0);
    }
    let last = i32(resolution.x) - 1;
    let step = clamp(p.speed, 1, i32(resolution.x));
    if at.x <= last - step {
        return textureLoad(history, at + vec2i(step, 0), 0);
    }
    let held = textureLoad(history, vec2i(last, at.y), 0);
    let dims = vec2i(textureDimensions(state));
    if !wired(dims, textureLoad(state, vec2i(0, 0), 0).a) {
        return held;
    }
    let now = density(at.y, dims);
    return vec4f(now, select(0.0, 1.0, abs(now - held.r) > 0.001), 0.0, 1.0);
}

fn shade(uv: vec2f) -> vec4f {
    let cell = textureLoad(history, vec2i(floor(uv * resolution)), 0);
    let base = mix(GROUND, vec3f(0.58, 0.62, 0.70), clamp(cell.r, 0.0, 1.0));
    let churning = p.changes != 0u && cell.g > 0.5;
    return vec4f(select(base, mix(base, vec3f(1.00, 0.70, 0.32), 0.85), churning), 1.0);
}
