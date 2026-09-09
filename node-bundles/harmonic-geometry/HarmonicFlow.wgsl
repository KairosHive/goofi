/* goofi
{ "doc": "Persistent ink carried by a harmonic flow field.\nWire HarmonicTransport.flow to flow. Seeded ink is advected by that vector field and by the curl of its own slow memory. There is no hidden time animation. rate 0 freezes the state; reset seeds while held true. diffusion can eventually smooth the image, so injection is available as an explicit control. Output red is ink, green is memory, blue is departure; use HarmonicInk in magnitude mode to color it.",
  "tags": ["image", "simulation"],
  "state": ["dye"],
  "inputs": [{"name": "flow", "kind": "ARRAY"}],
  "params": [
    {"group": "motion", "name": "rate", "kind": "float", "default": 1.0, "min": 0.0, "max": 3.0, "doc": "Advection steps per frame; 0 freezes all state."},
    {"group": "motion", "name": "travel", "kind": "float", "default": 2.0, "min": 0.0, "max": 8.0, "doc": "Travel in output pixels per frame at unit flow."},
    {"group": "motion", "name": "feedback", "kind": "float", "default": 0.7, "min": 0.0, "max": 4.0, "doc": "Extra flow from the curl of the ink's own memory."},
    {"group": "motion", "name": "memory", "kind": "float", "default": 0.97, "min": 0.5, "max": 0.999, "doc": "How much old memory remains after a step."},
    {"group": "motion", "name": "diffusion", "kind": "float", "default": 0.015, "min": 0.0, "max": 0.2, "doc": "Local mixing; higher values smooth the ink faster."},
    {"group": "seed", "name": "seed", "kind": "int", "default": 7, "min": 0, "max": 65535},
    {"group": "seed", "name": "grain", "kind": "float", "default": 4.0, "min": 1.0, "max": 32.0, "doc": "Initial ink grain size in pixels."},
    {"group": "seed", "name": "injection", "kind": "float", "default": 0.0, "min": 0.0, "max": 0.1, "doc": "Optional continuous replenishment; 0 preserves the seeded experiment."},
    {"group": "seed", "name": "reset", "kind": "bool", "default": false, "doc": "Hold true to seed, then release to evolve."},
    {"group": "common", "name": "width", "kind": "int", "default": 256, "min": 0, "max": 4096},
    {"group": "common", "name": "height", "kind": "int", "default": 256, "min": 0, "max": 4096}
  ] }
*/

fn noise(at: vec2f) -> f32 {
    let q = vec2u(vec2i(floor(at / p.grain)) + 65536);
    var h = q.x * 1597334673u ^ q.y * 3812015801u ^ u32(p.seed) * 1013904223u;
    h = (h ^ (h >> 16u)) * 2246822519u;
    h = (h ^ (h >> 13u)) * 3266489917u;
    return f32(h ^ (h >> 16u)) / 4294967296.0;
}

fn state_at(at: vec2i) -> vec4f {
    let size = vec2i(resolution);
    return textureLoad(dye, ((at % size) + size) % size, 0);
}

fn carried(at: vec2f) -> vec4f {
    let corner = vec2i(floor(at)); let part = fract(at);
    return mix(mix(state_at(corner), state_at(corner + vec2i(1, 0)), part.x),
               mix(state_at(corner + vec2i(0, 1)), state_at(corner + vec2i(1, 1)), part.x), part.y);
}

fn next_dye(uv: vec2f) -> vec4f {
    let at = vec2i(floor(uv * resolution));
    let force = textureSampleLevel(flow, samp, uv, 0.0);
    let coverage = force.a;
    if frame == 0u || p.reset != 0u {
        let seed = noise(vec2f(at));
        return vec4f(seed, seed, 0.0, coverage);
    }
    let old = state_at(at);
    if p.rate == 0.0 { return old; }
    let dx = vec2i(1, 0); let dy = vec2i(0, 1);
    let grad = 0.5 * vec2f(state_at(at + dx).g - state_at(at - dx).g,
                           state_at(at + dy).g - state_at(at - dy).g);
    let velocity = force.rg * p.travel + vec2f(-grad.y, grad.x) * p.feedback * 12.0;
    let moved = carried(vec2f(at) - velocity * p.rate);
    let neighbors = 0.25 * (state_at(at + dx).r + state_at(at - dx).r + state_at(at + dy).r + state_at(at - dy).r);
    var ink = mix(moved.r, neighbors, min(p.diffusion * p.rate, 0.5));
    ink = mix(ink, noise(vec2f(at)), min(p.injection * p.rate, 1.0));
    let memory = mix(ink, moved.g, pow(p.memory, p.rate));
    return vec4f(clamp(ink, 0.0, 1.0), memory, abs(ink - memory), coverage);
}

fn shade(uv: vec2f) -> vec4f {
    return state_at(vec2i(floor(uv * resolution)));
}
