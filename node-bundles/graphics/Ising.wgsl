/* goofi
{ "doc": "magnets that agree with their neighbours, against the heat\nDrag temperature through about 2.27 and the whole grid changes character: ordered below it, noise above, and long tangled domains right at it.",
  "tags": ["image", "generator", "simulation"],
  "state": ["spins"],
  "params": [
    {"group": "ising", "name": "temperature", "kind": "float", "default": 2.27, "min": 0.05, "max": 8.0},
    {"group": "ising", "name": "coupling", "kind": "float", "default": 1.0, "min": -2.0, "max": 2.0},
    {"group": "ising", "name": "field", "kind": "float", "default": 0.0, "min": -2.0, "max": 2.0},
    {"group": "ising", "name": "density", "kind": "float", "default": 0.5, "min": 0.0, "max": 1.0},
    {"group": "ising", "name": "seed", "kind": "float", "default": 0.0, "min": 0.0, "max": 1000.0} ] }
*/
fn hash3(v: vec3f) -> f32 {
    let q = vec3u(vec3i(v) + 65536);
    var h = q.x * 1597334673u ^ q.y * 3812015801u ^ q.z * 2654435761u;
    h = (h ^ (h >> 16u)) * 2246822519u;
    h = (h ^ (h >> 13u)) * 3266489917u;
    return f32(h ^ (h >> 16u)) * (1.0 / 4294967296.0);
}

// A spin is stored as 0 or 1 and read as -1 or +1; the grid wraps on both axes.
fn spin(at: vec2i) -> f32 {
    let size = vec2i(resolution);
    return textureLoad(spins, ((at % size) + size) % size, 0).r * 2.0 - 1.0;
}

fn next_spins(uv: vec2f) -> vec4f {
    let at = vec2i(floor(uv * resolution));
    if frame == 0u {
        return vec4f(vec3f(step(1.0 - p.density, hash3(vec3f(vec2f(at), p.seed)))), 1.0);
    }
    let here = spin(at);
    // Metropolis needs each spin to see settled neighbours, so the two sublattices take turns.
    // This is what `frame` is for: `time` is a float and cannot say which turn it is.
    let sublattice = u32(at.x + at.y) & 1u;
    if sublattice != (frame & 1u) {
        return vec4f(vec3f(here * 0.5 + 0.5), 1.0);
    }
    let around = spin(at + vec2i(1, 0)) + spin(at + vec2i(-1, 0)) + spin(at + vec2i(0, 1)) + spin(at + vec2i(0, -1));
    let cost = 2.0 * here * (p.coupling * around + p.field);
    let roll = hash3(vec3f(vec2f(at), f32(frame) + p.seed));
    let flip = cost <= 0.0 || roll < exp(-cost / max(p.temperature, 1e-3));
    let after = select(here, -here, flip);
    return vec4f(vec3f(after * 0.5 + 0.5), 1.0);
}

fn shade(uv: vec2f) -> vec4f {
    return vec4f(vec3f(spin(vec2i(floor(uv * resolution))) * 0.5 + 0.5), 1.0);
}
