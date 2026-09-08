/* goofi
{ "doc": "the activity carpet an echo state network is read from\nOne row per unit and one column per tick, oldest at the left, cool for a unit pushed negative and warm for one pushed positive. A pool with a long memory holds its stripes for many columns; one with a short memory does not. More units than rows fold into a band each.",
  "tags": ["image", "simulation"],
  "inputs": [{"name": "state", "kind": "ARRAY"}],
  "state": ["history"],
  "params": [
    {"group": "carpet", "name": "speed", "kind": "int", "default": 1, "min": 1, "max": 16},
    {"group": "carpet", "name": "gain", "kind": "float", "default": 1.0, "min": 0.05, "max": 8.0} ] }
*/

const MAX_FOLD: i32 = 32;

const GROUND: vec3f = vec3f(0.05, 0.06, 0.09);

fn diverge(v: f32) -> vec3f {
    let cool = vec3f(0.30, 0.60, 1.00);
    let warm = vec3f(1.00, 0.62, 0.25);
    return mix(GROUND, select(cool, warm, v > 0.0), clamp(abs(v), 0.0, 1.0));
}

fn wired(dims: vec2i, alpha: f32) -> bool {
    return dims.x > 1 || dims.y > 1 || alpha >= 0.5;
}

fn units(dims: vec2i) -> i32 {
    return select(dims.y, dims.x, dims.y == 1);
}

/// The mean of the band this raster row covers, at the newest step.
fn level(row: i32, dims: vec2i) -> f32 {
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
    let dims = vec2i(textureDimensions(state));
    if !wired(dims, textureLoad(state, vec2i(0, 0), 0).a) {
        return textureLoad(history, vec2i(last, at.y), 0);
    }
    return vec4f(level(at.y, dims), 0.0, 0.0, 1.0);
}

fn shade(uv: vec2f) -> vec4f {
    let cell = textureLoad(history, vec2i(floor(uv * resolution)), 0);
    return vec4f(diverge(cell.r * p.gain), 1.0);
}
