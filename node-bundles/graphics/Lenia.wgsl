/* goofi
{ "doc": "a continuous cellular automaton\nLife's smooth relative: a ring-shaped neighbourhood and a growth curve instead of a birth rule. Rings is the kernel's own shape and the widest knob here — one ring gives worms, three gives stars and cells. Radius is the ring as a fraction of the frame, so the creatures are the same size at any resolution. At full density the frame is a soup; lower it and separate colonies grow into the black, some of which die.",
  "tags": ["image", "generator", "simulation"],
  "state": ["field"],
  "params": [
    {"group": "lenia", "name": "radius", "kind": "float", "default": 0.2, "min": 0.06, "max": 0.5},
    {"group": "lenia", "name": "rings", "kind": "int", "default": 3, "min": 1, "max": 4},
    {"group": "lenia", "name": "falloff", "kind": "float", "default": 1.0, "min": 0.1, "max": 2.0},
    {"group": "lenia", "name": "mu", "kind": "float", "default": 0.2, "min": 0.0, "max": 1.0},
    {"group": "lenia", "name": "sigma", "kind": "float", "default": 0.017, "min": 0.001, "max": 0.3},
    {"group": "lenia", "name": "rate", "kind": "float", "default": 0.05, "min": 0.001, "max": 0.5},
    {"group": "lenia", "name": "density", "kind": "float", "default": 1.0, "min": 0.0, "max": 1.0},
    {"group": "lenia", "name": "seed", "kind": "float", "default": 0.0, "min": 0.0, "max": 1000.0} ] }
*/
// The ring is this many cells wide whatever the frame is, which is what makes `radius` a zoom
// rather than a cost; the span ceiling is what holds that cost down at the smallest radius.
const CELLS: i32 = 24;
const SPAN_MAX: f32 = 384.0;

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

// Cells a side. A radius of zero divides to infinity, which the clamp answers like any other
// radius too small to draw.
fn span() -> i32 {
    return i32(clamp(f32(CELLS) / p.radius, 8.0, min(SPAN_MAX, min(resolution.x, resolution.y))));
}

// The grid wraps, so a creature that leaves one edge arrives at the other.
fn amount(at: vec2i, n: i32) -> f32 {
    return textureLoad(field, ((at % n) + n) % n, 0).r;
}

// Whole patches a kernel wide are seeded or left empty, and the grain inside one is what a
// creature grows out of.
fn soup(at: vec2i) -> f32 {
    let live = step(1.0 - p.density, hash2(floor(vec2f(at) / f32(CELLS)) + p.seed));
    return live * hash2(floor(vec2f(at) / 4.0) + p.seed + 71.0);
}

fn next_field(uv: vec2f) -> vec4f {
    let n = span();
    let at = vec2i(floor(uv * resolution));
    // The lattice is the texture's own corner, one texel a cell; `shade` reads it and nothing else does.
    if at.x >= n || at.y >= n {
        return vec4f(0.0);
    }
    if frame == 0u {
        return vec4f(vec3f(soup(at)), 1.0);
    }
    let peaks = f32(p.rings);
    var total = 0.0;
    var weight = 0.0;
    for (var dy = -CELLS; dy <= CELLS; dy++) {
        for (var dx = -CELLS; dx <= CELLS; dx++) {
            let d = sqrt(f32(dx * dx + dy * dy)) / f32(CELLS);
            if d <= 1.0 && d > 0.0 {
                let k = pow(p.falloff, floor(d * peaks)) * bell(fract(d * peaks), 0.5, 0.15);
                weight = weight + k;
                total = total + k * amount(at + vec2i(dx, dy), n);
            }
        }
    }
    let growth = 2.0 * bell(total / weight, p.mu, p.sigma) - 1.0;
    return vec4f(vec3f(clamp(amount(at, n) + p.rate * growth, 0.0, 1.0)), 1.0);
}

// The picture is the lattice the last tick left, smoothed up to the frame — a cell is many pixels
// wide, and a straight bilinear would show every one of them as a facet.
fn shade(uv: vec2f) -> vec4f {
    let n = span();
    let g = uv * f32(n) - 0.5;
    let at = vec2i(floor(g));
    let t = smoothstep(vec2f(0.0), vec2f(1.0), fract(g));
    let top = mix(amount(at, n), amount(at + vec2i(1, 0), n), t.x);
    let bottom = mix(amount(at + vec2i(0, 1), n), amount(at + vec2i(1, 1), n), t.x);
    return vec4f(vec3f(mix(top, bottom, t.y)), 1.0);
}
