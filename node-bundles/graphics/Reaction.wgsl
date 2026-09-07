/* goofi
{ "doc": "two chemicals that spread and eat each other\nThe Turing pattern: spots, stripes, worms and mitosis, all out of the same two numbers. Feed and kill are the pair worth dragging.",
  "tags": ["image", "generator", "simulation"],
  "state": ["chem"],
  "params": [
    {"group": "reaction", "name": "model", "kind": "str", "default": "grayscott", "options": ["grayscott", "brusselator"]},
    {"group": "reaction", "name": "feed", "kind": "float", "default": 0.037, "min": 0.0, "max": 4.0},
    {"group": "reaction", "name": "kill", "kind": "float", "default": 0.06, "min": 0.0, "max": 6.0},
    {"group": "reaction", "name": "spread_a", "kind": "float", "default": 1.0, "min": 0.0, "max": 2.0},
    {"group": "reaction", "name": "spread_b", "kind": "float", "default": 0.5, "min": 0.0, "max": 2.0},
    {"group": "reaction", "name": "rate", "kind": "float", "default": 1.0, "min": 0.01, "max": 1.5},
    {"group": "reaction", "name": "density", "kind": "float", "default": 0.02, "min": 0.0, "max": 1.0},
    {"group": "reaction", "name": "seed", "kind": "float", "default": 0.0, "min": 0.0, "max": 1000.0} ] }
*/
fn hash2(v: vec2f) -> f32 {
    let q = vec2u(vec2i(v) + 65536);
    var h = q.x * 1597334673u ^ q.y * 3812015801u;
    h = (h ^ (h >> 16u)) * 2246822519u;
    h = (h ^ (h >> 13u)) * 3266489917u;
    return f32(h ^ (h >> 16u)) * (1.0 / 4294967296.0);
}

fn pair(at: vec2i) -> vec2f {
    let size = vec2i(resolution);
    return textureLoad(chem, ((at % size) + size) % size, 0).rg;
}

// The nine-point stencil, which is isotropic enough that stripes do not follow the pixel grid.
fn laplacian(at: vec2i) -> vec2f {
    var sum = -pair(at);
    let side = 0.2;
    let corner = 0.05;
    sum = sum + side * (pair(at + vec2i(1, 0)) + pair(at + vec2i(-1, 0)));
    sum = sum + side * (pair(at + vec2i(0, 1)) + pair(at + vec2i(0, -1)));
    sum = sum + corner * (pair(at + vec2i(1, 1)) + pair(at + vec2i(-1, 1)));
    sum = sum + corner * (pair(at + vec2i(1, -1)) + pair(at + vec2i(-1, -1)));
    return sum;
}

fn next_chem(uv: vec2f) -> vec4f {
    let at = vec2i(floor(uv * resolution));
    if frame == 0u {
        // Gray-Scott starts full of A and needs a seeded blot of B to have anything to do.
        let blot = step(1.0 - p.density, hash2(floor(vec2f(at) / 4.0) + p.seed));
        if p.model == 1u {
            return vec4f(1.0 + 0.1 * blot, 1.0, 0.0, 1.0);
        }
        return vec4f(1.0, blot, 0.0, 1.0);
    }
    let s = pair(at);
    let d = laplacian(at);
    var da = 0.0;
    var db = 0.0;
    if p.model == 1u {
        // Brusselator: `feed` is A and `kill` is B, and it needs no seeded blot to break symmetry.
        da = p.spread_a * d.x + p.feed - (p.kill + 1.0) * s.x + s.x * s.x * s.y;
        db = p.spread_b * d.y + p.kill * s.x - s.x * s.x * s.y;
    } else {
        let eaten = s.x * s.y * s.y;
        da = p.spread_a * d.x - eaten + p.feed * (1.0 - s.x);
        db = p.spread_b * d.y + eaten - (p.feed + p.kill) * s.y;
    }
    let next = clamp(s + p.rate * vec2f(da, db), vec2f(0.0), vec2f(4.0));
    return vec4f(next, 0.0, 1.0);
}

// The second chemical is the one that draws the pattern; the first is its background.
fn shade(uv: vec2f) -> vec4f {
    let s = pair(vec2i(floor(uv * resolution)));
    return vec4f(vec3f(clamp(s.y, 0.0, 1.0)), 1.0);
}
