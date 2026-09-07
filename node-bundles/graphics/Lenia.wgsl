/* goofi
{ "doc": "a continuous cellular automaton\nLife's smooth relative: a ring-shaped neighbourhood and a growth curve instead of a birth rule. Creatures glide, split and die; mu and sigma decide which. The kernel is (2*radius+1) squared samples per pixel, so a large grid is expensive — set common/width and height down before raising radius.",
  "tags": ["image", "generator", "simulation"],
  "state": ["field"],
  "params": [
    {"group": "lenia", "name": "radius", "kind": "int", "default": 10, "min": 2, "max": 24},
    {"group": "lenia", "name": "mu", "kind": "float", "default": 0.15, "min": 0.0, "max": 1.0},
    {"group": "lenia", "name": "sigma", "kind": "float", "default": 0.017, "min": 0.001, "max": 0.3},
    {"group": "lenia", "name": "rate", "kind": "float", "default": 0.1, "min": 0.001, "max": 0.5},
    {"group": "lenia", "name": "density", "kind": "float", "default": 0.4, "min": 0.0, "max": 1.0},
    {"group": "lenia", "name": "scale", "kind": "float", "default": 12.0, "min": 1.0, "max": 64.0},
    {"group": "lenia", "name": "seed", "kind": "float", "default": 0.0, "min": 0.0, "max": 1000.0} ] }
*/
fn hash2(v: vec2f) -> f32 {
    let q = vec2u(vec2i(v) + 65536);
    var h = q.x * 1597334673u ^ q.y * 3812015801u;
    h = (h ^ (h >> 16u)) * 2246822519u;
    h = (h ^ (h >> 13u)) * 3266489917u;
    return f32(h ^ (h >> 16u)) * (1.0 / 4294967296.0);
}

fn bell(x: f32, m: f32, s: f32) -> f32 {
    let d = (x - m) / s;
    return exp(-0.5 * d * d);
}

// The grid wraps, so a creature that leaves one edge arrives at the other.
fn amount(at: vec2i) -> f32 {
    let size = vec2i(resolution);
    return textureLoad(field, ((at % size) + size) % size, 0).r;
}

// Blobs several cells wide, because a field seeded per pixel is below the kernel's reach and
// dissolves before anything can form.
fn soup(at: vec2i) -> f32 {
    let cell = floor(vec2f(at) / max(p.scale, 1.0));
    return step(1.0 - p.density, hash2(cell + p.seed));
}

fn next_field(uv: vec2f) -> vec4f {
    let at = vec2i(floor(uv * resolution));
    if frame == 0u {
        return vec4f(vec3f(soup(at)), 1.0);
    }
    let r = p.radius;
    var total = 0.0;
    var weight = 0.0;
    for (var dy = -r; dy <= r; dy++) {
        for (var dx = -r; dx <= r; dx++) {
            let d = sqrt(f32(dx * dx + dy * dy)) / f32(r);
            if d <= 1.0 && d > 0.0 {
                let k = bell(d, 0.5, 0.15);
                weight = weight + k;
                total = total + k * amount(at + vec2i(dx, dy));
            }
        }
    }
    let u = total / max(weight, 1e-6);
    let growth = 2.0 * bell(u, p.mu, p.sigma) - 1.0;
    return vec4f(vec3f(clamp(amount(at) + p.rate * growth, 0.0, 1.0)), 1.0);
}

// The kernel is hundreds of samples wide, so the picture reads the field the last tick left
// rather than computing it a second time.
fn shade(uv: vec2f) -> vec4f {
    return vec4f(vec3f(amount(vec2i(floor(uv * resolution)))), 1.0);
}
