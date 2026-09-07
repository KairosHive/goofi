/* goofi
{ "doc": "Wolfram's one-dimensional automaton, drawn as it scrolls\nOne row of cells, one rule number from 0 to 255, and every generation pushed down the screen. 30 makes noise out of one cell; 110 is a computer.",
  "tags": ["image", "generator", "simulation"],
  "state": ["rows"],
  "params": [
    {"group": "elementary", "name": "rule", "kind": "int", "default": 30, "min": 0, "max": 255},
    {"group": "elementary", "name": "start", "kind": "str", "default": "single", "options": ["single", "random"]},
    {"group": "elementary", "name": "density", "kind": "float", "default": 0.5, "min": 0.0, "max": 1.0},
    {"group": "elementary", "name": "seed", "kind": "float", "default": 0.0, "min": 0.0, "max": 1000.0} ] }
*/
fn hash2(v: vec2f) -> f32 {
    let q = vec2u(vec2i(v) + 65536);
    var h = q.x * 1597334673u ^ q.y * 3812015801u;
    h = (h ^ (h >> 16u)) * 2246822519u;
    h = (h ^ (h >> 13u)) * 3266489917u;
    return f32(h ^ (h >> 16u)) * (1.0 / 4294967296.0);
}

// The row wraps left to right, so the pattern has no edge to reflect off.
fn cell(at: vec2i) -> f32 {
    let size = vec2i(resolution);
    return step(0.5, textureLoad(rows, ((at % size) + size) % size, 0).r);
}

fn first(at: vec2i) -> f32 {
    if p.start == 1u {
        return step(1.0 - p.density, hash2(vec2f(at) + p.seed));
    }
    return step(f32(vec2i(resolution).x) * 0.5 - 1.0, f32(at.x)) * step(f32(at.x), f32(vec2i(resolution).x) * 0.5);
}

fn next_rows(uv: vec2f) -> vec4f {
    let at = vec2i(floor(uv * resolution));
    if frame == 0u {
        // The newest generation is row 0 and history scrolls down, so only the top row is seeded.
        return vec4f(vec3f(select(0.0, first(at), at.y == 0)), 1.0);
    }
    if at.y > 0 {
        return vec4f(vec3f(cell(at - vec2i(0, 1))), 1.0);
    }
    let left = cell(vec2i(at.x - 1, 0));
    let here = cell(vec2i(at.x, 0));
    let right = cell(vec2i(at.x + 1, 0));
    let index = u32(left) * 4u + u32(here) * 2u + u32(right);
    return vec4f(vec3f(f32((u32(p.rule) >> index) & 1u)), 1.0);
}

fn shade(uv: vec2f) -> vec4f {
    return vec4f(vec3f(cell(vec2i(floor(uv * resolution)))), 1.0);
}
