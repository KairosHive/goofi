/* goofi
{ "doc": "the cascades a branching network throws, and how big they got\nOne row per unit and one column per tick, oldest at the left, with every column carrying the colour of the avalanche running through it — dim red for a small one, pale yellow for a network-wide storm. The activity profile runs along the bottom. Wire `state`, `activity` and `avalanche`.",
  "tags": ["image", "simulation"],
  "inputs": [
    {"name": "state", "kind": "ARRAY"},
    {"name": "activity", "kind": "ARRAY"},
    {"name": "avalanche", "kind": "ARRAY"} ],
  "state": ["history"],
  "params": [
    {"group": "cascade", "name": "speed", "kind": "int", "default": 1, "min": 1, "max": 16},
    {"group": "cascade", "name": "largest", "kind": "int", "default": 1000, "min": 2, "max": 1000000},
    {"group": "cascade", "name": "profile", "kind": "float", "default": 0.2, "min": 0.0, "max": 0.6},
    {"group": "cascade", "name": "gain", "kind": "float", "default": 1.0, "min": 0.1, "max": 100.0} ] }
*/

// A band may cover a hundred thousand units, so it walks at most this many of them.
const MAX_FOLD: i32 = 32;

const GROUND: vec3f = vec3f(0.05, 0.06, 0.09);

fn heat(v: f32) -> vec3f {
    let x = clamp(v, 0.0, 1.0);
    return clamp(vec3f(1.5 * x, 1.5 * x * x, 2.0 * x * x * x - 0.15), vec3f(0.0), vec3f(1.0));
}

fn wired(dims: vec2i, alpha: f32) -> bool {
    return dims.x > 1 || dims.y > 1 || alpha >= 0.5;
}

fn units(dims: vec2i) -> i32 {
    return select(dims.y, dims.x, dims.y == 1);
}

/// The share of the band this raster row covers that is awake, at the newest step.
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

/// The newest value of a per-step scalar, which is one row however the model emitted it.
fn scalar(tex: texture_2d<f32>) -> f32 {
    let dims = vec2i(textureDimensions(tex));
    if !wired(dims, textureLoad(tex, vec2i(0, 0), 0).a) {
        return 0.0;
    }
    return textureLoad(tex, vec2i(dims.x - 1, dims.y - 1), 0).r;
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
    // An avalanche spans decades, so it is carried as its own logarithm against the largest one
    // the picture is scaled for.
    let size = log(max(scalar(avalanche), 1.0)) / log(f32(max(p.largest, 2)));
    return vec4f(density(at.y, dims), scalar(activity), clamp(size, 0.0, 1.0), 1.0);
}

fn shade(uv: vec2f) -> vec4f {
    let at = vec2i(floor(uv * resolution));
    let split = 1.0 - p.profile;
    if p.profile > 0.0 && uv.y >= split {
        let column = textureLoad(history, vec2i(at.x, 0), 0);
        let reach = clamp(column.g * p.gain, 0.0, 1.0);
        let filled = (uv.y - split) / p.profile >= 1.0 - reach;
        return vec4f(select(GROUND, heat(0.3 + 0.7 * reach), filled), 1.0);
    }
    let cell = textureLoad(history, at, 0);
    return vec4f(mix(GROUND, heat(0.25 + 0.75 * cell.b), clamp(cell.r, 0.0, 1.0)), 1.0);
}
