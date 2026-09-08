/* goofi
{ "doc": "the particles a swarm is made of, where they are and where they are going\nA dot per particle in the model's own unit box, with a streak along its velocity. Wire `positions`, and `velocities` and `phases` for the streak and the colour. A third column is depth: it shrinks and dims what is far away. Past 512 particles the picture samples them.",
  "tags": ["image", "simulation"],
  "inputs": [
    {"name": "positions", "kind": "ARRAY"},
    {"name": "velocities", "kind": "ARRAY"},
    {"name": "phases", "kind": "ARRAY"} ],
  "params": [
    {"group": "swarm", "name": "colour", "kind": "str", "default": "species",
     "options": ["species", "phase", "speed", "plain"]},
    {"group": "swarm", "name": "species", "kind": "int", "default": 4, "min": 1, "max": 16},
    {"group": "swarm", "name": "size", "kind": "float", "default": 3.0, "min": 0.5, "max": 32.0},
    {"group": "swarm", "name": "streak", "kind": "float", "default": 0.25, "min": 0.0, "max": 4.0},
    {"group": "swarm", "name": "depth", "kind": "bool", "default": true} ] }
*/

// A swarm of two thousand is two thousand texture reads at every texel, so the picture walks at
// most this many of them.
const MAX_UNITS: i32 = 512;

fn hue(t: f32) -> vec3f {
    return 0.55 + 0.45 * cos(6.2831853 * (t + vec3f(0.0, 0.33, 0.67)));
}

fn ink(under: vec4f, colour: vec3f, cover: f32) -> vec4f {
    return vec4f(mix(under.rgb, colour, cover), max(under.a, cover));
}

/// Whether an input carries anything: an unwired one is the shared transparent texel.
fn wired(dims: vec2i, alpha: f32) -> bool {
    return dims.x > 1 || dims.y > 1 || alpha >= 0.5;
}

/// Row `i`, column `k` of an `[n, dims]` frame; 0 for an axis the model does not carry.
fn place(i: i32, k: i32) -> f32 {
    if k >= i32(textureDimensions(positions).x) {
        return 0.0;
    }
    return textureLoad(positions, vec2i(k, i), 0).r;
}

fn drift(i: i32, k: i32) -> f32 {
    let dims = vec2i(textureDimensions(velocities));
    if k >= dims.x || i >= dims.y {
        return 0.0;
    }
    return textureLoad(velocities, vec2i(k, i), 0).r;
}

fn to_segment(here: vec2f, a: vec2f, b: vec2f) -> f32 {
    let run = b - a;
    let t = clamp(dot(here - a, run) / max(dot(run, run), 1e-6), 0.0, 1.0);
    let off = here - (a + run * t);
    return dot(off, off);
}

/// What one particle is drawn in: its species, its phase, how fast it is going, or nothing.
fn colour_of(i: i32, speed: f32) -> vec3f {
    let plain = vec3f(0.90, 0.92, 0.96);
    switch p.colour {
        case 1u: {
            let dims = vec2i(textureDimensions(phases));
            if !wired(dims, textureLoad(phases, vec2i(0, 0), 0).a) || i >= dims.x {
                return plain;
            }
            return hue(textureLoad(phases, vec2i(i, 0), 0).r / 6.2831853);
        }
        case 2u: {
            let top = max(abs(p.velocities_lo), abs(p.velocities_hi));
            return hue(0.62 - 0.62 * clamp(speed / max(top, 1e-6), 0.0, 1.0));
        }
        case 3u: { return plain; }
        default: { return hue(f32(i % p.species) / f32(p.species)); }
    }
}

fn shade(uv: vec2f) -> vec4f {
    let dims = vec2i(textureDimensions(positions));
    if !wired(dims, textureLoad(positions, vec2i(0, 0), 0).a) {
        return vec4f(0.0);
    }
    let n = dims.y;
    let here = uv * resolution;
    var out = vec4f(0.0);
    let stride = max(1, (n + MAX_UNITS - 1) / MAX_UNITS);
    for (var i = 0; i < n; i = i + stride) {
        let far = select(0.5, clamp(place(i, 2), 0.0, 1.0), p.depth != 0u && dims.x > 2);
        // Behind is small and dim, in front is large and bright: the whole of what depth buys.
        let at = vec2f(place(i, 0), place(i, 1)) * resolution;
        let velocity = vec2f(drift(i, 0), drift(i, 1));
        let near = sqrt(to_segment(here, at, at + velocity * p.streak * resolution));
        let cover = clamp(p.size * mix(0.6, 1.4, far) + 0.5 - near, 0.0, 1.0);
        if cover > 0.0 {
            out = ink(out, colour_of(i, length(velocity)) * mix(0.45, 1.0, far), cover);
        }
    }
    return out;
}
