/* goofi
{ "doc": "grains pile up until the pile knocks itself over\nSelf-organized criticality: nothing tunes it, and it settles by itself at the point where one grain can start an avalanche of any size.",
  "tags": ["image", "generator", "simulation"],
  "state": ["grains"],
  "params": [
    {"group": "sandpile", "name": "threshold", "kind": "int", "default": 4, "min": 2, "max": 8},
    {"group": "sandpile", "name": "rain", "kind": "float", "default": 0.002, "min": 0.0, "max": 0.2},
    {"group": "sandpile", "name": "start", "kind": "float", "default": 2.0, "min": 0.0, "max": 8.0},
    {"group": "sandpile", "name": "seed", "kind": "float", "default": 0.0, "min": 0.0, "max": 1000.0} ] }
*/
fn hash3(v: vec3f) -> f32 {
    let q = vec3u(vec3i(v) + 65536);
    var h = q.x * 1597334673u ^ q.y * 3812015801u ^ q.z * 2654435761u;
    h = (h ^ (h >> 16u)) * 2246822519u;
    h = (h ^ (h >> 13u)) * 3266489917u;
    return f32(h ^ (h >> 16u)) * (1.0 / 4294967296.0);
}

fn height(at: vec2i) -> f32 {
    let size = vec2i(resolution);
    return textureLoad(grains, ((at % size) + size) % size, 0).r;
}

// A cell topples when it holds `threshold` grains, sending one to each of its four neighbours.
fn toppling(at: vec2i) -> f32 {
    return step(f32(p.threshold), height(at));
}

fn next_grains(uv: vec2f) -> vec4f {
    let at = vec2i(floor(uv * resolution));
    if frame == 0u {
        return vec4f(vec3f(floor(hash3(vec3f(vec2f(at), p.seed)) * (p.start + 1.0))), 1.0);
    }
    // Everything settles in one pass because the toppling is read from the NEIGHBOURS rather than
    // written to them: each cell gives away what it must and takes what it is owed.
    let given = toppling(at) * f32(p.threshold);
    let taken = toppling(at + vec2i(1, 0)) + toppling(at + vec2i(-1, 0))
        + toppling(at + vec2i(0, 1)) + toppling(at + vec2i(0, -1));
    // Grains only rain onto a pile that is already at rest, so an avalanche runs to its end and
    // its size means something.
    let quiet = 1.0 - step(0.5, given + taken);
    let fell = quiet * step(1.0 - p.rain, hash3(vec3f(vec2f(at), f32(frame) + p.seed)));
    return vec4f(vec3f(height(at) - given + taken + fell), 1.0);
}

// Height as brightness, against the threshold, so a full cell reads white.
fn shade(uv: vec2f) -> vec4f {
    let h = height(vec2i(floor(uv * resolution))) / f32(p.threshold);
    return vec4f(vec3f(clamp(h, 0.0, 1.0)), 1.0);
}
