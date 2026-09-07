/* goofi
{ "doc": "noise, in four kinds\nPeriod is how far apart the peaks are, harmonics how many finer layers ride on top of them, and roughness how loud those layers are. Speed drifts the field, x and y pan across it. Brightness and contrast are Level's job.",
  "tags": ["image", "generator"],
  "params": [
    {"group": "noise", "name": "kind", "kind": "str", "default": "simplex",
     "options": ["simplex", "perlin", "worley", "random"]},
    {"group": "noise", "name": "period", "kind": "float", "default": 0.25, "min": 0.002, "max": 4.0},
    {"group": "noise", "name": "harmonics", "kind": "int", "default": 3, "min": 0, "max": 8},
    {"group": "noise", "name": "spread", "kind": "float", "default": 2.0, "min": 1.0, "max": 8.0},
    {"group": "noise", "name": "rough", "kind": "float", "default": 0.5, "min": 0.0, "max": 1.0},
    {"group": "noise", "name": "exponent", "kind": "float", "default": 1.0, "min": 0.1, "max": 8.0},
    {"group": "noise", "name": "mono", "kind": "bool", "default": true},
    {"group": "noise", "name": "seed", "kind": "float", "default": 0.0, "min": 0.0, "max": 1000.0},
    {"group": "move", "name": "x", "kind": "float", "default": 0.0, "min": -8.0, "max": 8.0},
    {"group": "move", "name": "y", "kind": "float", "default": 0.0, "min": -8.0, "max": 8.0},
    {"group": "move", "name": "speed", "kind": "float", "default": 0.2, "min": -10.0, "max": 10.0} ] }
*/
// Integer bit-mixing, not `fract(sin(dot(..)) * 43758)`: that hash turns a one-ulp difference in
// its argument into a wholly different value, so every lattice boundary showed as a hard seam.
fn mix32(cell: vec3i, salt: u32) -> u32 {
    let q = vec3u(cell + 65536);
    var h = q.x * 1597334673u ^ q.y * 3812015801u ^ q.z * 2654435761u ^ salt;
    h = (h ^ (h >> 16u)) * 2246822519u;
    h = (h ^ (h >> 13u)) * 3266489917u;
    return h ^ (h >> 16u);
}

// The seed salts the hash rather than sliding the field, so each seed is a whole new pattern.
fn rand(cell: vec3i, k: u32) -> f32 {
    let h = mix32(cell, bitcast<u32>(p.seed) ^ (k * 2654435761u));
    return f32(h) * (1.0 / 4294967296.0);
}

fn gradient(cell: vec3i) -> vec3f {
    let g = vec3f(rand(cell, 0u), rand(cell, 1u), rand(cell, 2u)) * 2.0 - 1.0;
    return g / max(length(g), 1e-4);
}

fn corner(cell: vec3i, x: vec3f) -> f32 {
    let t = 0.6 - dot(x, x);
    if t <= 0.0 {
        return 0.0;
    }
    return (t * t) * (t * t) * dot(gradient(cell), x);
}

fn simplex(v: vec3f) -> f32 {
    let skew = floor(v + (v.x + v.y + v.z) / 3.0);
    let x0 = v - skew + (skew.x + skew.y + skew.z) / 6.0;
    let g = step(x0.yzx, x0.xyz);
    let i1 = min(g, 1.0 - g.zxy);
    let i2 = max(g, 1.0 - g.zxy);
    var n = corner(vec3i(skew), x0);
    n = n + corner(vec3i(skew + i1), x0 - i1 + 1.0 / 6.0);
    n = n + corner(vec3i(skew + i2), x0 - i2 + 2.0 / 6.0);
    n = n + corner(vec3i(skew + 1.0), x0 - 1.0 + 3.0 / 6.0);
    return 32.0 * n;
}

fn perlin(v: vec3f) -> f32 {
    let base = floor(v);
    let f = v - base;
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    var n = 0.0;
    for (var k = 0; k < 8; k++) {
        let o = vec3f(f32(k & 1), f32((k >> 1) & 1), f32((k >> 2) & 1));
        let w = mix(1.0 - u, u, o);
        n = n + dot(gradient(vec3i(base) + vec3i(o)), f - o) * w.x * w.y * w.z;
    }
    return n * 1.1547;
}

fn worley(v: vec3f) -> f32 {
    let base = floor(v);
    let f = v - base;
    var near = 8.0;
    for (var z = -1; z <= 1; z++) {
        for (var y = -1; y <= 1; y++) {
            for (var x = -1; x <= 1; x++) {
                let o = vec3f(f32(x), f32(y), f32(z));
                let cell = vec3i(base) + vec3i(x, y, z);
                let point = o + vec3f(rand(cell, 3u), rand(cell, 4u), rand(cell, 5u));
                near = min(near, length(point - f));
            }
        }
    }
    return 1.0 - 2.0 * clamp(near, 0.0, 1.0);
}

fn one(v: vec3f) -> f32 {
    switch p.kind {
        case 1u: { return perlin(v); }
        case 2u: { return worley(v); }
        case 3u: { return rand(vec3i(floor(v)), 6u) * 2.0 - 1.0; }
        default: { return simplex(v); }
    }
}

fn field(v: vec3f) -> f32 {
    var sum = 0.0;
    var norm = 0.0;
    var amp = 1.0;
    var freq = 1.0;
    for (var h = 0; h <= p.harmonics; h++) {
        sum = sum + amp * one(v * freq);
        norm = norm + amp;
        amp = amp * p.rough;
        freq = freq * p.spread;
    }
    // Dividing by the amplitudes makes roughness a change of character, never of brightness.
    return clamp(sum / max(norm, 1e-6), -1.0, 1.0);
}

fn shade(uv: vec2f) -> vec4f {
    let aspect = vec2f(resolution.x / max(resolution.y, 1.0), 1.0);
    let at = vec3f((uv + vec2f(p.x, p.y)) * aspect, time * p.speed) / max(p.period, 1e-4);
    var rgb = vec3f(field(at));
    if p.mono == 0u {
        rgb = vec3f(rgb.r, field(at + vec3f(0.0, 0.0, 71.0)), field(at + vec3f(0.0, 0.0, 149.0)));
    }
    rgb = sign(rgb) * pow(abs(rgb), vec3f(p.exponent));
    return vec4f(rgb * 0.5 + 0.5, 1.0);
}
