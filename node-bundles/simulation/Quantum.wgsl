/* goofi
{ "doc": "the two pictures a few qubits are read from\nA Bloch sphere per qubit — x across, z up, and y into the screen, which warms and widens the head as the vector comes towards you — over the statevector as a bar per basis state. Wire `bloch` and `probabilities`.",
  "tags": ["image", "simulation"],
  "inputs": [
    {"name": "bloch", "kind": "ARRAY"},
    {"name": "probabilities", "kind": "ARRAY"} ],
  "params": [
    {"group": "quantum", "name": "view", "kind": "str", "default": "both",
     "options": ["both", "bloch", "probabilities"]},
    {"group": "quantum", "name": "bars", "kind": "float", "default": 0.32, "min": 0.05, "max": 0.9},
    {"group": "quantum", "name": "sphere", "kind": "float", "default": 0.8, "min": 0.1, "max": 1.0},
    {"group": "quantum", "name": "guides", "kind": "bool", "default": true} ] }
*/

// Twelve qubits is four thousand basis states over a thousand columns, so a column folds at most
// this many of them.
const MAX_FOLD: i32 = 32;

fn heat(v: f32) -> vec3f {
    let x = clamp(v, 0.0, 1.0);
    return clamp(vec3f(1.5 * x, 1.5 * x * x, 2.0 * x * x * x - 0.15), vec3f(0.0), vec3f(1.0));
}

fn ink(under: vec4f, colour: vec3f, cover: f32) -> vec4f {
    return vec4f(mix(under.rgb, colour, cover), max(under.a, cover));
}

fn wired(dims: vec2i, alpha: f32) -> bool {
    return dims.x > 1 || dims.y > 1 || alpha >= 0.5;
}

fn to_segment(here: vec2f, a: vec2f, b: vec2f) -> f32 {
    let run = b - a;
    let t = clamp(dot(here - a, run) / max(dot(run, run), 1e-6), 0.0, 1.0);
    let off = here - (a + run * t);
    return dot(off, off);
}

/// One qubit's vector, as three columns of one row.
fn vector(q: i32) -> vec3f {
    return vec3f(
        textureLoad(bloch, vec2i(0, q), 0).r,
        textureLoad(bloch, vec2i(1, q), 0).r,
        textureLoad(bloch, vec2i(2, q), 0).r,
    );
}

fn spheres(rel: vec2f, size: vec2f) -> vec4f {
    let dims = vec2i(textureDimensions(bloch));
    if !wired(dims, textureLoad(bloch, vec2i(0, 0), 0).a) {
        return vec4f(0.0);
    }
    let qubits = dims.y;
    let cols = i32(ceil(sqrt(f32(qubits))));
    let rows = (qubits + cols - 1) / cols;
    let over = rel * vec2f(f32(cols), f32(rows));
    let cell = vec2i(floor(over));
    let index = cell.y * cols + cell.x;
    if index >= qubits {
        return vec4f(0.0);
    }
    let box = size / vec2f(f32(cols), f32(rows));
    let here = (fract(over) - vec2f(0.5)) * box;
    let radius = p.sphere * 0.5 * min(box.x, box.y);
    let line = max(1.0, radius * 0.035);

    var out = vec4f(0.0);
    if p.guides != 0u {
        let rim = clamp(line + 0.5 - abs(length(here) - radius), 0.0, 1.0);
        let equator = select(0.0, clamp(line * 0.7 + 0.5 - abs(here.y), 0.0, 1.0), abs(here.x) < radius);
        out = ink(out, vec3f(0.28, 0.31, 0.38), max(rim, equator));
    }
    let v = vector(index);
    let tip = vec2f(v.x, -v.z) * radius;
    let towards = clamp(v.y * 0.5 + 0.5, 0.0, 1.0);
    let path = clamp(line * 1.6 + 0.5 - sqrt(to_segment(here, vec2f(0.0), tip)), 0.0, 1.0);
    let head = clamp(radius * 0.09 * mix(0.6, 1.4, towards) + 0.5 - distance(here, tip), 0.0, 1.0);
    return ink(out, mix(vec3f(0.36, 0.55, 0.95), vec3f(1.00, 0.72, 0.36), towards), max(path, head));
}

fn bars(rel: vec2f, width: f32) -> vec4f {
    let dims = vec2i(textureDimensions(probabilities));
    if !wired(dims, textureLoad(probabilities, vec2i(0, 0), 0).a) {
        return vec4f(0.0);
    }
    let states = select(dims.y, dims.x, dims.y == 1);
    let lo = i32(rel.x * f32(states));
    let hi = max(lo + 1, i32((rel.x + 1.0 / max(width, 1.0)) * f32(states)));
    let stride = max(1, (hi - lo + MAX_FOLD - 1) / MAX_FOLD);
    var most = 0.0;
    for (var i = lo; i < min(hi, states); i = i + stride) {
        most = max(most, textureLoad(probabilities, select(vec2i(dims.x - 1, i), vec2i(i, 0), dims.y == 1), 0).r);
    }
    let reach = clamp(most / max(p.probabilities_hi, 1e-6), 0.0, 1.0);
    if rel.y < 1.0 - reach {
        return vec4f(0.0);
    }
    return vec4f(heat(0.25 + 0.75 * reach), 1.0);
}

fn shade(uv: vec2f) -> vec4f {
    if p.view == 1u {
        return spheres(uv, resolution);
    }
    if p.view == 2u {
        return bars(uv, resolution.x);
    }
    let split = 1.0 - p.bars;
    if uv.y < split {
        return spheres(vec2f(uv.x, uv.y / split), vec2f(resolution.x, resolution.y * split));
    }
    return bars(vec2f(uv.x, (uv.y - split) / p.bars), resolution.x);
}
